//! Salted, iterated password hashing (PBKDF2-HMAC-SHA256).
//!
//! Format: `pbkdf2-sha256$<iterations>$<salt hex>$<hash hex>`.
//! The hash format is versioned by its prefix so a stronger scheme (argon2, ...) can be added
//! later while still verifying old hashes.

use crate::sha256::hmac_sha256;
use crate::{from_hex, random_bytes, to_hex};

pub const DEFAULT_ITERATIONS: u32 = 120_000;

fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    // single block (dkLen = 32 = hLen)
    let mut s = salt.to_vec();
    s.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac_sha256(password, &s);
    let mut t = u;
    for _ in 1..iterations {
        u = hmac_sha256(password, &u);
        for (a, b) in t.iter_mut().zip(u.iter()) {
            *a ^= b;
        }
    }
    t
}

pub fn hash_password(password: &str) -> String {
    hash_with(password, DEFAULT_ITERATIONS)
}

pub fn hash_with(password: &str, iterations: u32) -> String {
    let salt = random_bytes(16);
    let h = pbkdf2(password.as_bytes(), &salt, iterations);
    format!("pbkdf2-sha256${}${}${}", iterations, to_hex(&salt), to_hex(&h))
}

/// Constant-time comparison of two byte strings.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 4 || parts[0] != "pbkdf2-sha256" {
        return false;
    }
    let Ok(iterations) = parts[1].parse::<u32>() else { return false };
    let (Some(salt), Some(expected)) = (from_hex(parts[2]), from_hex(parts[3])) else {
        return false;
    };
    ct_eq(&pbkdf2(password.as_bytes(), &salt, iterations), &expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbkdf2_known_vector() {
        // RFC 7914 section 11 style vector for PBKDF2-HMAC-SHA256, c=1, dkLen=32 ("password"/"salt")
        assert_eq!(
            to_hex(&pbkdf2(b"password", b"salt", 1)),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
        assert_eq!(
            to_hex(&pbkdf2(b"password", b"salt", 4096)),
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a"
        );
    }

    #[test]
    fn hash_and_verify() {
        let h = hash_with("hunter2", 10);
        assert!(verify_password("hunter2", &h));
        assert!(!verify_password("hunter3", &h));
        assert!(!verify_password("hunter2", "garbage"));
    }
}
