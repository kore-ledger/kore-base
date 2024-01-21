use super::utils::{get_key, Element};
use crate::crypto::KeyPair;
use crate::utils::{deserialize, serialize};
use crate::DbError;
use crate::{DatabaseCollection, DatabaseManager, Derivable, KeyIdentifier};
use std::sync::Arc;

/// KeysDb is a wrapper around a database collection that stores public keys
/// from keypairs.
pub(crate) struct KeysDb<C: DatabaseCollection> {
    collection: C,
    prefix: String,
}

/// KeysDb implementation
impl<C: DatabaseCollection> KeysDb<C> {

    /// Create a new KeysDb instance
    /// 
    /// # Arguments
    /// 
    /// * `manager` - The database manager
    /// 
    /// # Returns
    /// 
    /// A new KeysDb instance
    /// 
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            collection: manager.create_collection("transfer"),
            prefix: "keys".to_string(),
        }
    }

    /// Get the keypair for a given public key identifier.
    /// 
    /// # Arguments
    /// 
    /// * `public_key` - The public key identifier
    /// 
    /// # Returns
    /// 
    /// The keypair (only the public key) for the given public key identifier
    /// 
    /// # Errors
    /// 
    /// An error is returned if the keypair cannot be found or deserialized.
    /// 
    pub fn get_keys(&self, public_key: &KeyIdentifier) -> Result<KeyPair, DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(public_key.to_str()),
        ];
        let key = get_key(key_elements)?;
        let value = self.collection.get(&key)?;
        let result = deserialize::<KeyPair>(&value).map_err(|_| DbError::DeserializeError)?;
        Ok(result)
    }

    /// Set the keypair for a given public key identifier.
    /// 
    /// # Arguments
    /// 
    /// * `public_key` - The public key identifier
    /// * `keypair` - The keypair (only the public key)
    /// 
    /// # Returns
    /// 
    /// Nothing
    /// 
    /// # Errors
    /// 
    /// An error is returned if the keypair cannot be serialized or stored.
    /// 
    pub fn set_keys(&self, public_key: &KeyIdentifier, keypair: KeyPair) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(public_key.to_str()),
        ];
        let key = get_key(key_elements)?;
        let Ok(data) = serialize::<KeyPair>(&keypair) else {
            return Err(DbError::SerializeError);
        };
        self.collection.put(&key, &data)
    }

    /// Delete the keypair for a given public key identifier.
    /// 
    /// # Arguments
    /// 
    /// * `public_key` - The public key identifier
    /// 
    /// # Returns
    /// 
    /// Nothing
    /// 
    /// # Errors
    /// 
    /// An error is returned if the keypair cannot be deleted.
    /// 
    pub fn del_keys(&self, public_key: &KeyIdentifier) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(public_key.to_str()),
        ];
        let key = get_key(key_elements)?;
        self.collection.del(&key)
    }

}

#[cfg(test)]
mod test {
    use super::*;
    use crate::crypto::KeyMaterial;
    use crate::database::DatabaseManager;
    use crate::database::MemoryManager;
    use crate::crypto::{KeyPair, KeyGenerator, Ed25519KeyPair};
    use crate::KeyIdentifier;
    use std::sync::Arc;

    #[test]
    fn test_keys_db() {
        let manager = Arc::new(MemoryManager::default());
        let db = KeysDb::new(&manager);
        let keypair = KeyPair::Ed25519(Ed25519KeyPair::new());
        let public_key = KeyIdentifier::new(crate::KeyDerivator::Ed25519, &keypair.public_key_bytes());
        db.set_keys(&public_key, keypair.clone()).unwrap();
        let result = db.get_keys(&public_key).unwrap();
        assert_eq!(keypair.public_key_bytes(), result.public_key_bytes());
        db.del_keys(&public_key).unwrap();
        let result = db.get_keys(&public_key);
        assert!(result.is_err());
    }
}