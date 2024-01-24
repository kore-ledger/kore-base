pub use crate::protocol::KoreMessages;
use crate::{signature::Signed, ApprovalResponse, EvaluationResponse};

#[allow(dead_code)]
pub fn create_evaluator_response(evaluator_response: Signed<EvaluationResponse>) -> KoreMessages {
    KoreMessages::EventMessage(crate::event::EventCommand::EvaluatorResponse { evaluator_response })
}

#[allow(dead_code)]
pub fn create_approver_response(approval: Signed<ApprovalResponse>) -> KoreMessages {
    KoreMessages::EventMessage(crate::event::EventCommand::ApproverResponse { approval })
}
