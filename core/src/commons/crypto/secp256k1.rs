//! Adapter for pure Rust implementation of the secp256k1 curve and fast ECDSA signatures
//!

use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::pkcs8::DecodePrivateKey;
use serde::{de::Deserializer, Deserialize, Serialize, Serializer};

use crate::identifier;
use identifier::error::Error;

use super::{
    create_seed, BaseKeyPair, KeyGenerator, KeyMaterial, KeyPair, KeyPairType, Payload, DHKE, DSA,
};
use k256::{
    ecdsa::{
        signature::{Signer, Verifier},
        Signature, SigningKey, VerifyingKey,
    },
    pkcs8::EncodePrivateKey,
    SecretKey,
};
use sha2::{Digest, Sha256};

/// Secp256k1 cryptographic key pair
pub type Secp256k1KeyPair = BaseKeyPair<VerifyingKey>;

/// Defines constants
pub const SECRET_KEY_LENGTH: usize = 32;
pub const KEYPAIR_LENGTH: usize = 65;

/// Keys generation
impl KeyGenerator for Secp256k1KeyPair {
    fn from_seed(seed: &[u8]) -> Self {
        let secret_seed = create_seed(seed).expect("invalid seed");
        let sk = SigningKey::from_slice(&secret_seed).expect("invalid seed");
        let pk = VerifyingKey::from(&sk);
        let mut kp = Secp256k1KeyPair {
            public_key: pk,
            secret_key: None,
        };
        let _ = kp.encrypt_secret_bytes(&secret_seed);
        kp
    }

    fn from_public_key(pk: &[u8]) -> Self {
        let pk = VerifyingKey::from_sec1_bytes(pk).expect("Could not parse public key");
        Secp256k1KeyPair {
            secret_key: None,
            public_key: pk,
        }
    }

    fn from_secret_key(secret_key: &[u8]) -> Self {
        let sk = SigningKey::from_slice(secret_key).expect("Could not parse secret key");
        let pk = VerifyingKey::from(&sk);

        let mut kp = Secp256k1KeyPair {
            public_key: pk,
            secret_key: None,
        };
        let _ = kp.encrypt_secret_bytes(secret_key);
        kp
    }
}

impl KeyMaterial for Secp256k1KeyPair {
    fn public_key_bytes(&self) -> Vec<u8> {
        self.public_key.to_sec1_bytes().to_vec()
    }

    fn secret_key_bytes(&self) -> Vec<u8> {
        self.decrypt_secret_bytes().unwrap_or_default()
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes: [u8; KEYPAIR_LENGTH] = [0u8; KEYPAIR_LENGTH];
        bytes[..SECRET_KEY_LENGTH].copy_from_slice(&self.secret_key_bytes());
        bytes[SECRET_KEY_LENGTH..].copy_from_slice(&self.public_key_bytes());
        bytes.to_vec()
    }

    fn to_secret_der(&self) -> Result<Vec<u8>, Error> {
        let secret_bytes = self.decrypt_secret_bytes()?;
        let signing_key = SigningKey::try_from(secret_bytes.as_slice())
            .map_err(|_| Error::KeyPair("Cannot generate signing key".into()))?;
        let secret_key = SecretKey::from(signing_key);
        let der = secret_key
            .to_pkcs8_der()
            .map_err(|_| Error::KeyPair("pkcs8 serialize error".into()))?;
        Ok(der.as_bytes().to_vec())
    }

    fn from_secret_der(kp_type: super::KeyPairType, der: &[u8]) -> Result<Self, Error>
    where
        Self: Sized,
    {
        match kp_type {
            KeyPairType::Secp256k1 => {
                let secret_key = SecretKey::from_pkcs8_der(der)
                    .map_err(|_| Error::KeyPair("pkcs8 deserialize error".into()))?;
                let signing_key = SigningKey::from(secret_key);
                let public_key = VerifyingKey::from(&signing_key);
                let mut kp = Secp256k1KeyPair {
                    public_key,
                    secret_key: None,
                };
                let _ = kp.encrypt_secret_bytes(&signing_key.to_bytes());
                Ok(kp)
            }
            _ => Err(Error::KeyPair("Key type not supported".into())),
        }
    }
}

impl DSA for Secp256k1KeyPair {
    fn sign(&self, payload: Payload) -> Result<Vec<u8>, Error> {
        let encr = self
            .secret_key
            .as_ref()
            .ok_or(Error::Sign("No secret key".into()))?;
        let sk = encr
            .decrypt()
            .map_err(|_| Error::Sign("Cannot decrypt secret key".into()))?;
        let signing_key = SigningKey::try_from(sk.as_ref())
            .map_err(|_| Error::Sign("Cannot generate signing key".into()))?;

        match payload {
            Payload::Buffer(payload) => {
                let message = get_hash(&payload);
                let signature: Signature = signing_key.sign(&message);
                Ok(signature.to_bytes().to_vec())
            }
            _ => Err(Error::Sign(
                "Payload type not supported for this key".into(),
            )),
        }
    }

    fn verify(&self, payload: Payload, signature: &[u8]) -> Result<(), Error> {
        let verified = match payload {
            Payload::Buffer(payload) => {
                let message = get_hash(&payload);
                let signature = Signature::from_slice(signature).expect("Couldn't parse signature");

                self.public_key.verify(&message, &signature).is_ok()
            }
            _ => unimplemented!("payload type not supported for this key"),
        };

        if verified {
            Ok(())
        } else {
            Err(Error::Sign("Signature verify failed".into()))
        }
    }
}

impl DHKE for Secp256k1KeyPair {
    fn key_exchange(&self, _: &Self) -> Result<Vec<u8>, Error> {
        unimplemented!("ECDH is not supported for this key type")
    }
}

/// Serde compatible Serialize
impl Serialize for Secp256k1KeyPair {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_str())
    }
}

/// Serde compatible Deserialize
impl<'de> Deserialize<'de> for Secp256k1KeyPair {
    fn deserialize<D>(deserializer: D) -> Result<Secp256k1KeyPair, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = general_purpose::URL_SAFE_NO_PAD
            .decode(s)
            .map_err(serde::de::Error::custom)?;

        Ok(Secp256k1KeyPair::from_secret_key(
            &bytes[..SECRET_KEY_LENGTH],
        ))
    }
}

fn get_hash(payload: &[u8]) -> [u8; 32] {
    let hash = Sha256::digest(payload);
    let mut output = [0u8; 32];
    output.copy_from_slice(&hash[..32]);
    output
}

impl From<Secp256k1KeyPair> for KeyPair {
    fn from(key_pair: Secp256k1KeyPair) -> Self {
        KeyPair::Secp256k1(key_pair)
    }
}

#[cfg(test)]
mod tests {

    use super::Secp256k1KeyPair;
    use crate::commons::crypto::{KeyGenerator, KeyMaterial, KeyPairType, Payload, DSA};

    #[test]
    fn test_ser_des() {
        let msg = b"message";
        let kp = Secp256k1KeyPair::new();
        let signature = kp.sign(Payload::Buffer(msg.to_vec())).unwrap();
        let kp_str = serde_json::to_string_pretty(&kp).unwrap();
        let new_kp: Result<Secp256k1KeyPair, serde_json::Error> = serde_json::from_str(&kp_str);
        assert!(new_kp.is_ok());
        let result = new_kp
            .unwrap()
            .verify(Payload::Buffer(msg.to_vec()), &signature);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sign_verify() {
        let msg = b"message";
        let kp = Secp256k1KeyPair::new();
        let signature = kp.sign(Payload::Buffer(msg.to_vec())).unwrap();
        let result = kp.verify(Payload::Buffer(msg.to_vec()), &signature);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sign_verify_fail() {
        let msg = b"message";
        let kp = Secp256k1KeyPair::new();
        let signature = kp.sign(Payload::Buffer(msg.to_vec())).unwrap();
        let result = kp.verify(Payload::Buffer(b"message2".to_vec()), &signature);
        assert!(result.is_err());
    }

    #[test]
    fn test_der() {
        let kp = Secp256k1KeyPair::new();
        let der = kp.to_secret_der().unwrap();
        //let pki = PrivateKeyInfo::try_from(der.as_slice()).unwrap();
        //let secret = pki.encrypt(rand::rngs::OsRng, b"example").unwrap();
        //let epki = EncryptedPrivateKeyInfo::try_from(secret.as_bytes()).unwrap();
        //let der = epki.decrypt( b"example").unwrap();
        let new_kp = Secp256k1KeyPair::from_secret_der(KeyPairType::Secp256k1, &der).unwrap();
        assert_eq!(kp.public_key_bytes(), new_kp.public_key_bytes());
    }
}
