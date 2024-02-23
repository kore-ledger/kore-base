use borsh::BorshSerialize;

use crate::{DigestDerivator, DigestIdentifier};

use super::errors::SubjectError;

pub mod approval;
pub mod evaluation;
pub mod event;
pub mod notification;
pub mod request;
pub mod signature;
pub mod state;
pub mod timestamp;
pub mod validation;
pub mod value_wrapper;

pub trait HashId: BorshSerialize {
    fn hash_id(&self, derivator: DigestDerivator) -> Result<DigestIdentifier, SubjectError>;
}

#[cfg(test)]
pub mod test {

    use super::*;
    use borsh::{BorshDeserialize, BorshSerialize};
    use serde::{Deserialize, Serialize};

    #[derive(
        Debug, Clone, Serialize, Deserialize, Eq, PartialEq, BorshSerialize, BorshDeserialize,
    )]
    pub struct Content(pub String);

    impl HashId for Content {
        fn hash_id(&self, derivator: DigestDerivator) -> Result<DigestIdentifier, SubjectError> {
            DigestIdentifier::from_serializable_borsh(self, derivator).map_err(|_| {
                SubjectError::SignatureCreationFails("HashId for Event Fails".to_string())
            })
        }
    }
}
