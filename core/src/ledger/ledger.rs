use std::collections::{hash_map::Entry, HashMap, HashSet};

use json_patch::{patch, Patch};
use serde_json::Value;

use crate::{
    commons::{
        channel::SenderEnd,
        models::{approval::Approval, state::Subject},
    },
    database::DB,
    distribution::{error::DistributionErrorResponses, DistributionMessagesNew},
    event_content::Metadata,
    event_request::{EventRequest, EventRequestType},
    governance::{stage::ValidationStage, GovernanceAPI, GovernanceInterface},
    identifier::{Derivable, DigestIdentifier, KeyIdentifier},
    message::{MessageConfig, MessageTaskCommand},
    protocol::protocol_message_manager::TapleMessages,
    signature::Signature,
    utils::message::ledger::request_event,
    DatabaseManager, Event,
};

use super::errors::LedgerError;

pub struct LedgerState {
    pub current_sn: Option<u64>,
    pub head: Option<u64>,
}

pub struct Ledger<D: DatabaseManager> {
    gov_api: GovernanceAPI,
    database: DB<D>,
    subject_is_gov: HashMap<DigestIdentifier, bool>,
    ledger_state: HashMap<DigestIdentifier, LedgerState>,
    message_channel: SenderEnd<MessageTaskCommand<TapleMessages>, ()>,
    distribution_channel:
        SenderEnd<DistributionMessagesNew, Result<(), DistributionErrorResponses>>,
    our_id: KeyIdentifier,
}

impl<D: DatabaseManager> Ledger<D> {
    pub fn new(
        gov_api: GovernanceAPI,
        database: DB<D>,
        message_channel: SenderEnd<MessageTaskCommand<TapleMessages>, ()>,
        distribution_channel: SenderEnd<
            DistributionMessagesNew,
            Result<(), DistributionErrorResponses>,
        >,
        our_id: KeyIdentifier,
    ) -> Self {
        Self {
            gov_api,
            database,
            subject_is_gov: HashMap::new(),
            ledger_state: HashMap::new(),
            message_channel,
            distribution_channel,
            our_id,
        }
    }

    pub async fn init(&mut self) -> Result<(), LedgerError> {
        // Revisar si tengo sujetos a medio camino entre estado actual y LCE
        // Actualizar hashmaps
        let subjects = self.database.get_all_subjects();
        for subject in subjects.into_iter() {
            // Añadirlo a is_gov
            if self
                .gov_api
                .is_governance(subject.subject_id.clone())
                .await?
            {
                self.subject_is_gov.insert(subject.subject_id.clone(), true);
                // Enviar mensaje a gov de governance updated con el id y el sn
            } else {
                self.subject_is_gov
                    .insert(subject.subject_id.clone(), false);
            }
            // Actualizar ledger_state para ese subject
            let mut last_two_events =
                self.database
                    .get_events_by_range(&subject.subject_id, Some(-1), 2)?;
            let last_event = match last_two_events.pop() {
                Some(event) => event,
                None => return Err(LedgerError::ZeroEventsSubject(subject.subject_id.to_str())),
            };
            let pre_last_event = match last_two_events.pop() {
                Some(event) => event,
                None => {
                    self.ledger_state.insert(
                        subject.subject_id,
                        LedgerState {
                            current_sn: Some(0),
                            head: None,
                        },
                    );
                    continue;
                }
            };
            if last_event.content.event_proposal.proposal.sn
                == pre_last_event.content.event_proposal.proposal.sn + 1
            {
                if subject.sn != last_event.content.event_proposal.proposal.sn {
                    return Err(LedgerError::WrongSnInSubject(subject.subject_id.to_str()));
                }
                self.ledger_state.insert(
                    subject.subject_id,
                    LedgerState {
                        current_sn: Some(last_event.content.event_proposal.proposal.sn),
                        head: None,
                    },
                );
            } else {
                if subject.sn != pre_last_event.content.event_proposal.proposal.sn {
                    return Err(LedgerError::WrongSnInSubject(subject.subject_id.to_str()));
                }
                self.ledger_state.insert(
                    subject.subject_id,
                    LedgerState {
                        current_sn: Some(pre_last_event.content.event_proposal.proposal.sn),
                        head: Some(last_event.content.event_proposal.proposal.sn),
                    },
                );
            }
        }
        Ok(())
    }

    pub async fn genesis(&mut self, event_request: EventRequest) -> Result<(), LedgerError> {
        // Añadir a subject_is_gov si es una governance y no está
        let EventRequestType::Create(create_request) = event_request.request.clone() else {
            return Err(LedgerError::StateInGenesis)
        };
        let governance_version = self
            .gov_api
            .get_governance_version(create_request.governance_id.clone())
            .await?;
        let init_state = self
            .gov_api
            .get_init_state(
                create_request.governance_id,
                create_request.schema_id.clone(),
                governance_version,
            )
            .await?;
        let init_state_string = serde_json::to_string(&init_state)
            .map_err(|_| LedgerError::ErrorParsingJsonString("Init State".to_owned()))?;
        // Crear sujeto a partir de genesis y evento
        let subject = Subject::from_genesis_request(event_request.clone(), init_state_string)
            .map_err(LedgerError::SubjectError)?;
        // Crear evento a partir de event_request
        let event = Event::from_genesis_request(
            event_request,
            subject.keys.clone().unwrap(),
            governance_version,
            &init_state,
        )
        .map_err(LedgerError::SubjectError)?;
        // Añadir sujeto y evento a base de datos
        let subject_id = subject.subject_id.clone();
        if &create_request.schema_id == "governance" {
            self.subject_is_gov.insert(subject_id.clone(), true);
            // Enviar mensaje a gov de governance updated con el id y el sn
        } else {
            self.subject_is_gov.insert(subject_id.clone(), false);
        }
        self.database.set_subject(&subject_id, subject)?;
        self.database.set_event(&subject_id, event)?;
        // Actualizar Ledger State
        match self.ledger_state.entry(subject_id.clone()) {
            Entry::Occupied(mut ledger_state) => {
                let ledger_state = ledger_state.get_mut();
                ledger_state.current_sn = Some(0);
            }
            Entry::Vacant(entry) => {
                entry.insert(LedgerState {
                    current_sn: Some(0),
                    head: None,
                });
            }
        }
        // Mandar subject_id y evento en mensaje a distribution manager
        self.distribution_channel
            .tell(DistributionMessagesNew::SignaturesNeeded { subject_id, sn: 0 })
            .await?;
        Ok(())
    }

    pub async fn event_validated(
        &mut self,
        event: Event,
        signatures: HashSet<Signature>,
    ) -> Result<(), LedgerError> {
        let sn = event.content.event_proposal.proposal.sn;
        let EventRequestType::State(state_request) = &event.content.event_proposal.proposal.event_request.request
            else {
                return Err(LedgerError::StateInGenesis)
            };
        let subject_id = state_request.subject_id.clone();
        self.database.set_signatures(
            &subject_id,
            event.content.event_proposal.proposal.sn,
            signatures,
        )?;
        // Aplicar event sourcing
        let mut subject = self
            .database
            .get_subject(&subject_id)
            .map_err(|error| match error {
                crate::DbError::EntryNotFound => LedgerError::SubjectNotFound(subject_id.to_str()),
                _ => LedgerError::DatabaseError(error),
            })?;
        let json_patch = event.content.event_proposal.proposal.json_patch.as_str();
        subject.update_subject(json_patch, event.content.event_proposal.proposal.sn)?;
        self.database.set_event(&subject_id, event)?;
        self.database
            .set_subject(&subject_id, subject)
            .map_err(|error| LedgerError::DatabaseError(error))?;
        // Comprobar is_gov
        let is_gov = self.subject_is_gov.get(&subject_id);
        match is_gov {
            Some(true) => {
                // Enviar mensaje a gov de governance updated con el id y el sn
                // TODO
            }
            Some(false) => {}
            None => {
                // Si no está en el mapa, añadirlo y enviar mensaje a gov de subject updated con el id y el sn
                if self.gov_api.is_governance(subject_id.clone()).await? {
                    self.subject_is_gov.insert(subject_id.clone(), true);
                    // Enviar mensaje a gov de governance updated con el id y el sn
                } else {
                    self.subject_is_gov.insert(subject_id.clone(), false);
                }
            }
        }
        // Actualizar Ledger State
        match self.ledger_state.entry(subject_id.clone()) {
            Entry::Occupied(mut ledger_state) => {
                let ledger_state = ledger_state.get_mut();
                let current_sn = ledger_state.current_sn.as_mut().unwrap();
                *current_sn = *current_sn + 1;
            }
            Entry::Vacant(entry) => {
                entry.insert(LedgerState {
                    current_sn: Some(0),
                    head: None,
                });
            }
        }
        // Enviar a Distribution info del nuevo event y que lo distribuya
        self.distribution_channel
            .tell(DistributionMessagesNew::SignaturesNeeded { subject_id, sn })
            .await?;
        Ok(())
    }

    pub async fn external_event(
        &mut self,
        event: Event,
        signatures: HashSet<Signature>,
        sender: KeyIdentifier,
    ) -> Result<(), LedgerError> {
        // Comprobaciones criptográficas
        event.check_signatures()?;
        // Comprobar si es genesis o state
        match event
            .content
            .event_proposal
            .proposal
            .event_request
            .request
            .clone()
        {
            EventRequestType::Create(create_request) => {
                // Comprobar que evaluation es None
                if event.content.event_proposal.proposal.evaluation.is_some() {
                    return Err(LedgerError::ErrorParsingJsonString(
                        "Evaluation should be None in external genesis event".to_owned(),
                    ));
                }
                // Comprobaciones criptográficas
                let subject_id = create_subject_id(&event)?;
                match self.database.get_subject(&subject_id) {
                    Ok(_) => {
                        return Err(LedgerError::SubjectAlreadyExists(
                            subject_id.to_str().to_owned(),
                        ))
                    }
                    Err(crate::DbError::EntryNotFound) => {}
                    Err(error) => {
                        return Err(LedgerError::DatabaseError(error));
                    }
                };
                let our_gov_version = self
                    .gov_api
                    .get_governance_version(create_request.governance_id.clone())
                    .await?;
                let metadata = Metadata {
                    namespace: create_request.namespace,
                    subject_id: subject_id.clone(),
                    governance_id: create_request.governance_id,
                    governance_version: our_gov_version,
                    schema_id: create_request.schema_id,
                    owner: event
                        .content
                        .event_proposal
                        .proposal
                        .event_request
                        .signature
                        .content
                        .signer
                        .clone(),
                    creator: event
                        .content
                        .event_proposal
                        .proposal
                        .event_request
                        .signature
                        .content
                        .signer
                        .clone(),
                };
                let mut witnesses = self.get_witnesses(metadata).await?;
                if !witnesses.contains(&self.our_id) {
                    return Err(LedgerError::WeAreNotWitnesses(subject_id.to_str()));
                }
                self.check_genesis(event, subject_id.clone()).await?;
                match self.ledger_state.get_mut(&subject_id) {
                    Some(ledger_state) => {
                        ledger_state.current_sn = Some(0);
                    }
                    None => {
                        self.ledger_state.insert(
                            subject_id.clone(),
                            LedgerState {
                                current_sn: Some(0),
                                head: None,
                            },
                        );
                    }
                }
                // Enviar mensaje a distribution manager
                self.distribution_channel
                    .tell(DistributionMessagesNew::SignaturesNeeded { subject_id, sn: 0 })
                    .await?;
            }
            EventRequestType::State(state_request) => {
                // Comprobaciones criptográficas
                match self.ledger_state.get(&state_request.subject_id) {
                    Some(ledger_state) => {
                        let mut subject = match self.database.get_subject(&state_request.subject_id)
                        {
                            Ok(subject) => subject,
                            Err(crate::DbError::EntryNotFound) => {
                                // Pedir génesis
                                let msg = request_event(state_request.subject_id, 0);
                                self.message_channel
                                    .tell(MessageTaskCommand::Request(
                                        None,
                                        msg,
                                        vec![sender],
                                        MessageConfig {
                                            timeout: 2000,
                                            replication_factor: 1.0,
                                        },
                                    ))
                                    .await?;
                                return Err(LedgerError::SubjectNotFound("".into()));
                            }
                            Err(error) => {
                                return Err(LedgerError::DatabaseError(error));
                            }
                        };
                        // Comprobar que las firmas son válidas y suficientes
                        let metadata = Metadata {
                            namespace: subject.namespace.clone(),
                            subject_id: subject.subject_id.clone(),
                            governance_id: subject.governance_id.clone(),
                            governance_version: event.content.event_proposal.proposal.gov_version,
                            schema_id: subject.schema_id.clone(),
                            owner: subject.owner.clone(),
                            creator: subject.creator.clone(),
                        };
                        let mut witnesses = self.get_witnesses(metadata.clone()).await?;
                        if !witnesses.contains(&self.our_id) {
                            return Err(LedgerError::WeAreNotWitnesses(
                                state_request.subject_id.to_str(),
                            ));
                        }
                        self.check_event(event.clone(), metadata.clone()).await?;
                        let (signers, quorum) = self
                            .get_signers_and_quorum(metadata, ValidationStage::Validate)
                            .await?;
                        verify_signatures(
                            &signatures,
                            &signers,
                            quorum,
                            &event.signature.content.event_content_hash,
                        )?;
                        // Comprobar si es evento siguiente o LCE
                        if event.content.event_proposal.proposal.sn == subject.sn + 1
                            && ledger_state.head.is_none()
                        {
                            // Caso Evento Siguiente
                            let sn = event.content.event_proposal.proposal.sn;
                            let json_patch =
                                event.content.event_proposal.proposal.json_patch.as_str();
                            subject.update_subject(
                                json_patch,
                                event.content.event_proposal.proposal.sn,
                            )?;
                            // TODO: No guardar firmas si hay un head con mayor sn
                            self.database.set_signatures(
                                &state_request.subject_id,
                                sn,
                                signatures,
                            )?;
                            self.database.set_event(&state_request.subject_id, event)?;
                            self.database
                                .set_subject(&state_request.subject_id, subject)
                                .map_err(|error| LedgerError::DatabaseError(error))?;
                            self.ledger_state.insert(
                                state_request.subject_id.clone(),
                                LedgerState {
                                    current_sn: Some(sn),
                                    head: ledger_state.head,
                                },
                            );
                            // Mandar firma de testificacion a distribution manager o el evento en sí
                            self.distribution_channel
                                .tell(DistributionMessagesNew::SignaturesNeeded {
                                    subject_id: state_request.subject_id,
                                    sn,
                                })
                                .await?;
                        } else if event.content.event_proposal.proposal.sn > subject.sn {
                            // Caso LCE
                            // Comprobar que LCE es mayor y quedarnos con el mas peque si tenemos otro
                            let last_lce = match ledger_state.head {
                                Some(head) => {
                                    if event.content.event_proposal.proposal.sn > head {
                                        return Err(LedgerError::LCEBiggerSN);
                                    }
                                    Some(head)
                                }
                                None => {
                                    // Va a ser el nuevo LCE
                                    None
                                }
                            };
                            // Si hemos llegado aquí es porque va a ser nuevo LCE
                            let sn = event.content.event_proposal.proposal.sn;
                            self.database.set_signatures(
                                &state_request.subject_id,
                                sn,
                                signatures,
                            )?;
                            self.database.set_event(&state_request.subject_id, event)?;
                            if last_lce.is_some() {
                                let last_lce_sn = last_lce.unwrap();
                                self.database
                                    .del_signatures(&state_request.subject_id, last_lce_sn)?;
                                self.database
                                    .del_event(&state_request.subject_id, last_lce_sn)?;
                            }
                            self.ledger_state.insert(
                                state_request.subject_id.clone(),
                                LedgerState {
                                    current_sn: ledger_state.current_sn,
                                    head: Some(sn),
                                },
                            );
                            // Pedir evento siguiente a current_sn
                            witnesses.insert(subject.owner);
                            let msg = request_event(state_request.subject_id, 0);
                            self.message_channel
                                .tell(MessageTaskCommand::Request(
                                    None,
                                    msg,
                                    witnesses.into_iter().collect(),
                                    MessageConfig {
                                        timeout: 2000,
                                        replication_factor: 0.8,
                                    },
                                ))
                                .await?;
                        } else {
                            // Caso evento repetido
                            return Err(LedgerError::EventAlreadyExists);
                        }
                    }
                    None => {
                        // Es LCE y no tenemos Subject, no podemos hacer comprobaciones de governance
                        let sn = event.content.event_proposal.proposal.sn;
                        self.database
                            .set_signatures(&state_request.subject_id, sn, signatures)?;
                        self.database.set_event(&state_request.subject_id, event)?;
                        self.ledger_state.insert(
                            state_request.subject_id.clone(),
                            LedgerState {
                                current_sn: None,
                                head: Some(sn),
                            },
                        );
                        // Pedir evento 0
                        let msg = request_event(state_request.subject_id, 0);
                        self.message_channel
                            .tell(MessageTaskCommand::Request(
                                None,
                                msg,
                                vec![sender],
                                MessageConfig {
                                    timeout: 2000,
                                    replication_factor: 1.0,
                                },
                            ))
                            .await?;
                    }
                };
            }
        }
        Ok(())
    }

    pub async fn external_intermediate_event(&mut self, event: Event) -> Result<(), LedgerError> {
        // Comprobaciones criptográficas
        event.check_signatures()?;
        // Comprobar si es genesis o state
        let subject_id = match &event.content.event_proposal.proposal.event_request.request {
            EventRequestType::Create(create_request) => {
                // Comprobar si había un LCE previo o es genesis puro, si es genesis puro rechazar y que manden por la otra petición aunque sea con hashset de firmas vacío
                create_subject_id(&event)?
            }
            EventRequestType::State(state_request) => state_request.subject_id.clone(),
        };
        let ledger_state = self.ledger_state.get(&subject_id);
        match ledger_state {
            Some(ledger_state) => {
                // Comprobar que tengo firmas de un evento mayor y que es el evento siguiente que necesito para este subject
                match ledger_state.head {
                    Some(head) => {
                        match ledger_state.current_sn {
                            Some(current_sn) => {
                                let mut subject = match self.database.get_subject(&subject_id) {
                                    Ok(subject) => subject,
                                    Err(crate::DbError::EntryNotFound) => {
                                        return Err(LedgerError::SubjectNotFound("".into()));
                                    }
                                    Err(error) => {
                                        return Err(LedgerError::DatabaseError(error));
                                    }
                                };
                                let metadata = Metadata {
                                    namespace: subject.namespace.clone(),
                                    subject_id: subject.subject_id.clone(),
                                    governance_id: subject.governance_id.clone(),
                                    governance_version: event
                                        .content
                                        .event_proposal
                                        .proposal
                                        .gov_version,
                                    schema_id: subject.schema_id.clone(),
                                    owner: subject.owner.clone(),
                                    creator: subject.creator.clone(),
                                };
                                // Comprobar que el evento es el siguiente
                                if event.content.event_proposal.proposal.sn == current_sn + 1 {
                                    // Comprobar que el evento es el que necesito
                                    self.check_event(event.clone(), metadata.clone()).await?;
                                    // Guardar Evento
                                    self.database.set_event(&subject_id, event)?;
                                    // Hacer event sourcing del evento y actualizar subject
                                    self.event_sourcing(subject_id.clone(), current_sn + 1)?;
                                    if head == current_sn + 2 {
                                        // Hacer event sourcing del LCE tambien y actualizar subject
                                        self.event_sourcing(subject_id.clone(), head)?;
                                        self.ledger_state.insert(
                                            subject_id.clone(),
                                            LedgerState {
                                                current_sn: Some(head),
                                                head: None,
                                            },
                                        );
                                        // Se llega hasta el LCE con el event sourcing
                                        // Pedir firmas de testificación
                                        self.distribution_channel
                                            .tell(DistributionMessagesNew::SignaturesNeeded {
                                                subject_id: subject_id.clone(),
                                                sn: head,
                                            })
                                            .await?;
                                    } else {
                                        self.ledger_state.insert(
                                            subject_id.clone(),
                                            LedgerState {
                                                current_sn: Some(current_sn + 1),
                                                head: Some(head),
                                            },
                                        );
                                        // No se llega hasta el LCE con el event sourcing
                                        // Pedir siguiente evento
                                        let mut witnesses =
                                            self.get_witnesses(metadata.clone()).await?;
                                        witnesses.insert(metadata.owner);
                                        let msg = request_event(subject_id, current_sn + 2);
                                        self.message_channel
                                            .tell(MessageTaskCommand::Request(
                                                None,
                                                msg,
                                                witnesses.into_iter().collect(),
                                                MessageConfig {
                                                    timeout: 2000,
                                                    replication_factor: 0.8,
                                                },
                                            ))
                                            .await?;
                                    }
                                    Ok(())
                                } else {
                                    // El evento no es el que necesito
                                    Err(LedgerError::EventNotNext)
                                }
                            }
                            None => {
                                // El siguiente es el evento 0
                                if event.content.event_proposal.proposal.sn == 0 {
                                    // Comprobar que el evento 0 es el que necesito
                                    self.check_genesis(event.clone(), subject_id.clone())
                                        .await?;
                                    // Guardar Evento
                                    self.database.set_event(&subject_id, event.clone())?;

                                    // Guardar sujeto
                                    let EventRequestType::Create(create_request) = &event
                                        .content
                                        .event_proposal
                                        .proposal
                                        .event_request
                                        .request else {
                                        return Err(LedgerError::StateInGenesis);
                                        };
                                    let init_state = self
                                        .gov_api
                                        .get_init_state(
                                            create_request.governance_id.clone(),
                                            create_request.schema_id.clone(),
                                            event.content.event_proposal.proposal.gov_version,
                                        )
                                        .await?;
                                    let init_state_string = serde_json::to_string(&init_state)
                                        .map_err(|_| {
                                            LedgerError::ErrorParsingJsonString(
                                                "Init State".to_owned(),
                                            )
                                        })?;
                                    let governance_version =
                                        event.content.event_proposal.proposal.gov_version;
                                    let subject =
                                        Subject::from_genesis_event(event, init_state_string)?;
                                    let metadata = Metadata {
                                        namespace: subject.namespace.clone(),
                                        subject_id: subject.subject_id.clone(),
                                        governance_id: subject.governance_id.clone(),
                                        governance_version,
                                        schema_id: subject.schema_id.clone(),
                                        owner: subject.owner.clone(),
                                        creator: subject.creator.clone(),
                                    };
                                    self.database.set_subject(&subject_id, subject)?;
                                    if head == 1 {
                                        // Hacer event sourcing del evento 1 tambien y actualizar subject
                                        self.event_sourcing(subject_id.clone(), 1)?;
                                        self.ledger_state.insert(
                                            subject_id.clone(),
                                            LedgerState {
                                                current_sn: Some(1),
                                                head: None,
                                            },
                                        );
                                        // Se llega hasta el LCE con el event sourcing
                                        // Pedir firmas de testificación
                                        self.distribution_channel
                                            .tell(DistributionMessagesNew::SignaturesNeeded {
                                                subject_id: subject_id.clone(),
                                                sn: 1,
                                            })
                                            .await?;
                                    } else {
                                        self.ledger_state.insert(
                                            subject_id.clone(),
                                            LedgerState {
                                                current_sn: Some(0),
                                                head: Some(head),
                                            },
                                        );
                                        // No se llega hasta el LCE con el event sourcing
                                        // Pedir siguiente evento
                                        let mut witnesses =
                                            self.get_witnesses(metadata.clone()).await?;
                                        witnesses.insert(metadata.owner);
                                        let msg = request_event(subject_id, 1);
                                        self.message_channel
                                            .tell(MessageTaskCommand::Request(
                                                None,
                                                msg,
                                                witnesses.into_iter().collect(),
                                                MessageConfig {
                                                    timeout: 2000,
                                                    replication_factor: 0.8,
                                                },
                                            ))
                                            .await?;
                                    }
                                    Ok(())
                                } else {
                                    // El evento 0 no es el que necesito
                                    Err(LedgerError::UnsignedUnknowEvent)
                                }
                            }
                        }
                    }
                    None => Err(LedgerError::UnsignedUnknowEvent),
                }
            }
            None => Err(LedgerError::UnsignedUnknowEvent),
        }
    }

    async fn get_witnesses(
        &self,
        metadata: Metadata,
    ) -> Result<HashSet<KeyIdentifier>, LedgerError> {
        let signers = self
            .gov_api
            .get_signers(metadata, ValidationStage::Witness)
            .await?;
        Ok(signers)
    }

    // TODO Existe otra igual en event manager, unificar en una sola y poner en utils
    async fn get_signers_and_quorum(
        &self,
        metadata: Metadata,
        stage: ValidationStage,
    ) -> Result<(HashSet<KeyIdentifier>, u32), LedgerError> {
        let signers = self
            .gov_api
            .get_signers(metadata.clone(), stage.clone())
            .await?;
        let quorum_size = self.gov_api.get_quorum(metadata, stage).await?;
        Ok((signers, quorum_size))
    }

    async fn check_genesis(
        &self,
        event: Event,
        subject_id: DigestIdentifier,
    ) -> Result<(), LedgerError> {
        let EventRequestType::Create(create_request) = &event.content.event_proposal.proposal.event_request.request else {
            return Err(LedgerError::StateInGenesis);
        };
        let invoker = event
            .content
            .event_proposal
            .proposal
            .event_request
            .signature
            .content
            .signer
            .clone();
        let metadata = Metadata {
            namespace: create_request.namespace.clone(),
            subject_id: subject_id.clone(),
            governance_id: create_request.governance_id.clone(),
            governance_version: event.content.event_proposal.proposal.gov_version,
            schema_id: create_request.schema_id.clone(),
            owner: invoker.clone(),
            creator: invoker.clone(),
        };
        // Ignoramos las firmas por ahora
        // Comprobar que el creador tiene permisos de creación
        let creation_roles = self
            .gov_api
            .get_signers(metadata.clone(), ValidationStage::Create)
            .await?;
        if !creation_roles.contains(&invoker) {
            return Err(LedgerError::Unauthorized("Crreator not allowed".into()));
        } // TODO: No estamos comprobando que pueda ser un external el que cree el subject y lo permitamos si tenia permisos.
          // Crear sujeto y añadirlo a base de datos
        let init_state = self
            .gov_api
            .get_init_state(
                metadata.governance_id,
                metadata.schema_id,
                metadata.governance_version,
            )
            .await?;
        let init_state = serde_json::to_string(&init_state)
            .map_err(|_| LedgerError::ErrorParsingJsonString("Init state".to_owned()))?;
        let subject = Subject::from_genesis_event(event.clone(), init_state)?;
        self.database.set_event(&subject_id, event)?;
        self.database.set_subject(&subject_id, subject)?;
        Ok(())
    }

    fn event_sourcing(&self, subject_id: DigestIdentifier, sn: u64) -> Result<(), LedgerError> {
        let prev_event =
            self.database
                .get_event(&subject_id, sn - 1)
                .map_err(|error| match error {
                    crate::database::Error::EntryNotFound => {
                        LedgerError::UnexpectEventMissingInEventSourcing
                    }
                    _ => LedgerError::DatabaseError(error),
                })?;
        let event = self
            .database
            .get_event(&subject_id, sn)
            .map_err(|error| match error {
                crate::database::Error::EntryNotFound => {
                    LedgerError::UnexpectEventMissingInEventSourcing
                }
                _ => LedgerError::DatabaseError(error),
            })?;
        // Comprobar evento previo encaja
        if event.content.event_proposal.proposal.hash_prev_event
            != prev_event.signature.content.event_content_hash
        {
            return Err(LedgerError::EventDoesNotFitHash);
        }
        let mut subject = self.database.get_subject(&subject_id)?;
        subject.update_subject(
            &event.content.event_proposal.proposal.json_patch,
            event.content.event_proposal.proposal.sn,
        )?;
        self.database.set_subject(&subject_id, subject)?;
        Ok(())
    }

    async fn check_event(&self, event: Event, metadata: Metadata) -> Result<(), LedgerError> {
        // Comprobar que las firmas de evaluación y/o aprobación hacen quorum
        let (signers, quorum) = self
            .get_signers_and_quorum(metadata.clone(), ValidationStage::Evaluate)
            .await?;
        let evaluation_hash = DigestIdentifier::from_serializable_borsh(
            &event
                .content
                .event_proposal
                .proposal
                .evaluation
                .clone()
                .unwrap(),
        )
        .map_err(|_| {
            LedgerError::CryptoError(String::from(
                "Error calculating the hash of the serializable evaluation",
            ))
        })?;
        verify_signatures(
            &event.content.event_proposal.proposal.evaluation_signatures,
            &signers,
            quorum,
            &evaluation_hash,
        )?;
        if event
            .content
            .event_proposal
            .proposal
            .evaluation
            .clone()
            .unwrap()
            .approval_required
        {
            let (signers, quorum) = self
                .get_signers_and_quorum(metadata, ValidationStage::Approve)
                .await?;
            verify_approval_signatures(&event.content.approvals, &signers, quorum)?;
        }
        Ok(())
    }
}

fn verify_approval_signatures(
    approvals: &HashSet<Approval>,
    signers: &HashSet<KeyIdentifier>,
    quorum_size: u32,
) -> Result<(), LedgerError> {
    let mut actual_signers = HashSet::new();
    for approval in approvals.iter() {
        let approval_hash =
            DigestIdentifier::from_serializable_borsh(&approval.content).map_err(|_| {
                LedgerError::CryptoError(String::from(
                    "Error calculating the hash of the serializable approval",
                ))
            })?;
        let signature = approval.signature.clone();
        let signer = signature.content.signer.clone();
        if &signature.content.event_content_hash != &approval_hash {
            log::error!("Invalid Event Hash in Approval");
            continue;
        }
        match signature.verify() {
            Ok(_) => (),
            Err(_) => {
                log::error!("Invalid Signature Detected");
                continue;
            }
        }
        if !signers.contains(&signer) {
            log::error!("Signer {} not allowed", signer.to_str());
            continue;
        }
        if !actual_signers.insert(signer.clone()) {
            log::error!(
                "Signer {} in more than one validation signature",
                signer.to_str()
            );
            continue;
        }
    }
    if actual_signers.len() < quorum_size as usize {
        log::error!(
            "Not enough signatures. Expected: {}, Actual: {}",
            quorum_size,
            actual_signers.len()
        );
        return Err(LedgerError::NotEnoughSignatures("Approval failed".into()));
    }
    Ok(())
}

fn verify_signatures(
    signatures: &HashSet<Signature>,
    signers: &HashSet<KeyIdentifier>,
    quorum_size: u32,
    event_hash: &DigestIdentifier,
) -> Result<(), LedgerError> {
    let mut actual_signers = HashSet::new();
    for signature in signatures.iter() {
        let signer = signature.content.signer.clone();
        if &signature.content.event_content_hash != event_hash {
            log::error!("Invalid Event Hash in Signature");
            continue;
        }
        match signature.verify() {
            Ok(_) => (),
            Err(_) => {
                log::error!("Invalid Signature Detected");
                continue;
            }
        }
        if !signers.contains(&signer) {
            log::error!("Signer {} not allowed", signer.to_str());
            continue;
        }
        if !actual_signers.insert(signer.clone()) {
            log::error!(
                "Signer {} in more than one validation signature",
                signer.to_str()
            );
            continue;
        }
    }
    if actual_signers.len() < quorum_size as usize {
        log::error!(
            "Not enough signatures. Expected: {}, Actual: {}",
            quorum_size,
            actual_signers.len()
        );
        return Err(LedgerError::NotEnoughSignatures(event_hash.to_str()));
    }
    Ok(())
}

fn create_subject_id(event: &Event) -> Result<DigestIdentifier, LedgerError> {
    match DigestIdentifier::from_serializable_borsh((
        &event
            .content
            .event_proposal
            .proposal
            .event_request
            .signature
            .content
            .event_content_hash,
        &event
            .content
            .event_proposal
            .proposal
            .event_request
            .signature
            .content
            .signer
            .public_key, // No estoy seguro que esto equivalga al vector de bytes pero creo que si
    )) {
        Ok(subject_id) => Ok(subject_id),
        Err(_) => Err(LedgerError::CryptoError(
            "Error creating subject_id in external event".to_owned(),
        )),
    }
}