//! Possible errors of a TAPLE Database
use thiserror::Error;

/// Errors that can be generated by Taple database implementations
#[derive(Error, Debug, Clone, PartialEq)]
pub enum Error {
    /// The entry does not exist in the database
    #[error("Entry Not Found")]
    EntryNotFound,
    /// A serialization error occurred when writing to the database.
    #[error("Error while serializing")]
    SerializeError,
    /// A deserialization error occurred while reading from the database.
    #[error("Error while deserializing")]
    DeserializeError,
    /// An error occurred while updating a subject in the database.
    #[error("Subject Apply failed")]
    SubjectApplyFailed,
    /// Conversion of a data to [DigestIdentifier] failed
    #[error("Conversion to Digest Identifier failed")]
    NoDigestIdentifier,
    #[error("Key Elements must have more than one element")]
    KeyElementsError,
    /// User-specifiable custom error
    #[error("An error withing the database custom implementation {0}")]
    CustomError(String),
    #[error("State non existent, possibilities are: Pending or Voted.")]
    NonExistentStatus,
    #[error("Encrypt could not be performed, {0}")]
    Encrypt(String),
    #[error("Decryption could not be performed, {0}")]
    Decrypt(String),
    #[error("Nonce is not found, {0}")]
    Nonce(String),
}
