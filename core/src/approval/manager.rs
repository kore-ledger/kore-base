use async_trait::async_trait;
use identity::identifier::{derive::digest::DigestDerivator, Derivable, KeyIdentifier};
use tokio_util::sync::CancellationToken;

use crate::{
    commons::{
        channel::{ChannelData, MpscChannel, SenderEnd},
        models::approval::ApprovalEntity,
        self_signature_manager::{SelfSignature, SelfSignatureManager},
    },
    governance::{GovernanceAPI, GovernanceUpdatedMessage},
    identifier::DigestIdentifier,
    message::{MessageConfig, MessageContent, MessageTaskCommand},
    protocol::KoreMessages,
    signature::Signed,
    utils::message::event::create_approver_response,
    ApprovalRequest, DatabaseCollection,
};

use super::{
    error::{ApprovalErrorResponse, ApprovalManagerError},
    inner_manager::InnerApprovalManager,
    ApprovalMessages, ApprovalResponses, EmitVote,
};

pub struct ApprovalManagerChannels {
    input_channel: MpscChannel<ApprovalMessages, ApprovalResponses>,
    messenger_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
    input_channel_protocol: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
}

impl ApprovalManagerChannels {
    pub fn new(
        input_channel: MpscChannel<ApprovalMessages, ApprovalResponses>,
        messenger_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
        input_channel_protocol: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
    ) -> Self {
        Self {
            input_channel,
            messenger_channel,
            input_channel_protocol,
        }
    }
}

pub struct ApprovalManager<C: DatabaseCollection> {
    input_channel: MpscChannel<ApprovalMessages, ApprovalResponses>,
    token: CancellationToken,
    governance_update_channel: tokio::sync::broadcast::Receiver<GovernanceUpdatedMessage>,
    messenger_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
    inner_manager: InnerApprovalManager<GovernanceAPI, C>,
    own_identifier: KeyIdentifier,
    channel_protocol: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
    signature_manager: SelfSignatureManager,
    derivator: DigestDerivator,
}

pub struct ApprovalAPI {
    input_channel: SenderEnd<ApprovalMessages, ApprovalResponses>,
}

impl ApprovalAPI {
    pub fn new(input_channel: SenderEnd<ApprovalMessages, ApprovalResponses>) -> Self {
        Self { input_channel }
    }
}

#[async_trait]
pub trait ApprovalAPIInterface {
    // TODO: Remove this???
    #[allow(dead_code)]
    async fn request_approval(
        &self,
        data: Signed<ApprovalRequest>,
    ) -> Result<(), ApprovalErrorResponse>;
    async fn emit_vote(
        &self,
        request_id: DigestIdentifier,
        acceptance: bool,
    ) -> Result<ApprovalEntity, ApprovalErrorResponse>;
    async fn get_all_requests(&self) -> Result<Vec<ApprovalEntity>, ApprovalErrorResponse>;
    async fn get_single_request(
        &self,
        request_id: DigestIdentifier,
    ) -> Result<ApprovalEntity, ApprovalErrorResponse>;
}

#[async_trait]
impl ApprovalAPIInterface for ApprovalAPI {
    async fn request_approval(
        &self,
        data: Signed<ApprovalRequest>,
    ) -> Result<(), ApprovalErrorResponse> {
        self.input_channel
            .tell(ApprovalMessages::RequestApproval(data))
            .await
            .map_err(|_| ApprovalErrorResponse::APIChannelNotAvailable)?;
        Ok(())
    }
    async fn emit_vote(
        &self,
        request_id: DigestIdentifier,
        acceptance: bool,
    ) -> Result<ApprovalEntity, ApprovalErrorResponse> {
        let result = self
            .input_channel
            .ask(ApprovalMessages::EmitVote(EmitVote {
                request_id,
                acceptance,
            }))
            .await
            .map_err(|_| ApprovalErrorResponse::APIChannelNotAvailable)?;
        match result {
            ApprovalResponses::EmitVote(data) => data,
            _ => unreachable!(),
        }
    }
    async fn get_all_requests(&self) -> Result<Vec<ApprovalEntity>, ApprovalErrorResponse> {
        let result = self
            .input_channel
            .ask(ApprovalMessages::GetAllRequest)
            .await
            .map_err(|_| ApprovalErrorResponse::APIChannelNotAvailable)?;
        match result {
            ApprovalResponses::GetAllRequest(data) => Ok(data),
            _ => unreachable!(),
        }
    }
    async fn get_single_request(
        &self,
        request_id: DigestIdentifier,
    ) -> Result<ApprovalEntity, ApprovalErrorResponse> {
        let result = self
            .input_channel
            .ask(ApprovalMessages::GetSingleRequest(request_id))
            .await
            .map_err(|_| ApprovalErrorResponse::APIChannelNotAvailable)?;
        match result {
            ApprovalResponses::GetSingleRequest(data) => data,
            _ => unreachable!(),
        }
    }
}

impl<C: DatabaseCollection> ApprovalManager<C> {
    pub fn new(
        token: CancellationToken,
        governance_update_channel: tokio::sync::broadcast::Receiver<GovernanceUpdatedMessage>,
        inner_manager: InnerApprovalManager<GovernanceAPI, C>,
        signature_manager: SelfSignatureManager,
        derivator: DigestDerivator,
        channels: ApprovalManagerChannels,
    ) -> Self {
        Self {
            input_channel: channels.input_channel,
            token,
            messenger_channel: channels.messenger_channel,
            governance_update_channel,
            inner_manager,
            own_identifier: signature_manager.get_own_identifier(),
            channel_protocol: channels.input_channel_protocol,
            signature_manager,
            derivator,
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
                },
                update = self.governance_update_channel.recv() => {
                    match update {
                        Ok(data) => {
                            match data {
                                GovernanceUpdatedMessage::GovernanceUpdated { governance_id, governance_version: _ } => {
                                    if let Err(error) = self.inner_manager.new_governance_version(&governance_id) {
                                        log::error!("NEW GOV VERSION APPROVAL ERROR: {}", error);
                                        break;
                                    }
                                }
                            }
                        },
                        Err(_) => {
                            break;
                        }
                    }
                }
            }
        }
        self.token.cancel();
        log::info!("Ended");
    }

    async fn process_command(
        &mut self,
        command: ChannelData<ApprovalMessages, ApprovalResponses>,
    ) -> Result<(), ApprovalManagerError> {
        let (sender_top, data) = match command {
            ChannelData::AskData(data) => {
                let (sender, data) = data.get();
                (Some(sender), data)
            }
            ChannelData::TellData(data) => {
                let data = data.get();
                (None, data)
            }
        };

        match data {
            ApprovalMessages::RequestApproval(_) => {
                log::error!("Request Approval without sender in approval manager");
                return Ok(());
            }
            ApprovalMessages::RequestApprovalWithSender { approval, sender } => {
                if sender_top.is_some() {
                    return Err(ApprovalManagerError::AskNoAllowed);
                }
                let result = self
                    .inner_manager
                    .process_approval_request(approval, sender)
                    .await?;
                match result {
                    Ok(Some((approval, sender))) => {
                        let msg = create_approver_response(approval);
                        self.send_message(None, msg, sender, MessageConfig::direct_response())
                            .await?;
                    }
                    Ok(None) => {}
                    Err(error) => match error {
                        ApprovalErrorResponse::OurGovIsLower {
                            our_id,
                            sender,
                            gov_id,
                        } => {
                            self.send_message(
                                None,
                                KoreMessages::LedgerMessages(
                                    crate::ledger::LedgerCommand::GetLCE {
                                        who_asked: our_id,
                                        subject_id: gov_id,
                                    },
                                ),
                                sender,
                                MessageConfig::direct_response(),
                            )
                            .await?
                        }
                        ApprovalErrorResponse::OurGovIsHigher {
                            our_id,
                            sender,
                            gov_id,
                        } => {
                            self.send_message(
                                None,
                                KoreMessages::EventMessage(
                                    crate::event::EventCommand::HigherGovernanceExpected {
                                        governance_id: gov_id,
                                        who_asked: our_id,
                                    },
                                ),
                                sender,
                                MessageConfig::direct_response(),
                            )
                            .await?
                        }
                        _ => {}
                    },
                }
            }
            ApprovalMessages::EmitVote(message) => {
                let result = self
                    .inner_manager
                    .generate_vote(&message.request_id, message.acceptance)
                    .await?;
                match result {
                    Ok((vote, owner)) => {
                        let msg = create_approver_response(vote.response.clone().unwrap());
                        self.send_message(None, msg, owner, MessageConfig::direct_response())
                            .await?;
                        if sender_top.is_some() {
                            sender_top
                                .unwrap()
                                .send(ApprovalResponses::EmitVote(Ok(vote)))
                                .map_err(|_| ApprovalManagerError::ResponseChannelClosed)?;
                        }
                    }
                    Err(error) => {
                        if sender_top.is_some() {
                            sender_top
                                .unwrap()
                                .send(ApprovalResponses::EmitVote(Err(error)))
                                .map_err(|_| ApprovalManagerError::ResponseChannelClosed)?;
                        }
                    }
                }
            }
            ApprovalMessages::GetAllRequest => {
                let result = self.inner_manager.get_all_request();
                if sender_top.is_some() {
                    sender_top
                        .unwrap()
                        .send(ApprovalResponses::GetAllRequest(result))
                        .map_err(|_| ApprovalManagerError::ResponseChannelClosed)?;
                }
            }
            ApprovalMessages::GetSingleRequest(request_id) => {
                let result = self.inner_manager.get_single_request(&request_id);
                if sender_top.is_some() {
                    sender_top
                        .unwrap()
                        .send(ApprovalResponses::GetSingleRequest(result))
                        .map_err(|_| ApprovalManagerError::ResponseChannelClosed)?;
                }
            }
        };

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
    ) -> Result<(), ApprovalManagerError> {
        if sender == self.own_identifier {
            let complete_message = Signed::<MessageContent<KoreMessages>>::new(
                self.own_identifier.clone(),
                self.own_identifier.clone(),
                event_message,
                &self.signature_manager,
                self.derivator,
            )
            .unwrap();

            self.channel_protocol
                .tell(complete_message)
                .await
                .map_err(|_| ApprovalManagerError::MessageChannelFailed)?
        } else {
            self.messenger_channel
                .tell(MessageTaskCommand::Request(
                    subject_id,
                    event_message,
                    vec![sender],
                    message_config,
                ))
                .await
                .map_err(|_| ApprovalManagerError::MessageChannelFailed)?
        }
        Ok(())
    }
}
