use super::utils::{get_key, Element};
use crate::commons::models::validation::ValidationProof;
use crate::utils::{deserialize, serialize};
use crate::DbError;
use crate::{DatabaseCollection, DatabaseManager, Derivable, DigestIdentifier};
use std::sync::Arc;

/// Last committed event validation proofs database.
pub(crate) struct LceValidationProofs<C: DatabaseCollection> {
    collection: C,
    prefix: String,
}

/// Last committed event validation proofs database implementation.
impl<C: DatabaseCollection> LceValidationProofs<C> {
    /// Create a new last committed event validation proofs database.
    /// 
    /// # Arguments
    /// 
    /// * `manager` - Database manager.
    /// 
    /// # Returns
    /// 
    /// * `Self` - Returns last committed event validation proofs database.
    /// 
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            collection: manager.create_collection("lce_validation_proofs"),
            prefix: "lce_validation_proofs".to_string(),
        }
    }

    /// Get the last committed event validation proof.
    /// 
    /// # Arguments
    /// 
    /// * `subject_id` - Subject id.
    /// 
    /// # Returns
    /// 
    /// * `Result<ValidationProof, DbError>` - Returns the last committed event validation proof.
    /// 
    pub fn get_lce_validation_proof(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<ValidationProof, DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let lce_validation_proof = self.collection.get(&key)?;
        deserialize::<ValidationProof>(&lce_validation_proof).map_err(|_| DbError::DeserializeError)
    }

    /// Put the last committed event validation proof.
    /// 
    /// # Arguments
    /// 
    /// * `subject_id` - Subject id.
    /// * `lce_validation_proof` - Last committed event validation proof.
    /// 
    /// # Returns
    /// 
    /// * `Result<(), DbError>` - Returns Ok if the last committed event validation proof is put.
    /// 
    pub fn set_lce_validation_proof(
        &self,
        subject_id: &DigestIdentifier,
        lce_validation_proof: ValidationProof,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let Ok(data) = serialize::<ValidationProof>(&lce_validation_proof) else {
            return Err(DbError::SerializeError);
        };
        self.collection.put(&key, &data)
    }

    /// Delete the last committed event validation proof.
    /// 
    /// # Arguments
    /// 
    /// * `subject_id` - Subject id.
    /// 
    /// # Returns
    /// 
    /// * `Result<(), DbError>` - Returns Ok if the last committed event validation proof is deleted.
    /// 
    pub fn del_lce_validation_proof(&self, subject_id: &DigestIdentifier) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        self.collection.del(&key)
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::database::MemoryManager;
    
    #[test]
    fn test_lce_validation_proofs() {
        let manager = Arc::new(MemoryManager::new());
        let lce_validation_proofs_db = LceValidationProofs::new(&manager);
        let subject_id = DigestIdentifier::default();
        let lce_validation_proof = ValidationProof::default();
        lce_validation_proofs_db
            .set_lce_validation_proof(&subject_id, lce_validation_proof.clone())
            .unwrap();
        let lce_validation_proof = lce_validation_proofs_db
            .get_lce_validation_proof(&subject_id)
            .unwrap();
        assert_eq!(lce_validation_proof, lce_validation_proof);
        lce_validation_proofs_db.del_lce_validation_proof(&subject_id).unwrap();
        let lce_validation_proof = lce_validation_proofs_db
            .get_lce_validation_proof(&subject_id);
        assert!(lce_validation_proof.is_err());
    }
}