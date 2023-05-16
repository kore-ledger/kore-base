use serde::{Deserialize, Serialize};

use crate::{
    commons::models::{event::ValidationProof, notary::NotaryEventResponse},
    identifier::{DigestIdentifier, KeyIdentifier},
    signature::Signature,
};

use self::errors::NotaryError;

pub mod errors;
pub mod manager;
pub mod notary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotaryCommand {
    NotaryEvent(NotaryEvent),
}

#[derive(Debug, Clone)]
pub enum NotaryResponse {
    NotaryEventResponse(Result<NotaryEventResponse, NotaryError>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotaryEvent {
    pub proof: ValidationProof,
    pub subject_signature: Signature,
}
