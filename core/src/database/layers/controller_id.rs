use super::utils::{get_key, Element};
use crate::utils::{deserialize, serialize};
use crate::DbError;
use crate::{DatabaseCollection, DatabaseManager};
use std::sync::Arc;

/// Controller id database.
/// This database stores the controller id.
/// The key is the controller id.
/// 
pub(crate) struct ControllerIdDb<C: DatabaseCollection> {
    collection: C,
    prefix: String,
}

/// Controller id database implementation.
impl<C: DatabaseCollection> ControllerIdDb<C> {
    /// Create a new controller id database.
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            collection: manager.create_collection("controller-id"),
            prefix: "controller-id".to_string(),
        }
    }

    /// Get the controller id.
    /// 
    /// # Errors
    /// 
    /// Returns an error if the controller id is not found.
    /// 
    /// # Returns
    /// 
    /// Ok with the controller id.
    /// 
    pub fn get_controller_id(&self) -> Result<String, DbError> {
        let key_elements: Vec<Element> = vec![Element::S(self.prefix.clone())];
        let key = get_key(key_elements)?;
        let controller_id = self.collection.get(&key)?;
        Ok(deserialize::<String>(&controller_id).map_err(|_| DbError::DeserializeError)?)
    }

    /// Put the controller id.  
    /// 
    /// # Arguments
    /// 
    /// * `controller_id` - The controller id.
    /// 
    /// # Errors
    /// 
    /// Returns an error if the controller id is not valid.
    /// 
    /// # Returns
    /// 
    /// Ok if the controller id is put.
    /// 
    pub fn set_controller_id(&self, controller_id: String) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![Element::S(self.prefix.clone())];
        let key = get_key(key_elements)?;
        let Ok(data) = serialize::<String>(&controller_id) else {
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
    fn test_controller_id() {
        let manager = Arc::new(MemoryManager::new());
        let controller_id_db = ControllerIdDb::new(&manager);
        let controller_id = "controller_id".to_string();
        controller_id_db
            .set_controller_id(controller_id.clone())
            .unwrap();
        let controller_id = controller_id_db.get_controller_id().unwrap();
        assert_eq!(controller_id, "controller_id".to_string());
    }
}
