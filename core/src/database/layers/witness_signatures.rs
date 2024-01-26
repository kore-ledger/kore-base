use super::utils::{get_key, Element};
use crate::signature::Signature;
use crate::utils::{deserialize, serialize};
use crate::DbError;
use crate::{DatabaseCollection, DatabaseManager, Derivable, DigestIdentifier};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

/// Witness signatures database.
pub(crate) struct WitnessSignaturesDb<C: DatabaseCollection> {
    collection: C,
    prefix: String,
}

/// Witness signatures database implementation.
impl<C: DatabaseCollection> WitnessSignaturesDb<C> {
    /// Creates a new instance of witness signatures database.
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            collection: manager.create_collection("witness_signatures"),
            prefix: "witness_signatures".to_string(),
        }
    }

    /// Gets the witness signatures for a given subject.
    /// Returns the last sequence number and the set of signatures.
    ///
    /// # Arguments
    ///
    /// * `subject_id` - The subject identifier.
    ///
    /// # Returns
    ///
    /// Returns the last sequence number and the set of signatures.
    ///
    /// # Errors
    ///
    /// Returns an error if the subject identifier is invalid or if the
    /// database is corrupted.
    ///
    pub fn get_witness_signatures(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<(u64, HashSet<Signature>), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let witness_signatures = self.collection.get(&key)?;
        deserialize::<(u64, HashSet<Signature>)>(&witness_signatures)
            .map_err(|_| DbError::DeserializeError)
    }

    /// Gets all the witness signatures.
    /// Returns a vector of tuples containing the subject identifier, the last
    /// sequence number and the set of signatures.
    ///
    /// # Returns
    ///
    /// Returns a vector of tuples containing the subject identifier, the last
    /// sequence number and the set of signatures.
    ///
    /// # Errors
    ///
    /// Returns an error if the database is corrupted.
    ///
    pub fn get_all_witness_signatures(
        &self,
    ) -> Result<Vec<(DigestIdentifier, u64, HashSet<Signature>)>, DbError> {
        let iter = self
            .collection
            .iter(false, format!("{}{}", self.prefix, char::MAX).as_str());
        Ok(iter
            .map(|ws| {
                let ws_1 = deserialize::<(u64, HashSet<Signature>)>(&ws.1).unwrap();
                (DigestIdentifier::from_str(&ws.0).unwrap(), ws_1.0, ws_1.1)
            })
            .collect())
    }

    /// Sets the witness signatures for a given subject.
    ///
    /// # Arguments
    ///
    /// * `subject_id` - The subject identifier.
    /// * `sn` - The sequence number.
    ///
    /// # Returns
    ///
    /// Returns the last sequence number and the set of signatures.
    ///
    /// # Errors
    ///
    /// Returns an error if the subject identifier is invalid or if the
    /// database is corrupted.
    ///
    pub fn set_witness_signatures(
        &self,
        subject_id: &DigestIdentifier,
        sn: u64,
        signatures: HashSet<Signature>,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let total_signatures = match self.collection.get(&key) {
            Ok(other) => {
                let other = deserialize::<(u64, HashSet<Signature>)>(&other).unwrap();
                signatures.union(&other.1).cloned().collect()
            }
            Err(DbError::EntryNotFound) => signatures,
            Err(error) => {
                return Err(error);
            }
        };
        let Ok(data) = serialize::<(u64, HashSet<Signature>)>(&(sn, total_signatures)) else {
            return Err(DbError::SerializeError);
        };
        self.collection.put(&key, &data)
    }

    /// Deletes the witness signatures for a given subject.
    ///
    /// # Arguments
    ///
    /// * `subject_id` - The subject identifier.
    ///
    /// # Returns
    ///
    /// Returns the last sequence number and the set of signatures.
    ///
    /// # Errors
    ///
    /// Returns an error if the subject identifier is invalid or if the
    /// database is corrupted.
    ///
    pub fn del_witness_signatures(&self, subject_id: &DigestIdentifier) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        self.collection.del(&key)
    }
}

#[cfg(test)]
mod test {

    use super::*;

    use crate::database::memory::MemoryManager;

    #[test]
    fn test_witness_signatures() {
        let manager = Arc::new(MemoryManager::new());
        let db = WitnessSignaturesDb::new(&manager);
        let subject_id = DigestIdentifier::default();
        let sn = 0;
        let signatures = HashSet::new();
        let result = db.get_witness_signatures(&subject_id);
        assert!(result.is_err());
        assert!(db
            .set_witness_signatures(&subject_id, sn, signatures.clone())
            .is_ok());
        assert_eq!(
            db.get_witness_signatures(&subject_id).unwrap(),
            (sn, signatures.clone())
        );
        assert!(db
            .set_witness_signatures(&subject_id, sn, signatures.clone())
            .is_ok());
        assert_eq!(
            db.get_witness_signatures(&subject_id).unwrap(),
            (sn, signatures.clone())
        );
        assert!(db.del_witness_signatures(&subject_id).is_ok());
        assert!(db.get_witness_signatures(&subject_id).is_err());
    }
}
