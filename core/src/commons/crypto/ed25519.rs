use super::{create_seed, BaseKeyPair, KeyGenerator, KeyMaterial, KeyPair, Payload, DHKE, DSA};

use crate::identifier;
use ed25519_dalek::pkcs8::DecodePrivateKey;
use ed25519_dalek::pkcs8::EncodePrivateKey;
use identifier::error::Error;

use ed25519_dalek::{
    Signature, Signer, SigningKey, Verifier, VerifyingKey, KEYPAIR_LENGTH, SECRET_KEY_LENGTH,
};

use base64::{engine::general_purpose, Engine as _};
use serde::{de::Deserializer, Deserialize, Serialize, Serializer};
use std::convert::TryFrom;
use std::convert::TryInto;

/// Ed25519 cryptographic key pair
pub type Ed25519KeyPair = BaseKeyPair<VerifyingKey>;

impl KeyGenerator for Ed25519KeyPair {
    fn from_seed(seed: &[u8]) -> Self {
        let secret_seed = create_seed(seed).expect("invalid seed");
        let sk = SigningKey::from_bytes(&secret_seed);
        let pk = sk.verifying_key();
        let mut kp = Self {
            public_key: pk,
            secret_key: None,
        };
        let _ = kp.encrypt_secret_bytes(&secret_seed);
        kp
    }

    fn from_public_key(public_key: &[u8]) -> Self {
        Self {
            public_key: VerifyingKey::try_from(public_key).expect("invalid public key"),
            secret_key: None,
        }
    }

    fn from_secret_key(secret_key: &[u8]) -> Ed25519KeyPair {
        let sk = SigningKey::try_from(secret_key).expect("cannot generate secret key");
        let pk = (&sk).try_into().expect("cannot generate public key");

        let mut kp = Ed25519KeyPair {
            secret_key: None,
            public_key: pk,
        };
        let _ = kp.encrypt_secret_bytes(secret_key);
        kp
    }
}

impl KeyMaterial for Ed25519KeyPair {
    fn public_key_bytes(&self) -> Vec<u8> {
        self.public_key.as_bytes().to_vec()
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
        let secet_bytes = self.decrypt_secret_bytes()?;
        let secret_key = SigningKey::try_from(secet_bytes.as_slice())?;
        let der = secret_key
            .to_pkcs8_der()
            .map_err(|_| Error::KeyPair("Cannot generate pkcs8 der".into()))?;
        Ok(der.as_bytes().to_vec())
    }

    fn from_secret_der(kp_type: super::KeyPairType, der: &[u8]) -> Result<Self, Error>
    where
        Self: Sized,
    {
        match kp_type {
            super::KeyPairType::Ed25519 => {
                let secret_key = SigningKey::from_pkcs8_der(der)
                    .map_err(|_| Error::KeyPair("Invalid pkcs8 der".into()))?;
                let public_key = VerifyingKey::from(&secret_key);
                let mut kp = Ed25519KeyPair {
                    public_key,
                    secret_key: None,
                };
                let _ = kp.encrypt_secret_bytes(secret_key.as_bytes());
                Ok(kp)
            }
            _ => Err(Error::KeyPair("Invalid key pair type".into())),
        }
    }
}

impl DSA for Ed25519KeyPair {
    fn sign(&self, payload: Payload) -> Result<Vec<u8>, Error> {
        let encr = self
            .secret_key
            .as_ref()
            .ok_or(Error::Sign("No secret key".into()))?;
        let sk = encr
            .decrypt()
            .map_err(|_| Error::Sign("Cannot decrypt secret key".into()))?;
        let signig_key = SigningKey::try_from(sk.as_ref())
            .map_err(|_| Error::Sign("Cannot generate signing key".into()))?;
        match payload {
            Payload::Buffer(msg) => Ok(signig_key.sign(msg.as_slice()).to_bytes().to_vec()),
            _ => Err(Error::Sign(
                "Payload type not supported for this key".into(),
            )),
        }
    }

    fn verify(&self, payload: Payload, signature: &[u8]) -> Result<(), Error> {
        let sig = Signature::try_from(signature)
            .map_err(|_| Error::Sign("Invalid signature data".into()))?;
        match payload {
            Payload::Buffer(payload) => match self.public_key.verify(payload.as_slice(), &sig) {
                Ok(_) => Ok(()),
                _ => Err(Error::Sign("Signature verify failed".into())),
            },
            _ => Err(Error::Sign(
                "Payload type not supported for this key".into(),
            )),
        }
    }
}

impl DHKE for Ed25519KeyPair {
    fn key_exchange(&self, _: &Self) -> Result<Vec<u8>, Error> {
        unimplemented!("ECDH is not supported for this key type")
    }
}

impl From<Ed25519KeyPair> for KeyPair {
    fn from(key_pair: Ed25519KeyPair) -> Self {
        KeyPair::Ed25519(key_pair)
    }
}

// Serde compatible Serialize
impl Serialize for Ed25519KeyPair {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_str())
    }
}

// Serde compatible Deserialize
impl<'de> Deserialize<'de> for Ed25519KeyPair {
    fn deserialize<D>(deserializer: D) -> Result<Ed25519KeyPair, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = general_purpose::URL_SAFE_NO_PAD
            .decode(s)
            .map_err(serde::de::Error::custom)?;
        Ok(Ed25519KeyPair::from_secret_key(&bytes[..SECRET_KEY_LENGTH]))
    }
}

#[cfg(test)]
mod tests {

    use super::{super::Payload, Ed25519KeyPair, KeyGenerator, DSA};

    #[test]
    fn test_ed25519() {
        let keys: Ed25519KeyPair = Ed25519KeyPair::new();
        test_signature(&keys);
        test_signature(&keys);
    }

    fn test_signature(keys: &Ed25519KeyPair) {
        let msg = b"sdfrasasfdasfsa";
        let payload = Payload::Buffer(msg.to_vec());
        let signature = keys.sign(payload.clone()).unwrap();
        let result = keys.verify(payload.clone(), &signature);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ser_des() {
        let msg = b"message";
        let kp = Ed25519KeyPair::new();
        let signature = kp.sign(Payload::Buffer(msg.to_vec())).unwrap();
        let kp_str = serde_json::to_string_pretty(&kp).unwrap();
        let new_kp: Result<Ed25519KeyPair, serde_json::Error> = serde_json::from_str(&kp_str);
        assert!(new_kp.is_ok());
        let result = new_kp
            .unwrap()
            .verify(Payload::Buffer(msg.to_vec()), &signature);
        assert!(result.is_ok());
    }

    #[test]
    fn test_der() {
        use crate::commons::crypto::{KeyMaterial, KeyPairType};
        let kp = Ed25519KeyPair::new();
        let der = kp.to_secret_der().unwrap();
        let new_kp = Ed25519KeyPair::from_secret_der(KeyPairType::Ed25519, &der).unwrap();
        assert_eq!(kp.public_key_bytes(), new_kp.public_key_bytes());
    }
}
