use super::{create_seed, BaseKeyPair, KeyGenerator, KeyMaterial, KeyPair, Payload, DHKE, DSA};

use crate::identifier;
use identifier::error::Error;

use ed25519_dalek::{
    SigningKey, VerifyingKey, Signer, Signature, Verifier, KEYPAIR_LENGTH, SECRET_KEY_LENGTH,
};

use base64::{Engine as _, engine::general_purpose};
use serde::{de::Deserializer, Deserialize, Serialize, Serializer};
use std::convert::TryFrom;
use std::convert::TryInto;

/// Ed25519 cryptographic key pair
pub type Ed25519KeyPair = BaseKeyPair<VerifyingKey, SigningKey>;

impl KeyGenerator for Ed25519KeyPair {
    fn from_seed(seed: &[u8]) -> Self {
        let secret_seed = create_seed(seed).expect("invalid seed");
        let sk = SigningKey::from_bytes(&secret_seed);
        let pk = sk.verifying_key();
        Self {
            public_key: pk,
            secret_key: Some(sk),
        }
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

        Ed25519KeyPair {
            secret_key: Some(sk),
            public_key: pk,
        }
    }
}

impl KeyMaterial for Ed25519KeyPair {
    fn public_key_bytes(&self) -> Vec<u8> {
        self.public_key.as_bytes().to_vec()
    }

    fn secret_key_bytes(&self) -> Vec<u8> {
        self.secret_key
            .as_ref()
            .map_or(vec![], |x| x.to_bytes().to_vec())
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes: [u8; KEYPAIR_LENGTH] = [0u8; KEYPAIR_LENGTH];
        bytes[..SECRET_KEY_LENGTH].copy_from_slice(&self.secret_key_bytes());
        bytes[SECRET_KEY_LENGTH..].copy_from_slice(&self.public_key_bytes());
        bytes.to_vec()
    }
}

impl DSA for Ed25519KeyPair {
    fn sign(&self, payload: Payload) -> Result<Vec<u8>, Error> {
        let sk = self
            .secret_key
            .as_ref()
            .ok_or(Error::SignError("No secret key".into()))?;
        match payload {
            Payload::Buffer(msg) => Ok(sk.sign(msg.as_slice()).to_bytes().to_vec()),
            _ => Err(Error::SignError(
                "Payload type not supported for this key".into(),
            )),
        }
    }

    fn verify(&self, payload: Payload, signature: &[u8]) -> Result<(), Error> {
        let sig = Signature::try_from(signature)
            .map_err(|_| Error::SignError("Invalid signature data".into()))?;
        match payload {
            Payload::Buffer(payload) => match self.public_key.verify(payload.as_slice(), &sig) {
                Ok(_) => Ok(()),
                _ => Err(Error::SignError("Signature verify failed".into())),
            },
            _ => Err(Error::SignError(
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
        let bytes = general_purpose::URL_SAFE_NO_PAD.decode(&s).map_err(serde::de::Error::custom)?;

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
}
