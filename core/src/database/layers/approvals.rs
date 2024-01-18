use super::utils::{get_by_range, get_key, Element};
use crate::commons::models::approval::{ApprovalEntity, ApprovalState};
use crate::utils::{deserialize, serialize};
use crate::DbError;
use crate::{DatabaseCollection, DatabaseManager, Derivable, DigestIdentifier};
use std::sync::Arc;

/// Approvals data store.
pub(crate) struct ApprovalsDb<C: DatabaseCollection> {
    index_collection: C,
    index_by_governance_collection: C,
    collection: C,
    pending_collection: C,
    index_prefix: String,
    prefix: String,
    governance_prefix: String,
    pending_prefix: String,
}

/// Approvals data store implementation.
impl<C: DatabaseCollection> ApprovalsDb<C> {
    /// Create a new approvals data store.
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            index_collection: manager.create_collection("subjindex-approval-index"),
            index_by_governance_collection: manager.create_collection("governance-approval-index"),
            collection: manager.create_collection("approvals"),
            index_prefix: "subjindex-approval-index".to_string(),
            prefix: "approvals".to_string(),
            governance_prefix: "governance-approval-index".to_string(),
            pending_prefix: "pending-approval-index".to_string(),
            pending_collection: manager.create_collection("pending-approval-index"),
        }
    }

    /// Set the subject approval index.
    /// # Arguments
    /// * `subject_id` - The subject identifier.
    /// * `request_id` - The request identifier.
    /// # Errors
    /// This function may return:
    /// * `DbError::SerializeError` if the request identifier cannot be serialized.
    /// # Returns
    /// Ok if the operation is successful.
    ///
    pub fn set_subject_approval_index(
        &self,
        subject_id: &DigestIdentifier,
        request_id: &DigestIdentifier,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.index_prefix.clone()),
            Element::S(subject_id.to_str()),
            Element::S(request_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let Ok(data) = serialize::<DigestIdentifier>(request_id) else {
            return Err(DbError::SerializeError);
        };
        self.index_collection.put(&key, &data)
    }

    /// Delete the subject approval index.
    /// # Arguments
    /// * `subject_id` - The subject identifier.
    /// * `request_id` - The request identifier.
    /// # Errors
    /// This function may return:
    /// * `DbError::SerializeError` if the request identifier cannot be serialized.
    /// # Returns
    /// Ok if the operation is successful.
    pub fn del_subject_approval_index(
        &self,
        subject_id: &DigestIdentifier,
        request_id: &DigestIdentifier,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.index_prefix.clone()),
            Element::S(subject_id.to_str()),
            Element::S(request_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        self.index_collection.del(&key)
    }

    /// Set the governance approval index.
    /// # Arguments
    /// * `governance_id` - The governance identifier.
    /// * `request_id` - The request identifier.
    /// # Errors
    /// This function may return:
    /// * `DbError::SerializeError` if the request identifier cannot be serialized.
    /// # Returns
    /// Ok if the operation is successful.
    /// 
    pub fn set_governance_approval_index(
        &self,
        governance_id: &DigestIdentifier,
        request_id: &DigestIdentifier,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.governance_prefix.clone()),
            Element::S(governance_id.to_str()),
            Element::S(request_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let Ok(data) = serialize::<DigestIdentifier>(request_id) else {
            return Err(DbError::SerializeError);
        };
        self.index_by_governance_collection.put(&key, &data)
    }

    /// Delete the governance approval index.
    /// # Arguments
    /// * `governance_id` - The governance identifier.
    /// * `request_id` - The request identifier.
    /// # Errors
    /// This function may return:
    /// * `DbError::SerializeError` if the request identifier cannot be serialized.
    /// # Returns
    /// Ok if the operation is successful.
    /// 
    pub fn del_governance_approval_index(
        &self,
        governance_id: &DigestIdentifier,
        request_id: &DigestIdentifier,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.governance_prefix.clone()),
            Element::S(governance_id.to_str()),
            Element::S(request_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        self.index_by_governance_collection.del(&key)
    }

    /// Gets approvals by governance.
    /// # Arguments
    /// * `governance_id` - The governance identifier.
    /// # Errors
    /// This function may return:
    /// * `DbError::SerializeError` if the identifier cannot be serialized.
    /// * `DbError::EntryNotFound` if the .
    /// # Returns
    /// Ok with the list of approvals by governance.
    pub fn get_approvals_by_governance(
        &self,
        governance_id: &DigestIdentifier,
    ) -> Result<Vec<DigestIdentifier>, DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.governance_prefix.clone()),
            Element::S(governance_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let iter = self.index_by_governance_collection.iter(false, &key);
        let mut result = Vec::new();
        let mut to_delete = Vec::new();
        for (_, data) in iter {
            let Ok(request_id) = deserialize::<DigestIdentifier>(&data) else {
                return Err(DbError::SerializeError);
            };
            // Comprobamos si existe en la colección base
            match self.get_approval(&request_id) {
                Ok(data) => match data.state {
                    ApprovalState::Pending => result.push(request_id),
                    _ => to_delete.push(request_id),
                },
                Err(DbError::EntryNotFound) => to_delete.push(request_id),
                Err(error) => return Err(error),
            }
        }
        for request_id in to_delete {
            let key_elements: Vec<Element> = vec![
                Element::S(self.governance_prefix.clone()),
                Element::S(governance_id.to_str()),
                Element::S(request_id.to_str()),
            ];
            let key = get_key(key_elements)?;
            self.index_by_governance_collection.del(&key)?;
        }
        Ok(result)
    }

    /// Gets approvals by subject.
    /// # Arguments
    /// * `subject_id` - The subject identifier.
    /// # Errors
    /// This function may return:
    /// * `DbError::SerializeError` if the request identifier cannot be serialized.
    /// # Returns
    /// Ok with the list of approvals by suject.
    /// 
    pub fn get_approvals_by_subject(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<Vec<DigestIdentifier>, DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.index_prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let iter = self.index_collection.iter(false, &key);
        let mut result = Vec::new();
        let mut to_delete = Vec::new();
        for (_, data) in iter {
            let Ok(request_id) = deserialize::<DigestIdentifier>(&data) else {
                return Err(DbError::SerializeError);
            };
            // Comprobamos si existe en la colección base
            match self.get_approval(&request_id) {
                Ok(data) => match data.state {
                    ApprovalState::Pending => result.push(request_id),
                    _ => to_delete.push(request_id),
                },
                Err(DbError::EntryNotFound) => to_delete.push(request_id),
                Err(error) => return Err(error),
            }
        }
        for request_id in to_delete {
            let key_elements: Vec<Element> = vec![
                Element::S(self.index_prefix.clone()),
                Element::S(subject_id.to_str()),
                Element::S(request_id.to_str()),
            ];
            let key = get_key(key_elements)?;
            self.index_collection.del(&key)?;
        }
        Ok(result)
    }

    /// Gets approval.
    ///
    /// # Arguments
    /// * `request_id` - The request identifier.
    /// # Errors
    /// This function may return:
    /// * `DbError::DeserializeError` if the approval cannot be deserialized.
    /// * `DbError::EntryNotFound` if the approval is not found.
    /// # Returns
    /// Ok with the approval.
    /// 
    pub fn get_approval(&self, request_id: &DigestIdentifier) -> Result<ApprovalEntity, DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(request_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let approval = self.collection.get(&key)?;
        Ok(deserialize::<ApprovalEntity>(&approval).map_err(|_| DbError::DeserializeError)?)
    }

    /// Gets approvals.
    /// 
    /// # Arguments
    /// * `status` - The approval status.
    /// * `from` - The start key.
    /// * `quantity` - The quantity of approvals to get.
    /// # Errors
    /// This function may return:
    /// * `DbError::DeserializeError` if the approval cannot be deserialized.
    /// * `DbError::EntryNotFound` if the approval is not found.
    /// # Returns
    /// Ok with the list of approvals.
    /// 
    pub fn get_approvals(
        &self,
        status: Option<ApprovalState>,
        from: Option<String>,
        quantity: isize,
    ) -> Result<Vec<ApprovalEntity>, DbError> {
        let mut result = Vec::new();
        let quantity_is_positive = quantity > 0;
        let mut quantity = quantity;
        let mut from = from;
        let mut continue_while: bool = true;
        if let Some(ApprovalState::Pending) = status {
            while quantity != 0 && continue_while {
                let approvals = get_by_range(
                    from.clone(),
                    quantity,
                    &self.pending_collection,
                    &self.pending_prefix,
                )?;
                if approvals.len() < quantity.abs() as usize {
                    continue_while = false;
                }
                for approval_id in approvals.iter() {
                    let approval_id = deserialize::<DigestIdentifier>(&approval_id)
                        .map_err(|_| DbError::DeserializeError)?;
                    let key_elements: Vec<Element> = vec![
                        Element::S(self.prefix.clone()),
                        Element::S(approval_id.to_str()),
                    ];
                    let key: String = get_key(key_elements)?;
                    from = Some(key.clone());
                    match self.collection.get(&key) {
                        Ok(approval) => {
                            let approval_entity = deserialize::<ApprovalEntity>(&approval)
                                .map_err(|_| DbError::DeserializeError)?;
                            if approval_entity.state != ApprovalState::Pending {
                                self.pending_collection.del(&key)?;
                                continue;
                            }
                            if quantity_is_positive {
                                quantity -= 1;
                            } else {
                                quantity += 1;
                            }
                            result.push(approval_entity);
                        }
                        Err(e) => match e {
                            DbError::EntryNotFound => {
                                self.pending_collection.del(&key)?;
                                continue;
                            }
                            _ => return Err(e),
                        },
                    }
                }
            }
        } else {
            let approvals = get_by_range(from, quantity, &self.collection, &self.prefix)?;
            for approval in approvals.iter() {
                let approval = deserialize::<ApprovalEntity>(&approval).unwrap();
                if status.is_some() {
                    if status.as_ref().unwrap() == &approval.state {
                        result.push(approval);
                    }
                } else {
                    result.push(approval);
                }
            }
        }
        return Ok(result);
    }

    /// Sets approval.
    /// 
    /// # Arguments
    /// 
    /// * `request_id` - The request identifier.
    /// * `approval` - The approval.
    /// 
    /// # Errors
    /// 
    /// This function may return:
    /// 
    /// * `DbError::SerializeError` if the approval cannot be serialized.
    /// 
    /// # Returns
    /// 
    /// Ok if the operation is successful.
    /// 
    pub fn set_approval(
        &self,
        request_id: &DigestIdentifier,
        approval: ApprovalEntity,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(request_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let Ok(data) = serialize::<ApprovalEntity>(&approval) else {
            return Err(DbError::SerializeError);
        };
        // We assume that we only have pending and voted status.
        let index_key_elements: Vec<Element> = vec![
            Element::S(self.pending_prefix.clone()),
            Element::S(request_id.to_str()),
        ];
        let key2 = get_key(index_key_elements)?;
        // If it is a pending status request, it is first saved in the index and then in the supper collection.
        if approval.state == ApprovalState::Pending {
            let Ok(data2) = serialize::<DigestIdentifier>(&request_id) else {
                return Err(DbError::SerializeError);
            };
            self.pending_collection.put(&key2, &data2)?;
        } else if approval.state != ApprovalState::Pending {
            self.pending_collection.del(&key2)?;
        }
        self.collection.put(&key, &data)?;
        Ok(())
    }

    /// Deletes approval.
    /// 
    /// # Arguments
    /// 
    /// * `request_id` - The request identifier.
    /// 
    /// # Returns
    /// 
    /// Ok if the operation is successful.
    /// 
    /// # Errors
    /// 
    /// This function may return:
    /// 
    /// * `DbError::SerializeError` if the approval cannot be serialized.
    /// 
    pub fn del_approval(&self, request_id: &DigestIdentifier) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(request_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        self.collection.del(&key)
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::commons::models::approval::tests::get_signed_approval_request;
    use crate::commons::models::approval::{ApprovalEntity, ApprovalState};
    use crate::database::{MemoryCollection, MemoryManager};
    use crate::DigestIdentifier;

    #[test]
    fn test_suject_approval_index() {
        let manager = MemoryManager::new();
        let approvals_db = ApprovalsDb::<MemoryCollection>::new(&Arc::new(manager));
        let subject_id =
            DigestIdentifier::new(crate::DigestDerivator::Blake3_256, &"subject_id".as_bytes());
        let request_id =
            DigestIdentifier::new(crate::DigestDerivator::Blake3_256, &"request_id".as_bytes());
        let result = approvals_db
            .set_subject_approval_index(&subject_id, &request_id);
        assert!(result.is_ok());
        let result = approvals_db.del_subject_approval_index(&subject_id, &request_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_governance_approval_index() {
        let manager = MemoryManager::new();
        let approvals_db = ApprovalsDb::<MemoryCollection>::new(&Arc::new(manager));
        let governance_id =
            DigestIdentifier::new(crate::DigestDerivator::Blake3_256, &"governance_id".as_bytes());
        let request_id =
            DigestIdentifier::new(crate::DigestDerivator::Blake3_256, &"request_id".as_bytes());
        let result = approvals_db.set_governance_approval_index(&governance_id, &request_id);
        assert!(result.is_ok());
        let result = approvals_db.del_governance_approval_index(&governance_id, &request_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_approvals_by_governance() {
        let manager = MemoryManager::new();
        let approvals_db = ApprovalsDb::<MemoryCollection>::new(&Arc::new(manager));
        let governance_id =
            DigestIdentifier::new(crate::DigestDerivator::Blake3_256, &"governance_id".as_bytes());
        let request_id =
            DigestIdentifier::new(crate::DigestDerivator::Blake3_256, &"request_id".as_bytes());
        let result = approvals_db
            .set_governance_approval_index(&governance_id, &request_id);
        assert!(result.is_ok());
        let request = get_signed_approval_request();
        let approval = ApprovalEntity {
            id: request_id.clone(),
            state: ApprovalState::Pending,
            request: request.clone(),
            response: None,
            sender: request.signature.signer.clone(),
        };
        let result = approvals_db.set_approval(&request_id, approval);
        assert!(result.is_ok());
        let result = approvals_db.get_approval(&request_id);
        assert!(result.is_ok());
        let result = approvals_db.set_governance_approval_index(&governance_id, &request_id);
        assert!(result.is_ok());
        let result = approvals_db.get_approvals_by_governance(&governance_id);
        assert!(result.is_ok());
        let approval = result.unwrap();
        assert_eq!(approval[0], request_id);
    }
}
