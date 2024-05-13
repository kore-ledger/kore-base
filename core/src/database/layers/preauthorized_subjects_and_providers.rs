use super::utils::{get_key, Element};
use crate::utils::{deserialize, serialize};
use crate::{DatabaseCollection, DatabaseManager, Derivable, DigestIdentifier};
use crate::{DbError, KeyIdentifier};
use std::collections::HashSet;
use std::sync::Arc;

/// Preauthorized subjects and providers database.
pub(crate) struct PreauthorizedSbujectsAndProovidersDb<C: DatabaseCollection> {
    collection: C,
    prefix: String,
}

/// Preauthorized subjects and providers database implementation.
impl<C: DatabaseCollection> PreauthorizedSbujectsAndProovidersDb<C> {
    /// Create a new preauthorized subjects and providers database.
    ///
    /// # Arguments
    ///
    /// * `manager` - Database manager.
    ///
    /// # Returns
    ///
    /// * `Self` - Returns preauthorized subjects and providers database.
    ///
    pub fn new<M: DatabaseManager<C>>(manager: &Arc<M>) -> Self {
        Self {
            collection: manager.create_collection("preauthorized_subjects_and_providers"),
            prefix: "preauthorized_subjects_and_providers".to_string(),
        }
    }

    /// Get the preauthorized subject and providers.
    ///
    /// # Arguments
    ///
    /// * `subject_id` - Subject id.
    ///
    /// # Returns
    ///
    /// * `Result<HashSet<KeyIdentifier>, DbError>` - Returns the preauthorized subject and providers.
    ///
    pub fn get_preauthorized_subject_and_providers(
        &self,
        subject_id: &DigestIdentifier,
    ) -> Result<HashSet<KeyIdentifier>, DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let value = self.collection.get(&key)?;
        let result = deserialize::<(DigestIdentifier, HashSet<KeyIdentifier>)>(&value)
            .map_err(|_| DbError::DeserializeError)?;
        Ok(result.1)
    }

    /// Get the allowed subjects and providers.
    ///
    /// # Arguments
    ///
    /// * `from` - From.
    /// * `quantity` - Quantity.
    ///
    /// # Returns
    ///
    /// * `Result<Vec<(DigestIdentifier, HashSet<KeyIdentifier>)>, DbError>` - Returns the allowed subjects and providers.
    ///
    pub fn get_allowed_subjects_and_providers(
        &self,
        from: Option<String>,
        quantity: isize,
    ) -> Result<Vec<(DigestIdentifier, HashSet<KeyIdentifier>)>, DbError> {
        let result = self.collection.get_by_range(from, quantity, &self.prefix)?;
        let mut vec_result = vec![];
        for value in result {
            vec_result.push(
                deserialize::<(DigestIdentifier, HashSet<KeyIdentifier>)>(&value)
                    .map_err(|_| DbError::DeserializeError)?,
            );
        }
        Ok(vec_result)
    }

    /// Put the preauthorized subject and providers.
    ///
    /// # Arguments
    ///
    /// * `subject_id` - Subject id.
    /// * `providers` - Providersset.
    ///
    /// # Returns
    ///
    /// * `Result<(), DbError>` - Returns Ok if the preauthorized subject and providers is put.
    ///
    pub fn set_preauthorized_subject_and_providers(
        &self,
        subject_id: &DigestIdentifier,
        providers: HashSet<KeyIdentifier>,
    ) -> Result<(), DbError> {
        let key_elements: Vec<Element> = vec![
            Element::S(self.prefix.clone()),
            Element::S(subject_id.to_str()),
        ];
        let key = get_key(key_elements)?;
        let Ok(data) = serialize::<(DigestIdentifier, HashSet<KeyIdentifier>)>(&(
            subject_id.clone(),
            providers,
        )) else {
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
    fn test_preauthorized_subjects_and_providers() {
        let manager = Arc::new(MemoryManager::new());
        let preauthorized_subjects_and_providers_db =
            PreauthorizedSbujectsAndProovidersDb::new(&manager);
        let subject_id = DigestIdentifier::default();
        let providers = HashSet::new();
        preauthorized_subjects_and_providers_db
            .set_preauthorized_subject_and_providers(&subject_id, providers.clone())
            .unwrap();
        let providers = preauthorized_subjects_and_providers_db
            .get_preauthorized_subject_and_providers(&subject_id)
            .unwrap();
        assert_eq!(providers, providers);
    }
}
