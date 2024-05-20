use std::collections::HashSet;

use identity::identifier::derive::digest::DigestDerivator;

use crate::{
    commons::{channel::SenderEnd, self_signature_manager::{SelfSignature, SelfSignatureManager}},
    database::DB,
    ledger::LedgerCommand,
    message::{MessageConfig, MessageContent, MessageTaskCommand},
    protocol::KoreMessages,
    signature::Signed,
    DatabaseCollection, DigestIdentifier, KeyIdentifier,
};

use super::error::AuthorizedSubjectsError;

/// Structure that manages the pre-authorized subjects in a system and communicates with other components of the system through a message channel.
pub struct AuthorizedSubjects<C: DatabaseCollection> {
    /// Object that handles the connection to the database.
    database: DB<C>,
    /// Message channel used to communicate with other system components.
    message_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
    /// Unique identifier for the component using this structure.
    our_id: KeyIdentifier,
    protocol_channel: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
    signature_manager: SelfSignatureManager,
    derivator: DigestDerivator,
}

impl<C: DatabaseCollection> AuthorizedSubjects<C> {
    /// Creates a new instance of the `AuthorizedSubjects` structure.
    ///
    /// # Arguments
    ///
    /// * `database` - Database connection.
    /// * `message_channel` - Message channel.
    /// * `our_id` - Unique identifier.
    pub fn new(
        database: DB<C>,
        message_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
        signature_manager: SelfSignatureManager,
        derivator: DigestDerivator,
        protocol_channel: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
    ) -> Self {
        Self {
            database,
            message_channel,
            our_id: signature_manager.get_own_identifier(),
            protocol_channel,
            signature_manager,
            derivator,
        }
    }

    /// Obtains all pre-authorized subjects and sends a message to the associated providers through the message channel.
    ///
    /// # Errors
    ///
    /// Returns an error if the preauthorized subjects cannot be obtained or if a message cannot be sent through the message channel.
    pub async fn ask_for_all(&self) -> Result<(), AuthorizedSubjectsError> {
        // We obtain all pre-authorized subjects from the database.
        let preauthorized_subjects = match self
            .database
            .get_allowed_subjects_and_providers(None, 10000)
        {
            Ok(psp) => psp,
            Err(error) => return Err(AuthorizedSubjectsError::DatabaseError(error)),
        };

        // For each pre-authorized subject, we send a message to the associated providers through the message channel.
        for (subject_id, providers) in preauthorized_subjects.into_iter() {
            if !providers.is_empty() {
                self.send_message(
                    None,
                    KoreMessages::LedgerMessages(LedgerCommand::GetLCE {
                        who_asked: self.our_id.clone(),
                        subject_id,
                    }),
                    providers.into_iter().collect(),
                    MessageConfig::direct_response(),
                )
                .await?
            }
        }
        Ok(())
    }

    /// Add a new pre-authorized subject and send a message to the associated suppliers through the message channel.
    ///
    /// # Arguments
    ///
    /// * `subject_id` - Identifier of the new pre-authorized subject.
    /// * `providers` - Set of associated provider identifiers.
    ///
    /// # Errors
    ///
    /// Returns an error if a message cannot be sent through the message channel.
    pub async fn new_authorized_subject(
        &self,
        subject_id: DigestIdentifier,
        providers: HashSet<KeyIdentifier>,
    ) -> Result<(), AuthorizedSubjectsError> {
        self.database
            .set_preauthorized_subject_and_providers(&subject_id, providers.clone())?;
        if !providers.is_empty() {
            self.send_message(
                None,
                KoreMessages::LedgerMessages(LedgerCommand::GetLCE {
                    who_asked: self.our_id.clone(),
                    subject_id,
                }),
                providers.into_iter().collect(),
                MessageConfig::direct_response(),
            )
            .await?
        }
        Ok(())
    }

    /// This function is in charge of sending KoreMessages, if it is a message addressed
    /// to the node itself, the message is sent directly to Protocol, otherwise it is sent to Network.
    async fn send_message(
        &self,
        subject_id: Option<String>,
        event_message: KoreMessages,
        mut signers: HashSet<KeyIdentifier>,
        message_config: MessageConfig,
    ) -> Result<(), AuthorizedSubjectsError> {
        let total_signers = signers.len();
        // If only has 1 id and is our node
        let our_node_is_validator = signers.contains(&self.our_id.clone());
        if total_signers == 1 && signers.contains(&self.our_id.clone()) {
            self.send_message_local(event_message).await
        // If our node is in the hashset and is not the only one.
        } else if our_node_is_validator {
            // our node
            self.send_message_local(event_message.clone()).await?;
            // others nodes.
            signers.remove(&self.our_id);
            self.send_message_network(subject_id, event_message, signers.clone(), message_config)
                .await
        }
        // If our node is not in the hashset
        else {
            self.send_message_network(subject_id, event_message, signers.clone(), message_config)
                .await
        }
    }

    async fn send_message_network(
        &self,
        subject_id: Option<String>,
        event_message: KoreMessages,
        signers: HashSet<KeyIdentifier>,
        message_config: MessageConfig,
    ) -> Result<(), AuthorizedSubjectsError> {
        self.message_channel
            .tell(MessageTaskCommand::Request(
                subject_id,
                event_message,
                signers.into_iter().collect(),
                message_config,
            ))
            .await
            .map_err( AuthorizedSubjectsError::ChannelError)
    }

    async fn send_message_local(
        &self,
        event_message: KoreMessages,
    ) -> Result<(), AuthorizedSubjectsError> {
        let complete_message = Signed::<MessageContent<KoreMessages>>::new(
            self.our_id.clone(),
            self.our_id.clone(),
            event_message,
            &self.signature_manager,
            self.derivator,
        )
        .unwrap();

        self.protocol_channel
            .tell(complete_message)
            .await
            .map_err( AuthorizedSubjectsError::ChannelError)
    }
}
