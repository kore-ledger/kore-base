pub use crate::protocol::KoreMessages;
use crate::{commons::models::evaluation::EvaluationRequest, evaluator::EvaluatorMessage};

pub fn create_evaluator_request(event_pre_eval: EvaluationRequest) -> KoreMessages {
    KoreMessages::EvaluationMessage(EvaluatorMessage::AskForEvaluation(event_pre_eval))
}
