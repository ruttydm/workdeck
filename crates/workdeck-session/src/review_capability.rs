//! Cryptographically random per-process capability for one live review session.

use std::sync::OnceLock;
use workdeck_core::review_digest;

use crate::{
    BrokerCryptoError, NativeSessionBrokerCrypto, REVIEW_CAPABILITY_ENTROPY_BYTES, ReviewUrlError,
    SessionBrokerCrypto, encode_base64_url, review_url,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewCapability {
    pub token: String,
    pub digest: String,
}

/// Mint a capability from platform cryptographic randomness and publish only its digest.
pub fn create_review_capability() -> Result<ReviewCapability, BrokerCryptoError> {
    let bytes = NativeSessionBrokerCrypto.random_bytes(REVIEW_CAPABILITY_ENTROPY_BYTES)?;
    let token = encode_base64_url(&bytes);
    Ok(ReviewCapability {
        digest: review_digest(token.as_bytes()),
        token,
    })
}

static PROCESS_CAPABILITY: OnceLock<ReviewCapability> = OnceLock::new();

/// Lazily mint this process's stable review capability.
pub fn review_process_capability() -> Result<&'static ReviewCapability, BrokerCryptoError> {
    if let Some(capability) = PROCESS_CAPABILITY.get() {
        return Ok(capability);
    }
    let candidate = create_review_capability()?;
    let _ = PROCESS_CAPABILITY.set(candidate);
    Ok(PROCESS_CAPABILITY
        .get()
        .expect("the process capability was initialized or won by another thread"))
}

#[derive(Debug, thiserror::Error)]
pub enum ReviewProcessUrlError {
    #[error(transparent)]
    Crypto(#[from] BrokerCryptoError),
    #[error(transparent)]
    Url(#[from] ReviewUrlError),
}

/// Browser URL for this process's live session.
pub fn review_process_url(origin: &str, session_id: &str) -> Result<String, ReviewProcessUrlError> {
    Ok(review_url(
        origin,
        session_id,
        &review_process_capability()?.token,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{REVIEW_CAPABILITY_TOKEN_LENGTH, is_review_capability_token};

    #[test]
    fn minting_uses_256_bits_and_a_sha256_digest() {
        let first = create_review_capability().unwrap();
        let second = create_review_capability().unwrap();
        assert_eq!(first.token.len(), REVIEW_CAPABILITY_TOKEN_LENGTH);
        assert!(is_review_capability_token(&first.token));
        assert_eq!(first.digest, review_digest(first.token.as_bytes()));
        assert_eq!(first.digest.len(), 64);
        assert_ne!(first.token, second.token);
    }

    #[test]
    fn process_capability_is_lazy_stable_and_shared_with_its_url() {
        let first = review_process_capability().unwrap();
        let second = review_process_capability().unwrap();
        assert!(std::ptr::eq(first, second));
        let url = review_process_url("http://127.0.0.1:4300", "session-1").unwrap();
        assert!(url.ends_with(&format!("#capability={}", first.token)));
    }
}
