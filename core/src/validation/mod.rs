use std::collections::HashSet;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::{
    commons::{
        channel::SenderEnd,
        errors::ChannelErrors,
        models::validation::{ValidationEventResponse, ValidationProof},
        self_signature_manager::{SelfSignature, SelfSignatureManager},
    },
    database::{DatabaseCollection, DB},
    event::EventCommand,
    governance::{stage::ValidationStage, GovernanceAPI, GovernanceInterface},
    message::{MessageConfig, MessageContent, MessageTaskCommand},
    protocol::KoreMessages,
    signature::{Signature, Signed},
    Derivable, DigestDerivator, KeyIdentifier, Metadata,
};

use self::errors::ValidationError;

pub mod errors;
#[cfg(feature = "validation")]
pub mod manager;

#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum ValidationCommand {
    ValidationEvent {
        validation_event: ValidationEvent,
        sender: KeyIdentifier,
    },
    AskForValidation(ValidationEvent),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ValidationResponse {
    ValidationEventResponse(Result<ValidationEventResponse, ValidationError>),
}

#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ValidationEvent {
    pub proof: ValidationProof,
    pub subject_signature: Signature,
    pub previous_proof: Option<ValidationProof>,
    pub prev_event_validation_signatures: HashSet<Signature>,
}

pub struct Validation<C: DatabaseCollection> {
    gov_api: GovernanceAPI,
    database: DB<C>,
    signature_manager: SelfSignatureManager,
    message_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
    derivator: DigestDerivator,
    our_id: KeyIdentifier,
    protocol_channel: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
}

impl<C: DatabaseCollection> Validation<C> {
    pub fn new(
        gov_api: GovernanceAPI,
        database: DB<C>,
        signature_manager: SelfSignatureManager,
        message_channel: SenderEnd<MessageTaskCommand<KoreMessages>, ()>,
        derivator: DigestDerivator,
        protocol_channel: SenderEnd<Signed<MessageContent<KoreMessages>>, ()>,
    ) -> Self {
        Self {
            gov_api,
            database,
            our_id: signature_manager.get_own_identifier(),
            signature_manager,
            message_channel,
            derivator,
            protocol_channel,
        }
    }

    pub async fn validation_event(
        &self,
        validation_event: ValidationEvent,
        sender: KeyIdentifier,
    ) -> Result<ValidationEventResponse, ValidationError> {
        let actual_gov_version = if &validation_event.proof.schema_id == "governance"
            && validation_event.proof.sn == 0
        {
            0
        } else {
            match self
                .gov_api
                .get_governance_version(
                    validation_event.proof.governance_id.clone(),
                    validation_event.proof.subject_id.clone(),
                )
                .await
            {
                Ok(gov_version) => gov_version,
                Err(error) => match error {
                    crate::governance::error::RequestError::GovernanceNotFound(_)
                    | crate::governance::error::RequestError::SubjectNotFound
                    | crate::governance::error::RequestError::InvalidGovernanceID => {
                        return Err(ValidationError::GovernanceNotFound);
                    }
                    crate::governance::error::RequestError::ChannelClosed => {
                        return Err(ValidationError::ChannelError(ChannelErrors::ChannelClosed));
                    }
                    _ => return Err(ValidationError::GovApiUnexpectedResponse),
                },
            }
        };
        match actual_gov_version.cmp(&validation_event.proof.governance_version) {
            std::cmp::Ordering::Less => {
                return Err(ValidationError::GovernanceVersionTooHigh);
            }
            std::cmp::Ordering::Greater => {
                // Report outdated Gov.
                self.send_message(
                    None,
                    KoreMessages::EventMessage(
                        crate::event::EventCommand::HigherGovernanceExpected {
                            governance_id: validation_event.proof.governance_id.clone(),
                            who_asked: self.signature_manager.get_own_identifier(),
                        },
                    ),
                    sender,
                    MessageConfig::direct_response(),
                )
                .await?;

                return Err(ValidationError::GovernanceVersionTooLow);
            }
            std::cmp::Ordering::Equal => {}
        }
        let last_proof = {
            match self
                .database
                .get_validation_register(&validation_event.proof.subject_id)
            {
                Ok(last_proof) => Some(last_proof),
                Err(error) => match error {
                    crate::DbError::EntryNotFound => None,
                    _ => return Err(ValidationError::DatabaseError),
                },
            }
        };
        // Verify subject's signature on proof
        if validation_event
            .subject_signature
            .verify(&validation_event.proof)
            .is_err()
        {
            return Err(ValidationError::SubjectSignatureNotValid);
        }
        let subject_pk = self
            .check_proofs(
                &validation_event.proof,
                validation_event.previous_proof,
                validation_event.prev_event_validation_signatures,
                last_proof,
            )
            .await?;
        if validation_event.subject_signature.signer != subject_pk {
            return Err(ValidationError::SubjectSignatureNotValid);
        }
        self.database
            .set_validation_register(&validation_event.proof.subject_id, &validation_event.proof)
            .map_err(|_| ValidationError::DatabaseError)?;
        // Now we sign and send
        let validation_signature = self
            .signature_manager
            .sign(&validation_event.proof, self.derivator)
            .map_err(ValidationError::ProtocolErrors)?;

        self.send_message(
            None,
            KoreMessages::EventMessage(EventCommand::ValidatorResponse {
                event_hash: validation_event.proof.event_hash,
                signature: validation_signature.clone(),
                governance_version: actual_gov_version,
            }),
            sender,
            MessageConfig::direct_response(),
        )
        .await?;

        Ok(ValidationEventResponse {
            validation_signature,
            gov_version_validation: actual_gov_version,
        })
    }

    async fn check_proofs(
        &self,
        new_proof: &ValidationProof,
        previous_proof: Option<ValidationProof>,
        validation_signatures: HashSet<Signature>,
        last_proof: Option<ValidationProof>,
    ) -> Result<KeyIdentifier, ValidationError> {
        match last_proof {
            Some(last_proof) => {
                // Check that we have the proof of the previous event, if we don't have to check the one we get in the message as when we don't have the record
                if last_proof.sn > new_proof.sn {
                    Err(ValidationError::EventSnLowerThanLastSigned)
                } else if last_proof.sn == new_proof.sn && last_proof.sn != 0 {
                    // Verify that only the governance version changes
                    if !last_proof.is_similar(new_proof) {
                        Err(ValidationError::DifferentProofForEvent)
                    } else {
                        Ok(last_proof.subject_public_key)
                    }
                } else if last_proof.sn + 1 == new_proof.sn {
                    if previous_proof.is_none() {
                        return Err(ValidationError::PreviousProofLeft);
                    }
                    // Check that it is similar to the test of the previous event that comes to us in the message
                    if !last_proof.is_similar(&previous_proof.unwrap()) {
                        Err(ValidationError::DifferentProofForEvent)
                    } else {
                        self.validate_previous_proof(new_proof, last_proof, None)
                            .await
                    }
                } else {
                    // Same case as not found, I don't have the above test
                    if new_proof.sn == 0 {
                        // Check that it is exactly the same, you cannot change the gov version and not the subject_id, because the subject_id depends on it.
                        if &last_proof != new_proof {
                            Err(ValidationError::DifferentProofForEvent)
                        } else {
                            Ok(new_proof.subject_public_key.clone())
                        }
                    } else {
                        if previous_proof.is_none() {
                            return Err(ValidationError::PreviousProofLeft);
                        }
                        self.validate_previous_proof(
                            new_proof,
                            previous_proof.unwrap(),
                            Some(validation_signatures),
                        )
                        .await
                    }
                }
            }
            None => {
                // Check the above validation proof together with the validation signatures of that proof, its cryptographic validity and whether it reaches quorum
                if previous_proof.is_none() && new_proof.sn != 0 {
                    Err(ValidationError::PreviousProofLeft)
                } else if new_proof.sn != 0 {
                    self.validate_previous_proof(
                        new_proof,
                        previous_proof.unwrap(),
                        Some(validation_signatures),
                    )
                    .await
                } else {
                    if new_proof.governance_version != new_proof.genesis_governance_version {
                        return Err(ValidationError::GenesisGovVersionsDoesNotMatch(
                            new_proof.subject_id.to_str(),
                        ));
                    }
                    Ok(new_proof.subject_public_key.clone())
                }
            }
        }
    }

    async fn validate_previous_proof(
        &self,
        new_proof: &ValidationProof,
        previous_proof: ValidationProof,
        validation_signatures: Option<HashSet<Signature>>,
    ) -> Result<KeyIdentifier, ValidationError> {
        // Check that the previous one matches the new one
        // TODO: Check the other fields, such as subject_id, namespace...
        if previous_proof.event_hash != new_proof.prev_event_hash {
            return Err(ValidationError::DifferentProofForEvent);
        }
        if previous_proof.sn + 1 != new_proof.sn {
            return Err(ValidationError::DifferentProofForEvent);
        }
        if previous_proof.genesis_governance_version != new_proof.genesis_governance_version {
            return Err(ValidationError::DifferentProofForEvent);
        }
        if previous_proof.namespace != new_proof.namespace {
            return Err(ValidationError::DifferentProofForEvent);
        }
        if previous_proof.name != new_proof.name {
            return Err(ValidationError::DifferentProofForEvent);
        }
        if previous_proof.subject_id != new_proof.subject_id {
            return Err(ValidationError::DifferentProofForEvent);
        }
        if previous_proof.schema_id != new_proof.schema_id {
            return Err(ValidationError::DifferentProofForEvent);
        }
        if previous_proof.governance_id != new_proof.governance_id {
            return Err(ValidationError::DifferentProofForEvent);
        }
        if let Some(validation_signatures) = validation_signatures {
            let actual_signers: Result<HashSet<KeyIdentifier>, ValidationError> =
                validation_signatures
                    .into_iter()
                    .map(|signature| {
                        if signature.verify(&previous_proof).is_err() {
                            return Err(ValidationError::InvalidSignature);
                        }
                        Ok(signature.signer)
                    })
                    .collect();
            let actual_signers = actual_signers?;
            let (signers, quorum_size) = self
                .get_signers_and_quorum(previous_proof.get_metadata(), ValidationStage::Validate)
                .await?;
            if !actual_signers.is_subset(&signers) {
                return Err(ValidationError::InvalidSigner);
            }
            if actual_signers.len() < quorum_size as usize {
                return Err(ValidationError::QuorumNotReached);
            }
        }
        Ok(previous_proof.subject_public_key)
    }

    async fn get_signers_and_quorum(
        &self,
        metadata: Metadata,
        stage: ValidationStage,
    ) -> Result<(HashSet<KeyIdentifier>, u32), ValidationError> {
        let signers = self
            .gov_api
            .get_signers(metadata.clone(), stage.clone())
            .await
            .map_err(ValidationError::GovernanceError)?;
        let quorum_size = self
            .gov_api
            .get_quorum(metadata, stage)
            .await
            .map_err(ValidationError::GovernanceError)?;
        Ok((signers, quorum_size))
    }

    /// This function is in charge of sending KoreMessages, if it is a message addressed
    /// to the node itself, the message is sent directly to Protocol, otherwise it is sent to Network.
    async fn send_message(
        &self,
        subject_id: Option<String>,
        event_message: KoreMessages,
        sender: KeyIdentifier,
        message_config: MessageConfig,
    ) -> Result<(), ValidationError> {
        if sender == self.our_id {
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
                .map_err(ValidationError::ChannelError)
        } else {
            self.message_channel
                .tell(MessageTaskCommand::Request(
                    subject_id,
                    event_message,
                    vec![sender],
                    message_config,
                ))
                .await
                .map_err(ValidationError::ChannelError)
        }
    }
}
