//! État enregistré dans le dossier de l'application (`%APPDATA%\Owlery\state.json`, ou `OWLERY_DATA_DIR`).
//!
//! Comme l'état de Fawkes : signé par la version qui l'a écrit, copié à part avant qu'une autre version
//! n'écrive par-dessus, champs inconnus gardés. Rien n'est perdu :
//! - un fichier illisible est mis de côté, et rien n'est écrit tant que la mise de côté n'a pas réussi ;
//! - une entrée ou un choix illisible (écrit par une autre version) est gardé tel quel et réécrit ;
//! - un réglage illisible reprend sa valeur par défaut, et c'est signalé ;
//! - un verrou (`state.lock`) empêche deux instances d'écrire le même fichier.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::forges::Account;
use crate::inbox::{Filter, GroupBy, Inbox, Rules};
use crate::model::{Item, Kind, Mark};

/// Version du format de l'état. 2 : identifiants d'entrée sans le type (0.2.0).
pub const STATE_VERSION: u32 = 2;
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
    /// Dossier local choisi à la main pour un dépôt (`propriétaire/nom` → dossier).
    pub repo_dirs: BTreeMap<String, String>,
    /// Dossiers où chercher les dépôts locaux.
    pub repo_roots: Vec<String>,
    /// Chemin de `dragontree.exe` (vide : cherché dans le PATH).
    pub dragontree_exe: String,
    /// Chemin de `fawkes.exe` (vide : cherché dans le PATH).
    pub fawkes_exe: String,
    /// Comptes des forges (les jetons sont dans le coffre).
    pub accounts: Vec<Account>,
    /// Notifications Windows, et les types notifiés.
    pub notify: bool,
    pub notify_kinds: BTreeSet<Kind>,
    /// Dépôts ignorés, robots masqués.
    pub rules: Rules,
    /// Comptes écrits par une autre version et illisibles par celle-ci : réécrits tels quels.
    #[serde(skip)]
    pub unreadable_accounts: Vec<Value>,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            refresh_minutes: 5,
            group_by: GroupBy::Repo,
            theme: ThemeChoice::System,
            shortcuts: BTreeMap::new(),
            repo_dirs: BTreeMap::new(),
            repo_roots: Vec::new(),
            dragontree_exe: String::new(),
            fawkes_exe: String::new(),
            accounts: Vec::new(),
            notify: true,
            notify_kinds: crate::notify::default_kinds(),
            rules: Rules::default(),
            unreadable_accounts: Vec::new(),
            unknown: BTreeMap::new(),
        }
    }
}

/// Taille et position de la fenêtre, en points logiques.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct WindowState {
    pub size: [f32; 2],
    pub pos: Option<[f32; 2]>,
    pub maximized: bool,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl Default for WindowState {
    fn default() -> Self {
        WindowState { size: [1100.0, 700.0], pos: None, maximized: false, unknown: BTreeMap::new() }
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
    /// Filtres de la liste.
    #[serde(default)]
    pub view: Filter,
    #[serde(default)]
    pub window: WindowState,
    /// Champs écrits par une autre version, recopiés tels quels.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// État relu, et ce qu'il faut signaler à l'utilisateur.
pub struct Loaded {
    pub state: AppState,
    pub problems: Vec<String>,
    /// Faux si l'état existant n'a pu être ni lu ni mis de côté : l'enregistrer l'écraserait.
    pub writable: bool,
}

pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OWLERY_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    default_data_dir()
}

pub fn default_data_dir() -> PathBuf {
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Loaded { state: AppState::default(), problems, writable: true };
        }
        Err(e) => {
            problems.push(format!(
                "Lecture de l'état impossible ({e}) : rien ne sera enregistré pendant cette session, pour ne pas écraser {}.",
                path.display()
            ));
            return Loaded { state: AppState::default(), problems, writable: false };
        }
    };
    let value = match serde_json::from_slice::<Value>(&bytes) {
        Ok(v) if v.is_object() => v,
        other => {
            let why = other.err().map(|e| e.to_string()).unwrap_or_else(|| "pas un objet JSON".into());
            return match set_aside(&path, &bytes, "illisible") {
                Ok(aside) => {
                    problems.push(format!(
                        "État illisible ({why}) : mis de côté dans {}, démarrage avec un état vide.",
                        aside.display()
                    ));
                    Loaded { state: AppState::default(), problems, writable: true }
                }
                Err(e) => {
                    problems.push(format!(
                        "État illisible ({why}), et sa mise de côté a échoué ({e}) : rien ne sera enregistré pendant cette session."
                    ));
                    Loaded { state: AppState::default(), problems, writable: false }
                }
            };
        }
    };
    let version = value["version"].as_u64().unwrap_or(0) as u32;
    let saved_by = value["saved_by"].as_str().unwrap_or_default().to_string();
    if saved_by != APP_VERSION || version != STATE_VERSION {
        let previous = if saved_by.is_empty() { "ancienne-version".to_string() } else { saved_by.clone() };
        let copy = dir.join(format!("state.{previous}.json"));
        if !copy.exists() {
            let _ = std::fs::copy(&path, &copy);
        }
        if version > STATE_VERSION {
            problems.push(format!(
                "État enregistré par une version plus récente d'Owlery ({previous}) : relu au mieux, copie gardée dans {}.",
                copy.display()
            ));
        }
    }
    let state = parse_lenient(value, &mut problems);
    Loaded { state, problems, writable: true }
}

/// Lit l'état section par section, entrée par entrée : ce qui est illisible est gardé de côté (entrées, choix)
/// ou reprend sa valeur par défaut (réglages), sans faire échouer le reste.
fn parse_lenient(mut value: Value, problems: &mut Vec<String>) -> AppState {
    if value["version"].as_u64().unwrap_or(0) < 2 {
        migrate_v1(&mut value);
    }
    let Value::Object(mut top) = value else { return AppState::default() };
    let mut st = AppState {
        version: top.remove("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        saved_by: top.remove("saved_by").and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default(),
        ..AppState::default()
    };
    if let Some(mut raw) = top.remove("settings") {
        // Les comptes, un par un : un compte écrit par une version plus récente est gardé tel quel.
        let accounts = raw.as_object_mut().and_then(|m| m.remove("accounts"));
        st.settings = lenient_struct(raw, "Réglage", problems);
        for a in accounts.and_then(|a| a.as_array().cloned()).unwrap_or_default() {
            match serde_json::from_value::<Account>(a.clone()) {
                Ok(account) => st.settings.accounts.push(account),
                Err(_) => st.settings.unreadable_accounts.push(a),
            }
        }
        if !st.settings.unreadable_accounts.is_empty() {
            problems.push(format!(
                "{} compte(s) écrits par une autre version ne sont pas lisibles par celle-ci : gardés tels quels, mais pas lus.",
                st.settings.unreadable_accounts.len()
            ));
        }
    }
    if let Some(raw) = top.remove("view") {
        st.view = serde_json::from_value(raw).unwrap_or_default();
    }
    if let Some(raw) = top.remove("window") {
        st.window = serde_json::from_value(raw).unwrap_or_default();
    }
    if let Some(Value::Object(mut inbox)) = top.remove("inbox") {
        if let Some(Value::Object(items)) = inbox.remove("items") {
            for (id, raw) in items {
                match serde_json::from_value::<Item>(raw.clone()) {
                    Ok(item) => {
                        st.inbox.items.insert(id, item);
                    }
                    Err(_) => {
                        st.inbox.unreadable.insert(format!("items/{id}"), raw);
                    }
                }
            }
        }
        if let Some(Value::Object(marks)) = inbox.remove("marks") {
            for (id, raw) in marks {
                match serde_json::from_value::<Mark>(raw.clone()) {
                    Ok(mark) => {
                        st.inbox.marks.insert(id, mark);
                    }
                    Err(_) => {
                        st.inbox.unreadable.insert(format!("marks/{id}"), raw);
                    }
                }
            }
        }
        if let Some(Value::Object(seen)) = inbox.remove("last_seen") {
            st.inbox.last_seen = seen.into_iter().filter_map(|(k, v)| v.as_i64().map(|t| (k, t))).collect();
        }
        if let Some(raw) = inbox.remove("fetched") {
            st.inbox.fetched = serde_json::from_value(raw).unwrap_or_default();
        }
        st.inbox.unknown = inbox.into_iter().collect();
        let n = st.inbox.unreadable.len();
        if n > 0 {
            problems.push(format!(
                "{n} entrée(s) ou choix écrits par une autre version ne sont pas lisibles par celle-ci : ils sont gardés tels quels."
            ));
        }
    }
    st.unknown = top.into_iter().collect();
    st
}

/// Une structure lue champ par champ : un champ illisible reprend sa valeur par défaut (et c'est signalé),
/// les champs inconnus vont dans `unknown`.
fn lenient_struct<T: Serialize + for<'de> Deserialize<'de> + Default>(
    raw: Value,
    what: &str,
    problems: &mut Vec<String>,
) -> T {
    if let Ok(v) = serde_json::from_value::<T>(raw.clone()) {
        return v;
    }
    let Value::Object(raw) = raw else { return T::default() };
    let mut merged: Map<String, Value> = match serde_json::to_value(T::default()) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    for (k, v) in raw {
        let before = merged.insert(k.clone(), v);
        if serde_json::from_value::<T>(Value::Object(merged.clone())).is_err() {
            match before {
                Some(b) => merged.insert(k.clone(), b),
                None => merged.remove(&k),
            };
            problems.push(format!("{what} « {k} » illisible : valeur par défaut."));
        }
    }
    serde_json::from_value(Value::Object(merged)).unwrap_or_default()
}

/// Version 1 (0.1.0) : les identifiants d'entrée finissaient par le type (`GitHub:contoso/api#142:Mention`).
/// Ils le perdent ; deux entrées qui se rejoignent gardent le type le plus utile et le choix le plus récent.
fn migrate_v1(value: &mut Value) {
    fn strip(id: &str) -> String {
        for k in Kind::ALL {
            if let Some(base) = id.strip_suffix(&format!(":{k:?}")) {
                return base.to_string();
            }
        }
        id.to_string()
    }
    fn rank(item: &Value) -> u8 {
        match item["kind"].as_str() {
            Some("ReviewRequested") => 0,
            Some("CiFailed") => 1,
            Some("MyPr") => 2,
            Some("Assigned") => 3,
            _ => 4,
        }
    }
    let Some(inbox) = value.get_mut("inbox").and_then(Value::as_object_mut) else { return };
    if let Some(Value::Object(items)) = inbox.remove("items") {
        let mut out: Map<String, Value> = Map::new();
        for (id, mut item) in items {
            let new = strip(&id);
            // Une entrée abîmée (pas un objet) est gardée telle quelle : le chargeur la mettra de côté.
            if let Some(obj) = item.as_object_mut() {
                obj.insert("id".into(), Value::String(new.clone()));
            }
            if out.get(&new).is_none_or(|kept| rank(&item) < rank(kept)) {
                out.insert(new, item);
            }
        }
        inbox.insert("items".into(), Value::Object(out));
    }
    if let Some(Value::Object(marks)) = inbox.remove("marks") {
        let mut out: Map<String, Value> = Map::new();
        for (id, mark) in marks {
            let new = strip(&id);
            let seen = |m: &Value| m["seen_updated_at"].as_i64().unwrap_or(0);
            if out.get(&new).is_none_or(|kept| seen(&mark) > seen(kept)) {
                out.insert(new, mark);
            }
        }
        inbox.insert("marks".into(), Value::Object(out));
    }
    if let Some(Value::Object(seen)) = inbox.remove("last_seen") {
        let mut out: Map<String, Value> = Map::new();
        for (id, t) in seen {
            let new = strip(&id);
            if out.get(&new).is_none_or(|kept| t.as_i64() > kept.as_i64()) {
                out.insert(new, t);
            }
        }
        inbox.insert("last_seen".into(), Value::Object(out));
    }
}

/// Copie le contenu à côté sous un nom daté (renommage, sinon copie), et renvoie ce nom.
fn set_aside(path: &Path, bytes: &[u8], why: &str) -> std::io::Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let aside = path.with_file_name(format!("state.{why}-{stamp}.json"));
    if std::fs::rename(path, &aside).is_ok() {
        return Ok(aside);
    }
    std::fs::write(&aside, bytes)?;
    Ok(aside)
}

pub fn serialize(state: &AppState) -> Vec<u8> {
    let mut st = state.clone();
    st.version = STATE_VERSION;
    st.saved_by = APP_VERSION.to_string();
    let mut value = serde_json::to_value(&st).expect("l'état se sérialise toujours");
    // Les entrées et choix illisibles par cette version sont réécrits tels quels.
    for (key, raw) in &state.inbox.unreadable {
        let Some((section, id)) = key.split_once('/') else { continue };
        if let Some(Value::Object(map)) = value["inbox"].get_mut(section) {
            map.entry(id.to_string()).or_insert_with(|| raw.clone());
        }
    }
    if let Some(Value::Array(accounts)) = value["settings"].get_mut("accounts") {
        accounts.extend(state.settings.unreadable_accounts.iter().cloned());
    }
    serde_json::to_vec_pretty(&value).expect("l'état se sérialise toujours")
}

/// Écriture atomique : fichier temporaire, puis renommage.
pub fn save(dir: &Path, state: &AppState) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = state_path(dir);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serialize(state))?;
    std::fs::rename(&tmp, &path)
}

/// Verrou du dossier d'état, tenu pendant toute la session.
pub struct StateLock {
    _file: File,
}

/// Prend le verrou du dossier d'état ; erreur si une autre instance le tient.
pub fn lock(dir: &Path) -> Result<StateLock, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("state.lock"))
        .map_err(|e| e.to_string())?;
    match file.try_lock() {
        Ok(()) => Ok(StateLock { _file: file }),
        Err(std::fs::TryLockError::WouldBlock) => Err("une autre instance d'Owlery utilise ce dossier d'état".into()),
        Err(std::fs::TryLockError::Error(e)) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Forge, Status};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("owlery-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn item() -> Item {
        Item {
            id: Item::make_id(Forge::GitHub, "", "contoso/api", Some(1), false),
            account: "a".into(),
            kind: Kind::Mention,
            repo: "contoso/api".into(),
            number: Some(1),
            title: "Titre".into(),
            url: "https://github.com/contoso/api/issues/1".into(),
            updated_at: 10,
            ..Item::default()
        }
    }

    #[test]
    fn state_round_trips_with_marks() {
        let dir = temp("roundtrip");
        let mut st = AppState::default();
        st.inbox.merge("a", Forge::GitHub, vec![item()], 20);
        st.inbox.set_status(&item().id, Status::Snoozed { until: 99 }, None);
        st.settings.refresh_minutes = 15;
        st.view.waiting_only = true;
        save(&dir, &st).unwrap();
        let back = load(&dir);
        assert!(back.problems.is_empty(), "{:?}", back.problems);
        assert!(back.writable);
        assert_eq!(back.state.inbox, st.inbox);
        assert_eq!(back.state.settings.refresh_minutes, 15);
        assert!(back.state.view.waiting_only);
        assert_eq!(back.state.saved_by, APP_VERSION);
    }

    /// Un état écrit par la 0.1.0 (fichier figé) se relit sans perte : identifiants sans le type, choix gardés.
    #[test]
    fn state_written_by_0_1_0_still_loads() {
        let dir = temp("v1");
        std::fs::write(state_path(&dir), include_str!("testdata/state-0.1.0.json")).unwrap();
        let back = load(&dir);
        assert!(back.problems.is_empty(), "{:?}", back.problems);
        let inbox = &back.state.inbox;
        assert_eq!(inbox.items.len(), 2, "{:?}", inbox.items.keys().collect::<Vec<_>>());
        let review = &inbox.items["GitHub:contoso/api#142"];
        assert_eq!(review.kind, Kind::ReviewRequested, "la revue l'emporte sur la mention");
        assert_eq!(review.id, "GitHub:contoso/api#142");
        assert_eq!(inbox.marks["GitHub:contoso/api#142"].seen_updated_at, 300, "le choix le plus récent");
        assert_eq!(inbox.status(review, 1000), Status::Done);
        assert!(inbox.last_seen.contains_key("GitHub:fabrikam/web#57"));
        assert_eq!(back.state.settings.refresh_minutes, 10);
        assert!(dir.join("state.0.1.0.json").exists());
    }

    #[test]
    fn unknown_fields_are_kept_at_every_level() {
        let dir = temp("unknown");
        std::fs::write(
            state_path(&dir),
            r#"{"version":2,"saved_by":"0.0.9","futur":{"a":1},"settings":{"bientot":true},
                "inbox":{"items":{"GitHub:contoso/api#1":{"id":"GitHub:contoso/api#1","forge":"GitHub","kind":"Mention",
                "repo":"contoso/api","number":1,"title":"t","url":"https://x.test","updated_at":1,"nouveau":"oui"}}}}"#,
        )
        .unwrap();
        let back = load(&dir);
        assert!(dir.join("state.0.0.9.json").exists());
        let json: Value = serde_json::from_slice(&serialize(&back.state)).unwrap();
        assert_eq!(json["futur"]["a"], 1);
        assert_eq!(json["settings"]["bientot"], true);
        assert_eq!(json["inbox"]["items"]["GitHub:contoso/api#1"]["nouveau"], "oui");
    }

    #[test]
    fn an_unreadable_account_is_kept_and_the_others_read() {
        let dir = temp("comptes");
        std::fs::write(
            state_path(&dir),
            r#"{"version":2,"saved_by":"9.0.0","settings":{"refresh_minutes":7,"accounts":[
                {"id":"a1","forge":"GitHub","host":"github.com"},
                {"id":"b2","forge":"Bitbucket","host":"bitbucket.org"}]},
                "inbox":{"items":{},"plus_tard":{"x":1}},"view":{"waiting_only":true,"tri":"date"},
                "window":{"size":[800.0,600.0],"ecran":2}}"#,
        )
        .unwrap();
        let back = load(&dir);
        assert_eq!(back.state.settings.refresh_minutes, 7);
        assert_eq!(back.state.settings.accounts.len(), 1);
        assert!(back.problems.iter().any(|p| p.contains("1 compte(s)")), "{:?}", back.problems);
        let json: Value = serde_json::from_slice(&serialize(&back.state)).unwrap();
        assert_eq!(json["settings"]["accounts"].as_array().unwrap().len(), 2);
        assert_eq!(json["settings"]["accounts"][1]["forge"], "Bitbucket");
        assert_eq!(json["inbox"]["plus_tard"]["x"], 1);
        assert_eq!(json["view"]["tri"], "date");
        assert_eq!(json["window"]["ecran"], 2);
    }

    #[test]
    fn a_damaged_first_draft_entry_does_not_stop_the_start() {
        let dir = temp("v1-abime");
        std::fs::write(
            state_path(&dir),
            r#"{"version":1,"saved_by":"0.1.0","inbox":{"items":{"GitHub:contoso/api#1:Mention":"abîmé"}}}"#,
        )
        .unwrap();
        let back = load(&dir);
        assert!(back.state.inbox.items.is_empty());
        assert_eq!(back.state.inbox.unreadable.len(), 1);
    }

    #[test]
    fn unreadable_entries_and_settings_do_not_lose_the_rest() {
        let dir = temp("partial");
        std::fs::write(
            state_path(&dir),
            r#"{"version":2,"saved_by":"9.0.0","settings":{"theme":"Sepia","refresh_minutes":12},
                "inbox":{"items":{"GitHub:contoso/api#1":{"id":"GitHub:contoso/api#1","forge":"GitHub","kind":"Mention",
                "repo":"contoso/api","number":1,"title":"t","url":"https://x.test","updated_at":1},
                "GitHub:contoso/api#2":{"id":"GitHub:contoso/api#2","forge":"Bitbucket","kind":"Mention"}},
                "marks":{"GitHub:contoso/api#1":{"status":"Archived","seen_updated_at":1}}}}"#,
        )
        .unwrap();
        let back = load(&dir);
        assert_eq!(back.state.settings.refresh_minutes, 12);
        assert_eq!(back.state.settings.theme, ThemeChoice::System);
        assert_eq!(back.state.inbox.items.len(), 1);
        assert!(back.problems.iter().any(|p| p.contains("theme")), "{:?}", back.problems);
        assert!(back.problems.iter().any(|p| p.contains("2 entrée(s)")), "{:?}", back.problems);
        let json: Value = serde_json::from_slice(&serialize(&back.state)).unwrap();
        assert_eq!(json["inbox"]["items"]["GitHub:contoso/api#2"]["forge"], "Bitbucket");
        assert_eq!(json["inbox"]["marks"]["GitHub:contoso/api#1"]["status"], "Archived");
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
        assert!(back.writable);
        assert!(!state_path(&dir).exists());
        let aside: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(std::fs::read_to_string(aside[0].path()).unwrap(), "{ pas du json");
    }

    #[test]
    fn a_state_that_cannot_be_read_is_never_overwritten() {
        let dir = temp("unreadable");
        // Un dossier à la place du fichier : la lecture échoue autrement que par « absent ».
        std::fs::create_dir_all(state_path(&dir)).unwrap();
        let back = load(&dir);
        assert!(!back.writable);
        assert_eq!(back.problems.len(), 1);
    }

    #[test]
    fn a_second_instance_cannot_take_the_lock() {
        let dir = temp("lock");
        let first = lock(&dir).unwrap();
        assert!(lock(&dir).is_err());
        drop(first);
        assert!(lock(&dir).is_ok());
    }
}
