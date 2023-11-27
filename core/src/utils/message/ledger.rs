use crate::{
    identifier::{DigestIdentifier, KeyIdentifier},
    ledger::LedgerCommand,
};

use super::approval::KoreMessages;

pub fn request_lce(who_asked: KeyIdentifier, subject_id: DigestIdentifier) -> KoreMessages {
    KoreMessages::LedgerMessages(LedgerCommand::GetLCE {
        who_asked,
        subject_id,
    })
}

pub fn request_event(
    who_asked: KeyIdentifier,
    subject_id: DigestIdentifier,
    sn: u64,
) -> KoreMessages {
    KoreMessages::LedgerMessages(LedgerCommand::GetEvent {
        who_asked,
        subject_id,
        sn,
    })
}

pub fn request_gov_event(
    who_asked: KeyIdentifier,
    subject_id: DigestIdentifier,
    sn: u64,
) -> KoreMessages {
    KoreMessages::LedgerMessages(LedgerCommand::GetNextGov {
        who_asked,
        subject_id,
        sn,
    })
}
