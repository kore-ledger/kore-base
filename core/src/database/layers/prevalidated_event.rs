use super::utils::{get_key, Element};
use crate::signature::Signed;
use crate::utils::{deserialize, serialize};
use crate::DbError;
use crate::{DatabaseCollection, DatabaseManager, Derivable, DigestIdentifier, Event};
use std::sync::Arc;

/// Prevalidated event database.
pub(crate) struct PrevalidatedEventDb<C: DatabaseCollection> {
    collection: C,
    prefix: String,
}

/// Prevalidated event database implementation.
impl<C: DatabaseCollection> PrevalidatedEventDb<C> {

    /// Create a new prevalidated event database.
    /// 
    /// # Arguments
    /// 
    /// * `manager` - Database manager.
    /// 
    /// # Returns
    /// 
    /// * `Self` - Returns prevalidated event database.
    /// 
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            collection: manager.create_collection("prevalidated_event"),
            prefix: "prevalidated_event".to_string(),
        }
    }

    /// Get the prevalidated event.
    /// 
    /// # Arguments
    /// 
    /// * `subject_id` - Subject id.
    /// 
    /// # Returns
    /// 
    /// * `Result<Signed<Event>, DbError>` - Returns the prevalidated event.
    /// 
    pub fn get_prevalidated_event(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<Signed<Event>, DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let prevalidated_event = self.collection.get(&key)?;
        deserialize::<Signed<Event>>(&prevalidated_event).map_err(|_| DbError::DeserializeError)
    }

    /// Put the prevalidated event.
    /// 
    /// # Arguments
    /// 
    /// * `subject_id` - Subject id.
    /// * `event` - Prevalidated event.
    /// 
    /// # Returns
    /// 
    /// * `Result<(), DbError>` - Returns Ok if the prevalidated event is put.
    /// 
    pub fn set_prevalidated_event(
        &self,
        subject_id: &DigestIdentifier,
        event: Signed<Event>,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let Ok(data) = serialize::<Signed<Event>>(&event) else {
            return Err(DbError::SerializeError);
        };
        self.collection.put(&key, &data)
    }

    /// Delete the prevalidated event.
    /// 
    /// # Arguments
    /// 
    /// * `subject_id` - Subject id.
    /// 
    /// # Returns
    /// 
    /// * `Result<(), DbError>` - Returns Ok if the prevalidated event is deleted.
    /// 
    pub fn del_prevalidated_event(&self, subject_id: &DigestIdentifier) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        self.collection.del(&key)
    }
}

