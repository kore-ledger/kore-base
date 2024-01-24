pub use crate::protocol::KoreMessages;
use crate::{signature::Signed, ApprovalRequest};

pub fn create_approval_request(event_proposal: Signed<ApprovalRequest>) -> KoreMessages {
    KoreMessages::ApprovalMessages(crate::approval::ApprovalMessages::RequestApproval(
        event_proposal,
    ))
}
