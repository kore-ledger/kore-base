/// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later
mod error;

pub use error::ProtocolErrors;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[cfg(feature = "approval")]
use crate::approval::ApprovalResponses;
use crate::evaluator::EvaluatorMessage;
#[cfg(feature = "evaluation")]
use crate::evaluator::EvaluatorResponse;
use crate::validation::ValidationCommand;
#[cfg(feature = "validation")]
use crate::validation::ValidationResponse;
use crate::approval::ApprovalMessages;
use crate::{
    commons::channel::{ChannelData, MpscChannel, SenderEnd},
    distribution::{error::DistributionManagerError, DistributionMessagesNew},
    event::{EventCommand, EventResponse},
    ledger::{LedgerCommand, LedgerResponse},
    message::{MessageContent, TaskCommandContent},
    signature::Signed,
};

/// KoreMessages is an enum that contains all the messages that can be sent to the protocol manager.
/// It is used to send messages to the protocol manager through the input channel.
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum KoreMessages {
    DistributionMessage(Box<DistributionMessagesNew>),
    EvaluationMessage(EvaluatorMessage),
    ValidationMessage(Box<ValidationCommand>),
    EventMessage(EventCommand),
    ApprovalMessages(ApprovalMessages),
    LedgerMessages(LedgerCommand),
}

impl TaskCommandContent for KoreMessages {}

/// Protocol channels
pub struct ProtocolChannels {
    pub input_channel: MpscChannel<Signed<MessageContent<KoreMessages>>, ()>,
    pub distribution_channel:
        SenderEnd<DistributionMessagesNew, Result<(), DistributionManagerError>>,
    #[cfg(feature = "evaluation")]
    pub evaluation_channel: SenderEnd<EvaluatorMessage, EvaluatorResponse>,
    #[cfg(feature = "validation")]
    pub validation_channel: SenderEnd<ValidationCommand, ValidationResponse>,
    pub event_channel: SenderEnd<EventCommand, EventResponse>,
    #[cfg(feature = "approval")]
    pub approval_channel: SenderEnd<ApprovalMessages, ApprovalResponses>,
    pub ledger_channel: SenderEnd<LedgerCommand, LedgerResponse>,
}

/// Protocol manager that receives messages from the network and sends them to the corresponding
/// manager.
pub struct ProtocolManager {
    input: MpscChannel<Signed<MessageContent<KoreMessages>>, ()>,
    distribution_sx: SenderEnd<DistributionMessagesNew, Result<(), DistributionManagerError>>,
    #[cfg(feature = "evaluation")]
    evaluation_sx: SenderEnd<EvaluatorMessage, EvaluatorResponse>,
    #[cfg(feature = "validation")]
    validation_sx: SenderEnd<ValidationCommand, ValidationResponse>,
    event_sx: SenderEnd<EventCommand, EventResponse>,
    #[cfg(feature = "approval")]
    approval_sx: SenderEnd<ApprovalMessages, ApprovalResponses>,
    ledger_sx: SenderEnd<LedgerCommand, LedgerResponse>,
    token: CancellationToken,

}

impl ProtocolManager {
    pub fn new(
        channels: ProtocolChannels,
        token: CancellationToken,
    ) -> Self {
        Self {
            input: channels.input_channel,
            distribution_sx: channels.distribution_channel,
            #[cfg(feature = "evaluation")]
            evaluation_sx: channels.evaluation_channel,
            #[cfg(feature = "validation")]
            validation_sx: channels.validation_channel,
            event_sx: channels.event_channel,
            #[cfg(feature = "approval")]
            approval_sx: channels.approval_channel,
            ledger_sx: channels.ledger_channel,
            token,
        }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                command = self.input.receive() => {
                    match command {
                        Some(command) => {
                            let result = self.process_command(command).await;
                            if result.is_err() {
                                log::error!("Protocol Manager: {}", result.unwrap_err());
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

    #[allow(unused_variables)]
    async fn process_command(
        &self,
        command: ChannelData<Signed<MessageContent<KoreMessages>>, ()>,
    ) -> Result<(), ProtocolErrors> {
        let message = match command {
            ChannelData::AskData(_data) => return Err(ProtocolErrors::AskCommandDetected),
            ChannelData::TellData(data) => data.get(),
        };
        let msg = message.content.content;
        let sender = message.content.sender_id;
        match msg {
            KoreMessages::DistributionMessage(data) => {
                self.distribution_sx
                    .tell(*data)
                    .await
                    .map_err(|_| ProtocolErrors::ChannelClosed)?;
            }
            KoreMessages::EventMessage(data) => self
                .event_sx
                .tell(data)
                .await
                .map_err(|_| ProtocolErrors::ChannelClosed)?,
            KoreMessages::EvaluationMessage(data) => {
                #[cfg(feature = "evaluation")]
                {
                    let evaluation_command = match data {
                        EvaluatorMessage::EvaluationEvent { .. } => {
                            log::error!("Evaluation Event Received in protocol manager");
                            return Ok(());
                        }
                        EvaluatorMessage::AskForEvaluation(evaluation_request) => {
                            EvaluatorMessage::EvaluationEvent {
                                evaluation_request,
                                sender,
                            }
                        }
                    };
                    return self
                        .evaluation_sx
                        .tell(evaluation_command)
                        .await
                        .map_err(|_| ProtocolErrors::ChannelClosed);
                }
                #[cfg(not(feature = "evaluation"))]
                log::trace!("Evaluation Message received. Current node is not able to evaluate");
            }
            KoreMessages::ValidationMessage(data) => {
                #[cfg(feature = "validation")]
                {
                    let validation_command = match *data {
                        ValidationCommand::ValidationEvent { .. } => {
                            log::error!("Validation Event Received in protocol manager");
                            return Ok(());
                        }
                        ValidationCommand::AskForValidation(validation_event) => {
                            ValidationCommand::ValidationEvent {
                                validation_event,
                                sender,
                            }
                        }
                    };
                    return self
                        .validation_sx
                        .tell(validation_command)
                        .await
                        .map_err(|_| ProtocolErrors::ChannelClosed);
                }
                #[cfg(not(feature = "validation"))]
                log::trace!("Validation Message received. Current node is not able to validate");
            }
            KoreMessages::ApprovalMessages(data) => {
                #[cfg(feature = "approval")]
                {
                    let approval_command = match data {
                        ApprovalMessages::RequestApproval(approval) => {
                            ApprovalMessages::RequestApprovalWithSender { approval, sender }
                        }
                        ApprovalMessages::RequestApprovalWithSender { .. } => {
                            log::error!(
                                "Request Approval with Sender Received in protocol manager"
                            );
                            return Ok(());
                        }
                        _ => data,
                    };
                    return self
                        .approval_sx
                        .tell(approval_command)
                        .await
                        .map_err(|_| ProtocolErrors::ChannelClosed);
                }
                #[cfg(not(feature = "approval"))]
                log::trace!("Approval Message received. Current node is not able to aprove");
            }
            KoreMessages::LedgerMessages(data) => self
                .ledger_sx
                .tell(data)
                .await
                .map_err(|_| ProtocolErrors::ChannelClosed)?,
        }
        Ok(())
    }
}
