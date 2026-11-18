use std::fmt;
use std::str::FromStr;

use crate::sha256::sha256;

/// 128-bit identifier.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Uuid(pub [u8; 16]);

impl Uuid {
    pub const NIL: Uuid = Uuid([0; 16]);

    /// Random (version 4) uuid.
    pub fn new_v4() -> Self {
        let mut b: [u8; 16] = crate::random_bytes(16).try_into().unwrap();
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        Uuid(b)
    }

    /// Deterministic uuid derived from a namespace and a name (SHA-256 based, *not* RFC 4122 v5).
    pub fn from_name(namespace: &Uuid, name: &[u8]) -> Self {
        let mut input = namespace.0.to_vec();
        input.extend_from_slice(name);
        let h = sha256(&input);
        let mut b = [0u8; 16];
        b.copy_from_slice(&h[..16]);
        b[6] = (b[6] & 0x0f) | 0x50;
        b[8] = (b[8] & 0x3f) | 0x80;
        Uuid(b)
    }

    pub fn is_nil(&self) -> bool {
        *self == Self::NIL
    }

    pub fn parse(s: &str) -> Option<Uuid> {
        let hex: String = s.chars().filter(|c| *c != '-').collect();
        if hex.len() != 32 || !hex.is_ascii() {
            return None;
        }
        let mut b = [0u8; 16];
        for i in 0..16 {
            b[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(Uuid(b))
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.0;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        )
    }
}

impl fmt::Debug for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uuid({})", self)
    }
}

impl FromStr for Uuid {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Uuid::parse(s).ok_or_else(|| format!("invalid uuid: {s}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let u = Uuid::new_v4();
        assert_eq!(Uuid::parse(&u.to_string()), Some(u));
        assert_ne!(Uuid::new_v4(), u);
    }

    #[test]
    fn named_is_stable() {
        let ns = Uuid::new_v4();
        assert_eq!(Uuid::from_name(&ns, b"a"), Uuid::from_name(&ns, b"a"));
        assert_ne!(Uuid::from_name(&ns, b"a"), Uuid::from_name(&ns, b"b"));
    }

    #[test]
    fn bad_input() {
        assert!(Uuid::parse("nope").is_none());
    }
}
