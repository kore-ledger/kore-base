use super::utils::{get_key, Element};
use crate::commons::models::request::KoreRequest;
use crate::utils::{deserialize, serialize};
use crate::DbError;
use crate::{DatabaseCollection, DatabaseManager, Derivable, DigestIdentifier};
use std::sync::Arc;

pub(crate) struct RequestDb<C: DatabaseCollection> {
    collection: C,
    prefix: String,
}

impl<C: DatabaseCollection> RequestDb<C> {
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            collection: manager.create_collection("kore_request"),
            prefix: "kore_equest".to_string(),
        }
    }

    pub fn get_request(&self, request_id: &DigestIdentifier) -> Result<KoreRequest, DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(request_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let request = self.collection.get(&key)?;
        deserialize::<KoreRequest>(&request).map_err(|_| DbError::DeserializeError)
    }

    // TODO: What we do with this function?
    pub fn _get_all_request(&self) -> Vec<KoreRequest> {
        let mut result = Vec::new();
        for (_, request) in self
            .collection
            .iter(false, format!("{}{}", self.prefix, char::MAX).as_str())
        {
            let request = deserialize::<KoreRequest>(&request).unwrap();
            result.push(request);
        }
        result
    }

    pub fn set_request(
        &self,
        request_id: &DigestIdentifier,
        request: &KoreRequest,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(request_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let Ok(data) = serialize::<KoreRequest>(request) else {
            return Err(DbError::SerializeError);
        };
        self.collection.put(&key, &data)
    }

    // TODO: What we do with this function?
    pub fn _del_request(&self, request_id: &DigestIdentifier) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(request_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        self.collection.del(&key)
    }
}
