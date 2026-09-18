//! Native Ed25519, SHA-256, random-byte, and canonical base64url primitives for the broker.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::pkcs8::{DecodePrivateKey, DecodePublicKey};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrokerCryptoError {
    #[error("secure random byte generation failed: {0}")]
    Random(String),
    #[error("invalid Ed25519 private key: {0}")]
    PrivateKey(String),
    #[error("invalid Ed25519 public key: {0}")]
    PublicKey(String),
}

#[must_use]
pub fn encode_base64_url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

#[must_use]
pub fn decode_base64_url(value: &str) -> Option<Vec<u8>> {
    if value.len() % 4 == 1
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(value).ok()?;
    (encode_base64_url(&bytes) == value).then_some(bytes)
}

pub trait SessionBrokerCrypto: Send + Sync {
    fn random_bytes(&self, length: usize) -> Result<Vec<u8>, BrokerCryptoError>;
    fn sha256(&self, value: &[u8]) -> [u8; 32];
    fn sign(&self, private_key: &SigningKey, value: &[u8]) -> Vec<u8>;
    fn verify(&self, public_key: &VerifyingKey, signature: &[u8], value: &[u8]) -> bool;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NativeSessionBrokerCrypto;

impl SessionBrokerCrypto for NativeSessionBrokerCrypto {
    fn random_bytes(&self, length: usize) -> Result<Vec<u8>, BrokerCryptoError> {
        let mut bytes = vec![0_u8; length];
        getrandom::fill(&mut bytes)
            .map_err(|error| BrokerCryptoError::Random(error.to_string()))?;
        Ok(bytes)
    }

    fn sha256(&self, value: &[u8]) -> [u8; 32] {
        Sha256::digest(value).into()
    }

    fn sign(&self, private_key: &SigningKey, value: &[u8]) -> Vec<u8> {
        private_key.sign(value).to_bytes().to_vec()
    }

    fn verify(&self, public_key: &VerifyingKey, signature: &[u8], value: &[u8]) -> bool {
        let Ok(signature) = Signature::from_slice(signature) else {
            return false;
        };
        public_key.verify(value, &signature).is_ok()
    }
}

pub fn import_ed25519_public_key(spki: &[u8]) -> Result<VerifyingKey, BrokerCryptoError> {
    VerifyingKey::from_public_key_der(spki)
        .map_err(|error| BrokerCryptoError::PublicKey(error.to_string()))
}

pub fn import_ed25519_private_key(pkcs8: &[u8]) -> Result<SigningKey, BrokerCryptoError> {
    SigningKey::from_pkcs8_der(pkcs8)
        .map_err(|error| BrokerCryptoError::PrivateKey(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BrokerChallengeTranscriptInput, BrokerHelloProposal, BrokerRole,
        CallerRequestTranscriptInput, build_broker_challenge_transcript,
        build_caller_request_transcript,
    };

    fn hex(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn round_trips_only_canonical_unpadded_base64url() {
        assert_eq!(encode_base64_url(&[]), "");
        assert_eq!(decode_base64_url(""), Some(vec![]));
        assert_eq!(decode_base64_url("A"), None);
        assert_eq!(decode_base64_url("AA=="), None);
        assert_eq!(decode_base64_url("AB"), None);
        assert_eq!(decode_base64_url("AA"), Some(vec![0]));
    }

    #[test]
    fn matches_golden_transcript_signatures_and_sha256_fixture() {
        let mut private_der = hex("302e020100300506032b657004220420");
        private_der.extend(hex(
            "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        ));
        let mut public_der = hex("302a300506032b6570032100");
        public_der.extend(hex(
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        ));
        let private_key = import_ed25519_private_key(&private_der).unwrap();
        let public_key = import_ed25519_public_key(&public_der).unwrap();
        let crypto = NativeSessionBrokerCrypto;

        let transcript = build_broker_challenge_transcript(&BrokerChallengeTranscriptInput {
            role: BrokerRole::Caller,
            app_id: "dev.example".into(),
            generation: "generation-1".into(),
            endpoint: "http://127.0.0.1:47657/broker".into(),
            key_id: "caller-key-1".into(),
            grant_id: "caller-grant-1".into(),
            initiator_nonce: "nonce-a".into(),
            responder_nonce: "nonce-b".into(),
            proposal: BrokerHelloProposal {
                broker_revision: 1,
                app_revision: 7,
                features: vec!["z".into(), "a".into()],
            },
        })
        .unwrap();
        let signature = crypto.sign(&private_key, &transcript);
        assert_eq!(
            encode_base64_url(&signature),
            "65b1jMlE7LV9Mc8o6QjXS2fbl3aaGvgs_2cjNVdpW86yd9xdWOg0BtgW9_UWX5olXcPLOXhYEkUT2DhfYAq-Bw"
        );
        assert!(crypto.verify(&public_key, &signature, &transcript));
        assert!(!crypto.verify(&public_key, &[0; 63], &transcript));
        assert_eq!(
            encode_base64_url(&crypto.sha256(br#"{"action":"list"}"#)),
            "WE52AIFcTHUuQvjzwAqkvxWX8TuwjtXeRDszCX4aF1E"
        );

        let request = build_caller_request_transcript(&CallerRequestTranscriptInput {
            app_id: "dev.example".into(),
            generation: "generation-1".into(),
            caller_session_id: "caller-session-1".into(),
            key_id: "caller-key-1".into(),
            grant_id: "caller-grant-1".into(),
            hello_transcript_hash: "hello-hash".into(),
            method: "post".into(),
            target: "/broker?a=1&b=2".into(),
            body_digest: "body-hash".into(),
            request_id: "request-1".into(),
            sequence: "1".into(),
        })
        .unwrap();
        assert_eq!(
            encode_base64_url(&crypto.sign(&private_key, &request)),
            "wifCJmPnrVabHInagsfe_6B-jVNRU0FE1Hcw6RmJmcUF9CUoGj15KZ-gyqlmj5GlGi-hTFWTR_LC0SHE2E3cAg"
        );
    }

    #[test]
    fn random_bytes_return_the_requested_length() {
        let first = NativeSessionBrokerCrypto.random_bytes(32).unwrap();
        let second = NativeSessionBrokerCrypto.random_bytes(32).unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(second.len(), 32);
        assert_ne!(first, second);
    }
}
