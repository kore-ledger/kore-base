/// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later
use std::marker::PhantomData;

use identity::identifier::KeyIdentifier;
use tokio_util::sync::CancellationToken;

use super::compiler::manager::KoreCompiler;
use super::errors::EvaluatorError;
use super::{EvaluatorMessage, EvaluatorResponse};
use crate::commons::channel::{ChannelData, MpscChannel, SenderEnd};
use crate::commons::self_signature_manager::{SelfSignature, SelfSignatureManager};
use crate::database::{DatabaseCollection, DatabaseManager};
use crate::evaluator::errors::ExecutorErrorResponses;
use crate::evaluator::runner::manager::KoreRunner;
use crate::governance::GovernanceInterface;
use crate::message::{MessageConfig, MessageContent, MessageTaskCommand};
use crate::protocol::KoreMessages;
use crate::request::EventRequest;
use crate::signature::Signed;
use crate::utils::message::event::create_evaluator_response;
use crate::{DigestDerivator, EvaluationResponse};

pub struct EvaluatorManagerChannels {
    input_channel: MpscChannel<EvaluatorMessage, EvaluatorResponse>,
    messenger_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
    protocol_channel: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
}

impl EvaluatorManagerChannels {
    pub fn new(
        input_channel: MpscChannel<EvaluatorMessage, EvaluatorResponse>,
        messenger_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
        protocol_channel: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
    ) -> Self {
        Self {
            input_channel,
            messenger_channel,
            protocol_channel,
        }
    }
}
pub struct EvaluatorManager<
    M: DatabaseManager<C>,
    C: DatabaseCollection + 'static,
    G: GovernanceInterface + Send + Clone + 'static,
> {
    /// Communication channel for incoming petitions
    input_channel: MpscChannel<EvaluatorMessage, EvaluatorResponse>,
    /// Contract executioner
    runner: KoreRunner<C, G>,
    signature_manager: SelfSignatureManager,
    token: CancellationToken,
    messenger_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
    derivator: DigestDerivator,
    _m: PhantomData<M>,
    _g: PhantomData<G>,
    our_id: KeyIdentifier,
    protocol_channel: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
}

impl<
        M: DatabaseManager<C>,
        C: DatabaseCollection,
        G: GovernanceInterface + Send + Clone + 'static,
    > EvaluatorManager<M, C, G>
{
    pub fn new(
        compiler: KoreCompiler<C, G>,
        runner: KoreRunner<C, G>,
        signature_manager: SelfSignatureManager,
        token: CancellationToken,
        derivator: DigestDerivator,
        channels: EvaluatorManagerChannels,
    ) -> Self {
        // spawn compiler
        tokio::spawn(async move {
            compiler.start().await;
        });

        Self {
            input_channel: channels.input_channel,
            runner,
            our_id: signature_manager.get_own_identifier(),
            signature_manager,
            token,
            messenger_channel: channels.messenger_channel,
            _m: PhantomData,
            _g: PhantomData,
            derivator,
            protocol_channel: channels.protocol_channel,
        }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                command = self.input_channel.receive() => {
                    match command {
                        Some(command) => {
                            let result = self.process_command(command).await;
                            if result.is_err() {
                                log::error!("{}", result.unwrap_err());
                                break;
                            }
                        }
                        None => {
                            break;
                        },
                    }
                },
                _ = self.token.cancelled() => {
                    log::debug!("Shutdown received");
                    break;
                }
            }
        }
        self.token.cancel();
        log::info!("Ended");
    }

    async fn process_command(
        &mut self,
        command: ChannelData<EvaluatorMessage, EvaluatorResponse>,
    ) -> Result<(), EvaluatorError> {
        let (sender, data) = match command {
            ChannelData::AskData(data) => {
                let (sender, data) = data.get();
                (Some(sender), data)
            }
            ChannelData::TellData(data) => {
                let data = data.get();
                (None, data)
            }
        };
        let response = 'response: {
            match data {
                EvaluatorMessage::EvaluationEvent {
                    evaluation_request,
                    sender,
                } => {
                    let EventRequest::Fact(state_data) = &evaluation_request.event_request.content
                    else {
                        break 'response EvaluatorResponse::AskForEvaluation(Err(
                            super::errors::EvaluatorErrorResponses::CreateRequestNotAllowed,
                        ));
                    };
                    let result = self
                        .runner
                        .execute_contract(&evaluation_request, state_data)
                        .await;
                    match result {
                        Ok(executor_response) => {
                            let signature = self
                                .signature_manager
                                .sign(&executor_response, self.derivator)
                                .map_err(|_| EvaluatorError::SignatureGenerationFailed)?;
                            let signed_evaluator_response: crate::signature::Signed<
                                crate::EvaluationResponse,
                            > = Signed::<EvaluationResponse>::new(executor_response, signature);
                            let msg = create_evaluator_response(signed_evaluator_response);
                            self.send_message(None, msg, sender, MessageConfig::direct_response())
                                .await?;
                            EvaluatorResponse::AskForEvaluation(Ok(()))
                        }
                        Err(ExecutorErrorResponses::OurGovIsHigher) => {
                            // Mandar mensaje de actualización pendiente
                            self.send_message(
                                None,
                                KoreMessages::EventMessage(
                                    crate::event::EventCommand::HigherGovernanceExpected {
                                        governance_id: evaluation_request.context.governance_id,
                                        who_asked: self.signature_manager.get_own_identifier(),
                                    },
                                ),
                                sender,
                                MessageConfig::direct_response(),
                            )
                            .await?;

                            EvaluatorResponse::AskForEvaluation(Ok(()))
                        }
                        Err(ExecutorErrorResponses::OurGovIsLower) => {
                            // No podemos evaluar porque nos la van a rechazar
                            // Pedir LCE al que nos mando la petición
                            self.send_message(
                                None,
                                KoreMessages::LedgerMessages(
                                    crate::ledger::LedgerCommand::GetLCE {
                                        who_asked: self.signature_manager.get_own_identifier(),
                                        subject_id: evaluation_request.context.governance_id,
                                    },
                                ),
                                sender,
                                MessageConfig::direct_response(),
                            )
                            .await?;

                            EvaluatorResponse::AskForEvaluation(Ok(()))
                        }
                        Err(ExecutorErrorResponses::DatabaseError(error)) => {
                            return Err(EvaluatorError::DatabaseError(error))
                        }
                        Err(ExecutorErrorResponses::ContractNotFound(schema, gov)) => {
                            log::error!("Contract not found: {}, {}", schema, gov);
                            return Ok(());
                        }
                        Err(ExecutorErrorResponses::ContractNotUpdated) => {
                            log::error!("Contract not Updated");
                            return Ok(());
                        }
                        Err(ExecutorErrorResponses::GovernanceNotFound) => {
                            log::error!("Governance not found");
                            return Ok(());
                        }
                        Err(
                            ExecutorErrorResponses::StateJSONDeserializationFailed
                            | ExecutorErrorResponses::JSONPATCHDeserializationFailed,
                        ) => return Err(EvaluatorError::JSONDeserializationFailed),
                        Err(error) => {
                            log::error!("ERROR EVALUATOR: {:?}", error);
                            break 'response EvaluatorResponse::AskForEvaluation(Err(
                                super::errors::EvaluatorErrorResponses::ContractExecutionError(
                                    error,
                                ),
                            ));
                        }
                    }
                }
                EvaluatorMessage::AskForEvaluation(_) => {
                    log::error!("Ask for Evaluation in Evaluator Manager");
                    return Ok(());
                }
            }
        };
        if sender.is_some() {
            sender
                .unwrap()
                .send(response)
                .map_err(|_| EvaluatorError::ChannelNotAvailable)?;
        }
        Ok(())
    }

    /// This function is in charge of sending KoreMessages, if it is a message addressed
    /// to the node itself, the message is sent directly to Protocol, otherwise it is sent to Network.
    async fn send_message(
        &self,
        subject_id: Option<String>,
        event_message: KoreMessages,
        sender: KeyIdentifier,
        message_config: MessageConfig,
    ) -> Result<(), EvaluatorError> {
        if sender == self.our_id {
            let complete_message = Signed::<MessageContent<KoreMessages>>::new(
                self.our_id.clone(),
                self.our_id.clone(),
                event_message,
                &self.signature_manager,
                self.derivator,
            )
            .unwrap();

            self.protocol_channel
                .tell(complete_message)
                .await
                .map_err(|_| EvaluatorError::ChannelNotAvailable)
        } else {
            self.messenger_channel
                .tell(MessageTaskCommand::Request(
                    subject_id,
                    event_message,
                    vec![sender],
                    message_config,
                ))
                .await
                .map_err(|_| EvaluatorError::ChannelNotAvailable)
        }
    }
}

#[cfg(test)]
mod test {

    /* TODO - Fix this test
    use std::{collections::HashSet, fs, str::FromStr, sync::Arc};

    use async_trait::async_trait;
    use json_patch::diff;
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use tokio::sync::{broadcast::Sender, mpsc};
    use tokio_util::sync::CancellationToken;

    use crate::{
        commons::{
            channel::{ChannelData, MpscChannel, SenderEnd},
            crypto::{Ed25519KeyPair, KeyGenerator, KeyMaterial, KeyPair},
            models::{
                evaluation::{EvaluationRequest, SubjectContext},
                state::Subject,
            },
            schema_handler::gov_models::Contract,
            self_signature_manager::{SelfSignatureInterface, SelfSignatureManager},
        },
        database::{MemoryCollection, DB},
        evaluator::{EvaluatorMessage, EvaluatorResponse},
        event::EventCommand,
        governance::{
            error::RequestError, stage::ValidationStage, GovernanceInterface,
            GovernanceUpdatedMessage,
        },
        identifier::{DigestIdentifier, KeyIdentifier},
        message::MessageTaskCommand,
        protocol::KoreMessages,
        request::{EventRequest, FactRequest},
        signature::Signed,
        MemoryManager, Metadata, ValueWrapper,
    };

    use crate::evaluator::manager::EvaluatorManager;

    const SC_DIR: &str = "/tmp/taple_contracts/";

    pub fn create_dir() {
        remove_dir();
        match fs::create_dir(SC_DIR) {
            Ok(_) => (),
            Err(err) => {
                println!("Error creating directory: {}", err);
                assert!(false);
            }
        }
    }

    pub fn remove_dir() {
        if fs::metadata(SC_DIR).is_ok() {
            match fs::remove_dir_all(SC_DIR) {
                Ok(_) => (),
                Err(err) => {
                    println!("Error removing directory: {}", err);
                    assert!(false);
                }
            }
        }
    }

    // Subject State
    #[derive(Serialize, Deserialize, Debug)]
    pub struct Data {
        pub one: u32,
        pub two: u32,
        pub three: u32,
    }

    // Event Family
    #[derive(Serialize, Deserialize, Debug)]
    pub enum EventType {
        Notify {
            chunk: Vec<u8>,
        },
        ModOne {
            data: u32,
            chunk: Vec<u8>,
        },
        ModTwo {
            data: u32,
            chunk: Vec<u8>,
        },
        ModThree {
            data: u32,
            chunk: Vec<u8>,
        },
        ModAll {
            data: (u32, u32, u32),
            chunk: Vec<u8>,
        },
    }

    #[derive(Clone)]
    struct GovernanceMockup {}

    fn get_file_wrong() -> String {
        String::from(
            r#"
        #[no_mangle]
        pub unsafe fn main_function(state_ptr: i32, event_ptr: i32, is_owner: i32) {

        }
        "#,
        )
    }

    fn get_file_wrong2() -> String {
        String::from(
            r#"
        #[no_mangle]
        pub unsafe fn main_function(state_ptr: i32, event_ptr: i32, is_owner: i32) -> i32 {
            4
        }
        "#,
        )
    }

    fn get_file() -> String {
        String::from(
            r#"
            use taple_sc_rust as sdk;
            use serde::{Deserialize, Serialize};

            // Intento de simulación de cómo podría ser un contrato

            // Subject State
            #[derive(Serialize, Deserialize, Clone)]
            pub struct Data {
                pub one: u32,
                pub two: u32,
                pub three: u32,
            }

            // Event Family
            #[derive(Serialize, Deserialize, Debug)]
            pub enum EventType {
                Notify { chunk: Vec<u8> },
                ModOne { data: u32, chunk: Vec<u8> },
                ModTwo { data: u32, chunk: Vec<u8> },
                ModThree { data: u32, chunk: Vec<u8> },
                ModAll { data: (u32, u32, u32), chunk: Vec<u8> },
            }

            #[no_mangle]
            pub unsafe fn main_function(state_ptr: i32, event_ptr: i32, is_owner: i32) -> u32 {
                sdk::execute_contract(state_ptr, event_ptr, is_owner, contract_logic)
            }


                context -> inmutable con estado inicial roles y evento
                result -> mutable success y approvalRequired, y estado final
                approvalRequired por defecto a false y siempre false si KO o error


            // Lógica del contrato con los tipos de datos esperados
            // Devuelve el puntero a los datos escritos con el estado modificado
            fn contract_logic(
                context: &sdk::Context<Data, EventType>,
                contract_result: &mut sdk::ContractResult<Data>,
            ) {
                // Sería posible añadir gestión de errores
                // Podría ser interesante hacer las operaciones directamente como serde_json:Value en lugar de "Custom Data"
                let state = &mut contract_result.final_state;
                match &context.event {
                    EventType::Notify { chunk: _ } => {
                        // Evento que no modifica el estado
                        // Estos eventos se añadirían a la cadena, pero dentro del contrato apenas harían algo
                    }
                    EventType::ModOne { data, chunk: _ } => {
                        // Evento que modifica Data.one
                        if context.is_owner {
                            state.one = *data;
                        }
                    }
                    EventType::ModTwo { data, chunk: _ } => {
                        // Evento que modifica Data.two
                        state.two = *data;
                    }
                    EventType::ModThree { data, chunk: _ } => {
                        // Evento que modifica Data.three
                        state.three = *data;
                    }
                    EventType::ModAll { data, chunk: _ } => {
                        // Evento que modifica el estado entero
                        state.one = data.0;
                        state.two = data.1;
                        state.three = data.2;
                    }
                }
                contract_result.success = true;
            }
        "#,
        )
    }

    #[async_trait]
    impl GovernanceInterface for GovernanceMockup {
        async fn get_init_state(
            &self,
            _governance_id: DigestIdentifier,
            _schema_id: String,
            _governance_version: u64,
        ) -> Result<ValueWrapper, RequestError> {
            return Ok(ValueWrapper(serde_json::json!({
                "one": 10,
                "two": 11,
                "three": 13,
            })));
        }

        async fn get_schema(
            &self,
            _governance_id: DigestIdentifier,
            _schema_id: String,
            _governance_version: u64,
        ) -> Result<ValueWrapper, RequestError> {
            return Ok(ValueWrapper(serde_json::json!({
                "id": "test",
                "type": "object",
                "properties": {
                    "one": {
                        "type": "number"
                    },
                    "two": {
                        "type": "number"
                    },
                    "three": {
                        "type": "number"
                    },
                },
                "required": [ "one", "two", "three" ],
                "additionalProperties": false
            })));
        }

        async fn get_signers(
            &self,
            _metadata: Metadata,
            _stage: ValidationStage,
        ) -> Result<HashSet<KeyIdentifier>, RequestError> {
            unimplemented!()
        }

        async fn get_quorum(
            &self,
            _metadata: Metadata,
            _stage: ValidationStage,
        ) -> Result<u32, RequestError> {
            unimplemented!()
        }

        async fn get_invoke_info(
            &self,
            _metadata: Metadata,
            _stage: ValidationStage,
            _invoker: KeyIdentifier,
        ) -> Result<bool, RequestError> {
            unreachable!()
        }

        async fn get_contracts(
            &self,
            governance_id: DigestIdentifier,
            _governance_version: u64,
        ) -> Result<Vec<(Contract, String)>, RequestError> {
            if governance_id
                == DigestIdentifier::from_str("Jg2Nuv5bNs4swQGcPQ1CXs9MtcfwMVoeQDR2Ea1YNYJw")
                    .unwrap()
            {
                Ok(vec![(
                    Contract {
                        raw: String::from("test"),
                    },
                    "test".to_owned(),
                )])
            } else if governance_id
                == DigestIdentifier::from_str("Jg2Nuc5bNs4swQGcPQ1CXs9MtcfwMVoeQDR2Ea1YNYJw")
                    .unwrap()
            {
                Ok(vec![(
                    Contract {
                        raw: get_file_wrong().to_string(),
                    },
                    "test".to_owned(),
                )])
            } else if governance_id
                == DigestIdentifier::from_str("Jg2Nuc5bNs4swQGcPQ2CXs9MtcfwMVoeQDR2Ea2YNYJw")
                    .unwrap()
            {
                Ok(vec![(
                    Contract {
                        raw: get_file_wrong2().to_string(),
                    },
                    "test".to_owned(),
                )])
            } else {
                Ok(vec![(
                    Contract {
                        raw: get_file().to_string(),
                    },
                    "test".to_owned(),
                )])
            }
        }

        async fn get_governance_version(
            &self,
            _governance_id: DigestIdentifier,
            _subject_id: DigestIdentifier,
        ) -> Result<u64, RequestError> {
            unimplemented!()
        }

        async fn is_governance(&self, _subject_id: DigestIdentifier) -> Result<bool, RequestError> {
            unimplemented!()
        }

        async fn governance_updated(
            &self,
            _governance_id: DigestIdentifier,
            _governance_version: u64,
        ) -> Result<(), RequestError> {
            Ok(())
        }
    }

    fn build_module() -> (
        EvaluatorManager<MemoryManager, MemoryCollection, GovernanceMockup>,
        SenderEnd<EvaluatorMessage, EvaluatorResponse>,
        Sender<GovernanceUpdatedMessage>,
        SelfSignatureManager,
        MpscChannel<MessageTaskCommand<KoreMessages>, ()>,
    ) {
        let (rx, sx) = MpscChannel::new(100);
        let (msg_rx, msg_sx) = MpscChannel::new(100);
        let (sx_compiler, rx_compiler) = tokio::sync::broadcast::channel(100);
        let keypair = KeyPair::Ed25519(Ed25519KeyPair::from_seed(&[]));
        let pk = keypair.public_key_bytes();
        let signature_manager = SelfSignatureManager {
            keys: keypair,
            identifier: KeyIdentifier::new(crate::KeyDerivator::Ed25519, &pk),
            digest_derivator: crate::DigestDerivator::Blake3_256,
        };
        let (notification_tx, notification_rx) = mpsc::channel(1000);
        let token = CancellationToken::new();
        let governance = GovernanceMockup {};
        let collection = Arc::new(MemoryManager::new());
        let database = DB::new(collection.clone());
        database
            .set_subject(
                &DigestIdentifier::from_str("JGSPR6FL-vE7iZxWMd17o09qn7NeTqlcImDVWmijXczw")
                    .unwrap(),
                create_governance_test(signature_manager.identifier.clone()),
            )
            .unwrap();
        let manager = EvaluatorManager::new(
            rx,
            collection,
            signature_manager.clone(),
            rx_compiler,
            token,
            notification_tx,
            governance,
            SC_DIR.to_string(),
            msg_sx,
            crate::DigestDerivator::Blake3_256,
        );
        (manager, sx, sx_compiler, signature_manager, msg_rx)
    }

    fn create_governance_test(identifier: KeyIdentifier) -> Subject {
        let initial_state = Data {
            one: 10,
            two: 11,
            three: 13,
        };
        let initial_state_json = serde_json::to_value(&initial_state).unwrap();
        Subject {
            keys: None,
            subject_id: DigestIdentifier::from_str("JGSPR6FL-vE7iZxWMd17o09qn7NeTqlcImDVWmijXczw")
                .unwrap(),
            governance_id: DigestIdentifier::from_str("").unwrap(),
            sn: 0,
            public_key: identifier.clone(),
            namespace: "namespace1".into(),
            schema_id: "test".into(),
            owner: identifier.clone(),
            creator: identifier,
            properties: ValueWrapper(initial_state_json),
            active: true,
            name: "".to_owned(),
            genesis_gov_version: 0,
        }
    }

    fn create_event_request(
        json: Value,
        signature_manager: &SelfSignatureManager,
    ) -> Signed<EventRequest> {
        let request = EventRequest::Fact(FactRequest {
            subject_id: DigestIdentifier::from_str("JXtZRpNgBWVg9v5YG9AaTNfCpPd-rCTTKrFW9cV8-JKs")
                .unwrap(),
            payload: ValueWrapper(json),
        });
        let signature = signature_manager
            .sign(&request, crate::DigestDerivator::Blake3_256)
            .unwrap();
        let event_request = Signed::<EventRequest> {
            content: request,
            signature,
        };
        event_request
    }

    fn generate_json_patch(prev_state: Value, new_state: Value) -> Value {
        let patch = diff(&prev_state, &new_state);
        serde_json::to_value(&patch).unwrap()
    }

    // TODO - Fix this test

    #[test]
    fn contract_execution() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            create_dir();
            let (evaluator, sx_evaluator, sx_compiler, signature_manager, mut msg_rx) =
                build_module();
            let handler = tokio::spawn(async move {
                evaluator.run().await;
            });
            let initial_state = Data {
                one: 10,
                two: 11,
                three: 13,
            };
            let initial_state_json = serde_json::to_value(&initial_state).unwrap();
            let event = EventType::ModTwo {
                data: 100,
                chunk: vec![123, 45, 20],
            };
            let governance_id =
                DigestIdentifier::from_str("JGSPR6FL-vE7iZxWMd17o09qn7NeTqlcImDVWmijXczw").unwrap();
            println!("Compiling governance contract");
            tokio::time::sleep(tokio::time::Duration::from_secs(20)).await; // Pausa para compilar el contrato de la gobernanza
            println!("Finish compiling");
            sx_compiler
                .send(GovernanceUpdatedMessage::GovernanceUpdated {
                    governance_id: governance_id.to_owned(),
                    governance_version: 0,
                })
                .unwrap();
            println!("Compiling simulation contract");
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await; // Pausa para compilar el contrato de la simulación
            println!("Finish compiling");
            let response = sx_evaluator
                .ask(EvaluatorMessage::EvaluationEvent {
                    evaluation_request: EvaluationRequest {
                        event_request: create_event_request(
                            serde_json::to_value(&event).unwrap(),
                            &signature_manager,
                        ),
                        context: SubjectContext {
                            governance_id: governance_id.to_owned(),
                            schema_id: "test".into(),
                            namespace: "namespace1".into(),
                            is_owner: true,
                            state: ValueWrapper(initial_state_json.clone()),
                        },
                        sn: 1,
                        gov_version: 0,
                    },
                    sender: signature_manager.identifier.clone(),
                })
                .await
                .unwrap();
            let EvaluatorResponse::AskForEvaluation(result) = response;
            assert!(result.is_ok());
            let message = if let ChannelData::TellData(data) = msg_rx.receive().await.unwrap() {
                if let MessageTaskCommand::Request(_, data, _, _) = data.get() {
                    data
                } else {
                    panic!("Unexpected 2");
                }
            } else {
                panic!("Unexpected");
            };
            let evaluator_response = if let TapleMessages::EventMessage(event) = message {
                match event {
                    EventCommand::EvaluatorResponse { evaluator_response } => evaluator_response,
                    _ => {
                        panic!("Unexpected 4");
                    }
                }
            } else {
                panic!("Unexpected 3");
            };
            let new_state = Data {
                one: 10,
                two: 100,
                three: 13,
            };
            let new_state_json = serde_json::to_value(&new_state).unwrap();
            let patch = generate_json_patch(initial_state_json, new_state_json);
            assert_eq!(patch, evaluator_response.content.patch.0);
            let own_identifier = signature_manager.identifier.to_owned();
            assert_eq!(evaluator_response.signature.signer, own_identifier);
            remove_dir();
            handler.abort();
        });
    }


    #[test]
    fn contract_execution_fail() {
        // Fail reason: Bad Event
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let (evaluator, sx_evaluator, sx_compiler, signature_manager, msg_rx) = build_module();
            let initial_state = Data {
                one: 10,
                two: 11,
                three: 13,
            };
            let initial_state_json = serde_json::to_string(&initial_state).unwrap();
            let event = String::from("hola");

            let handler = tokio::spawn(async move {
                evaluator.start().await;
            });

            sx_compiler
                .send(GovernanceUpdatedMessage::GovernanceUpdated {
                    governance_id: DigestIdentifier::from_str(
                        "JGSPR6FL-vE7iZxWMd17o09qn7NeTqlcImDVWmijXczw",
                    )
                    .unwrap(),
                    governance_version: 0,
                })
                .unwrap();
            // sx_compiler
            //     .ask(CompilerMessages::NewGovVersion(NewGovVersion {
            //         governance_id: DigestIdentifier::from_str(
            //             "JGSPR6FL-vE7iZxWMd17o09qn7NeTqlcImDVWmijXczw",
            //         )
            //         .unwrap(),
            //         governance_version: 0,
            //     }))
            //     .await
            //     .unwrap();

            let response = sx_evaluator
                .ask(EvaluatorMessage::AskForEvaluation(EventPreEvaluation {
                    // invokation: create_event_request(
                    //     serde_json::to_string(&event).unwrap(),
                    //     &signature_manager,
                    // ),
                    // hash_request: DigestIdentifier::default().to_str(),
                    event_request: create_event_request(
                        serde_json::to_string(&event).unwrap(),
                        &signature_manager,
                    ),
                    context: Context {
                        governance_id: DigestIdentifier::from_str(
                            "JGSPR6FL-vE7iZxWMd17o09qn7NeTqlcImDVWmijXczw",
                        )
                        .unwrap(),
                        schema_id: "test".into(),
                        creator: KeyIdentifier::from_str(
                            "EF3E6fTSLrsEWzkD2tkB6QbJU9R7IOkunImqp0PB_ejg",
                        )
                        .unwrap(),
                        owner: KeyIdentifier::from_str(
                            "EF3E6fTSLrsEWzkD2tkB6QbJU9R7IOkunImqp0PB_ejg",
                        )
                        .unwrap(),
                        actual_state: initial_state_json.clone(),
                        namespace: "namespace1".into(),
                        governance_version: 0,
                    },
                    sn: 1,
                }))
                .await
                .unwrap();
            let EvaluatorResponse::AskForEvaluation(result) = response;
            assert!(result.is_ok());
            // let result = result.unwrap();
            // assert!(!result.success);
            handler.abort();
        });
    }

    #[test]
    fn contract_execution_fail2() {
        // Fail reason: Bad State
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let (evaluator, sx_evaluator, sx_compiler, signature_manager) = build_module();
            let initial_state = String::from("hola");
            let initial_state_json = serde_json::to_string(&initial_state).unwrap();
            let event = EventType::ModTwo {
                data: 100,
                chunk: vec![123, 45, 20],
            };

            let handler = tokio::spawn(async move {
                evaluator.start().await;
            });

            sx_compiler
                .ask(CompilerMessages::NewGovVersion(NewGovVersion {
                    governance_id: DigestIdentifier::from_str(
                        "JGSPR6FL-vE7iZxWMd17o09qn7NeTqlcImDVWmijXczw",
                    )
                    .unwrap(),
                    governance_version: 0,
                }))
                .await
                .unwrap();

            let response = sx_evaluator
                .ask(EvaluatorMessage::AskForEvaluation(
                    crate::evaluator::AskForEvaluation {
                        invokation: create_event_request(
                            serde_json::to_string(&event).unwrap(),
                            &signature_manager,
                        ),
                        // hash_request: DigestIdentifier::default().to_str(),
                        context: Context {
                            governance_id: DigestIdentifier::from_str(
                                "JGSPR6FL-vE7iZxWMd17o09qn7NeTqlcImDVWmijXczw",
                            )
                            .unwrap(),
                            schema_id: "test".into(),
                            invokator: KeyIdentifier::from_str(
                                "EF3E6fTSLrsEWzkD2tkB6QbJU9R7IOkunImqp0PB_ejg",
                            )
                            .unwrap(),
                            creator: KeyIdentifier::from_str(
                                "EF3E6fTSLrsEWzkD2tkB6QbJU9R7IOkunImqp0PB_ejg",
                            )
                            .unwrap(),
                            owner: KeyIdentifier::from_str(
                                "EF3E6fTSLrsEWzkD2tkB6QbJU9R7IOkunImqp0PB_ejg",
                            )
                            .unwrap(),
                            state: initial_state_json.clone(),
                            namespace: "namespace1".into(),
                        },
                        sn: 1,
                    },
                ))
                .await
                .unwrap();
            let EvaluatorResponse::AskForEvaluation(result) = response;
            assert!(result.is_ok());
            let result = result.unwrap();
            assert!(!result.success);
            handler.abort();
        });
    }

    #[test]
    fn contract_execution_wrong_gov_id() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let (evaluator, sx_evaluator, sx_compiler, signature_manager) = build_module();
            let initial_state = Data {
                one: 10,
                two: 11,
                three: 13,
            };
            let initial_state_json = serde_json::to_string(&initial_state).unwrap();
            let event = EventType::ModTwo {
                data: 100,
                chunk: vec![123, 45, 20],
            };

            let handler = tokio::spawn(async move {
                evaluator.start().await;
            });

            sx_compiler
                .ask(CompilerMessages::NewGovVersion(NewGovVersion {
                    governance_id: DigestIdentifier::from_str(
                        "JGSPR6FL-vE7iZxWMd17o09qn7NeTqlcImDVWmijXczw",
                    )
                    .unwrap(),
                    governance_version: 0,
                }))
                .await
                .unwrap();

            let response = sx_evaluator
                .ask(EvaluatorMessage::AskForEvaluation(
                    crate::evaluator::AskForEvaluation {
                        invokation: create_event_request(
                            serde_json::to_string(&event).unwrap(),
                            &signature_manager,
                        ),
                        // hash_request: DigestIdentifier::default().to_str(),
                        context: Context {
                            governance_id: DigestIdentifier::from_str(
                                "Jg2Nuv5bNs4swQGcPQ1CXs9MtcfwMVoeQDR2Ea1YNYJw",
                            )
                            .unwrap(),
                            schema_id: "test".into(),
                            invokator: KeyIdentifier::from_str(
                                "EF3E6fTSLrsEWzkD2tkB6QbJU9R7IOkunImqp0PB_ejg",
                            )
                            .unwrap(),
                            creator: KeyIdentifier::from_str(
                                "EF3E6fTSLrsEWzkD2tkB6QbJU9R7IOkunImqp0PB_ejg",
                            )
                            .unwrap(),
                            owner: KeyIdentifier::from_str(
                                "EF3E6fTSLrsEWzkD2tkB6QbJU9R7IOkunImqp0PB_ejg",
                            )
                            .unwrap(),
                            state: initial_state_json.clone(),
                            namespace: "namespace1".into(),
                        },
                        sn: 1,
                    },
                ))
                .await
                .unwrap();
            let EvaluatorResponse::AskForEvaluation(result) = response;
            assert!(result.is_ok());
            let result = result.unwrap();
            assert!(!result.success);
            handler.abort();
        });
    }

    #[test]
    fn contract_compilation_no_sdk() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let (evaluator, _sx_evaluator, sx_compiler, signature_manager) = build_module();

            let handler = tokio::spawn(async move {
                evaluator.start().await;
            });

            let response = sx_compiler
                .ask(CompilerMessages::NewGovVersion(NewGovVersion {
                    governance_id: DigestIdentifier::from_str(
                        "Jg2Nuc5bNs4swQGcPQ2CXs9MtcfwMVoeQDR2Ea2YNYJw",
                    )
                    .unwrap(),
                    governance_version: 0,
                }))
                .await
                .unwrap();
            if let CompilerResponses::CompileContract(Err(CompilerErrorResponses::NoSDKFound)) =
                response
            {
                handler.abort();
            } else {
                assert!(false)
            };
        });
    }

    #[test]
    fn compilation_error() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let (_evaluator, _sx_evaluator, sx_compiler, signature_manager) = build_module();

            let response = sx_compiler
                .ask(CompilerMessages::NewGovVersion(NewGovVersion {
                    governance_id: DigestIdentifier::from_str(
                        "Jg2Nuv5bNs4swQGcPQ1CXs9MtcfwMVoeQDR2Ea1YNYJw",
                    )
                    .unwrap(),
                    governance_version: 10,
                }))
                .await
                .unwrap();
            let CompilerResponses::CompileContract(result) = response else {
                panic!("Invalid response received");
            };
            assert!(result.is_err());
            let CompilerErrorResponses::CargoExecError = result.unwrap_err() else {
                panic!("Invalid response received");
            };
        });
    }*/
}
