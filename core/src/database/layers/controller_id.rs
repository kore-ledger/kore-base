use super::utils::{get_key, Element};
use crate::utils::{deserialize, serialize};
use crate::DbError;
use crate::{DatabaseCollection, DatabaseManager};
use std::sync::Arc;

pub(crate) struct ControllerIdDb<C: DatabaseCollection> {
    collection: C,
    prefix: String,
}

impl<C: DatabaseCollection> ControllerIdDb<C> {
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            collection: manager.create_collection("controller-id"),
            prefix: "controller-id".to_string(),
        }
    }

    pub fn get_controller_id(&self) -> Result<String, DbError> {
        let key_elements: Vec<Element> = vec![Element::S(self.prefix.clone())];
        let key = get_key(key_elements)?;
        let controller_id = self.collection.get(&key)?;
        Ok(deserialize::<String>(&controller_id).map_err(|_| DbError::DeserializeError)?)
    }

    pub fn set_controller_id(&self, controller_id: String) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![Element::S(self.prefix.clone())];
        let key = get_key(key_elements)?;
        let Ok(data) = serialize::<String>(&controller_id) else {
            return Err(DbError::SerializeError);
        };
        self.collection.put(&key, data)
    }
}
