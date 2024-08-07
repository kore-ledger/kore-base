use ed25519_dalek::ed25519;
use std::convert::Infallible;
use thiserror::Error;

use crate::governance::error::RequestError;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Schema Creation Error")]
    SchemaCreation,
    #[error("Errors that can never happen")]
    Infalible {
        #[from]
        source: Infallible,
    },

    //#[error("Unknown error: `{0}`")]
    //Unknown(String),

    //#[error("Verification error: {0}")]
    //Verification(String),

    //#[error("`{0}`")]
    //Payload(String),
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
    //#[error("Event error: {0}")]
    //Event(String),

    //#[error("Seed error: {0}")]
    //Seed(String),

    //#[error("Semantic error: {0}")]
    //Semantic(String),

    //#[error("Invalid identifier: {0}")]
    //InvalidIdentifier(String),

    //#[error("Sign error: {0}")]
    //Sign(String),

    //#[error("No signature error: {0}")]
    //NoSignature(String),

    //#[error("Key pair error: {0}")]
    //KeyPair(String),

    //#[error("Kore error: {0}")]
    //Kore(String),

    //#[error("Store error: {0}")]
    //Store(String),

    //#[error("Duplicate Event")]
    //DuplicateEvent,

    //#[error("Event out of order")]
    //OutOfOrder,

    //#[error("Schema not found")]
    //SchemaNotFound,

    //#[error("Subject not found")]
    //SubjectNotFound,

    //#[error("Memory Database write fails")]
    //MemoryDBWriteFailed,

    //#[error("Serialization with Borsh fails")]
    //BorshSerializationFailed,
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum ChannelErrors {
    #[error("Channel is closed at the other end. Cannot send data")]
    ChannelClosed,
    #[error("Consumer queue is full.")]
    FullQueue,
}

#[derive(Error, Debug, PartialEq, Clone)]
pub enum SubjectError {
    #[error("Event request type is not Create")]
    NotCreateEvent,
    #[error("Event request type is not State")]
    NotStateEvent,
    #[error("An event is already created waiting to get quorum")]
    EventAlreadyProcessing,
    #[error("An event which is already applied is not in the database")]
    EventAlreadyAppliedNotFound,
    #[error("Event SN is not 0")]
    SnNot0,
    #[error("Event sourcing is not in order")]
    EventSourcingNotInOrder(u64, u64),
    #[error("Hash of Subject_data after apply does not match to event subject_data_hash")]
    EventSourcingHashNotEqual,
    #[error("Applying to Subject without data")]
    ApplyInEmptySubject,
    #[error("Subject Not Found")]
    SubjectNotFound,
    #[error("We are not the owner of the subject")]
    NotOwnerOfSubject,
    #[error("Event Content failed at serialization")]
    EventContentSerializationFailed,
    #[error("Subject Signature Failed")]
    SubjectSignatureFailed,
    #[error("Subject has no data")]
    SubjectHasNoData,
    #[error("Delete Signatures Failed")]
    DeleteSignaturesFailed,
    #[error("Schema Validation Failed")]
    SchemaValidationFailed,
    #[error("Schema does not compile")]
    SchemaDoesNotCompile,
    #[error("Error in criptography")]
    CryptoError(String),
    #[error("InvalidPayload {0}")]
    InvalidPayload(String),
    #[error("Error parsing json string")]
    ErrorParsingJsonString(String),
    #[error("Error applying patch")]
    ErrorApplyingPatch(String),
    #[error("Duplicated schema or member")]
    DuplicatedSchemaOrMember,
    #[error("Policies Missing for Some Schema")]
    PoliciesMissing,
    #[error("Invalid Policies Id")]
    InvalidPoliciesId,
    #[error("Invalid Member in Policies")]
    InvalidMemberInPolicies,
    #[error("Invalid member identifier {0}")]
    InvalidMemberIdentifier(String),
    #[error("JSON-PATCH on Create Event not allowed")]
    InvalidUseOfJSONPATCH,
    #[error("Approvers is not subset of validators")]
    ApproversAreNotValidators,
    #[error("Error creating subject id")]
    ErrorCreatingSubjectId,
    #[error("Signature Creation Fails: {0}")]
    SignatureCreationFails(String),
    #[error("Signature Verify Fails: {0}")]
    SignatureVerifyFails(String),
    #[error("Signature Repeated: {0}")]
    RepeatedSignature(String),
    #[error("Signers Error: {0}")]
    SignersError(String),
}

#[derive(Error, Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum ProtocolErrors {
    #[error("Errors that can never happen")]
    InfalibleError {
        #[from]
        source: Infallible,
    },
    #[error("Secret Key not found")]
    SignatureError,
    #[error("Channel unavaible")]
    ChannelError {
        #[from]
        source: crate::commons::errors::ChannelErrors,
    },
    #[error("Oneshot channel not available")]
    OneshotUnavailable,
    #[error("Ledger response not expected")]
    UnexpectedLedgerResponse,
    // #[error("Ledger error")]
    // LedgerError {
    //     #[from]
    //     source: LedgerManagerError,
    // },
    #[error("Not validator")]
    NotValidator,
    #[error("Governance error")]
    GovernanceError {
        #[from]
        source: RequestError,
    },
}

/// Errors that can occur during the generation of a [ListenAddr].
#[derive(Error, Debug, Clone)]
pub enum ListenAddrErrors {
    #[error("Invalid ListenAddr provided. Neither a valid IP or memory multiaddress")]
    InvalidListenAddr,
    #[error("Invalid multiaddres protocol specified. Supported are IP and Memory")]
    InvalidProtocolSpecified,
    #[error("The IPv4 specified is not valid")]
    InvalidIP4,
    #[error("The IPv6 specified is not valid")]
    InvalidIP6,
    #[error("No transport protocol specified")]
    NoTransportProtocolSpecified,
    #[error("Transpor protocol must be tcp")]
    NoTCP,
    #[error("Port must be a 32 bits unsigned")]
    NoU32Port,
    #[error("An IP must be combined with a tcp port")]
    InvalidCombination,
}
