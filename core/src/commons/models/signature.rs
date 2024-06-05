//! Define the data structures related to signatures
use crate::{
    commons::errors::SubjectError,
    identifier::{DigestIdentifier, KeyIdentifier, SignatureIdentifier},
    keys::{KeyMaterial, KeyPair, Payload, DSA},
    Derivable, DigestDerivator,
};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

use super::{timestamp::TimeStamp, HashId};

/// Defines the data used to generate the signature, as well as the signer's identifier.
// #[derive(
//     Debug,
//     Clone,
//     Serialize,
//     Deserialize,
//     Eq,
//     BorshSerialize,
//     BorshDeserialize,
//     PartialOrd,
//     PartialEq,
// )]
// pub struct SignatureContent {
//     pub signer: KeyIdentifier,
//     pub event_content_hash: DigestIdentifier,
// }

/// The format, in addition to the signature, includes additional
/// information, namely the signer's identifier, the signature timestamp
/// and the hash of the signed contents.
#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    PartialOrd,
    Ord,
    PartialEq,
    Hash,
)]
pub struct Signature {
    /// Signer identifier
    pub signer: KeyIdentifier,
    /// Timestamp of the signature
    pub timestamp: TimeStamp,
    /// Hash of the content signed
    pub content_hash: DigestIdentifier,
    /// The signature itself
    pub value: SignatureIdentifier,
}

impl Signature {
    /// It allows the creation of a new signature
    /// # Arguments
    /// - content: The content to sign
    /// - keys: The [KeyPair] to use to generate the signature
    pub fn new<T: HashId>(
        content: &T,
        keys: &KeyPair,
        derivator: DigestDerivator,
    ) -> Result<Self, SubjectError> {
        let signer = KeyIdentifier::new(keys.get_key_derivator(), &keys.public_key_bytes());
        let timestamp = TimeStamp::now();
        // TODO: Analyze if we should remove HashId and change it for BorshSerialize
        // let content_hash = content.hash_id()?;
        let signature_hash =
            DigestIdentifier::from_serializable_borsh((&content, &timestamp), derivator).map_err(
                |_| SubjectError::SignatureCreationFails("Signature hash fails".to_string()),
            )?;
        let signature = keys
            .sign(Payload::Buffer(signature_hash.derivative()))
            .map_err(|_| SubjectError::SignatureCreationFails("Keys sign fails".to_owned()))?;
        Ok(Signature {
            signer: signer.clone(),
            timestamp,
            content_hash: signature_hash,
            value: SignatureIdentifier::new(signer.to_signature_derivator(), &signature),
        })
    }

    /// It allow verify the signature. It checks if the content and the signer are correct
    pub fn verify<T: HashId>(&self, content: &T) -> Result<(), SubjectError> {
        let derivator = self.content_hash.derivator;
        // let content_hash = content.hash_id(derivator)?;
        let signature_hash =
            DigestIdentifier::from_serializable_borsh((&content, &self.timestamp), derivator)
                .map_err(|_| {
                    SubjectError::SignatureCreationFails("Signature hash fails".to_string())
                })?;
        self.signer
            .verify(&signature_hash.digest, &self.value)
            .map_err(|_| SubjectError::SignatureVerifyFails("Signature verify fails".to_owned()))
    }
}

#[derive(
    Debug, Clone, Serialize, Deserialize, Eq, BorshSerialize, BorshDeserialize, PartialOrd,
)]
pub(crate) struct UniqueSignature {
    pub signature: Signature,
}

impl PartialEq for UniqueSignature {
    fn eq(&self, other: &Self) -> bool {
        self.signature.signer == other.signature.signer
    }
}

impl Hash for UniqueSignature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.signature.signer.hash(state);
    }
}

/// Represents any signed data entity
#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    Eq,
    PartialEq,
    BorshSerialize,
    BorshDeserialize,
    PartialOrd,
    Hash,
)]
pub struct Signed<T: BorshSerialize + BorshDeserialize + Clone> {
    #[serde(flatten)]
    /// The data that is signed
    pub content: T,
    /// The signature accompanying the data
    pub signature: Signature,
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::commons::models::{
        approval::tests::get_approval_request, request::tests::get_eol_request,
    };
    use crate::keys::{Ed25519KeyPair, KeyGenerator, KeyPair};

    /// get signature eol request for tests
    pub fn get_signature_eol_request() -> Signature {
        let keypair = KeyPair::Ed25519(Ed25519KeyPair::new());
        let derivator = DigestDerivator::Blake3_256;
        let signature = Signature::new(&get_eol_request(), &keypair, derivator).unwrap();
        signature
    }

    /// get signature approval request for tests
    pub fn get_signature_approval_request() -> Signature {
        let keypair = KeyPair::Ed25519(Ed25519KeyPair::new());
        let derivator = DigestDerivator::Blake3_256;
        let signature = Signature::new(&get_approval_request(), &keypair, derivator).unwrap();
        signature
    }
}
