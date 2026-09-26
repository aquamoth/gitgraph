//! Git object ids (SHA-1 or SHA-256), stored inline so they are `Copy`.

use std::fmt;

/// A git object id. Supports both SHA-1 (20 bytes) and SHA-256 (32 bytes) repositories.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Oid {
    bytes: [u8; 32],
    len: u8,
}

impl Oid {
    /// Parses a full-length hex object id (40 or 64 hex digits).
    pub fn from_hex(hex: &str) -> Option<Oid> {
        let hex = hex.as_bytes();
        if hex.len() != 40 && hex.len() != 64 {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (i, [hi, lo]) in hex.as_chunks::<2>().0.iter().enumerate() {
            bytes[i] = (nibble(*hi)? << 4) | nibble(*lo)?;
        }
        Some(Oid {
            bytes,
            len: (hex.len() / 2) as u8,
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// Full lowercase hex representation.
    pub fn to_hex(&self) -> String {
        self.short(usize::MAX)
    }

    /// The first `digits` hex digits (clamped to the full length).
    pub fn short(&self, digits: usize) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(self.len as usize * 2);
        for b in self.as_bytes() {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0xf) as usize] as char);
        }
        s.truncate(digits.min(s.len()));
        s
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Oid({})", self.short(12))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_sha1_and_sha256() {
        let sha1 = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(Oid::from_hex(sha1).unwrap().to_hex(), sha1);
        let sha256 = "0123456789abcdef".repeat(4);
        assert_eq!(Oid::from_hex(&sha256).unwrap().to_hex(), sha256);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Oid::from_hex("abc").is_none());
        assert!(Oid::from_hex(&"g".repeat(40)).is_none());
    }

    #[test]
    fn short_is_prefix() {
        let oid = Oid::from_hex(&"AbCdEf0123".repeat(4)).unwrap();
        assert_eq!(oid.short(7), "abcdef0");
        assert_eq!(oid.short(1000).len(), 40);
    }
}
