//

use super::{errors::ProtocolErrors, models::HashId};
use crate::{
    commons::{models::signature::Signature, settings::Settings},
    identifier::{derive::digest::DigestDerivator, KeyIdentifier},
    keys::{KeyMaterial, KeyPair},
};

/// Self signature trait.
///
pub trait SelfSignature {
    /*
        /// Change settings.
        ///
        /// # Arguments
        ///
        /// * `settings` - Settings.
        ///
        /// # Returns
        ///
        /// * `()` - Returns nothing.
        ///
        fn change_settings(&mut self, settings: &Settings);
    */
    /// Get own identifier.
    ///
    /// # Returns
    ///
    /// * `KeyIdentifier` - Returns owned key identifier.
    fn get_own_identifier(&self) -> KeyIdentifier;

    /// Sign content.
    ///
    /// # Arguments
    ///
    /// * `content` - Content to sign.
    /// * `derivator` - Digest derivator.
    ///
    /// # Returns
    ///
    /// * `Result<Signature, ProtocolErrors>` - Returns signature or error.
    ///
    fn sign<T: HashId>(
        &self,
        content: &T,
        derivator: DigestDerivator,
    ) -> Result<Signature, ProtocolErrors>;

    // /// Check if signature present.
    //fn check_if_signature_present(&self, signers: &HashSet<KeyIdentifier>) -> bool;
}

/// Self signature manager.
#[derive(Clone, Debug)]
pub struct SelfSignatureManager {
    /// Owned key pair.
    pub keys: KeyPair,
    /// Derived key identifier.
    pub identifier: KeyIdentifier,
    /// Digest derivator.
    pub digest_derivator: DigestDerivator,
}

/// Implementation of self signature manager.
impl SelfSignatureManager {
    /// Create new self signature manager.
    ///
    /// # Arguments
    ///
    /// * `keys` - Key pair.
    /// * `settings` - Settings.
    ///
    /// # Returns
    ///
    /// * `SelfSignatureManager` - Returns self signature manager.
    ///
    pub fn new(keys: KeyPair, settings: &Settings) -> Self {
        let identifier = KeyIdentifier::new(keys.get_key_derivator(), &keys.public_key_bytes());
        Self {
            keys,
            identifier,
            digest_derivator: settings.node.digest_derivator,
        }
    }
}

/// Implementation of self signature trait.
impl SelfSignature for SelfSignatureManager {
    //fn change_settings(&mut self, settings: &Settings) {
    //    self.digest_derivator = settings.node.digest_derivator;
    //}

    fn get_own_identifier(&self) -> KeyIdentifier {
        self.identifier.clone()
    }

    fn sign<T: HashId>(
        &self,
        content: &T,
        derivator: DigestDerivator,
    ) -> Result<Signature, ProtocolErrors> {
        Signature::new(content, &self.keys, derivator).map_err(|_| ProtocolErrors::SignatureError)
    }

    //fn check_if_signature_present(&self, signers: &HashSet<KeyIdentifier>) -> bool {
    //    signers.contains(&self.identifier)
    //}
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::commons::models::test::Content;
    use crate::commons::settings::Settings;
    use crate::keys::{Ed25519KeyPair, KeyGenerator, KeyPair};

    #[test]
    fn test_self_signature_manager() {
        let settings = Settings::default();
        let keypair = KeyPair::Ed25519(Ed25519KeyPair::new());
        let self_signature_manager = SelfSignatureManager::new(keypair, &settings);
        let content = Content(String::from("test"));
        let signature = self_signature_manager
            .sign(&content, self_signature_manager.digest_derivator)
            .unwrap();
        assert!(signature.verify(&content).is_ok());
    }
}
