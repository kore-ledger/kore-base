use crate::governance::error::RequestError;
// use crate::ledger::errors::LedgerManagerError;
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum ProtocolErrors {
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
    #[error("Not validator")]
    NotValidator,
    #[error("Governance error")]
    GovernanceError {
        #[from]
        source: RequestError,
    },
}

#[derive(Error, Debug, Clone)]
pub enum EventCreationError {
    #[error("No owner of subject")]
    NoOwnerOfSubject,
    #[error("Event creation not possible")]
    EventCreationNotAvailable,
    #[error("Subject not available for new events")]
    SubjectNotAvailable,
}

#[derive(Error, Debug, Clone)]
pub enum ResponseError {
    #[error("Subject not found")]
    SubjectNotFound,
    #[error("Event not found")]
    EventNotFound,
    #[error("Governance Error: {}", source)]
    GovernanceError {
        #[from]
        source: RequestError,
    },
    #[error("EventCreationError")]
    EventCreationError {
        #[from]
        source: EventCreationError,
    },
    #[error("Comunnication with Leyer closed")]
    LedgerChannelClosed,
    #[error("Comunnication with manager closed")]
    ComunnicationClosed,
    #[error("Unexpect Command Response")]
    UnexpectedCommandResponse,
    #[error("Not valid set operation")]
    InvalidSetOperation,
    #[error("Simulation failed")]
    SimulationFailed,
    #[error("Approval is not needed")]
    ApprovalNotNeeded,
    #[error("The event to be voted on has already been included in the chain")]
    EventAlreadyOnChain,
    #[error("Subject not synchronized")]
    NoSynchronizedSubject,
    #[error("Invalid invokation caller of event request")]
    InvalidCaller,
    #[error("Subject already being approved")]
    SubjectNotAvailable,
    #[error("The subject is being validated")]
    SubjectBeingValidated,
    #[error("SN not expected")]
    UnexpectedSN,
    #[error("Invalid Hash in ApprovalResponse")]
    InvalidHash,
    #[error("Can't process approval. The subject is not controlled by current node")]
    NotOwnerOfSubject,
    #[error("Voting is not required for the specified request")]
    VoteNotNeeded,
    #[error("Request not found")]
    RequestNotFound,
    #[error("Request already known")]
    RequestAlreadyKnown,
    #[error("Request Type not supported")]
    RequestTypeError,
    #[error("Event request verification against schema failed")]
    EventRequestVerificationFailed,
    #[error("Schema {0} not found")]
    SchemaNotFound(String),
    #[error("Governance subjects cannot refer to other governance and their schema_id must be \"governance\".")]
    CantCreateGovernance,
}
