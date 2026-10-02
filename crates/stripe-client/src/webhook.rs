//! Verification of the `Stripe-Signature` header.
//!
//! Stripe signs `"{timestamp}.{raw_body}"` with HMAC-SHA256 using the
//! endpoint's signing secret. The header looks like
//! `t=1700000000,v1=<hex>,v1=<hex>,v0=<hex>`; any matching `v1` passes.
//! Always verify against the raw request bytes, before any JSON parsing.

use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;
use subtle::ConstantTimeEq;

/// Stripe's recommended tolerance between the signed timestamp and now.
pub const DEFAULT_TOLERANCE_SECS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    #[error("missing or malformed Stripe-Signature header")]
    Malformed,
    #[error("no v1 signature matched")]
    Mismatch,
    #[error("timestamp outside tolerance")]
    Expired,
}

/// Verifies `payload` against `header` using any of `secrets` (more than one
/// during secret rotation). `now` is unix seconds, passed in for testability.
pub fn verify(
    payload: &[u8],
    header: &str,
    secrets: &[SecretString],
    now: i64,
    tolerance_secs: i64,
) -> Result<(), SignatureError> {
    let mut timestamp: Option<i64> = None;
    let mut signatures: Vec<Vec<u8>> = Vec::new();
    for part in header.split(',') {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        match key {
            "t" => timestamp = value.parse().ok(),
            "v1" => {
                if let Ok(sig) = hex::decode(value) {
                    signatures.push(sig);
                }
            }
            _ => {}
        }
    }
    let timestamp = timestamp.ok_or(SignatureError::Malformed)?;
    if signatures.is_empty() || secrets.is_empty() {
        return Err(SignatureError::Malformed);
    }

    let matched = secrets.iter().any(|secret| {
        let expected = sign(secret.expose_secret().as_bytes(), timestamp, payload);
        signatures
            .iter()
            .any(|sig| bool::from(sig.as_slice().ct_eq(expected.as_slice())))
    });
    if !matched {
        return Err(SignatureError::Mismatch);
    }
    if (now - timestamp).abs() > tolerance_secs {
        return Err(SignatureError::Expired);
    }
    Ok(())
}

fn sign(secret: &[u8], timestamp: i64, payload: &[u8]) -> Vec<u8> {
    // HMAC accepts keys of any length, so this cannot fail.
    let mut mac = <Hmac<Sha256>>::new_from_slice(secret).unwrap_or_else(|_| unreachable!());
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(payload);
    mac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "whsec_test_secret";
    const BODY: &[u8] = br#"{"id":"evt_1","type":"invoice.paid"}"#;
    const T: i64 = 1_700_000_000;

    fn header_for(secret: &str, t: i64, body: &[u8]) -> String {
        format!("t={t},v1={}", hex::encode(sign(secret.as_bytes(), t, body)))
    }

    fn secrets(list: &[&str]) -> Vec<SecretString> {
        list.iter().map(|s| SecretString::from(s.to_string())).collect()
    }

    #[test]
    fn accepts_valid_signature() {
        let h = header_for(SECRET, T, BODY);
        assert_eq!(
            verify(BODY, &h, &secrets(&[SECRET]), T + 10, DEFAULT_TOLERANCE_SECS),
            Ok(())
        );
    }

    #[test]
    fn accepts_any_secret_during_rotation_and_any_v1() {
        let good = hex::encode(sign(SECRET.as_bytes(), T, BODY));
        let h = format!("t={T},v1=deadbeef,v1={good},v0=ignored");
        assert_eq!(
            verify(BODY, &h, &secrets(&["whsec_old", SECRET]), T, DEFAULT_TOLERANCE_SECS),
            Ok(())
        );
    }

    #[test]
    fn rejects_tampered_body() {
        let h = header_for(SECRET, T, BODY);
        let tampered = br#"{"id":"evt_1","type":"invoice.paid","x":1}"#;
        assert_eq!(
            verify(tampered, &h, &secrets(&[SECRET]), T, DEFAULT_TOLERANCE_SECS),
            Err(SignatureError::Mismatch)
        );
    }

    #[test]
    fn rejects_wrong_secret() {
        let h = header_for("whsec_other", T, BODY);
        assert_eq!(
            verify(BODY, &h, &secrets(&[SECRET]), T, DEFAULT_TOLERANCE_SECS),
            Err(SignatureError::Mismatch)
        );
    }

    #[test]
    fn rejects_old_timestamp() {
        let h = header_for(SECRET, T, BODY);
        assert_eq!(
            verify(BODY, &h, &secrets(&[SECRET]), T + 301, DEFAULT_TOLERANCE_SECS),
            Err(SignatureError::Expired)
        );
    }

    #[test]
    fn rejects_malformed_header() {
        for h in ["", "t=abc,v1=00", "v1=00", "t=1700000000", "garbage"] {
            assert_eq!(
                verify(BODY, h, &secrets(&[SECRET]), T, DEFAULT_TOLERANCE_SECS),
                Err(SignatureError::Malformed),
                "{h}"
            );
        }
    }
}
