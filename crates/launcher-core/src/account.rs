//! Offline accounts. The `Account` trait leaves room for Microsoft login later.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// What the launcher needs from any kind of account.
pub trait Account {
    fn player_name(&self) -> &str;
    /// Hyphenated UUID.
    fn uuid(&self) -> &str;
    fn access_token(&self) -> String;
    fn user_type(&self) -> &str;
}

/// 3-16 characters from `[A-Za-z0-9_]`.
pub fn validate_name(name: &str) -> Result<()> {
    let len = name.chars().count();
    if !(3..=16).contains(&len) {
        return Err(Error::InvalidPlayerName("长度需为 3-16 个字符".into()));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(Error::InvalidPlayerName("只能包含字母、数字和下划线".into()));
    }
    Ok(())
}

/// Same algorithm as Java's `UUID.nameUUIDFromBytes("OfflinePlayer:<name>")`:
/// MD5, then set version = 3 and the RFC 4122 variant.
pub fn offline_uuid(name: &str) -> String {
    let mut bytes = md5::compute(format!("OfflinePlayer:{name}").as_bytes()).0;
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h = hex::encode(bytes);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineAccount {
    pub name: String,
    pub uuid: String,
}

impl OfflineAccount {
    pub fn new(name: &str) -> Result<Self> {
        validate_name(name)?;
        Ok(Self { name: name.to_string(), uuid: offline_uuid(name) })
    }
}

impl Account for OfflineAccount {
    fn player_name(&self) -> &str {
        &self.name
    }
    fn uuid(&self) -> &str {
        &self.uuid
    }
    fn access_token(&self) -> String {
        "0".into()
    }
    fn user_type(&self) -> &str {
        "legacy"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notch_offline_uuid_matches_known_value() {
        assert_eq!(offline_uuid("Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }

    #[test]
    fn uuid_has_version_3_and_rfc4122_variant() {
        let u = offline_uuid("Steve");
        let parts: Vec<_> = u.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert!(parts[2].starts_with('3'));
        assert!("89ab".contains(parts[3].chars().next().unwrap()));
    }

    #[test]
    fn name_validation() {
        for ok in ["Steve", "Alex_123", "abc", "a_b_c_d_e_f_g_h_"] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in ["ab", "this_name_is_too_long", "带中文", "has space", "dash-name", ""] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }
}
