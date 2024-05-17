use std::collections::HashSet;

use identity::identifier::derive::digest::DigestDerivator;
use tokio::time::{interval, Duration};
use tokio_util::sync::CancellationToken;

use crate::commons::self_signature_manager::SelfSignatureManager;
use crate::database::Error as DbError;
use crate::message::MessageContent;
use crate::signature::Signed;
use crate::{
    commons::channel::{ChannelData, MpscChannel, SenderEnd},
    database::DB,
    message::MessageTaskCommand,
    protocol::KoreMessages,
    DatabaseCollection, DigestIdentifier, KeyIdentifier,
};

use super::{
    authorized_subjects::AuthorizedSubjects, error::AuthorizedSubjectsError,
    AuthorizedSubjectsCommand, AuthorizedSubjectsResponse,
};

#[derive(Clone, Debug)]
pub struct AuthorizedSubjectsAPI {
    sender: SenderEnd<AuthorizedSubjectsCommand, AuthorizedSubjectsResponse>,
}

impl AuthorizedSubjectsAPI {
    pub fn new(sender: SenderEnd<AuthorizedSubjectsCommand, AuthorizedSubjectsResponse>) -> Self {
        Self { sender }
    }

    pub async fn new_authorized_subject(
        &self,
        subject_id: DigestIdentifier,
        providers: HashSet<KeyIdentifier>,
    ) -> Result<(), AuthorizedSubjectsError> {
        self.sender
            .tell(AuthorizedSubjectsCommand::NewAuthorizedSubject {
                subject_id,
                providers,
            })
            .await?;
        Ok(())
    }
}

/// Manages authorized subjects and their providers.
pub struct AuthorizedSubjectsManager<C: DatabaseCollection> {
    /// Communication channel for incoming petitions
    input_channel: MpscChannel<AuthorizedSubjectsCommand, AuthorizedSubjectsResponse>,
    inner_authorized_subjects: AuthorizedSubjects<C>,
    token: CancellationToken,
}

pub struct AuthorizedSubjectChannels {
    input_channel: MpscChannel<AuthorizedSubjectsCommand, AuthorizedSubjectsResponse>,
    message_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
    channel_protocol: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
}

impl AuthorizedSubjectChannels {
    pub fn new(
        input_channel: MpscChannel<AuthorizedSubjectsCommand, AuthorizedSubjectsResponse>,
        message_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
        channel_protocol: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
    ) -> Self {
        Self {
            input_channel,
            message_channel,
            channel_protocol,
        }
    }
}

impl<C: DatabaseCollection> AuthorizedSubjectsManager<C> {
    /// Creates a new `AuthorizedSubjectsManager` with the given input channel, database, message channel, ID, and shutdown channels.
    pub fn new(
        database: DB<C>,
        our_id: KeyIdentifier,
        token: CancellationToken,
        signature_manager: SelfSignatureManager,
        derivator: DigestDerivator,
        channels: AuthorizedSubjectChannels
    ) -> Self {
        Self {
            input_channel: channels.input_channel,
            inner_authorized_subjects: AuthorizedSubjects::new(database, channels.message_channel, our_id, signature_manager, derivator, channels.channel_protocol),
            token,
        }
    }

    /// Starts the `AuthorizedSubjectsManager` and processes incoming commands.
    pub async fn run(mut self) {
        // Ask for all authorized subjects from the database
        match self.inner_authorized_subjects.ask_for_all().await {
            Ok(_) => {}
            Err(AuthorizedSubjectsError::DatabaseError(DbError::EntryNotFound)) => {}
            Err(error) => {
                log::error!("{}", error);
                self.token.cancel();
                return;
            }
        };
        // Set up a timer to periodically ask for all authorized subjects from the database
        let mut timer = interval(Duration::from_secs(15));
        loop {
            tokio::select! {
                // Process incoming commands from the input channel
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
                // Ask for all authorized subjects from the database when the timer ticks
                _ = timer.tick() => {
                    match self.inner_authorized_subjects.ask_for_all().await {
                        Ok(_) => {}
                        Err(AuthorizedSubjectsError::DatabaseError(DbError::EntryNotFound)) => {}
                        Err(error) => {
                            log::error!("{}", error);
                            break;
                        }
                    };
                },
                // Shutdown the manager when a shutdown signal is received
                _ = self.token.cancelled() => {
                    log::debug!("Shutdown received");
                    break;
                }
            }
        }
        self.token.cancel();
        log::info!("Ended");
    }

    /// Processes an incoming command from the input channel.
    async fn process_command(
        &mut self,
        command: ChannelData<AuthorizedSubjectsCommand, AuthorizedSubjectsResponse>,
    ) -> Result<(), AuthorizedSubjectsError> {
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
                AuthorizedSubjectsCommand::NewAuthorizedSubject {
                    subject_id,
                    providers,
                } => {
                    let response = self
                        .inner_authorized_subjects
                        .new_authorized_subject(subject_id, providers)
                        .await;
                    match response {
                        Ok(_) => {}
                        Err(error) => match error {
                            AuthorizedSubjectsError::DatabaseError(db_error) => match db_error {
                                crate::DbError::EntryNotFound => todo!(),
                                _ => return Err(AuthorizedSubjectsError::DatabaseError(db_error)),
                            },
                            _ => return Err(error),
                        },
                    }
                    AuthorizedSubjectsResponse::NoResponse
                }
            }
        };
        if let Some(sender) = sender {
            sender.send(response).expect("Sender Dropped");
        }
        Ok(())
    }
}
