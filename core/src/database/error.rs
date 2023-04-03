//! Possible errors of a TAPLE Database
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
  #[error("Entry Not Found")]
  EntryNotFound,
  #[error("Error while serializing")]
  SerializeError,
  #[error("Error while deserializing")]
  DeserializeError,
  #[error("Subject Apply failed")]
  SubjectApplyFailed,
  #[error("Conversion to Digest Identifier failed")]
  NoDigestIdentifier,
  #[error("An error withing the database custom implementation")]
  CustomError(Box<dyn std::error::Error + 'static>)
}