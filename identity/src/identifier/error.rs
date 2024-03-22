use ed25519_dalek::ed25519;
use std::convert::Infallible;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Borsh serialization failed")]
    BorshSerializationFailed,

    #[error("Errors that can never happen")]
    Infalible {
        #[from]
        source: Infallible,
    },

    #[error("Unknown error: `{0}`")]
    Unknown(String),

    #[error("Verification error: {0}")]
    Verification(String),

    #[error("`{0}`")]
    Payload(String),

    #[error("Deserialization error")]
    Deserialization,

    #[error("Base58 Decoding error")]
    Base64Decoding {
        #[from]
        source: base64::DecodeError,
    },

    #[error("Ed25519 error")]
    Ed25519 {
        #[from]
        source: ed25519::Error,
    },

    #[error("Serde JSON error")]
    SerdeJson {
        #[from]
        source: serde_json::Error,
    },

    #[error("MessagePack serialize error")]
    MsgPackSerialize {
        #[from]
        source: rmp_serde::encode::Error,
    },

    #[error("MessagePack deserialize error")]
    MsgPackDeserialize {
        #[from]
        source: rmp_serde::decode::Error,
    },

    #[error("Event error: {0}")]
    Event(String),

    #[error("Seed error: {0}")]
    SeedError(String),

    #[error("Semantic error: {0}")]
    Semantic(String),

    #[error("Invalid identifier: {0}")]
    InvalidIdentifier(String),

    #[error("Sign error: {0}")]
    Sign(String),

    #[error("No signature error: {0}")]
    NoSignature(String),

    #[error("Key pair error: {0}")]
    KeyPair(String),

    #[error("Kore error: {0}")]
    Kore(String),

    #[error("Store error: {0}")]
    Store(String),

    #[error("Duplicate Event")]
    DuplicateEvent,

    #[error("Event out of order")]
    OutOfOrder,

    #[error("Schema not found")]
    SchemaNotFound,

    #[error("Subject not found")]
    SubjectNotFound,
}
