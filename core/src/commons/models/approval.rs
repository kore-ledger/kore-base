//! Contains the data structures related to event  to send to approvers, or to validators if approval is not required.
use std::hash::Hasher;

use crate::{
    commons::errors::SubjectError,
    identifier::DigestIdentifier,
    signature::{Signature, Signed},
    DigestDerivator, EventRequest, KeyIdentifier, ValueWrapper,
};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use std::hash::Hash;

use super::HashId;

/// A struct representing an approval request.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, BorshSerialize, BorshDeserialize)]
pub struct ApprovalRequest {
    /// The signed event request.
    pub event_request: Signed<EventRequest>,
    /// The sequence number of the event.
    pub sn: u64,
    /// The version of the governance contract.
    pub gov_version: u64,
    /// The patch to apply to the state.
    pub patch: ValueWrapper,
    /// The hash of the state after applying the patch.
    pub state_hash: DigestIdentifier,
    /// The hash of the previous event.
    pub hash_prev_event: DigestIdentifier,
    /// The hash of the previous event.
    pub gov_id: DigestIdentifier,
}

impl HashId for ApprovalRequest {
    fn hash_id(&self, derivator: DigestDerivator) -> Result<DigestIdentifier, SubjectError> {
        DigestIdentifier::from_serializable_borsh(self, derivator).map_err(|_| {
            SubjectError::SignatureCreationFails("HashId for ApprovalRequest Fails".to_string())
        })
    }
}

impl Signed<ApprovalRequest> {
    pub fn new(content: ApprovalRequest, signature: Signature) -> Self {
        Self { content, signature }
    }

    pub fn verify(&self) -> Result<(), SubjectError> {
        self.signature.verify(&self.content)
    }
}

/// A struct representing an approval response.
#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    PartialOrd,
    PartialEq,
    Hash,
)]
pub struct ApprovalResponse {
    /// The hash of the approval request being responded to.
    pub appr_req_hash: DigestIdentifier,
    /// Whether the approval request was approved or not.
    pub approved: bool,
}

impl HashId for ApprovalResponse {
    fn hash_id(&self, derivator: DigestDerivator) -> Result<DigestIdentifier, SubjectError> {
        DigestIdentifier::from_serializable_borsh(self, derivator).map_err(|_| {
            SubjectError::SignatureCreationFails("HashId for ApprovalResponse Fails".to_string())
        })
    }
}

impl Signed<ApprovalResponse> {
    pub fn new(
        event_proposal_hash: DigestIdentifier,
        approved: bool,
        signature: Signature,
    ) -> Self {
        let content = ApprovalResponse {
            appr_req_hash: event_proposal_hash,
            approved,
        };
        Self { content, signature }
    }

    pub fn verify(&self) -> Result<(), SubjectError> {
        self.signature.verify(&self.content)
    }
}

#[derive(
    Debug, Clone, Serialize, Deserialize, Eq, BorshSerialize, BorshDeserialize, PartialOrd,
)]
pub struct UniqueApproval {
    pub approval: Signed<ApprovalResponse>,
}

impl PartialEq for UniqueApproval {
    fn eq(&self, other: &Self) -> bool {
        self.approval.signature.signer == other.approval.signature.signer
    }
}

impl Hash for UniqueApproval {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.approval.signature.signer.hash(state);
    }
}

/// An enum representing the state of an approval entity.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, BorshSerialize, BorshDeserialize)]
pub enum ApprovalState {
    /// The approval entity is pending a response.
    Pending,
    /// Request for approval which is in responded status and accepted
    RespondedAccepted,
    /// Request for approval which is in responded status and rejected
    RespondedRejected,
    /// The approval entity is obsolete.
    Obsolete,
}

/// A struct representing an approval entity.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, BorshSerialize, BorshDeserialize)]
pub struct ApprovalEntity {
    /// The identifier of the approval entity.
    pub id: DigestIdentifier,
    /// The signed approval request.
    pub request: Signed<ApprovalRequest>,
    /// The signed approval response, if one has been received.
    pub response: Option<Signed<ApprovalResponse>>,
    /// The state of the approval entity.
    pub state: ApprovalState,
    /// The sender of the approval request.
    pub sender: KeyIdentifier,
}

#[cfg(test)]
pub mod tests {

    use super::*;

    use crate::commons::models::{
        request::tests::get_signed_eol_request, signature::tests::get_signature_approval_request,
    };
    use serde_json::Value;

    /// Returns an approval approval reguest for testing.
    pub fn get_approval_request() -> ApprovalRequest {
        let signed_eol_request = get_signed_eol_request();
        ApprovalRequest {
            event_request: signed_eol_request.clone(),
            sn: 0,
            gov_version: 0,
            patch: ValueWrapper(Value::String("value".to_string())),
            state_hash: DigestIdentifier::new(DigestDerivator::Blake3_256, "state_hash".as_bytes()),
            hash_prev_event: DigestIdentifier::new(
                DigestDerivator::Blake3_256,
                "hash_prev_event".as_bytes(),
            ),
            gov_id: DigestIdentifier::new(DigestDerivator::Blake3_256, "gov_id".as_bytes()),
        }
    }

    /// Returns a signed approval request for testing.
    pub fn get_signed_approval_request() -> Signed<ApprovalRequest> {
        let approval_request = get_approval_request();
        let signature = get_signature_approval_request();
        Signed::<ApprovalRequest>::new(approval_request, signature)
    }
}
