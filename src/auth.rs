//! Authentication and session management.
//!
//! Handles user sessions with HMAC-signed cookies. Authentication is optional
//! and enabled by setting the RECIPES_PASSWORD environment variable.

use axum_extra::extract::CookieJar;
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::Utc;
use hmac::{Hmac, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::env;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub const SESSION_COOKIE: &str = "recipes_session";
pub const SESSION_TTL_HOURS: i64 = 24;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Session {
    created: i64,
    expires: i64,
    nonce: String,
}

/// Get the secret key from environment (RECIPES_PASSWORD).
pub fn get_secret_key() -> Option<Vec<u8>> {
    env::var("RECIPES_PASSWORD").ok().map(|p| p.into_bytes())
}

/// Check if proxy-level auth is trusted (e.g., behind Authelia).
/// When TRUST_PROXY_AUTH is set, all requests are treated as authenticated.
fn trust_proxy_auth() -> bool {
    env::var("TRUST_PROXY_AUTH").is_ok()
}

/// Check if authentication is enabled.
pub fn is_auth_enabled() -> bool {
    trust_proxy_auth() || get_secret_key().is_some()
}

/// Create a new session token.
pub fn create_session() -> Option<String> {
    let secret = get_secret_key()?;
    let now = Utc::now().timestamp();
    let expires = now + (SESSION_TTL_HOURS * 3600);
    let nonce: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();

    let session = Session {
        created: now,
        expires,
        nonce,
    };
    let session_json = serde_json::to_string(&session).ok()?;

    let mut mac = HmacSha256::new_from_slice(&secret).ok()?;
    mac.update(session_json.as_bytes());
    let signature = hex_encode(mac.finalize().into_bytes().as_slice());

    Some(format!("{}.{}", base64_encode(&session_json), signature))
}

/// Verify a session token.
pub fn verify_session(token: &str, secret: &[u8]) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        return false;
    }

    let session_json = match base64_decode(parts[0]) {
        Some(s) => s,
        None => return false,
    };

    let mut mac = match HmacSha256::new_from_slice(secret) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(session_json.as_bytes());
    let expected_sig = hex_encode(mac.finalize().into_bytes().as_slice());

    let sig_bytes = parts[1].as_bytes();
    let expected_bytes = expected_sig.as_bytes();
    if sig_bytes.len() != expected_bytes.len() {
        return false;
    }
    if sig_bytes.ct_eq(expected_bytes).unwrap_u8() != 1 {
        return false;
    }

    let session: Session = match serde_json::from_str(&session_json) {
        Ok(s) => s,
        Err(_) => return false,
    };

    Utc::now().timestamp() < session.expires
}

/// Check if the user is logged in via cookie.
pub fn is_logged_in(jar: &CookieJar) -> bool {
    if trust_proxy_auth() {
        return true;
    }

    let secret = match get_secret_key() {
        Some(s) => s,
        None => return false,
    };

    match jar.get(SESSION_COOKIE) {
        Some(cookie) => verify_session(cookie.value(), &secret),
        None => false,
    }
}

pub fn base64_encode(s: &str) -> String {
    STANDARD.encode(s.as_bytes())
}

pub fn base64_decode(s: &str) -> Option<String> {
    let bytes = STANDARD.decode(s).ok()?;
    String::from_utf8(bytes).ok()
}

pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_base64_roundtrip() {
        let original = r#"{"created":1000,"expires":2000,"nonce":"abc123"}"#;
        let encoded = base64_encode(original);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn test_session_create_and_verify() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::set_var("RECIPES_PASSWORD", "test_secret_123");
        let token = create_session().unwrap();
        let secret = get_secret_key().unwrap();
        assert!(verify_session(&token, &secret));
        env::remove_var("RECIPES_PASSWORD");
    }

    #[test]
    fn test_session_tampered() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::set_var("RECIPES_PASSWORD", "test_secret_456");
        let token = create_session().unwrap();
        let secret = get_secret_key().unwrap();

        // Tamper with the signature
        let tampered = format!("{}x", token);
        assert!(!verify_session(&tampered, &secret));

        // Tamper with payload
        let parts: Vec<&str> = token.split('.').collect();
        let tampered_payload = format!("{}aa.{}", parts[0], parts[1]);
        assert!(!verify_session(&tampered_payload, &secret));
        env::remove_var("RECIPES_PASSWORD");
    }

    #[test]
    fn test_session_expired() {
        let secret = b"test_expired";
        let session = Session {
            created: 1000,
            expires: 1001, // long expired
            nonce: "test".to_string(),
        };
        let session_json = serde_json::to_string(&session).unwrap();
        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        mac.update(session_json.as_bytes());
        let sig = hex_encode(mac.finalize().into_bytes().as_slice());
        let token = format!("{}.{}", base64_encode(&session_json), sig);
        assert!(!verify_session(&token, secret));
    }

    #[test]
    fn test_verify_invalid_format() {
        assert!(!verify_session("no-dot-here", b"secret"));
        assert!(!verify_session("too.many.dots", b"secret"));
        assert!(!verify_session("", b"secret"));
    }
}
