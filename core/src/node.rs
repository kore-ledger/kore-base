// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::approval::manager::ApprovalManagerChannels;
#[cfg(feature = "approval")]
use crate::approval::manager::{ApprovalAPI, ApprovalManager};
#[cfg(feature = "approval")]
use crate::approval::{inner_manager::InnerApprovalManager, ApprovalMessages, ApprovalResponses};
use crate::authorized_subjecs::manager::{AuthorizedSubjectChannels, AuthorizedSubjectsAPI, AuthorizedSubjectsManager};
use crate::authorized_subjecs::{AuthorizedSubjectsCommand, AuthorizedSubjectsResponse};
use crate::commons::channel::MpscChannel;
use crate::commons::models::notification::Notification;
use crate::commons::self_signature_manager::{SelfSignature, SelfSignatureManager};
use crate::commons::settings::Settings;
use crate::database::{DatabaseCollection, DatabaseManager, DB};
use crate::distribution::error::DistributionManagerError;
use crate::distribution::inner_manager::InnerDistributionManager;
use crate::distribution::manager::DistributionManager;
use crate::distribution::DistributionMessagesNew;
#[cfg(feature = "evaluation")]
use crate::evaluator::{
    compiler::manager::KoreCompiler, runner::manager::KoreRunner, EvaluatorManager, EvaluatorManagerChannels,
    EvaluatorMessage, EvaluatorResponse,
};
use crate::event::event_completer::{EventCompleter, EventCompleterChannels};
use crate::event::manager::{EventAPI, EventManager};
use crate::event::{EventCommand, EventResponse};
use crate::governance::GovernanceAPI;
use crate::governance::{main_governance::Governance, GovernanceMessage, GovernanceResponse};
use crate::identifier::{Derivable, KeyIdentifier};
use crate::keys::{KeyMaterial, KeyPair};
use crate::ledger::inner_ledger::{Ledger, LedgerChannels};
use crate::ledger::manager::EventManagerAPI;
use crate::ledger::{manager::LedgerManager, LedgerCommand, LedgerResponse};
use crate::message::{
    MessageContent, MessageReceiver, MessageSender, MessageTaskCommand, MessageTaskManager,
};
//use crate::network::processor::NetworkProcessor;
use crate::protocol::{KoreMessages, ProtocolChannels, ProtocolManager};
use crate::signature::Signed;
#[cfg(feature = "validation")]
use crate::validation::{manager::ValidationManager, Validation};
#[cfg(feature = "validation")]
use crate::validation::{ValidationCommand, ValidationResponse};

use network::{Event as NetworkEvent, NetworkWorker};

use ::futures::Future;
use log::{error, info};
use prometheus_client::registry::Registry;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::*;
use tokio_util::sync::CancellationToken;

use crate::api::{inner_api::InnerApi, Api, ApiManager};
use crate::error::Error;

const BUFFER_SIZE: usize = 1000;

/// Structure representing a TAPLE node
///
/// A node must be instantiated using the [`Taple::build`] method, which requires a set
/// of [configuration](Settings) parameters in order to be properly initialized.
///
#[derive(Debug)]
pub struct Node<M: DatabaseManager<C>, C: DatabaseCollection> {
    notification_rx: mpsc::Receiver<Notification>,
    token: CancellationToken,
    _m: PhantomData<M>,
    _c: PhantomData<C>,
}

impl<M: DatabaseManager<C> + 'static, C: DatabaseCollection + 'static> Node<M, C> {
    /// This method creates and initializes a Kore base node.
    /// # Possible results
    /// If the process is successful, the method will return `Ok(())`.
    /// An error will be returned only if it has not been possible to generate the necessary data
    /// for the initialization of the components, mainly due to problems in the initial [configuration](Settings).
    /// # Panics
    /// This method panics if it has not been possible to generate the network layer.
    pub fn build(
        settings: Settings,
        key_pair: KeyPair,
        registry: &mut Registry,
        database: M,
    ) -> Result<Api, Error> {
        let (api_rx, api_tx) = MpscChannel::new(BUFFER_SIZE);

        //let (notification_tx, notification_rx) = mpsc::channel(BUFFER_SIZE);

        let (network_tx, network_rx): (mpsc::Sender<NetworkEvent>, mpsc::Receiver<NetworkEvent>) =
            mpsc::channel(BUFFER_SIZE);

        let (event_rx, event_tx) = MpscChannel::<EventCommand, EventResponse>::new(BUFFER_SIZE);

        let (ledger_rx, ledger_tx) = MpscChannel::<LedgerCommand, LedgerResponse>::new(BUFFER_SIZE);

        let (as_rx, as_tx) =
            MpscChannel::<AuthorizedSubjectsCommand, AuthorizedSubjectsResponse>::new(BUFFER_SIZE);

        let (governance_rx, governance_tx) =
            MpscChannel::<GovernanceMessage, GovernanceResponse>::new(BUFFER_SIZE);

        // TODO: broadcast channel. Is a lag corretly managed?
        let (governance_update_sx, governance_update_rx) = broadcast::channel(BUFFER_SIZE);

        let (task_rx, task_tx) =
            MpscChannel::<MessageTaskCommand<KoreMessages>, ()>::new(BUFFER_SIZE);

        let (protocol_rx, protocol_tx) =
            MpscChannel::<Signed<MessageContent<KoreMessages>>, ()>::new(BUFFER_SIZE);

        let (distribution_rx, distribution_tx) = MpscChannel::<
            DistributionMessagesNew,
            Result<(), DistributionManagerError>,
        >::new(BUFFER_SIZE);

        #[cfg(feature = "approval")]
        let (approval_rx, approval_tx) =
            MpscChannel::<ApprovalMessages, ApprovalResponses>::new(BUFFER_SIZE);

        #[cfg(feature = "evaluation")]
        let (evaluation_rx, evaluation_tx) =
            MpscChannel::<EvaluatorMessage, EvaluatorResponse>::new(BUFFER_SIZE);

        #[cfg(feature = "validation")]
        let (validation_rx, validation_tx) =
            MpscChannel::<ValidationCommand, ValidationResponse>::new(BUFFER_SIZE);

        let database = Arc::new(database);

        let controller_id = Self::register_node_key(key_pair.clone(), DB::new(database.clone()))?;
        info!("Controller ID: {}", &controller_id);

        let token = CancellationToken::new();

        let mut worker = match NetworkWorker::new(
            registry,
            key_pair.clone(),
            settings.network.clone(),
            network_tx.clone(),
            token.clone(),
        ) {
            Ok(worker) => worker,
            Err(e) => {
                error!("Error creating network worker: {}", e);
                return Err(Error::NetworkError(e.to_string()));
            }
        };

        //TODO: change name. It's not a task
        let signature_manager = SelfSignatureManager::new(key_pair.clone(), &settings);

        //TODO: change name. It's a task
        let network_rx = MessageReceiver::new(
            network_rx,
            protocol_tx.clone(),
            token.clone(),
            signature_manager.get_own_identifier(),
        );

        // TODO: refactor network service access.
        let service = worker.service().clone();
        let service = match service.read() {
            Ok(service) => service,
            Err(e) => {
                error!("Error accessing network service: {}", e);
                return Err(Error::NetworkError(e.to_string()));
            }
        };
        let network_tx = MessageSender::new(
            service.sender(),
            controller_id.clone(),
            signature_manager.clone(),
            settings.node.digest_derivator,
        );

        let task_manager = MessageTaskManager::new(network_tx, task_rx, token.clone());

        // Defines protocol channels
        let channels = ProtocolChannels {
            input_channel: protocol_rx,
            distribution_channel: distribution_tx.clone(),
            #[cfg(feature = "evaluation")]
            evaluation_channel: evaluation_tx,
            #[cfg(feature = "validation")]
            validation_channel: validation_tx,
            event_channel: event_tx.clone(),
            #[cfg(feature = "approval")]
            approval_channel: approval_tx.clone(),
            ledger_channel: ledger_tx.clone(),
        };

        // Build protocol manager
        let protocol_manager = ProtocolManager::new(channels, token.clone());

        // Build governance
        let mut governance_manager = Governance::<M, C>::new(
            governance_rx,
            token.clone(),
            DB::new(database.clone()),
            governance_update_sx.clone(),
        );

        let event_completer_channels =
            EventCompleterChannels::new(task_tx.clone(), ledger_tx.clone(), protocol_tx.clone());
        // Build event completer
        let event_completer = EventCompleter::new(
            GovernanceAPI::new(governance_tx.clone()),
            DB::new(database.clone()),
            event_completer_channels,
            signature_manager.clone(),
            settings.node.digest_derivator
        );

        // Build event manager
        let event_manager = EventManager::new(
            event_rx,
            governance_update_rx,
            token.clone(),
            event_completer,
        );

        let inner_ledger_channels = LedgerChannels::new(
            task_tx.clone(),
            distribution_tx,
            protocol_tx.clone()
        );

        // Build inner ledger
        let inner_ledger = Ledger::new(
            GovernanceAPI::new(governance_tx.clone()),
            DB::new(database.clone()),
            signature_manager.clone(),
            settings.node.digest_derivator,
            inner_ledger_channels
        );

        // Build ledger manager
        let ledger_manager = LedgerManager::new(ledger_rx, inner_ledger, token.clone());

        let authorized_subjects_channels = AuthorizedSubjectChannels::new(as_rx, task_tx.clone(), protocol_tx.clone());
        // Build authorized subjects
        let as_manager = AuthorizedSubjectsManager::new(
            DB::new(database.clone()),
            token.clone(),
            signature_manager.clone(),
            settings.node.digest_derivator,
            authorized_subjects_channels,
        );

        // Build api
        let api_manager = {
            let inner_api = InnerApi::new(
                EventAPI::new(event_tx),
                AuthorizedSubjectsAPI::new(as_tx),
                DB::new(database.clone()),
                #[cfg(feature = "approval")]
                ApprovalAPI::new(approval_tx),
                EventManagerAPI::new(ledger_tx),
            );

            ApiManager::new(api_rx, token.clone(), inner_api)
        };

        // Build evaluation
        #[cfg(feature = "evaluation")]
        let evaluator_manager: EvaluatorManager<M, C, GovernanceAPI> = {
            use wasmtime::Engine;
            let engine = Engine::default();

            // Build core compiler
            let compiler = KoreCompiler::new(
                governance_update_sx.subscribe(),
                DB::new(database.clone()),
                GovernanceAPI::new(governance_tx.clone()),
                settings.node.smartcontracts_directory.clone(),
                engine.clone(),
                token.clone(),
            );

            // Build core runner
            let runner = KoreRunner::new(
                DB::new(database.clone()),
                engine,
                GovernanceAPI::new(governance_tx.clone()),
                settings.node.digest_derivator,
            );

            let evaluator_manager_channels = EvaluatorManagerChannels::new(
                evaluation_rx,
                task_tx.clone(),
                protocol_tx.clone()
            );

            // Build evaluation manager
            EvaluatorManager::new(
                
                compiler,
                runner,
                signature_manager.clone(),
                token.clone(),
            
                settings.node.digest_derivator,
                evaluator_manager_channels
            )
        };

        // Build approval
        #[cfg(feature = "approval")]
        let approval_manager = {
            let passvotation = settings.node.passvotation.into();
            let inner_approval = InnerApprovalManager::new(
                GovernanceAPI::new(governance_tx.clone()),
                DB::new(database.clone()),
                signature_manager.clone(),
                passvotation,
                settings.node.digest_derivator,
            );

            let approval_channels = ApprovalManagerChannels::new(approval_rx, task_tx.clone(), protocol_tx.clone());

            ApprovalManager::new(
                token.clone(),
                governance_update_sx.subscribe(),
                inner_approval,
                signature_manager.clone(),
                settings.node.digest_derivator,
                approval_channels
            )
        };
        // Build inner distribution
        let inner_distribution = InnerDistributionManager::new(
            GovernanceAPI::new(governance_tx.clone()),
            DB::new(database.clone()),
            task_tx.clone(),
            signature_manager.clone(),
            settings.clone(),
            settings.node.digest_derivator,
            protocol_tx.clone()
        );

        // Build distribution manager
        let distribution_manager = DistributionManager::new(
            distribution_rx,
            governance_update_sx.subscribe(),
            token.clone(),
            inner_distribution,
        );

        // Build validation
        #[cfg(feature = "validation")]
        let validation_manager = {
            let inner_validation = Validation::new(
                GovernanceAPI::new(governance_tx.clone()),
                DB::new(database.clone()),
                signature_manager.clone(),
                task_tx.clone(),
                settings.node.digest_derivator,
                protocol_tx.clone()
            );

            ValidationManager::new(validation_rx, inner_validation, token.clone())
        };

        let api = Api::new(
            worker.local_peer_id().to_owned(),
            controller_id.to_str(),
            key_pair.public_key_bytes(),
            api_tx,
        );

        // Start network worker
        tokio::spawn(async move {
            worker.run().await;
        });

        tokio::spawn(async move {
            governance_manager.run().await;
        });

        tokio::spawn(async move {
            ledger_manager.run().await;
        });

        tokio::spawn(async move {
            event_manager.run().await;
        });

        tokio::spawn(async move {
            task_manager.run().await;
        });

        tokio::spawn(async move {
            protocol_manager.run().await;
        });

        tokio::spawn(async move {
            network_rx.run().await;
        });

        #[cfg(feature = "evaluation")]
        tokio::spawn(async move {
            evaluator_manager.run().await;
        });

        #[cfg(feature = "validation")]
        tokio::spawn(async move {
            validation_manager.run().await;
        });

        tokio::spawn(async move {
            distribution_manager.run().await;
        });

        #[cfg(feature = "approval")]
        tokio::spawn(async move {
            approval_manager.run().await;
        });

        tokio::spawn(async move {
            as_manager.run().await;
        });

        tokio::spawn(async move {
            api_manager.run().await;
        });

        Ok(api)
    }

    /// Receive a single notification
    ///
    /// All notifications must be consumed. If the notification buffer is full the node
    /// will be blocked until there is space in the buffer. Notifications can be consumed
    /// in different ways.
    ///
    /// `recv_notification` allows to consume the notifications one by one and keep control
    /// of the execution flow.  
    pub async fn recv_notification(&mut self) -> Option<Notification> {
        self.notification_rx.recv().await
    }

    /// Handle all notifications
    ///
    /// All notifications must be consumed. If the notification buffer is full the node
    /// will be blocked until there is space in the buffer. Notifications can be consumed
    /// in different ways.
    ///
    /// `handle_notifications` processes all notifications from the node. For this purpose,
    /// the function in charge of processing the notifications is passed as input.  This
    /// function blocks the task where it is invoked until the shutdown signal is produced.
    pub async fn handle_notifications<H>(mut self, handler: H)
    where
        H: Fn(Notification),
    {
        while let Some(notification) = self.recv_notification().await {
            handler(notification);
        }
    }

    /// Drop all notifications
    ///
    /// All notifications must be consumed. If the notification buffer is full the node
    /// will be blocked until there is space in the buffer. Notifications can be consumed
    /// in different ways.
    ///
    /// `drop_notifications` discards all notifications from the node.
    pub async fn drop_notifications(self) {
        self.handle_notifications(|_| {}).await;
    }

    /// Bind the node with a shutdown signal.
    ///
    /// When the signal completes, the server will start the graceful shutdown
    /// process. The node can be bind to multiple signals.
    pub fn bind_with_shutdown(&self, signal: impl Future<Output = ()> + Send + 'static) {
        let token = self.token.clone();
        tokio::spawn(async move {
            signal.await;
            token.cancel();
        });
    }

    /// Shutdown gracefully the node
    ///
    /// This function triggers the shutdown signal and waits until the node is safely terminated.
    /// This function can only be used if Y or Z has not been used to process the notifications.
    pub async fn shutdown_gracefully(self) {
        self.token.cancel();
        self.drop_notifications().await;
    }

    /// Register the node key. If the key is already registered, it will be checked that it is the same.
    /// If the key is not registered, it will be registered.
    ///
    /// # Arguments
    ///
    /// * `key_pair` - Key pair to register.
    /// * `db` - Database to use.
    ///
    /// # Errors
    ///
    /// If the key is not registered and it is not possible to register it, an error will be returned.
    ///
    /// # Returns
    ///
    /// Nothing.
    ///
    fn register_node_key(key_pair: KeyPair, db: DB<C>) -> Result<KeyIdentifier, Error> {
        let key_identifier =
            KeyIdentifier::new(key_pair.get_key_derivator(), &key_pair.public_key_bytes());
        let identifier = key_identifier.to_str();
        let stored_identifier = db.get_controller_id().ok();
        if let Some(stored_identifier) = stored_identifier {
            if identifier != stored_identifier {
                error!("Invalid key. There is a differente key stored");
                return Err(Error::InvalidKeyPairSpecified(stored_identifier));
            }
        } else {
            db.set_controller_id(identifier)
                .map_err(|e| Error::DatabaseError(e.to_string()))?;
        }
        Ok(key_identifier)
    }
}
