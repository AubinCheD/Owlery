//! Jetons dans le Gestionnaire d'identifiants de Windows (service « Owlery »). Ils ne sont jamais écrits,
//! affichés ni journalisés ailleurs.

/// Nom du service dans le Gestionnaire d'identifiants.
pub const SERVICE: &str = "Owlery";

/// Un coffre de secrets : celui de l'OS, ou un faux pour les tests.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// Le Gestionnaire d'identifiants de Windows.
pub struct OsStore;

impl SecretStore for OsStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match entry(key)?.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        entry(key)?.set_password(secret).map_err(|e| e.to_string())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        match entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

fn entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, key).map_err(|e| e.to_string())
}

/// Enregistre un jeton saisi : refusé s'il est vide.
pub fn store_token(store: &dyn SecretStore, key: &str, token: &str) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("le jeton est vide".into());
    }
    store.set(key, token)
}

/// Faux coffre en mémoire, pour les tests.
#[derive(Default)]
pub struct MemoryStore(pub std::sync::Mutex<std::collections::BTreeMap<String, String>>);

impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(key.into(), secret.into());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_trimmed_and_empty_ones_refused() {
        let store = MemoryStore::default();
        assert!(store_token(&store, "account:test", "  \n").is_err());
        store_token(&store, "account:test", " ghp_fictif \n").unwrap();
        assert_eq!(store.get("account:test").unwrap().as_deref(), Some("ghp_fictif"));
        store.delete("account:test").unwrap();
        assert_eq!(store.get("account:test").unwrap(), None);
    }
}
