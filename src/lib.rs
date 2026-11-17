//! SigmaOS-apt-key — GPG keyring fingerprint management.
//!
//! Inspired by Debian's `apt-key`: manages the trusted keyring used to verify
//! package signatures — adding, listing, and removing keys by fingerprint or
//! key id. This is the mechanism layer: it stores key records and validates
//! fingerprint shapes; actual cryptographic verification is delegated to the
//! caller (separation of mechanism and policy).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::fmt;

/// Errors raised by this component.
#[derive(Debug, thiserror::Error)]
pub enum ComponentError {
    /// The fingerprint/key-id was not well-formed.
    #[error("invalid fingerprint: {0}")]
    InvalidFingerprint(String),
    /// The key is already present in the keyring.
    #[error("key already present: {0}")]
    Duplicate(String),
    /// The key was not found in the keyring.
    #[error("key not found: {0}")]
    NotFound(String),
}

/// Result alias for component operations.
pub type ComponentResult<T> = Result<T, ComponentError>;

/// Trust level assigned to a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Trust {
    /// Never trusted / revoked (lowest).
    Never,
    /// Marginal trust.
    Marginal,
    /// Fully trusted.
    Full,
    /// Ultimately trusted (root of trust, highest).
    Ultimate,
}

impl fmt::Display for Trust {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Trust::Ultimate => "ultimate",
            Trust::Full => "full",
            Trust::Marginal => "marginal",
            Trust::Never => "never",
        })
    }
}

/// One keyring entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    /// 40-hex-char primary fingerprint (lowercased).
    pub fingerprint: String,
    /// Optional 16-hex-char long key id (last 16 of fingerprint).
    pub key_id: Option<String>,
    /// Trust level.
    pub trust: Trust,
    /// Free-form comment (e.g. key owner / source).
    pub comment: String,
}

/// A keyring: fingerprint → Key. `BTreeMap` keeps listing deterministic.
#[derive(Debug, Default)]
pub struct Keyring {
    keys: BTreeMap<String, Key>,
}

impl Keyring {
    /// Empty keyring.
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate and normalise a fingerprint to lowercase 40-hex.
    /// Accepts optional `0x` prefix and spaces/colons between groups.
    pub fn normalize_fingerprint(raw: &str) -> ComponentResult<String> {
        let cleaned: String = raw
            .trim()
            .trim_start_matches("0x")
            .chars()
            .filter(|c| !c.is_whitespace() && *c != ':')
            .collect();
        let lower = cleaned.to_lowercase();
        if lower.len() == 40 && lower.chars().all(|c| c.is_ascii_hexdigit()) {
            Ok(lower)
        } else {
            Err(ComponentError::InvalidFingerprint(raw.to_string()))
        }
    }

    /// Derive the 16-char long key id from a 40-char fingerprint.
    pub fn key_id_of(fingerprint: &str) -> String {
        fingerprint.chars().rev().take(16).collect::<String>().chars().rev().collect()
    }

    /// Add a key. `fingerprint` may be in any accepted form.
    pub fn add(&mut self, fingerprint: &str, trust: Trust, comment: &str) -> ComponentResult<String> {
        let fp = Self::normalize_fingerprint(fingerprint)?;
        if self.keys.contains_key(&fp) {
            return Err(ComponentError::Duplicate(fp));
        }
        let kid = Some(Self::key_id_of(&fp));
        self.keys.insert(fp.clone(), Key {
            fingerprint: fp.clone(),
            key_id: kid,
            trust,
            comment: comment.to_string(),
        });
        Ok(fp)
    }

    /// Remove a key by fingerprint (any accepted form).
    pub fn remove(&mut self, fingerprint: &str) -> ComponentResult<()> {
        let fp = Self::normalize_fingerprint(fingerprint)?;
        self.keys.remove(&fp).map(|_| ()).ok_or(ComponentError::NotFound(fp))
    }

    /// Look up a key by fingerprint.
    pub fn get(&self, fingerprint: &str) -> Option<&Key> {
        let fp = Self::normalize_fingerprint(fingerprint).ok()?;
        self.keys.get(&fp)
    }

    /// Update a key's trust level.
    pub fn set_trust(&mut self, fingerprint: &str, trust: Trust) -> ComponentResult<()> {
        let fp = Self::normalize_fingerprint(fingerprint)?;
        let k = self.keys.get_mut(&fp).ok_or(ComponentError::NotFound(fp.clone()))?;
        k.trust = trust;
        Ok(())
    }

    /// All keys sorted by fingerprint.
    pub fn list(&self) -> Vec<&Key> {
        self.keys.values().collect()
    }

    /// Keys at or above a trust level.
    pub fn trusted(&self, min: Trust) -> Vec<&Key> {
        self.keys.values().filter(|k| k.trust >= min).collect()
    }

    /// Number of keys.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP1: &str = "0x1234567890abcdef1234567890abcdef12345678";
    const FP2: &str = "abcdef1234567890abcdef1234567890abcdef12";

    #[test]
    fn normalizes_fingerprint() {
        assert_eq!(
            Keyring::normalize_fingerprint(FP1).unwrap(),
            "1234567890abcdef1234567890abcdef12345678"
        );
        // spaced/colon grouped form
        assert_eq!(
            Keyring::normalize_fingerprint("1234 5678 90ab cdef 1234 5678 90ab cdef 1234 5678").unwrap(),
            "1234567890abcdef1234567890abcdef12345678"
        );
    }

    #[test]
    fn rejects_bad_fingerprint() {
        assert!(Keyring::normalize_fingerprint("xyz").is_err());
        assert!(Keyring::normalize_fingerprint("1234").is_err());
    }

    #[test]
    fn add_list_remove() {
        let mut kr = Keyring::new();
        kr.add(FP1, Trust::Full, "debian archive key").unwrap();
        kr.add(FP2, Trust::Ultimate, "sigmaos release key").unwrap();
        assert_eq!(kr.len(), 2);
        assert!(kr.get(FP1).is_some());
        assert_eq!(kr.get(FP1).unwrap().trust, Trust::Full);
        assert_eq!(kr.list().len(), 2);
        kr.remove(FP1).unwrap();
        assert_eq!(kr.len(), 1);
        assert!(kr.remove(FP1).is_err());
    }

    #[test]
    fn duplicate_rejected() {
        let mut kr = Keyring::new();
        kr.add(FP1, Trust::Full, "").unwrap();
        assert!(matches!(kr.add(FP1, Trust::Full, ""), Err(ComponentError::Duplicate(_))));
    }

    #[test]
    fn trust_filter_and_update() {
        let mut kr = Keyring::new();
        kr.add(FP1, Trust::Marginal, "").unwrap();
        kr.add(FP2, Trust::Ultimate, "").unwrap();
        assert_eq!(kr.trusted(Trust::Full).len(), 1);
        kr.set_trust(FP1, Trust::Full).unwrap();
        assert_eq!(kr.trusted(Trust::Full).len(), 2);
    }

    #[test]
    fn key_id_derived() {
        let kid = Keyring::key_id_of("1234567890abcdef1234567890abcdef12345678");
        assert_eq!(kid.len(), 16);
        assert_eq!(kid, "90abcdef12345678");
    }
}
