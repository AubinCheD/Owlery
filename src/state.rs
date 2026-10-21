//! État enregistré dans le dossier de l'application (`%APPDATA%\Owlery\state.json`, ou `OWLERY_DATA_DIR`).
//!
//! Comme l'état de Fawkes : signé par la version qui l'a écrit, copié à part avant qu'une autre version
//! n'écrive par-dessus, champs inconnus gardés, et un fichier illisible mis de côté plutôt qu'écrasé.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::inbox::{GroupBy, Inbox};

/// Version du format de l'état.
pub const STATE_VERSION: u32 = 1;
/// Version de l'application qui écrit l'état.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Intervalle de lecture périodique, en minutes (0 : seulement à la demande).
    pub refresh_minutes: u32,
    pub group_by: GroupBy,
    pub theme: ThemeChoice,
    /// Raccourcis réglés par l'utilisateur : identifiant de commande → combinaisons (`"Ctrl+R"`).
    pub shortcuts: BTreeMap<String, Vec<String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { refresh_minutes: 5, group_by: GroupBy::Repo, theme: ThemeChoice::System, shortcuts: BTreeMap::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct AppState {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub saved_by: String,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub inbox: Inbox,
    /// Champs écrits par une autre version, recopiés tels quels.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, serde_json::Value>,
}

/// État relu, et ce qu'il faut signaler à l'utilisateur.
pub struct Loaded {
    pub state: AppState,
    pub problems: Vec<String>,
}

pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OWLERY_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("Owlery")
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join("state.json")
}

pub fn load(dir: &Path) -> Loaded {
    let path = state_path(dir);
    let mut problems = Vec::new();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded { state: AppState::default(), problems },
        Err(e) => {
            problems.push(format!("Lecture de l'état impossible ({e}) : démarrage avec un état vide, le fichier est laissé en place."));
            return Loaded { state: AppState::default(), problems };
        }
    };
    let state = match serde_json::from_slice::<AppState>(&bytes) {
        Ok(st) => st,
        Err(e) => {
            let aside = set_aside(&path, "illisible");
            problems.push(format!("État illisible ({e}) : mis de côté dans {}, démarrage avec un état vide.", aside.display()));
            return Loaded { state: AppState::default(), problems };
        }
    };
    if state.saved_by != APP_VERSION || state.version != STATE_VERSION {
        let previous = if state.saved_by.is_empty() { "ancienne-version" } else { state.saved_by.as_str() };
        let copy = dir.join(format!("state.{previous}.json"));
        if !copy.exists() {
            let _ = std::fs::copy(&path, &copy);
        }
        if state.version > STATE_VERSION {
            problems.push(format!(
                "État enregistré par une version plus récente d'Owlery ({previous}) : relu au mieux, copie gardée dans {}.",
                copy.display()
            ));
        }
    }
    Loaded { state, problems }
}

/// Copie le fichier à côté sous un nom daté, et renvoie ce nom.
fn set_aside(path: &Path, why: &str) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let aside = path.with_file_name(format!("state.{why}-{stamp}.json"));
    let _ = std::fs::rename(path, &aside);
    aside
}

pub fn serialize(state: &AppState) -> Vec<u8> {
    let mut st = state.clone();
    st.version = STATE_VERSION;
    st.saved_by = APP_VERSION.to_string();
    serde_json::to_vec_pretty(&st).expect("l'état se sérialise toujours")
}

/// Écriture atomique : fichier temporaire, puis renommage.
pub fn save(dir: &Path, state: &AppState) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = state_path(dir);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serialize(state))?;
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Forge, Item, Kind, Status};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("owlery-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn state_round_trips_with_marks() {
        let dir = temp("roundtrip");
        let mut st = AppState::default();
        let item = Item {
            id: Item::make_id(Forge::GitHub, "contoso/api", Some(1), Kind::Mention),
            forge: Forge::GitHub,
            kind: Kind::Mention,
            repo: "contoso/api".into(),
            number: Some(1),
            title: "Titre".into(),
            url: "https://github.com/contoso/api/issues/1".into(),
            branch: None,
            author: String::new(),
            updated_at: 10,
            ci: None,
            summary: String::new(),
        };
        st.inbox.merge(Forge::GitHub, vec![item.clone()], 20);
        st.inbox.set_status(&item.id, Status::Snoozed { until: 99 });
        st.settings.refresh_minutes = 15;
        save(&dir, &st).unwrap();
        let back = load(&dir);
        assert!(back.problems.is_empty());
        assert_eq!(back.state.inbox, st.inbox);
        assert_eq!(back.state.settings.refresh_minutes, 15);
        assert_eq!(back.state.saved_by, APP_VERSION);
    }

    #[test]
    fn unknown_fields_are_kept_and_other_versions_copied_aside() {
        let dir = temp("unknown");
        std::fs::write(state_path(&dir), r#"{"version":1,"saved_by":"0.0.9","futur":{"a":1}}"#).unwrap();
        let back = load(&dir);
        assert!(dir.join("state.0.0.9.json").exists());
        let json: serde_json::Value = serde_json::from_slice(&serialize(&back.state)).unwrap();
        assert_eq!(json["futur"]["a"], 1);
    }

    #[test]
    fn newer_state_is_reported() {
        let dir = temp("newer");
        std::fs::write(state_path(&dir), r#"{"version":99,"saved_by":"9.0.0"}"#).unwrap();
        assert_eq!(load(&dir).problems.len(), 1);
    }

    #[test]
    fn unreadable_state_is_set_aside_not_lost() {
        let dir = temp("broken");
        std::fs::write(state_path(&dir), "{ pas du json").unwrap();
        let back = load(&dir);
        assert_eq!(back.problems.len(), 1);
        assert!(!state_path(&dir).exists());
        let aside: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(std::fs::read_to_string(aside[0].path()).unwrap(), "{ pas du json");
    }
}
