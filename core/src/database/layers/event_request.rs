use super::utils::{get_key, Element};
use crate::signature::Signed;
use crate::utils::{deserialize, serialize};
use crate::{DatabaseCollection, DatabaseManager, Derivable, DigestIdentifier};
use crate::{DbError, EventRequest};
use std::sync::Arc;

pub(crate) struct EventRequestDb<C: DatabaseCollection> {
    collection: C,
    prefix: String,
}

impl<C: DatabaseCollection> EventRequestDb<C> {
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            collection: manager.create_collection("request"),
            prefix: "request".to_string(),
        }
    }

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

    pub fn get_all_request(&self) -> Vec<Signed<EventRequest>> {
        let mut result = Vec::new();
        for (_, request) in self
            .collection
            .iter(false, format!("{}{}", self.prefix, char::MAX))
        {
            let request = deserialize::<Signed<EventRequest>>(&request).unwrap();
            result.push(request);
        }
        result
    }

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
        self.collection.put(&key, data)
    }

    pub fn del_request(&self, subject_id: &DigestIdentifier) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        self.collection.del(&key)
    }
}
