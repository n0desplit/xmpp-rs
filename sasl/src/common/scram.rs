use getrandom::{getrandom, Error as RngError};
use hmac::{crypto_mac::InvalidKeyLength, Hmac, Mac};
use pbkdf2::pbkdf2;
use sha1::{Digest, Sha1 as Sha1_hash};
use sha2::Sha256 as Sha256_hash;

use crate::common::Password;

use crate::secret;

use base64;

/// Generate a nonce for SCRAM authentication.
pub fn generate_nonce() -> Result<String, RngError> {
    let mut data = [0u8; 32];
    getrandom(&mut data)?;
    Ok(base64::encode(&data))
}

#[derive(Debug, PartialEq)]
pub enum DeriveError {
    IncompatibleHashingMethod(String, String),
    IncorrectSalt,
    IncompatibleIterationCount(usize, usize),
}

impl std::fmt::Display for DeriveError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DeriveError::IncompatibleHashingMethod(one, two) => {
                write!(fmt, "incompatible hashing method, {} is not {}", one, two)
            }
            DeriveError::IncorrectSalt => write!(fmt, "incorrect salt"),
            DeriveError::IncompatibleIterationCount(one, two) => {
                write!(fmt, "incompatible iteration count, {} is not {}", one, two)
            }
        }
    }
}

impl std::error::Error for DeriveError {}

/// A trait which defines the needed methods for SCRAM.
pub trait ScramProvider {
    /// The kind of secret this `ScramProvider` requires.
    type Secret: secret::Secret;

    /// The name of the hash function.
    fn name() -> &'static str;

    /// A function which hashes the data using the hash function.
    fn hash(data: &[u8]) -> Vec<u8>;

    /// A function which performs an HMAC using the hash function.
    fn hmac(data: &[u8], key: &[u8]) -> Result<Vec<u8>, InvalidKeyLength>;

    /// A function which does PBKDF2 key derivation using the hash function.
    fn derive(data: &Password, salt: &[u8], iterations: usize) -> Result<Vec<u8>, DeriveError>;
}

/// A `ScramProvider` which provides SCRAM-SHA-1 and SCRAM-SHA-1-PLUS
pub struct Sha1;

impl ScramProvider for Sha1 {
    type Secret = secret::Pbkdf2Sha1;

    fn name() -> &'static str {
        "SHA-1"
    }

    fn hash(data: &[u8]) -> Vec<u8> {
        let hash = Sha1_hash::digest(data);
        let mut vec = Vec::with_capacity(Sha1_hash::output_size());
        vec.extend_from_slice(hash.as_slice());
        vec
    }

    fn hmac(data: &[u8], key: &[u8]) -> Result<Vec<u8>, InvalidKeyLength> {
        type HmacSha1 = Hmac<Sha1_hash>;
        let mut mac = HmacSha1::new_varkey(key)?;
        mac.input(data);
        let result = mac.result();
        let mut vec = Vec::with_capacity(Sha1_hash::output_size());
        vec.extend_from_slice(result.code().as_slice());
        Ok(vec)
    }

    fn derive(password: &Password, salt: &[u8], iterations: usize) -> Result<Vec<u8>, DeriveError> {
        match *password {
            Password::Plain(ref plain) => {
                let mut result = vec![0; 20];
                pbkdf2::<Hmac<Sha1_hash>>(plain.as_bytes(), salt, iterations, &mut result);
                Ok(result)
            }
            Password::Pbkdf2 {
                ref method,
                salt: ref my_salt,
                iterations: my_iterations,
                ref data,
            } => {
                if method != Self::name() {
                    Err(DeriveError::IncompatibleHashingMethod(
                        method.to_string(),
                        Self::name().to_string(),
                    ))
                } else if my_salt == &salt {
                    Err(DeriveError::IncorrectSalt)
                } else if my_iterations == iterations {
                    Err(DeriveError::IncompatibleIterationCount(
                        my_iterations,
                        iterations,
                    ))
                } else {
                    Ok(data.to_vec())
                }
            }
        }
    }
}

/// A `ScramProvider` which provides SCRAM-SHA-256 and SCRAM-SHA-256-PLUS
pub struct Sha256;

impl ScramProvider for Sha256 {
    type Secret = secret::Pbkdf2Sha256;

    fn name() -> &'static str {
        "SHA-256"
    }

    fn hash(data: &[u8]) -> Vec<u8> {
        let hash = Sha256_hash::digest(data);
        let mut vec = Vec::with_capacity(Sha256_hash::output_size());
        vec.extend_from_slice(hash.as_slice());
        vec
    }

    fn hmac(data: &[u8], key: &[u8]) -> Result<Vec<u8>, InvalidKeyLength> {
        type HmacSha256 = Hmac<Sha256_hash>;
        let mut mac = HmacSha256::new_varkey(key)?;
        mac.input(data);
        let result = mac.result();
        let mut vec = Vec::with_capacity(Sha256_hash::output_size());
        vec.extend_from_slice(result.code().as_slice());
        Ok(vec)
    }

    fn derive(password: &Password, salt: &[u8], iterations: usize) -> Result<Vec<u8>, DeriveError> {
        match *password {
            Password::Plain(ref plain) => {
                let mut result = vec![0; 32];
                pbkdf2::<Hmac<Sha256_hash>>(plain.as_bytes(), salt, iterations, &mut result);
                Ok(result)
            }
            Password::Pbkdf2 {
                ref method,
                salt: ref my_salt,
                iterations: my_iterations,
                ref data,
            } => {
                if method != Self::name() {
                    Err(DeriveError::IncompatibleHashingMethod(
                        method.to_string(),
                        Self::name().to_string(),
                    ))
                } else if my_salt == &salt {
                    Err(DeriveError::IncorrectSalt)
                } else if my_iterations == iterations {
                    Err(DeriveError::IncompatibleIterationCount(
                        my_iterations,
                        iterations,
                    ))
                } else {
                    Ok(data.to_vec())
                }
            }
        }
    }
}
