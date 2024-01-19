mod db;
mod error;
mod layers;
mod memory;

pub use self::memory::{MemoryCollection, MemoryManager};
pub use db::DB;
pub use error::Error;

/// Trait to define a database compatible with Taple
pub trait DatabaseManager<C>: Sync + Send
where
    C: DatabaseCollection,
{
    /// Default constructor for the database manager. Is is mainly used for the battery test
    fn default() -> Self;
    /// Creates a database collection
    /// # Arguments
    /// - identifier: The identifier of the collection
    fn create_collection(&self, identifier: &str) -> C;
}

/// A trait representing a collection of key-value pairs in a database.
pub trait DatabaseCollection: Sync + Send {
    /// Retrieves the value associated with the given key.
    fn get(&self, key: &str) -> Result<Vec<u8>, Error>;
    /// Associates the given value with the given key.
    fn put(&self, key: &str, data: &[u8]) -> Result<(), Error>;
    /// Removes the value associated with the given key.
    fn del(&self, key: &str) -> Result<(), Error>;
    /// Returns an iterator over the key-value pairs in the collection.
    fn iter<'a>(
        &'a self,
        reverse: bool,
        prefix: &str,
    ) -> Box<dyn Iterator<Item = (String, Vec<u8>)> + 'a>;

    /// Returns a vector of values in the collection that are in the given range.
    fn get_by_range(
        &self,
        from: Option<String>,
        quantity: isize,
        prefix: &str,
    ) -> Result<Vec<Vec<u8>>, Error> {
        fn convert<'a>(
            iter: impl Iterator<Item = (String, Vec<u8>)> + 'a,
        ) -> Box<dyn Iterator<Item = (String, Vec<u8>)> + 'a> {
            Box::new(iter)
        }
        let (mut iter, quantity) = match from {
            Some(key) => {
                // Find the key
                let iter = if quantity >= 0 {
                    self.iter(false, prefix)
                } else {
                    self.iter(true, format!("{}{}", prefix, char::MAX).as_str())
                };
                let mut iter = iter.peekable();
                loop {
                    let Some((current_key, _)) = iter.peek() else {
                        return Err(Error::EntryNotFound);
                    };
                    if current_key == &key {
                        break;
                    }
                    iter.next();
                }
                iter.next(); // Exclusive From
                (convert(iter), quantity.abs())
            }
            None => {
                if quantity >= 0 {
                    (self.iter(false, prefix), quantity)
                } else {
                    (self.iter(true, prefix), quantity.abs())
                }
            }
        };
        let mut result = Vec::new();
        let mut counter = 0;
        while counter < quantity {
            let Some((_, event)) = iter.next() else {
                break;
            };
            result.push(event);
            counter += 1;
        }
        Ok(result)
    }
    
}

/// Allows a TAPLE database implementation to be subjected to a battery of tests.
/// The use must specify both a valid implementation of [DatabaseManager] and [DatabaseCollection]
/// # Example
/// ```rs
/// test_database_manager_trait! {
///    unit_test_memory_manager:crate::MemoryManager:MemoryCollection
/// }
/// ```
#[macro_export]
macro_rules! test_database_manager_trait {
    ($name:ident: $type:ty: $type2:ty) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;
            #[allow(unused_imports)]
            use borsh::{to_vec, BorshDeserialize, BorshSerialize};

            #[derive(BorshSerialize, BorshDeserialize, Clone, PartialEq, Eq, Debug)]
            struct Data {
                id: usize,
                value: String,
            }

            #[allow(dead_code)]
            fn get_data() -> Result<Vec<Vec<u8>>, Error> {
                let data1 = Data {
                    id: 1,
                    value: "A".into(),
                };
                let data2 = Data {
                    id: 2,
                    value: "B".into(),
                };
                let data3 = Data {
                    id: 3,
                    value: "C".into(),
                };
                #[rustfmt::skip] // let-else not supported yet
                let Ok(data1) = to_vec(&data1) else {
                    return Err(Error::SerializeError);
                };
                #[rustfmt::skip] // let-else not supported yet
                let Ok(data2) = to_vec(&data2) else {
                    return Err(Error::SerializeError);
                };
                #[rustfmt::skip] // let-else not supported yet
                let Ok(data3) = to_vec(&data3) else {
                    return Err(Error::SerializeError);
                };
                Ok(vec![data1, data2, data3])
            }

            #[test]
            fn basic_operations_test() {
                let db = <$type>::default();
                let first_collection: $type2 = db.create_collection("first");
                let data = get_data().unwrap();
                // PUT & GET Operations
                // PUT
                let result = first_collection.put("a", &data[0]);
                assert!(result.is_ok());
                let result = first_collection.put("b", &data[1]);
                assert!(result.is_ok());
                let result = first_collection.put("c", &data[2]);
                assert!(result.is_ok());
                // GET
                let result = first_collection.get("a");
                assert!(result.is_ok());
                assert_eq!(result.unwrap(), data[0]);
                let result = first_collection.get("b");
                assert!(result.is_ok());
                assert_eq!(result.unwrap(), data[1]);
                let result = first_collection.get("c");
                assert!(result.is_ok());
                assert_eq!(result.unwrap(), data[2]);
                // DEL
                let result = first_collection.del("a");
                assert!(result.is_ok());
                let result = first_collection.del("b");
                assert!(result.is_ok());
                let result = first_collection.del("c");
                assert!(result.is_ok());
                // GET OF DELETED ENTRIES
                let result = first_collection.get("a");
                assert!(result.is_err());
                let result = first_collection.get("b");
                assert!(result.is_err());
                let result = first_collection.get("c");
                assert!(result.is_err());
            }
            /*
                        #[test]
                        fn partitions_test() {
                            let db = <$type>::default();
                            let first_collection: $type2 = db.create_collection("first");
                            let second_collection: $type2 = db.create_collection("second");
                            let data = get_data().unwrap();
                            // PUT UNIQUE ENTRIES IN EACH PARTITION
                            let result = first_collection.put("a", data[0].to_owned());
                            assert!(result.is_ok());
                            let result = second_collection.put("b", data[1].to_owned());
                            assert!(result.is_ok());
                            // NO EXIST IDIVIDUALITY
                            let result = first_collection.get("b");
                            assert_eq!(result.unwrap(), data[1]);
                            let result = second_collection.get("a");
                            assert_eq!(result.unwrap(), data[0]);
                        }
            */
            #[allow(dead_code)]
            fn build_state(collection: &$type2) {
                let data = get_data().unwrap();
                let result = collection.put("a", &data[0]);
                assert!(result.is_ok());
                let result = collection.put("b", &data[1]);
                assert!(result.is_ok());
                let result = collection.put("c", &data[2]);
                assert!(result.is_ok());
            }

            #[allow(dead_code)]
            fn build_initial_data() -> (Vec<&'static str>, Vec<Vec<u8>>) {
                let keys = vec!["a", "b", "c"];
                let data = get_data().unwrap();
                let values = vec![data[0].to_owned(), data[1].to_owned(), data[2].to_owned()];
                (keys, values)
            }

            #[test]
            fn iterator_test() {
                let db = <$type>::default();
                let first_collection: $type2 = db.create_collection("first");
                build_state(&first_collection);
                // ITER TEST
                let mut iter = first_collection.iter(false, "");
                //assert!(iter.next().is_some());

                let (keys, data) = build_initial_data();
                for i in 0..3 {
                    let (key, val) = iter.next().unwrap();
                    assert_eq!(keys[i], key);
                    assert_eq!(data[i], val);
                }
                assert!(iter.next().is_none());
            }

            #[test]
            fn rev_iterator_test() {
                let db = <$type>::default();
                let first_collection: $type2 = db.create_collection("first");
                build_state(&first_collection);
                // ITER TEST
                let mut iter = first_collection.iter(true, "");
                let (keys, data) = build_initial_data();
                for i in (0..3).rev() {
                    let (key, val) = iter.next().unwrap();
                    assert_eq!(keys[i], key);
                    assert_eq!(data[i], val);
                }
                assert!(iter.next().is_none());
            }

            #[test]
            fn get_by_range_test() {
                let db = <$type>::default();
                let first_collection: $type2 = db.create_collection("first");
                build_state(&first_collection);
                // GET BY RANGE TEST
                let result = first_collection.get_by_range(None, 2, "");
                assert!(result.is_ok());
                let result = result.unwrap();
                let (key, data) = build_initial_data();
                for i in 0..2 {
                    assert_eq!(data[i], result[i]);
                    assert_eq!(data[i], result[i]);
                }
                let result = first_collection.get_by_range(Some(key[1].to_owned()), 1, "");
                assert!(result.is_ok());
                let result = result.unwrap();
                assert_eq!(data[2], result[0]);
            }
        }
    };
}
