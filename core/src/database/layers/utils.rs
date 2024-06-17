//

use chacha20poly1305::{aead::Aead, AeadCore, ChaCha20Poly1305, KeyInit, Nonce};
use rand::rngs::OsRng;

use crate::DbError;

const MAX_U64: usize = 17; // Max size u64

/// Nonce size.
const NONCE_SIZE: usize = 12;

pub(crate) enum Element {
    N(u64),
    S(String),
}

fn get_u64_as_hexadecimal(value: u64) -> String {
    format!("{:0width$}", format!("{:016x}", value), width = MAX_U64)
}

pub(crate) fn get_key(key_elements: Vec<Element>) -> Result<String, DbError> {
    if !key_elements.is_empty() {
        let mut key: String = String::from("");
        for key_element in key_elements.iter().take(key_elements.len() - 1) {
            key.push_str(&{
                match key_element {
                    Element::N(n) => get_u64_as_hexadecimal(*n),
                    Element::S(s) => s.to_string(),
                }
            });
            key.push_str(&char::MAX.to_string());
        }
        key.push_str(&{
            match &key_elements[key_elements.len() - 1] {
                Element::N(n) => get_u64_as_hexadecimal(*n),
                Element::S(s) => s.to_string(),
            }
        });
        Ok(key.to_owned())
    } else {
        Err(DbError::KeyElementsError)
    }
}

/// Encrypt bytes.
///
pub fn encrypt(key: &[u8], bytes: &[u8]) -> Result<Vec<u8>, DbError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng); // 96-bits; unique per message
    let ciphertext: Vec<u8> = cipher
        .encrypt(&nonce, bytes.as_ref())
        .map_err(|e| DbError::Encrypt(format!("Encrypt error: {}", e)))?;

    Ok([nonce.to_vec(), ciphertext].concat())
}

/// Decrypt bytes
///
pub fn decrypt(key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, DbError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce: [u8; 12] = ciphertext[..NONCE_SIZE]
        .try_into()
        .map_err(|e| DbError::Nonce(format!("Nonce error: {}", e)))?;
    let nonce = Nonce::from_slice(&nonce);
    let ciphertext = &ciphertext[NONCE_SIZE..];
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| DbError::Decrypt(format!("Decrypt error: {}", e)))?;
    Ok(plaintext)
}

/*pub(crate) fn get_by_range<C: DatabaseCollection>(
    from: Option<String>,
    quantity: isize,
    collection: &C,
    prefix: &str,
) -> Result<Vec<Vec<u8>>, DbError> {
    fn convert<'a>(
        iter: impl Iterator<Item = (String, Vec<u8>)> + 'a,
    ) -> Box<dyn Iterator<Item = (String, Vec<u8>)> + 'a> {
        Box::new(iter)
    }
    let (mut iter, quantity) = match from {
        Some(key) => {
            // Find the key
            let iter = if quantity >= 0 {
                collection.iter(false, prefix)
            } else {
                collection.iter(true, format!("{}{}", prefix, char::MAX).as_str())
            };
            let mut iter = iter.peekable();
            loop {
                let Some((current_key, _)) = iter.peek() else {
                    return Err(DbError::EntryNotFound);
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
                (collection.iter(false, prefix), quantity)
            } else {
                (collection.iter(true, prefix), quantity.abs())
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
}*/
