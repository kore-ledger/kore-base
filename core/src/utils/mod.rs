use borsh::{from_slice, to_vec, BorshDeserialize, BorshSerialize};

use crate::Error;
pub mod message;
pub mod patch;

pub fn serialize<T: BorshSerialize>(data: &T) -> Result<Vec<u8>, Error> {
    to_vec(data).map_err(|_| Error::SerializeError)
}

pub fn deserialize<T: BorshDeserialize>(data: &[u8]) -> Result<T, Error> {
    from_slice::<T>(data).map_err(|_| Error::DeSerializeError)
}
