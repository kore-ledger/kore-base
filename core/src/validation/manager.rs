use tokio_util::sync::CancellationToken;

use super::{errors::ValidationError, Validation, ValidationCommand, ValidationResponse};
use crate::commons::channel::{ChannelData, MpscChannel, SenderEnd};
use crate::database::DatabaseCollection;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ValidationAPI {
    sender: SenderEnd<ValidationCommand, ValidationResponse>,
}

#[allow(dead_code)]
impl ValidationAPI {
    pub fn new(sender: SenderEnd<ValidationCommand, ValidationResponse>) -> Self {
        Self { sender }
    }
}

pub struct ValidationManager<C: DatabaseCollection> {
    /// Communication channel for incoming petitions
    input_channel: MpscChannel<ValidationCommand, ValidationResponse>,
    /// Validation functions
    inner_validation: Validation<C>,
    token: CancellationToken,
}

impl<C: DatabaseCollection> ValidationManager<C> {
    pub fn new(
        input_channel: MpscChannel<ValidationCommand, ValidationResponse>,
        inner_validation: Validation<C>,
        token: CancellationToken,
    ) -> Self {
        Self {
            input_channel,
            inner_validation,
            token,
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
        command: ChannelData<ValidationCommand, ValidationResponse>,
    ) -> Result<(), ValidationError> {
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
                ValidationCommand::ValidationEvent {
                    validation_event,
                    sender,
                } => {
                    let result = self
                        .inner_validation
                        .validation_event(validation_event, sender)
                        .await;
                    match result {
                        Err(ValidationError::ChannelError(_)) => return result.map(|_| ()),
                        _ => ValidationResponse::ValidationEventResponse(result),
                    }
                }
                ValidationCommand::AskForValidation(_) => {
                    log::error!("Ask for Validation in Validation Manager");
                    return Ok(());
                }
            }
        };
        if let Some(sender) = sender {
            sender.send(response).expect("Sender Dropped");
        }
        Ok(())
    }
}
