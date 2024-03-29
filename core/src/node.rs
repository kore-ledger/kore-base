/// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

#[cfg(feature = "approval")]
use crate::approval::manager::{ApprovalAPI, ApprovalManager};
#[cfg(feature = "approval")]
use crate::approval::{
    inner_manager::{InnerApprovalManager, RequestNotifier},
    ApprovalMessages, ApprovalResponses,
};
use crate::authorized_subjecs::manager::{AuthorizedSubjectsAPI, AuthorizedSubjectsManager};
use crate::authorized_subjecs::{AuthorizedSubjectsCommand, AuthorizedSubjectsResponse};
use crate::commons::channel::MpscChannel;
use crate::commons::crypto::{KeyMaterial, KeyPair};
use crate::commons::identifier::{Derivable, KeyIdentifier};
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
    compiler::manager::KoreCompiler, runner::manager::KoreRunner, EvaluatorManager,
    EvaluatorMessage, EvaluatorResponse,
};
use crate::event::event_completer::EventCompleter;
use crate::event::manager::{EventAPI, EventManager};
use crate::event::{EventCommand, EventResponse};
use crate::governance::GovernanceAPI;
use crate::governance::{main_governance::Governance, GovernanceMessage, GovernanceResponse};
use crate::ledger::inner_ledger::Ledger;
use crate::ledger::manager::EventManagerAPI;
use crate::ledger::{manager::LedgerManager, LedgerCommand, LedgerResponse};
use crate::message::{
    MessageContent, MessageReceiver, MessageSender, MessageTaskCommand, MessageTaskManager,
    NetworkEvent,
};
use crate::network::network_processor::NetworkProcessor;
use crate::protocol::{KoreMessages, ProtocolChannels, ProtocolManager};
use crate::signature::Signed;
#[cfg(feature = "validation")]
use crate::validation::{manager::ValidationManager, Validation};
#[cfg(feature = "validation")]
use crate::validation::{ValidationCommand, ValidationResponse};
use ::futures::Future;
use libp2p::{Multiaddr, PeerId};
use log::{error, info};
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
    pub fn build(settings: Settings, key_pair: KeyPair, database: M) -> Result<(Self, Api), Error> {
        let (api_rx, api_tx) = MpscChannel::new(BUFFER_SIZE);

        let (notification_tx, notification_rx) = mpsc::channel(BUFFER_SIZE);

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

        let network_manager = NetworkProcessor::new(
            settings.network.listen_addr.clone(),
            network_access_points(&settings.network.known_nodes)?,
            network_tx,
            key_pair.clone(),
            token.clone(),
            notification_tx.clone(),
            external_addresses(&settings.network.external_address)?,
        )
        .expect("Network created");

        //TODO: change name. It's not a task
        let signature_manager = SelfSignatureManager::new(key_pair.clone(), &settings);

        //TODO: change name. It's a task
        let network_rx = MessageReceiver::new(
            network_rx,
            protocol_tx,
            token.clone(),
            notification_tx.clone(),
            signature_manager.get_own_identifier(),
        );

        let network_tx = MessageSender::new(
            network_manager.client(),
            controller_id.clone(),
            signature_manager.clone(),
            settings.node.digest_derivator,
        );

        let task_manager =
            MessageTaskManager::new(network_tx, task_rx, token.clone(), notification_tx.clone());

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
        let protocol_manager =
            ProtocolManager::new(channels, token.clone(), notification_tx.clone());

        // Build governance
        let mut governance_manager = Governance::<M, C>::new(
            governance_rx,
            token.clone(),
            notification_tx.clone(),
            DB::new(database.clone()),
            governance_update_sx.clone(),
        );

        // Build event completer
        let event_completer = EventCompleter::new(
            GovernanceAPI::new(governance_tx.clone()),
            DB::new(database.clone()),
            task_tx.clone(),
            notification_tx.clone(),
            ledger_tx.clone(),
            signature_manager.clone(),
            settings.node.digest_derivator,
        );

        // Build event manager
        let event_manager = EventManager::new(
            event_rx,
            governance_update_rx,
            token.clone(),
            event_completer,
        );

        // Build inner ledger
        let inner_ledger = Ledger::new(
            GovernanceAPI::new(governance_tx.clone()),
            DB::new(database.clone()),
            task_tx.clone(),
            distribution_tx,
            controller_id.clone(),
            notification_tx.clone(),
            settings.node.digest_derivator,
        );

        // Build ledger manager
        let ledger_manager = LedgerManager::new(ledger_rx, inner_ledger, token.clone());

        let as_manager = AuthorizedSubjectsManager::new(
            as_rx,
            DB::new(database.clone()),
            task_tx.clone(),
            controller_id.clone(),
            token.clone(),
            notification_tx.clone(),
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
                notification_tx.clone(),
            );

            // Build core runner
            let runner = KoreRunner::new(
                DB::new(database.clone()),
                engine,
                GovernanceAPI::new(governance_tx.clone()),
                settings.node.digest_derivator,
            );

            // Build evaluation manager
            EvaluatorManager::new(
                evaluation_rx,
                compiler,
                runner,
                signature_manager.clone(),
                token.clone(),
                task_tx.clone(),
                settings.node.digest_derivator,
            )
        };

        // Build approval
        #[cfg(feature = "approval")]
        let approval_manager = {
            let passvotation = settings.node.passvotation.into();
            let inner_approval = InnerApprovalManager::new(
                GovernanceAPI::new(governance_tx.clone()),
                DB::new(database.clone()),
                RequestNotifier::new(notification_tx.clone()),
                signature_manager.clone(),
                passvotation,
                settings.node.digest_derivator,
            );

            ApprovalManager::new(
                approval_rx,
                token.clone(),
                task_tx.clone(),
                governance_update_sx.subscribe(),
                inner_approval,
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
        );

        // Build distribution manager
        let distribution_manager = DistributionManager::new(
            distribution_rx,
            governance_update_sx.subscribe(),
            token.clone(),
            notification_tx.clone(),
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
            );

            ValidationManager::new(
                validation_rx,
                inner_validation,
                token.clone(),
                notification_tx,
            )
        };

        let taple = Node {
            notification_rx,
            token,
            _m: PhantomData,
            _c: PhantomData,
        };

        let api = Api::new(
            network_manager.local_peer_id().to_owned(),
            controller_id.to_str(),
            key_pair.public_key_bytes(),
            api_tx,
        );

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
            network_manager.run().await;
        });

        tokio::spawn(async move {
            api_manager.run().await;
        });

        Ok((taple, api))
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

// TODO: move to better place, maybe settings
fn network_access_points(points: &[String]) -> Result<Vec<(PeerId, Multiaddr)>, Error> {
    let mut access_points: Vec<(PeerId, Multiaddr)> = Vec::new();
    for point in points {
        let data: Vec<&str> = point.split("/p2p/").collect();
        if data.len() != 2 {
            return Err(Error::AcessPointError(point.to_string()));
        }
        if let Some(value) = multiaddr(point) {
            if let Ok(id) = data[1].parse::<PeerId>() {
                access_points.push((id, value));
            } else {
                return Err(Error::AcessPointError(format!(
                    "Invalid PeerId conversion: {}",
                    point
                )));
            }
        } else {
            return Err(Error::AcessPointError(format!(
                "Invalid MultiAddress conversion: {}",
                point
            )));
        }
    }
    Ok(access_points)
}

// TODO: move to better place, maybe settings
fn external_addresses(addresses: &[String]) -> Result<Vec<Multiaddr>, Error> {
    let mut external_addresses: Vec<Multiaddr> = Vec::new();
    for address in addresses {
        if let Some(value) = multiaddr(address) {
            external_addresses.push(value);
        } else {
            return Err(Error::AcessPointError(format!(
                "Invalid MultiAddress conversion in External Address: {}",
                address
            )));
        }
    }
    Ok(external_addresses)
}

// TODO: move to better place, maybe settings
fn multiaddr(addr: &str) -> Option<Multiaddr> {
    match addr.parse::<Multiaddr>() {
        Ok(a) => Some(a),
        Err(_) => None,
    }
}
