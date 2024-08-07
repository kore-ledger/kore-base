use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::{
    commons::channel::{ChannelData, MpscChannel, SenderEnd},
    governance::error::RequestError,
    DatabaseCollection, KeyDerivator, KeyIdentifier,
};

use super::{errors::LedgerError, inner_ledger::Ledger, LedgerCommand, LedgerResponse};

#[async_trait]
pub trait EventManagerInterface {
    async fn generate_keys(&self, derivator: KeyDerivator) -> Result<KeyIdentifier, LedgerError>;
}

#[derive(Debug, Clone)]
pub struct EventManagerAPI {
    sender: SenderEnd<LedgerCommand, LedgerResponse>,
}

impl EventManagerAPI {
    pub fn new(sender: SenderEnd<LedgerCommand, LedgerResponse>) -> Self {
        Self { sender }
    }
}

#[async_trait]
impl EventManagerInterface for EventManagerAPI {
    async fn generate_keys(&self, derivator: KeyDerivator) -> Result<KeyIdentifier, LedgerError> {
        let response = self
            .sender
            .ask(LedgerCommand::GenerateKey(derivator))
            .await
            .map_err(|_| LedgerError::ChannelClosed)?;
        if let LedgerResponse::GenerateKey(public_key) = response {
            public_key
        } else {
            Err(LedgerError::UnexpectedResponse)
        }
    }
}

pub struct LedgerManager<C: DatabaseCollection> {
    /// Communication channel for incoming petitions
    input_channel: MpscChannel<LedgerCommand, LedgerResponse>,
    inner_ledger: Ledger<C>,
    token: CancellationToken,
}

impl<C: DatabaseCollection> LedgerManager<C> {
    pub fn new(
        input_channel: MpscChannel<LedgerCommand, LedgerResponse>,
        inner_ledger: Ledger<C>,
        token: CancellationToken,
    ) -> Self {
        Self {
            input_channel,
            inner_ledger,
            token,
        }
    }

    pub async fn run(mut self) {
        match self.inner_ledger.init().await {
            Ok(_) => {}
            Err(error) => {
                log::error!("Ledger Manager Init fails: {:?}", error);
                self.token.cancel();
                return;
            }
        };
        loop {
            tokio::select! {
                command = self.input_channel.receive() => {
                    match command {
                        Some(command) => {
                            let result = self.process_command(command).await;
                            if result.is_err() {
                                log::error!("{}", result.unwrap_err());
                                // TODO: Manage error. For now, we cancel the token.
                                self.token.cancel();
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
        command: ChannelData<LedgerCommand, LedgerResponse>,
    ) -> Result<(), LedgerError> {
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
        let response = {
            match data {
                LedgerCommand::GenerateKey(derivator) => {
                    let response = self.inner_ledger.generate_key(derivator).await;
                    if let Err(error) = &response {
                        match error {
                            LedgerError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            LedgerError::GovernanceError(inner_error)
                                if *inner_error == RequestError::ChannelClosed =>
                            {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            _ => {}
                        }
                    }
                    LedgerResponse::GenerateKey(response)
                }
                LedgerCommand::OwnEvent {
                    event,
                    signatures,
                    validation_proof,
                } => {
                    let response = self
                        .inner_ledger
                        .event_validated(event, signatures, validation_proof)
                        .await;
                    if let Err(error) = response {
                        match error {
                            LedgerError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            LedgerError::GovernanceError(RequestError::ChannelClosed) => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            _ => {
                                log::error!("ERROR IN LEDGER {}", error);
                            }
                        }
                    }
                    LedgerResponse::NoResponse
                }
                LedgerCommand::Genesis {
                    event,
                    signatures,
                    validation_proof,
                } => {
                    let response = self
                        .inner_ledger
                        .genesis(event, signatures, validation_proof)
                        .await;
                    if let Err(error) = response {
                        match error {
                            LedgerError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            LedgerError::GovernanceError(RequestError::ChannelClosed) => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            _ => {}
                        }
                    }
                    LedgerResponse::NoResponse
                }
                LedgerCommand::ExternalEvent {
                    sender,
                    event,
                    signatures,
                    validation_proof,
                } => {
                    let response = self
                        .inner_ledger
                        .external_event(event, signatures, sender, validation_proof)
                        .await;
                    if let Err(error) = response {
                        match error {
                            LedgerError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            LedgerError::GovernanceError(RequestError::ChannelClosed) => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            _ => {}
                        }
                    }
                    LedgerResponse::NoResponse
                }
                LedgerCommand::ExternalIntermediateEvent { event } => {
                    let response = self.inner_ledger.external_intermediate_event(event).await;
                    if let Err(error) = response {
                        match error {
                            LedgerError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            LedgerError::GovernanceError(RequestError::ChannelClosed) => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            _ => {}
                        }
                    }
                    LedgerResponse::NoResponse
                }
                LedgerCommand::GetEvent {
                    who_asked,
                    subject_id,
                    sn,
                } => {
                    let response = self.inner_ledger.get_event(who_asked, subject_id, sn).await;
                    let response = match response {
                        Err(error) => match error.clone() {
                            LedgerError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            LedgerError::DatabaseError(crate::DbError::EntryNotFound) => {
                                return Ok(());
                            }
                            _ => Err(error),
                        },
                        Ok(event) => Ok(event),
                    };
                    LedgerResponse::GetEvent(response)
                }
                LedgerCommand::GetLCE {
                    who_asked,
                    subject_id,
                } => {
                    let response = self.inner_ledger.get_lce(who_asked, subject_id).await;
                    let response = match response {
                        Err(error) => match error.clone() {
                            LedgerError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            LedgerError::DatabaseError(crate::DbError::EntryNotFound) => {
                                return Ok(());
                            }
                            _ => Err(error),
                        },
                        Ok(event) => Ok(event),
                    };
                    LedgerResponse::GetLCE(response)
                }
                LedgerCommand::GetNextGov {
                    who_asked,
                    subject_id,
                    sn,
                } => {
                    let response = self
                        .inner_ledger
                        .get_next_gov(who_asked, subject_id, sn)
                        .await;
                    let response = match response {
                        Err(error) => match error.clone() {
                            LedgerError::ChannelClosed => {
                                self.token.cancel();
                                return Err(LedgerError::ChannelClosed);
                            }
                            LedgerError::DatabaseError(crate::DbError::EntryNotFound) => {
                                return Ok(());
                            }
                            _ => Err(error),
                        },
                        Ok(event) => Ok(event),
                    };
                    LedgerResponse::GetNextGov(response)
                }
            }
        };
        if let Some(sender) = sender {
            sender.send(response).expect("Sender Dropped");
        }
        Ok(())
    }
}
