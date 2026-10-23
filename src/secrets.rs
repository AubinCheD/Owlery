//! Jetons dans le Gestionnaire d'identifiants de Windows. Ceux des comptes sont communs à toute la suite
//! EyrieSuite (service « EyrieSuite », voir [`SuiteStore`]) ; le reste est dans le service « Owlery ». Ils ne sont
//! jamais écrits, affichés ni journalisés ailleurs.

/// Nom du service dans le Gestionnaire d'identifiants.
pub const SERVICE: &str = "Owlery";

/// Un coffre de secrets : celui de l'OS, ou un faux pour les tests.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
    /// Est-ce le coffre de l'OS (et pas un coffre en mémoire) ?
    fn is_os(&self) -> bool {
        false
    }
}

/// Le coffre de la session : celui de l'OS, ou en mémoire pour la démonstration (qui ne doit jamais toucher
/// aux vrais jetons).
pub fn store_for(demo: bool) -> std::sync::Arc<dyn SecretStore> {
    if demo {
        std::sync::Arc::new(MemoryStore::default())
    } else {
        std::sync::Arc::new(SuiteStore::new(
            std::sync::Arc::new(suite_accounts::OsVault::suite()),
            std::sync::Arc::new(OsStore),
        ))
    }
}

/// Le Gestionnaire d'identifiants de Windows.
pub struct OsStore;

impl SecretStore for OsStore {
    fn is_os(&self) -> bool {
        true
    }

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

/// Les jetons des comptes (`account:<id>`) dans le coffre de la suite, communs à Eyrie, DragonTree et Owlery ;
/// relus dans celui d'Owlery pour un compte pas encore repris. Les autres clés restent dans celui d'Owlery.
pub struct SuiteStore {
    suite: std::sync::Arc<dyn suite_accounts::Vault>,
    own: std::sync::Arc<dyn SecretStore>,
}

impl SuiteStore {
    pub fn new(suite: std::sync::Arc<dyn suite_accounts::Vault>, own: std::sync::Arc<dyn SecretStore>) -> Self {
        SuiteStore { suite, own }
    }
}

impl SecretStore for SuiteStore {
    fn is_os(&self) -> bool {
        self.own.is_os()
    }

    fn get(&self, key: &str) -> Result<Option<String>, String> {
        if let Some(id) = key.strip_prefix("account:")
            && let Some(token) = self.suite.get(id)?
        {
            return Ok(Some(token));
        }
        self.own.get(key)
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        match key.strip_prefix("account:") {
            Some(id) => self.suite.set(id, secret),
            None => self.own.set(key, secret),
        }
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        if let Some(id) = key.strip_prefix("account:") {
            self.suite.delete(id)?;
        }
        self.own.delete(key)
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
    fn demo_never_touches_the_os_store() {
        assert!(!store_for(true).is_os());
        assert!(store_for(false).is_os());
    }

    #[test]
    fn account_tokens_go_to_the_suite_vault() {
        use suite_accounts::Vault;
        let vault = std::sync::Arc::new(suite_accounts::MemoryVault::default());
        let own = std::sync::Arc::new(MemoryStore::default());
        own.set("account:x", "ancien").unwrap();
        own.set("autre", "valeur").unwrap();
        let s = SuiteStore::new(vault.clone(), own.clone());
        assert_eq!(s.get("account:x").unwrap().as_deref(), Some("ancien"), "repli sur le coffre d'Owlery");
        s.set("account:x", "nouveau").unwrap();
        assert_eq!(vault.get("x").unwrap().as_deref(), Some("nouveau"));
        assert_eq!(s.get("account:x").unwrap().as_deref(), Some("nouveau"));
        assert_eq!(s.get("autre").unwrap().as_deref(), Some("valeur"), "les autres clés restent à Owlery");
        s.delete("account:x").unwrap();
        assert_eq!(vault.get("x").unwrap(), None);
        assert_eq!(own.get("account:x").unwrap(), None, "l'ancien jeton part aussi");
    }

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
