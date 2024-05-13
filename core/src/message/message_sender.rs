use crate::identifier::KeyIdentifier;
use crate::commons::self_signature_manager::SelfSignatureManager;
use crate::signature::Signed;
use crate::DigestDerivator;

use log::debug;
use rmp_serde;
use std::sync::{Arc, RwLock};

use super::{MessageContent, TaskCommandContent};

use super::error::Error;

use network::{NetworkService, Command};

const LOG_TARGET: &str = "MESSAGE_SENDER";

/// Network MessageSender struct
#[derive(Clone)]
pub struct MessageSender {
    service: Arc<RwLock<NetworkService>>,
    controller_id: KeyIdentifier,
    signature_manager: SelfSignatureManager,
    derivator: DigestDerivator,
}

/// Network MessageSender implementation
impl MessageSender {
    /// New MessageSender
    pub fn new(
        service: Arc<RwLock<NetworkService>>,
        controller_id: KeyIdentifier,
        signature_manager: SelfSignatureManager,
        derivator: DigestDerivator
    ) -> Self {
        Self {
            service,
            controller_id,
            signature_manager,
            derivator,
        }
    }

    /// Start listening in Taple netword
    pub async fn send_message<T: TaskCommandContent>(
        &mut self,
        target: KeyIdentifier,
        message: T,
    ) -> Result<(), Error> {
        // TODO: Define type of invalid identifier error
        let complete_message = Signed::<MessageContent<T>>::new(
            self.controller_id.clone(),
            target.clone(),
            message,
            &self.signature_manager,
            self.derivator,
        )?;
        let bytes = rmp_serde::to_vec(&complete_message)
            .map_err(|error| Error::MsgPackSerialize { source: error })?;
        match self.service.write() {
            Ok(mut service) => {
                service.send_command(Command::SendMessage { 
                    peer: target.public_key, 
                    message: bytes, 
                })
                    .await
                    .map_err(|_| Error::ChannelClosed)?;
                Ok(())
            }
            Err(_) => {
                Err(Error::NetworkService)
            }
        }
    }

    #[allow(dead_code)]
    /// Set node as a provider of keys
    pub async fn start_providing(&mut self, keys: Vec<String>) -> Result<(), Error> {
        match self.service.write() {
            Ok(mut service) => {
                service.send_command(Command::StartProviding { keys })
                    .await
                    .map_err(|_| Error::ChannelClosed)?;
                Ok(())
            }
            Err(_) => {
                Err(Error::NetworkService)
            }
        }
    }

    #[allow(dead_code)]
    pub async fn bootstrap(&mut self) -> Result<(), Error> {
        debug!("{}: Starting Bootstrap", LOG_TARGET);
        match self.service.write() {
            Ok(mut service) => {
                service.send_command(Command::Bootstrap)
                    .await
                    .map_err(|_| Error::ChannelClosed)?;
                Ok(())
            }
            Err(_) => {
                Err(Error::NetworkService)
            }
        }
    }
}
