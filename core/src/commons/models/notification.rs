/// Notifications generated by [NotificationHandler].
/// These notifications identify the type of message and its content.
/// In addition, the message is accompanied by the data used to construct
/// the message, so that it can be used to construct customized messages.
#[derive(Clone, Debug)]
pub enum Notification {
    /// A new subject has been generated
    NewSubject {
        subject_id: String,
    },
    /// A new event has been generated
    NewEvent {
        sn: u64,
        subject_id: String,
    },
    /// A subject has been synchronized
    StateUpdated {
        sn: u64,
        subject_id: String,
    },
    /// An approval request has been received
    ApprovalReceived {
        id: String,
        subject_id: String,
        sn: u64,
    },
    /// Approval Obsoleted because gov version changed or event confirmed without us
    ObsoletedApproval {
        id: String,
        subject_id: String,
        sn: u64,
    },
    UnrecoverableError {
        error: String,
    },
}
