/// Copyright 2024 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later
use std::collections::HashSet;
use std::sync::Arc;

use crate::commons::models::approval::ApprovalEntity;
use crate::commons::models::request::KoreRequest;
use crate::commons::models::state::Subject;
use crate::commons::models::validation::ValidationProof;
use crate::identifier::{DigestIdentifier, KeyIdentifier};
use crate::keys::KeyPair;
use crate::signature::{Signature, Signed};
use crate::{ApprovalState, Event, EventRequest};

use super::error::Error;
use super::layers::lce_validation_proofs::LceValidationProofs;
use super::layers::request::RequestDb;
use super::{
    layers::{
        approvals::ApprovalsDb, contract::ContractDb, controller_id::ControllerIdDb,
        event::EventDb, event_request::EventRequestDb, keys::KeysDb,
        preauthorized_subjects_and_providers::PreauthorizedSbujectsAndProovidersDb,
        prevalidated_event::PrevalidatedEventDb, signature::SignatureDb, subject::SubjectDb,
        subject_by_governance::SubjectByGovernanceDb, validation::ValidationDb,
        witness_signatures::WitnessSignaturesDb,
    },
    DatabaseCollection, DatabaseManager,
};

/// A struct representing a database that stores TAPLE data.
pub struct DB<C: DatabaseCollection> {
    /// The signature database.
    signature_db: SignatureDb<C>,
    /// The subject database.
    subject_db: SubjectDb<C>,
    /// The event database.
    event_db: EventDb<C>,
    /// The prevalidated event database.
    prevalidated_event_db: PrevalidatedEventDb<C>,
    /// The event request database.
    event_request_db: EventRequestDb<C>,
    /// The request database.
    request_db: RequestDb<C>,
    /// The controller ID database.
    controller_id_db: ControllerIdDb<C>,
    /// The validation database.
    validation_db: ValidationDb<C>,
    /// The contract database.
    contract_db: ContractDb<C>,
    /// The witness signatures database.
    witness_signatures_db: WitnessSignaturesDb<C>,
    /// The subject by governance database.
    subject_by_governance_db: SubjectByGovernanceDb<C>,
    /// The keys database.
    keys_db: KeysDb<C>,
    /// The preauthorized subjects and providers database.
    preauthorized_subjects_and_providers_db: PreauthorizedSbujectsAndProovidersDb<C>,
    /// The LCE validation proofs database.
    lce_validation_proofs_db: LceValidationProofs<C>,
    /// The approvals database.
    approvals_db: ApprovalsDb<C>,
}

impl<C: DatabaseCollection> DB<C> {
    pub fn new<M: DatabaseManager<C>>(manager: Arc<M>, password: [u8; 32]) -> Self {
        let signature_db = SignatureDb::new(&manager);
        let subject_db = SubjectDb::new(&manager);
        let event_db = EventDb::new(&manager);
        let prevalidated_event_db = PrevalidatedEventDb::new(&manager);
        let event_request_db = EventRequestDb::new(&manager);
        let request_db = RequestDb::new(&manager);
        let controller_id_db = ControllerIdDb::new(&manager);
        let validation_db = ValidationDb::new(&manager);
        let contract_db = ContractDb::new(&manager);
        let witness_signatures_db = WitnessSignaturesDb::new(&manager);
        let subject_by_governance_db = SubjectByGovernanceDb::new(&manager);
        let transfer_events_db = KeysDb::new(&manager, password).unwrap();
        let preauthorized_subjects_and_providers_db =
            PreauthorizedSbujectsAndProovidersDb::new(&manager);
        let lce_validation_proofs_db = LceValidationProofs::new(&manager);
        let approvals_db = ApprovalsDb::new(&manager);
        Self {
            signature_db,
            subject_db,
            event_db,
            prevalidated_event_db,
            event_request_db,
            request_db,
            controller_id_db,
            validation_db,
            contract_db,
            witness_signatures_db,
            subject_by_governance_db,
            keys_db: transfer_events_db,
            preauthorized_subjects_and_providers_db,
            lce_validation_proofs_db,
            approvals_db,
        }
    }

    pub fn get_signatures(
        &self,
        subject_id: &DigestIdentifier,
        sn: u64,
    ) -> Result<(HashSet<Signature>, ValidationProof), Error> {
        self.signature_db.get_signatures(subject_id, sn)
    }

    pub fn set_signatures(
        &self,
        subject_id: &DigestIdentifier,
        sn: u64,
        signatures: HashSet<Signature>,
        validation_proof: ValidationProof,
    ) -> Result<(), Error> {
        self.signature_db
            .set_signatures(subject_id, sn, signatures, validation_proof)
    }

    pub fn del_signatures(&self, subject_id: &DigestIdentifier, sn: u64) -> Result<(), Error> {
        self.signature_db.del_signatures(subject_id, sn)
    }

    pub fn get_validation_proof(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<(HashSet<Signature>, ValidationProof), Error> {
        self.signature_db.get_validation_proof(subject_id)
    }

    pub fn get_subject(&self, subject_id: &DigestIdentifier) -> Result<Subject, Error> {
        self.subject_db.get_subject(subject_id)
    }

    pub fn set_subject(
        &self,
        subject_id: &DigestIdentifier,
        subject: Subject,
    ) -> Result<(), Error> {
        self.subject_db.set_subject(subject_id, subject)
    }

    pub fn get_subjects(
        &self,
        from: Option<String>,
        quantity: isize,
    ) -> Result<Vec<Subject>, Error> {
        self.subject_db.get_subjects(from, quantity)
    }

    // TODO: What we do with this function?
    pub fn _del_subject(&self, subject_id: &DigestIdentifier) -> Result<(), Error> {
        self.subject_db._del_subject(subject_id)
    }

    pub fn get_all_subjects(&self) -> Vec<Subject> {
        self.subject_db.get_all_subjects()
    }

    pub fn get_event(
        &self,
        subject_id: &DigestIdentifier,
        sn: u64,
    ) -> Result<Signed<Event>, Error> {
        self.event_db.get_event(subject_id, sn)
    }

    pub fn get_events_by_range(
        &self,
        subject_id: &DigestIdentifier,
        from: Option<i64>,
        quantity: isize,
    ) -> Result<Vec<Signed<Event>>, Error> {
        self.event_db
            .get_events_by_range(subject_id, from, quantity)
    }

    pub fn set_event(
        &self,
        subject_id: &DigestIdentifier,
        event: Signed<Event>,
    ) -> Result<(), Error> {
        self.event_db.set_event(subject_id, event)
    }

    pub fn del_event(&self, subject_id: &DigestIdentifier, sn: u64) -> Result<(), Error> {
        self.event_db.del_event(subject_id, sn)
    }

    pub fn get_prevalidated_event(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<Signed<Event>, Error> {
        self.prevalidated_event_db
            .get_prevalidated_event(subject_id)
    }

    pub fn set_prevalidated_event(
        &self,
        subject_id: &DigestIdentifier,
        event: Signed<Event>,
    ) -> Result<(), Error> {
        self.prevalidated_event_db
            .set_prevalidated_event(subject_id, event)
    }

    pub fn del_prevalidated_event(&self, subject_id: &DigestIdentifier) -> Result<(), Error> {
        self.prevalidated_event_db
            .del_prevalidated_event(subject_id)
    }

    pub fn get_request(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<Signed<EventRequest>, Error> {
        self.event_request_db.get_request(subject_id)
    }

    // TODO: What we do with this function?
    pub fn _get_all_request(&self) -> Vec<Signed<EventRequest>> {
        self.event_request_db.get_all_request()
    }

    pub fn set_request(
        &self,
        subject_id: &DigestIdentifier,
        request: Signed<EventRequest>,
    ) -> Result<(), Error> {
        self.event_request_db.set_request(subject_id, request)
    }

    pub fn del_request(&self, subject_id: &DigestIdentifier) -> Result<(), Error> {
        self.event_request_db.del_request(subject_id)
    }

    pub fn get_taple_request(&self, request_id: &DigestIdentifier) -> Result<KoreRequest, Error> {
        self.request_db.get_request(request_id)
    }

    // TODO: What we do with this function?
    pub fn _get_kore_all_request(&self) -> Vec<KoreRequest> {
        self.request_db._get_all_request()
    }

    pub fn set_taple_request(
        &self,
        request_id: &DigestIdentifier,
        request: &KoreRequest,
    ) -> Result<(), Error> {
        self.request_db.set_request(request_id, request)
    }

    // TODO: What we do with this function?
    pub fn _del_kore_request(&self, request_id: &DigestIdentifier) -> Result<(), Error> {
        self.request_db._del_request(request_id)
    }

    pub fn get_controller_id(&self) -> Result<String, Error> {
        self.controller_id_db.get_controller_id()
    }

    pub fn set_controller_id(&self, controller_id: String) -> Result<(), Error> {
        self.controller_id_db.set_controller_id(controller_id)
    }

    pub fn get_validation_register(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<ValidationProof, Error> {
        self.validation_db.get_validation_register(subject_id)
    }

    pub fn set_validation_register(
        &self,
        subject_id: &DigestIdentifier,
        validation_proof: &ValidationProof,
    ) -> Result<(), Error> {
        self.validation_db
            .set_validation_register(subject_id, validation_proof)
    }

    pub fn get_contract(
        &self,
        governance_id: &DigestIdentifier,
        schema_id: &str,
    ) -> Result<(Vec<u8>, DigestIdentifier, u64), Error> {
        self.contract_db.get_contract(governance_id, schema_id)
    }

    pub fn put_contract(
        &self,
        governance_id: &DigestIdentifier,
        schema_id: &str,
        contract: Vec<u8>,
        hash: DigestIdentifier,
        gov_version: u64,
    ) -> Result<(), Error> {
        self.contract_db
            .put_contract(governance_id, schema_id, contract, hash, gov_version)
    }

    pub fn get_witness_signatures(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<(u64, HashSet<Signature>), Error> {
        self.witness_signatures_db
            .get_witness_signatures(subject_id)
    }

    pub fn get_all_witness_signatures(
        &self,
    ) -> Result<Vec<(DigestIdentifier, u64, HashSet<Signature>)>, Error> {
        self.witness_signatures_db.get_all_witness_signatures()
    }

    pub fn set_witness_signatures(
        &self,
        subject_id: &DigestIdentifier,
        sn: u64,
        signatures: HashSet<Signature>,
    ) -> Result<(), Error> {
        self.witness_signatures_db
            .set_witness_signatures(subject_id, sn, signatures)
    }

    pub fn del_witness_signatures(&self, subject_id: &DigestIdentifier) -> Result<(), Error> {
        self.witness_signatures_db
            .del_witness_signatures(subject_id)
    }

    pub fn set_governance_index(
        &self,
        subject_id: &DigestIdentifier,
        governance_id: &DigestIdentifier,
    ) -> Result<(), Error> {
        self.subject_by_governance_db
            .set_governance_index(subject_id, governance_id)
    }

    pub fn get_subjects_by_governance(
        &self,
        governance_id: &DigestIdentifier,
    ) -> Result<Vec<DigestIdentifier>, Error> {
        self.subject_by_governance_db
            .get_subjects_by_governance(governance_id)
    }

    pub fn get_governances(
        &self,
        from: Option<String>,
        quantity: isize,
    ) -> Result<Vec<Subject>, Error> {
        self.subject_by_governance_db
            .get_governances(from, quantity)
    }

    pub fn get_governance_subjects(
        &self,
        governance_id: &DigestIdentifier,
        from: Option<String>,
        quantity: isize,
    ) -> Result<Vec<Subject>, Error> {
        self.subject_by_governance_db
            .get_governance_subjects(governance_id, from, quantity)
    }

    pub fn get_keys(&self, public_key: &KeyIdentifier) -> Result<KeyPair, Error> {
        self.keys_db.get_keys(public_key)
    }

    pub fn set_keys(&self, public_key: &KeyIdentifier, keypair: KeyPair) -> Result<(), Error> {
        self.keys_db.set_keys(public_key, keypair)
    }

    pub fn del_keys(&self, public_key: &KeyIdentifier) -> Result<(), Error> {
        self.keys_db.del_keys(public_key)
    }

    pub fn get_preauthorized_subject_and_providers(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<HashSet<KeyIdentifier>, Error> {
        self.preauthorized_subjects_and_providers_db
            .get_preauthorized_subject_and_providers(subject_id)
    }

    pub fn get_allowed_subjects_and_providers(
        &self,
        from: Option<String>,
        quantity: isize,
    ) -> Result<Vec<(DigestIdentifier, HashSet<KeyIdentifier>)>, Error> {
        self.preauthorized_subjects_and_providers_db
            .get_allowed_subjects_and_providers(from, quantity)
    }

    pub fn set_preauthorized_subject_and_providers(
        &self,
        subject_id: &DigestIdentifier,
        providers: HashSet<KeyIdentifier>,
    ) -> Result<(), Error> {
        self.preauthorized_subjects_and_providers_db
            .set_preauthorized_subject_and_providers(subject_id, providers)
    }

    pub fn get_lce_validation_proof(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<ValidationProof, Error> {
        self.lce_validation_proofs_db
            .get_lce_validation_proof(subject_id)
    }

    pub fn set_lce_validation_proof(
        &self,
        subject_id: &DigestIdentifier,
        proof: ValidationProof,
    ) -> Result<(), Error> {
        self.lce_validation_proofs_db
            .set_lce_validation_proof(subject_id, proof)
    }

    pub fn del_lce_validation_proof(&self, subject_id: &DigestIdentifier) -> Result<(), Error> {
        self.lce_validation_proofs_db
            .del_lce_validation_proof(subject_id)
    }

    pub fn get_approval(&self, request_id: &DigestIdentifier) -> Result<ApprovalEntity, Error> {
        self.approvals_db.get_approval(request_id)
    }

    pub fn get_approvals(
        &self,
        status: Option<ApprovalState>,
        from: Option<String>,
        quantity: isize,
    ) -> Result<Vec<ApprovalEntity>, Error> {
        self.approvals_db.get_approvals(status, from, quantity)
    }

    pub fn set_approval(
        &self,
        request_id: &DigestIdentifier,
        approval: ApprovalEntity,
    ) -> Result<(), Error> {
        self.approvals_db.set_approval(request_id, approval)
    }

    pub fn del_approval(&self, request_id: &DigestIdentifier) -> Result<(), Error> {
        self.approvals_db.del_approval(request_id)
    }

    pub fn get_approvals_by_subject(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<Vec<DigestIdentifier>, Error> {
        self.approvals_db.get_approvals_by_subject(subject_id)
    }

    pub fn del_subject_approval_index(
        &self,
        subject_id: &DigestIdentifier,
        request_id: &DigestIdentifier,
    ) -> Result<(), Error> {
        self.approvals_db
            .del_subject_approval_index(subject_id, request_id)
    }

    pub fn set_subject_approval_index(
        &self,
        subject_id: &DigestIdentifier,
        request_id: &DigestIdentifier,
    ) -> Result<(), Error> {
        self.approvals_db
            .set_subject_approval_index(subject_id, request_id)
    }

    pub fn get_approvals_by_governance(
        &self,
        governance_id: &DigestIdentifier,
    ) -> Result<Vec<DigestIdentifier>, Error> {
        self.approvals_db.get_approvals_by_governance(governance_id)
    }

    pub fn del_governance_approval_index(
        &self,
        governance_id: &DigestIdentifier,
        request_id: &DigestIdentifier,
    ) -> Result<(), Error> {
        self.approvals_db
            .del_governance_approval_index(governance_id, request_id)
    }

    pub fn set_governance_approval_index(
        &self,
        governance_id: &DigestIdentifier,
        request_id: &DigestIdentifier,
    ) -> Result<(), Error> {
        self.approvals_db
            .set_governance_approval_index(governance_id, request_id)
    }
}
