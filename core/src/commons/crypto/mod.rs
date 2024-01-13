//! This module provides the structs and traits for the generation of key pairs for
//! cryptographic operations.
//!

pub(crate) mod ed25519;
pub(crate) mod error;
#[cfg(feature = "secp256k1")]
pub(crate) mod secp256k1;

use std::io::Read;

use borsh::{BorshDeserialize, BorshSerialize};
use identifier::error::Error;

use memsecurity::EncryptedMem;

use base64::{engine::general_purpose, Engine as _};
pub use ed25519::Ed25519KeyPair;
#[cfg(feature = "secp256k1")]
pub use secp256k1::Secp256k1KeyPair;
use serde::{Deserialize, Serialize};

use crate::identifier::{self, derive::KeyDerivator};

/// Asymmetric key pair
#[derive(Serialize, Deserialize, Debug)]
pub enum KeyPair {
    Ed25519(Ed25519KeyPair),
    #[cfg(feature = "secp256k1")]
    Secp256k1(Secp256k1KeyPair),
}

/// Key pair types.
/// Used to derive key pair from hex string.
pub enum KeyPairType {
    Ed25519,
    Secp256k1,
}

impl KeyPair {
    pub fn get_key_derivator(&self) -> KeyDerivator {
        match self {
            KeyPair::Ed25519(_) => KeyDerivator::Ed25519,
            KeyPair::Secp256k1(_) => KeyDerivator::Secp256k1,
        }
    }

    pub fn from_hex(derivator: &KeyDerivator, hex_key: &str) -> Result<KeyPair, Error> {
        match derivator {
            KeyDerivator::Ed25519 => Ok(KeyPair::Ed25519(Ed25519KeyPair::from_secret_key(
                &hex::decode(hex_key).unwrap(),
            ))),
            KeyDerivator::Secp256k1 => Ok(KeyPair::Secp256k1(Secp256k1KeyPair::from_secret_key(
                &hex::decode(hex_key).unwrap(),
            ))),
        }
    }
}

/// Generate key pair
#[allow(dead_code)]
pub fn generate<T: KeyGenerator + DSA + Into<KeyPair>>(seed: Option<&[u8]>) -> KeyPair {
    T::from_seed(seed.map_or(vec![].as_slice(), |x| x)).into()
}

/// Base for asymmetric key pair
#[derive(Default, Debug)]
pub struct BaseKeyPair<P> {
    pub public_key: P,
    pub secret_key: Option<EncryptedMem>,
}

impl<P> BaseKeyPair<P> {
    /// Decrypt secret key from encrypted memory.
    fn decrypt_secret_bytes(&self) -> Result<Vec<u8>, Error> {
        match &self.secret_key {
            Some(x) => {
                let bytes = x
                    .decrypt()
                    .map_err(|_| Error::KeyPairError("secret key decrypting".to_owned()))?;
                Ok(Vec::from(bytes.as_ref()))
            }
            None => Err(Error::KeyPairError(
                "secret key is not available".to_owned(),
            )),
        }
    }

    /// Encrypt secret key into encrypted memory.
    fn encrypt_secret_bytes(&mut self, secret_key: &[u8]) -> Result<(), Error> {
        let encr = self.secret_key.get_or_insert(EncryptedMem::new());
        encr.encrypt(&secret_key.to_vec())
            .map_err(|_| Error::KeyPairError("cannot encrypt the secret in memory".to_owned()))?;
        Ok(())
    }
}

/// Represents asymetric key pair for storage (deprecated: KeyPair is serializable)
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CryptoBox {
    pub public_key: Vec<u8>,
    pub secret_key: Vec<u8>,
}

/// Return key material bytes
pub trait KeyMaterial {
    /// Returns the public key bytes as slice
    fn public_key_bytes(&self) -> Vec<u8>;

    /// Returns the secret key bytes as slice
    fn secret_key_bytes(&self) -> Vec<u8>;

    /// Returns bytes from key pair
    fn to_bytes(&self) -> Vec<u8>;

    /// Returns DER from secret key.
    fn to_secret_der(&self) -> Result<Vec<u8>, Error>;

    /// Returns key pair from secret key der.
    fn from_secret_der(kp_type: KeyPairType, der: &[u8]) -> Result<Self, Error>
    where
        Self: Sized;

    /// Returns String from key pair encoded in base64
    fn to_str(&self) -> String {
        general_purpose::URL_SAFE_NO_PAD.encode(self.to_bytes())
    }
}

/// Collection of methods to initialize a key pair
/// using random or deterministic manner
pub trait KeyGenerator: KeyMaterial {
    /// Generates random keys
    fn new() -> Self
    where
        Self: Sized,
    {
        Self::from_seed(vec![].as_slice())
    }

    /// Generates keys deterministically using a given seed
    fn from_seed(seed: &[u8]) -> Self
    where
        Self: Sized;

    /// Generates keys from existing public key
    fn from_public_key(public_key: &[u8]) -> Self
    where
        Self: Sized;

    /// Generate keys from existing secret key
    fn from_secret_key(private_key: &[u8]) -> Self
    where
        Self: Sized;
}

/// Used for Digital Signature Algorithm (DSA)
pub trait DSA {
    /// Performs sign operation
    fn sign(&self, payload: Payload) -> Result<Vec<u8>, Error>;

    /// Performs verify operation
    fn verify(&self, payload: Payload, signature: &[u8]) -> Result<(), Error>;
}

/// Used for Diffie–Hellman key exchange operations
pub trait DHKE {
    /// Perform key exchange operation
    fn key_exchange(&self, their_public: &Self) -> Result<Vec<u8>, Error>;
}

/// Clone key pair
impl Clone for KeyPair {
    fn clone(&self) -> Self {
        match self {
            KeyPair::Ed25519(kp) => {
                KeyPair::Ed25519(Ed25519KeyPair::from_secret_key(&kp.secret_key_bytes()))
            }
            KeyPair::Secp256k1(kp) => {
                KeyPair::Secp256k1(Secp256k1KeyPair::from_secret_key(&kp.secret_key_bytes()))
            }
            // KeyPair::X25519(kp) => KeyPair::X25519(
            //     X25519KeyPair::from_secret_key(&kp.secret_key_bytes()),
            // ),
        }
    }
}

impl KeyMaterial for KeyPair {
    fn public_key_bytes(&self) -> Vec<u8> {
        match self {
            KeyPair::Ed25519(x) => x.public_key_bytes(),
            #[cfg(feature = "secp256k1")]
            KeyPair::Secp256k1(x) => x.public_key_bytes(),
            // #[cfg(feature = "x25519")]
            // KeyPair::X25519(x) => x.public_key_bytes(),
        }
    }

    fn secret_key_bytes(&self) -> Vec<u8> {
        match self {
            KeyPair::Ed25519(x) => x.secret_key_bytes(),
            #[cfg(feature = "secp256k1")]
            KeyPair::Secp256k1(x) => x.secret_key_bytes(),
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        match self {
            KeyPair::Ed25519(x) => x.to_bytes(),
            #[cfg(feature = "secp256k1")]
            KeyPair::Secp256k1(x) => x.to_bytes(),
        }
    }

    fn to_secret_der(&self) -> Result<Vec<u8>, Error> {
        match self {
            KeyPair::Ed25519(x) => x.to_secret_der(),
            #[cfg(feature = "secp256k1")]
            KeyPair::Secp256k1(x) => x.to_secret_der(),
        }
    }

    fn from_secret_der(kp_type: KeyPairType, der: &[u8]) -> Result<Self, Error>
    where
        Self: Sized,
    {
        match kp_type {
            KeyPairType::Ed25519 => Ok(KeyPair::Ed25519(Ed25519KeyPair::from_secret_der(kp_type, der)?)),
            KeyPairType::Secp256k1 => Ok(KeyPair::Secp256k1(Secp256k1KeyPair::from_secret_der(
                kp_type, der,
            )?)),
        }
    }
}

impl DSA for KeyPair {
    fn sign(&self, payload: Payload) -> Result<Vec<u8>, Error> {
        match self {
            KeyPair::Ed25519(x) => x.sign(payload),
            #[cfg(feature = "secp256k1")]
            KeyPair::Secp256k1(x) => x.sign(payload),
        }
    }

    fn verify(&self, payload: Payload, signature: &[u8]) -> Result<(), Error> {
        match self {
            KeyPair::Ed25519(x) => x.verify(payload, signature),
            #[cfg(feature = "secp256k1")]
            KeyPair::Secp256k1(x) => x.verify(payload, signature),
        }
    }
}

impl BorshSerialize for KeyPair {
    #[inline]
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        match &self {
            KeyPair::Ed25519(x) => {
                BorshSerialize::serialize(&0u8, writer)?;
                let a: [u8; 32] = x.secret_key_bytes().try_into().unwrap();
                BorshSerialize::serialize(&a, writer)
            }
            KeyPair::Secp256k1(x) => {
                BorshSerialize::serialize(&1u8, writer)?;
                let a: [u8; 32] = x.secret_key_bytes().try_into().unwrap();
                BorshSerialize::serialize(&a, writer)
            }
        }
    }
}

impl BorshDeserialize for KeyPair {
    #[inline]
    fn deserialize_reader<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        let order: u8 = BorshDeserialize::deserialize_reader(reader)?;
        match order {
            0 => {
                let data: [u8; 32] = BorshDeserialize::deserialize_reader(reader)?;
                Ok(KeyPair::Ed25519(Ed25519KeyPair::from_secret_key(&data)))
            }
            1 => {
                let data: [u8; 32] = BorshDeserialize::deserialize_reader(reader)?;
                Ok(KeyPair::Secp256k1(Secp256k1KeyPair::from_secret_key(&data)))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid Value representation: {}", order),
            )),
        }
    }
}

// impl DHKE for KeyPair {
//     fn key_exchange(&self, key: &Self) -> Result<Vec<u8>, Error> {
//         match (self, key) {
//             #[cfg(feature = "x25519")]
//             (KeyPair::X25519(me), KeyPair::X25519(them)) => {
//                 me.key_exchange(them)
//             }
//             _ => Err(Error::KeyPairError(
//                 "DHKE is not supported for this key type".to_owned(),
//             )),
//         }
//     }
// }

/// Payloads
#[derive(Debug, Clone)]
pub enum Payload {
    Buffer(Vec<u8>),
    #[allow(dead_code)]
    BufferArray(Vec<Vec<u8>>),
}

/// Creates 32 bytes seed
pub fn create_seed(initial_seed: &[u8]) -> Result<[u8; 32], Error> {
    let mut seed = [0u8; 32];
    if initial_seed.is_empty() {
        getrandom::getrandom(&mut seed)
            .map_err(|_| Error::SeedError("couldn't generate random seed".to_owned()))?;
    } else if initial_seed.len() <= 32 {
        seed[..initial_seed.len()].copy_from_slice(initial_seed);
    } else {
        return Err(Error::SeedError("seed is greater than 32".to_owned()));
    }
    Ok(seed)
}

#[cfg(test)]
mod tests {

    use super::{create_seed, ed25519::Ed25519KeyPair, generate, Payload, DSA};

    #[cfg(feature = "secp256k1")]
    use super::secp256k1::Secp256k1KeyPair;

    #[test]
    fn test_create_seed() {
        assert!(create_seed(vec![].as_slice()).is_ok());
        let seed = "48s8j34fuadfeuijakqp93d56829ki21".as_bytes();
        assert!(create_seed(seed).is_ok());
        let seed = "witness".as_bytes();
        assert!(create_seed(seed).is_ok());
        let seed = "witnesssdfasfasfasfsafsafasfsafsa".as_bytes();
        assert!(create_seed(seed).is_err());
    }

    #[test]
    fn test_ed25519() {
        let key_pair = generate::<Ed25519KeyPair>(None);
        let message = b"secret message";
        let signature = key_pair.sign(Payload::Buffer(message.to_vec())).unwrap();
        println!("Tamaño: {}", signature.len());
        let valid = key_pair.verify(Payload::Buffer(message.to_vec()), &signature);
        matches!(valid, Ok(()));
    }

    #[test]
    #[cfg(feature = "secp256k1")]
    fn test_secp256k1() {
        let key_pair = generate::<Secp256k1KeyPair>(None);
        let message = b"secret message";
        let signature = key_pair.sign(Payload::Buffer(message.to_vec())).unwrap();
        println!("Tamaño: {}", signature.len());
        let valid = key_pair.verify(Payload::Buffer(message.to_vec()), &signature);
        matches!(valid, Ok(()));
    }
}
