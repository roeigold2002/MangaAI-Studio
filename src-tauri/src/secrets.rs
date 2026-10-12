//! API keys in the OS credential store (macOS Keychain, Windows Credential
//! Manager, Secret Service on Linux). Values never leave the Rust side.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{AppError, AppResult};

const SERVICE: &str = "com.mangaai.studio";

pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> AppResult<Option<String>>;
    fn set(&self, account: &str, value: &str) -> AppResult<()>;
    fn delete(&self, account: &str) -> AppResult<()>;
}

pub struct KeyringStore;

fn kerr(e: keyring::Error) -> AppError {
    AppError::new("keychain", format!("credential store error: {e}"))
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(kerr)?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(kerr(e)),
        }
    }
    fn set(&self, account: &str, value: &str) -> AppResult<()> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(kerr)?;
        entry.set_password(value).map_err(kerr)
    }
    fn delete(&self, account: &str) -> AppResult<()> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(kerr)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(kerr(e)),
        }
    }
}

/// In-memory store for tests.
#[derive(Default, Clone)]
pub struct MemoryStore(Arc<Mutex<HashMap<String, String>>>);

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> AppResult<Option<String>> {
        Ok(self.0.lock().get(account).cloned())
    }
    fn set(&self, account: &str, value: &str) -> AppResult<()> {
        self.0.lock().insert(account.into(), value.into());
        Ok(())
    }
    fn delete(&self, account: &str) -> AppResult<()> {
        self.0.lock().remove(account);
        Ok(())
    }
}

/// Replace anything that looks like a credential before logging or showing an error.
pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for s in secrets {
        if s.len() >= 6 {
            out = out.replace(s, "[REDACTED]");
        }
    }
    static KEY_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(sk-[A-Za-z0-9_\-]{8,}|AIza[0-9A-Za-z_\-]{20,}|(?i)bearer\s+[A-Za-z0-9._\-]{8,})").unwrap()
    });
    KEY_RE.replace_all(&out, "[REDACTED]").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_known_and_pattern_keys() {
        let t = "failed with key sk-ant-api03-abcdefghijkl and AIzaSyA1234567890abcdefghijk and mysecretvalue";
        let r = redact(t, &["mysecretvalue"]);
        assert!(!r.contains("sk-ant"));
        assert!(!r.contains("AIza"));
        assert!(!r.contains("mysecretvalue"));
    }

    #[test]
    fn memory_store_roundtrip() {
        let s = MemoryStore::default();
        assert_eq!(s.get("a").unwrap(), None);
        s.set("a", "v").unwrap();
        assert_eq!(s.get("a").unwrap().as_deref(), Some("v"));
        s.delete("a").unwrap();
        assert_eq!(s.get("a").unwrap(), None);
    }
}
