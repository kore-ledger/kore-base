use super::utils::{get_key, Element};
use crate::signature::Signed;
use crate::utils::{deserialize, serialize};
use crate::{DatabaseCollection, DatabaseManager, Derivable, DigestIdentifier};
use crate::{DbError, EventRequest};
use std::sync::Arc;

/// Event request data store.
pub(crate) struct EventRequestDb<C: DatabaseCollection> {
    collection: C,
    prefix: String,
}

/// Even request data store implementation.
impl<C: DatabaseCollection> EventRequestDb<C> {
    /// Create a new event request data store.
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            collection: manager.create_collection("request"),
            prefix: "request".to_string(),
        }
    }

    /// Get the event request associated with the given subject identifier.
    /// # Arguments
    /// * `subject_id` - The subject identifier.
    /// # Errors
    /// This function may return:
    /// * `DbError::DeserializeError` if the event request cannot be deserialized.
    /// * `DbError::MissingValue` if the event request is not found.
    /// * `DbError::SerializeError` if the event request cannot be serialized.
    /// # Returns
    /// The event request associated with the given subject identifier.
    pub fn get_request(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<Signed<EventRequest>, DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let request = self.collection.get(&key)?;
        Ok(deserialize::<Signed<EventRequest>>(&request).map_err(|_| DbError::DeserializeError)?)
    }

    #[allow(dead_code)] // TODO: remove
    pub fn get_all_request(&self) -> Vec<Signed<EventRequest>> {
        let mut result = Vec::new();
        for (_, request) in self
            .collection
            .iter(false, format!("{}{}", self.prefix, char::MAX).as_str())
        {
            let request = deserialize::<Signed<EventRequest>>(&request).unwrap();
            result.push(request);
        }
        result
    }

    /// Set the event request associated with the given subject identifier.
    /// # Arguments
    /// * `subject_id` - The subject identifier.
    /// * `request` - The event request.
    /// # Errors
    /// This function may return:
    /// * `DbError::SerializeError` if the event request cannot be serialized.
    /// # Returns
    /// The event request associated with the given subject identifier.
    ///
    pub fn set_request(
        &self,
        subject_id: &DigestIdentifier,
        request: Signed<EventRequest>,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let Ok(data) = serialize::<Signed<EventRequest>>(&request) else {
            return Err(DbError::SerializeError);
        };
        self.collection.put(&key, &data)
    }

    /// Delete the event request associated with the given subject identifier.
    /// # Arguments
    /// * `subject_id` - The subject identifier.
    /// # Errors
    /// This function may return:
    /// * `DbError::MissingValue` if the event request is not found.
    /// # Returns
    /// The event request associated with the given subject identifier.
    ///
    pub fn del_request(&self, subject_id: &DigestIdentifier) -> Result<(), DbError> {
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
    use crate::commons::models::request::tests::get_signed_eol_request;
    use crate::database::MemoryManager;

    #[test]
    fn test_request_db() {
        let manager = Arc::new(MemoryManager::new());
        let request_db = EventRequestDb::new(&manager);
        let subject_id = DigestIdentifier::default();
        let signed_request = get_signed_eol_request();
        request_db
            .set_request(&subject_id, signed_request.clone())
            .unwrap();
        let request2 = request_db.get_request(&subject_id).unwrap();
        assert_eq!(signed_request, request2);
        request_db.del_request(&subject_id).unwrap();
        assert!(request_db.get_request(&subject_id).is_err());
    }
}
