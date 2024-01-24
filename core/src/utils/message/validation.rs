pub use crate::protocol::KoreMessages;
use crate::validation::{ValidationCommand, ValidationEvent};

pub fn create_validator_request(validation_event: ValidationEvent) -> KoreMessages {
    KoreMessages::ValidationMessage(Box::new(ValidationCommand::AskForValidation(
        validation_event,
    )))
}
