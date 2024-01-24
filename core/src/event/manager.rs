use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::{errors::EventError, event_completer::EventCompleter, EventCommand, EventResponse};

use crate::commons::channel::{ChannelData, MpscChannel, SenderEnd};
use crate::database::DatabaseCollection;
use crate::governance::error::RequestError;
use crate::governance::GovernanceUpdatedMessage;
use crate::signature::Signed;
use crate::EventRequest;

#[derive(Clone, Debug)]
pub struct EventAPI {
    sender: SenderEnd<EventCommand, EventResponse>,
}

impl EventAPI {
    pub fn new(sender: SenderEnd<EventCommand, EventResponse>) -> Self {
        Self { sender }
    }
}

#[async_trait]
pub trait EventAPIInterface {
    async fn send_event_request(&self, event_request: Signed<EventRequest>) -> EventResponse;
}

#[async_trait]
impl EventAPIInterface for EventAPI {
    async fn send_event_request(&self, event_request: Signed<EventRequest>) -> EventResponse {
        match self.sender.ask(EventCommand::Event { event_request }).await {
            Ok(response) => response,
            Err(_) => EventResponse::Event(Err(EventError::EventApiChannelNotAvailable)),
        }
    }
}

pub struct EventManager<C: DatabaseCollection> {
    /// Communication channel for incoming petitions
    input_channel: MpscChannel<EventCommand, EventResponse>,
    input_channel_updated_gov: tokio::sync::broadcast::Receiver<GovernanceUpdatedMessage>,
    event_completer: EventCompleter<C>,
    token: CancellationToken,
    // TODO: What we do with this?
    //_notification_tx: tokio::sync::mpsc::Sender<Notification>,
}

impl<C: DatabaseCollection> EventManager<C> {
    pub fn new(
        input_channel: MpscChannel<EventCommand, EventResponse>,
        input_channel_updated_gov: tokio::sync::broadcast::Receiver<GovernanceUpdatedMessage>,
        token: CancellationToken,
        event_completer: EventCompleter<C>,
    ) -> Self {
        Self {
            input_channel,
            input_channel_updated_gov,
            event_completer,
            token,
        }
    }

    pub async fn run(mut self) {
        match self.event_completer.init().await {
            Ok(_) => {}
            Err(error) => {
                log::error!("{}", error);
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
                                break;
                            }
                        }
                        None => {
                            break;
                        },
                    }
                },
                gov_updated = self.input_channel_updated_gov.recv() => {
                    match gov_updated {
                        Ok(gov_updated) => {
                            match gov_updated {
                                GovernanceUpdatedMessage::GovernanceUpdated { governance_id, governance_version } => {
                                    let result = self.event_completer.new_governance_version(governance_id, governance_version).await;
                                    if result.is_err() {
                                        break;
                                    }
                                },
                            }
                        },
                        Err(_) => {
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
        command: ChannelData<EventCommand, EventResponse>,
    ) -> Result<(), EventError> {
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
                EventCommand::Event { event_request } => {
                    let response = self.event_completer.pre_new_event(event_request).await;
                    if let Err(error) = response.clone() {
                        match error {
                            EventError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(EventError::ChannelClosed);
                            }
                            EventError::GovernanceError(RequestError::ChannelClosed) => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(EventError::ChannelClosed);
                            }
                            _ => {}
                        }
                    }
                    EventResponse::Event(response)
                }
                EventCommand::EvaluatorResponse { evaluator_response } => {
                    let response = self
                        .event_completer
                        .evaluator_signatures(evaluator_response)
                        .await;
                    if let Err(error) = response {
                        match error {
                            EventError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(EventError::ChannelClosed);
                            }
                            EventError::GovernanceError(RequestError::ChannelClosed) => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(EventError::ChannelClosed);
                            }
                            _ => log::error!("{:?}", error),
                        }
                    }
                    EventResponse::NoResponse
                }
                EventCommand::ApproverResponse { approval } => {
                    if let Err(error) = self.event_completer.approver_signatures(approval).await {
                        match error {
                            EventError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(EventError::ChannelClosed);
                            }
                            EventError::GovernanceError(RequestError::ChannelClosed) => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(EventError::ChannelClosed);
                            }
                            _ => {}
                        }
                    }
                    EventResponse::NoResponse
                }
                EventCommand::ValidatorResponse {
                    event_hash,
                    signature,
                    governance_version,
                } => {
                    if let Err(error) = self
                        .event_completer
                        .validation_signatures(event_hash, signature, governance_version)
                        .await
                    {
                        match error {
                            EventError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(EventError::ChannelClosed);
                            }
                            EventError::GovernanceError(RequestError::ChannelClosed) => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(EventError::ChannelClosed);
                            }
                            _ => log::error!("VALIDATION ERROR: {:?}", error),
                        }
                    }
                    EventResponse::NoResponse
                }
                EventCommand::HigherGovernanceExpected {
                    governance_id,
                    who_asked,
                } => {
                    match self
                        .event_completer
                        .higher_governance_expected(governance_id, who_asked)
                        .await
                    {
                        Ok(_) => {}
                        Err(error) => match error {
                            EventError::ChannelClosed => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(EventError::ChannelClosed);
                            }
                            EventError::GovernanceError(RequestError::ChannelClosed) => {
                                log::error!("Channel Closed");
                                self.token.cancel();
                                return Err(EventError::ChannelClosed);
                            }
                            _ => log::error!("VALIDATION ERROR: {:?}", error),
                        },
                    }
                    EventResponse::NoResponse
                }
            }
        };
        if let Some(sender) = sender {
            sender.send(response).expect("Sender Dropped");
        }
        Ok(())
    }
}
