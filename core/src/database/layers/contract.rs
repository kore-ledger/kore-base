use super::utils::{get_key, Element};
use crate::utils::{deserialize, serialize};
use crate::DbError;
use crate::{DatabaseCollection, DatabaseManager, Derivable, DigestIdentifier};
use std::sync::Arc;

/// Contract database.
/// This database stores the contract for a given governance and schema.
/// The key is the governance id and the schema id.
pub(crate) struct ContractDb<C: DatabaseCollection> {
    collection: C,
    prefix: String,
}

/// Contract database implementation.
impl<C: DatabaseCollection> ContractDb<C> {
    /// Create a new contract database.
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            collection: manager.create_collection("contract"),
            prefix: "contract".to_string(),
        }
    }

    /// Get the contract for a given governance and schema.
    /// Returns the contract, the hash of the contract and the governance version.
    /// 
    /// # Arguments
    /// 
    /// * `governance_id` - The governance id.
    /// * `schema_id` - The schema id.
    /// 
    /// # Errors
    /// 
    /// Returns an error if the contract is not found or if the contract is not valid.
    /// 
    pub fn get_contract(
        &self,
        governance_id: &DigestIdentifier,
        schema_id: &str,
    ) -> Result<(Vec<u8>, DigestIdentifier, u64), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(governance_id.to_str()),
            Element::S(schema_id.to_string()),
        ];
        let key = get_key(key_elements)?;
        let contract = self.collection.get(&key)?;
        Ok(deserialize::<(Vec<u8>, DigestIdentifier, u64)>(&contract)
            .map_err(|_| DbError::DeserializeError)?)
    }

    /// Put a contract for a given governance and schema.
    /// 
    /// # Arguments
    /// 
    /// * `governance_id` - The governance id.
    /// * `schema_id` - The schema id.
    /// * `contract` - The contract.
    /// * `hash` - The hash of the contract.
    /// * `gov_version` - The governance version.
    /// 
    /// # Errors
    /// 
    /// Returns an error if the contract is not valid.
    /// 
    /// # Returns
    /// 
    /// Returns Ok if the contract is successfully stored.
    /// 
    pub fn put_contract(
        &self,
        governance_id: &DigestIdentifier,
        schema_id: &str,
        contract: Vec<u8>,
        hash: DigestIdentifier,
        gov_version: u64,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(governance_id.to_str()),
            Element::S(schema_id.to_string()),
        ];
        let key = get_key(key_elements)?;
        let Ok(data) =
            serialize::<(Vec<u8>, DigestIdentifier, u64)>(&(contract, hash, gov_version))
        else {
            return Err(DbError::SerializeError);
        };
        self.collection.put(&key, &data)
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use crate::database::MemoryManager;

    #[test]
    fn test_contract_db() {
        let manager = Arc::new(MemoryManager::new());
        let db = ContractDb::new(&manager);
        let governance_id = DigestIdentifier::default();
        let schema_id = "schema_id";
        let contract = vec![1, 2, 3];
        let hash = DigestIdentifier::default();
        let gov_version = 1;
        db.put_contract(
            &governance_id,
            schema_id,
            contract.clone(),
            hash.clone(),
            gov_version,
        )
        .unwrap();
        let (contract2, hash2, gov_version2) = db.get_contract(&governance_id, schema_id).unwrap();
        assert_eq!(contract, contract2);
        assert_eq!(hash, hash2);
        assert_eq!(gov_version, gov_version2);
    }
}
