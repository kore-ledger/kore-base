pub use crate::protocol::protocol_message_manager::KoreMessages;
use crate::validation::{ValidationCommand, ValidationEvent};

pub fn create_validator_request(validation_event: ValidationEvent) -> KoreMessages {
    KoreMessages::ValidationMessage(ValidationCommand::AskForValidation(validation_event))
}
