//! Contrat local entre les applications (voir `docs/contrat-local.md`) : une ligne JSON par message,
//! `"v": 1`, champ `demande` ; réponse `{"v": 1, "ok": true, …}` ou `{"v": 1, "ok": false, "erreur": …}`.
//! Ce module ne fait que lire et écrire les messages ; le transport (canal nommé) est dans `win::pipe`.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use crate::model::Kind;

/// Version du contrat prise en charge.
pub const VERSION: u64 = 1;
/// Taille maximale d'un message reçu.
pub const MAX_LINE: usize = 64 * 1024;

/// Demandes reçues par Owlery.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// Ramener la fenêtre au premier plan, filtrée sur un type si demandé.
    Show { kind: Option<Kind> },
    /// Lire les forges maintenant.
    Refresh,
    /// Nombre d'entrées en attente (badges), et revues demandées sur un dépôt si son adresse est donnée.
    Status { repo_url: Option<String> },
}

/// Nom d'un type d'entrée dans le contrat.
pub fn kind_id(kind: Kind) -> &'static str {
    match kind {
        Kind::ReviewRequested => "revue",
        Kind::CiFailed => "ci",
        Kind::Mention => "mention",
        Kind::Assigned => "ticket",
        Kind::MyPr => "ma_pr",
    }
}

fn kind_from_id(id: &str) -> Option<Kind> {
    Kind::ALL.into_iter().find(|k| kind_id(*k) == id)
}

/// Lit une demande ; l'erreur est le message renvoyé à l'émetteur.
pub fn parse_request(line: &str) -> Result<Incoming, String> {
    let v: Value =
        serde_json::from_str(line.trim()).map_err(|_| "demande illisible (une ligne JSON attendue)".to_string())?;
    match v["v"].as_u64() {
        Some(n) if n > VERSION => return Err("version".into()),
        Some(_) => {}
        None => return Err("version absente (champ « v »)".into()),
    }
    match v["demande"].as_str() {
        Some("afficher") => {
            let kind = match v["type"].as_str() {
                None => None,
                Some(t) => Some(kind_from_id(t).ok_or_else(|| format!("type inconnu : {t}"))?),
            };
            Ok(Incoming::Show { kind })
        }
        Some("actualiser") => Ok(Incoming::Refresh),
        Some("etat") => Ok(Incoming::Status { repo_url: v["depot_url"].as_str().map(str::to_owned) }),
        Some(other) => Err(format!("demande inconnue : {other}")),
        None => Err("demande absente (champ « demande »)".into()),
    }
}

pub fn ok_reply() -> String {
    json!({ "v": VERSION, "ok": true }).to_string()
}

pub fn error_reply(message: &str) -> String {
    json!({ "v": VERSION, "ok": false, "erreur": message }).to_string()
}

/// Ce que l'interface publie pour répondre à `etat` sans l'attendre.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub waiting: usize,
    pub by_kind: BTreeMap<Kind, usize>,
    /// Revues demandées non lues, par clé de dépôt (`localrepo::item_key`).
    pub reviews_by_repo: BTreeMap<String, usize>,
}

pub fn status_reply(s: &Snapshot, repo_url: Option<&str>) -> String {
    let by_kind: serde_json::Map<String, Value> =
        Kind::ALL.iter().map(|k| (kind_id(*k).to_string(), json!(s.by_kind.get(k).copied().unwrap_or(0)))).collect();
    let mut reply = json!({ "v": VERSION, "ok": true, "en_attente": s.waiting, "par_type": by_kind });
    if let Some(url) = repo_url {
        let Some(key) = crate::localrepo::parse_remote(url) else { return error_reply("adresse de dépôt illisible") };
        reply["depot"] = json!({ "a_relire": s.reviews_by_repo.get(&key).copied().unwrap_or(0) });
    }
    reply.to_string()
}

/// Traite une ligne reçue : la réponse, et la demande à passer à l'interface s'il y en a une.
pub fn handle(line: &str, snapshot: &Snapshot) -> (String, Option<Incoming>) {
    match parse_request(line) {
        Ok(Incoming::Status { repo_url }) => (status_reply(snapshot, repo_url.as_deref()), None),
        Ok(req) => (ok_reply(), Some(req)),
        Err(e) => (error_reply(&e), None),
    }
}

/// Demande *Corriger avec Claude* envoyée à Fawkes : session Claude dans un worktree de la branche, la
/// consigne collée dans l'invite sans être envoyée.
pub fn fawkes_open(dir: &Path, title: &str, branch: Option<&str>, prompt: &str) -> String {
    let mut v = json!({
        "v": VERSION,
        "demande": "ouvrir",
        "de": "owlery",
        "dir": dir.display().to_string(),
        "titre": title,
        "claude": true,
        "consigne": prompt,
    });
    if let Some(b) = branch {
        v["worktree"] = json!(b);
    }
    v.to_string()
}

/// Réponse d'une autre application : `Ok` si elle a accepté, sinon son message.
pub fn parse_reply(line: &str) -> Result<(), String> {
    let v: Value = serde_json::from_str(line.trim()).map_err(|_| "réponse illisible".to_string())?;
    if v["ok"].as_bool() == Some(true) {
        return Ok(());
    }
    Err(match v["erreur"].as_str().or_else(|| v["error"].as_str()) {
        Some("version") => "version du contrat non prise en charge".to_string(),
        Some(e) => e.to_string(),
        None => "demande refusée sans message".to_string(),
    })
}

/// Empreinte stable (FNV-1a 64 bits) d'un dossier d'état, pour nommer le canal et le verrou d'une
/// instance d'essai (`OWLERY_DATA_DIR`) à côté de celle de tous les jours.
fn scope_hash(dir: &Path) -> String {
    let norm = dir.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in norm.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// `\\.\pipe\<application>-<SID>`, suivi de l'empreinte du dossier d'état s'il n'est pas celui par défaut.
pub fn pipe_name(app: &str, sid: &str, data_dir: Option<&Path>) -> String {
    match data_dir {
        None => format!(r"\\.\pipe\{app}-{sid}"),
        Some(d) => format!(r"\\.\pipe\{app}-{sid}-{}", scope_hash(d)),
    }
}

/// Verrou d'instance unique, propre au dossier d'état comme dans Fawkes.
pub fn instance_name(data_dir: Option<&Path>) -> String {
    match data_dir {
        None => r"Local\Owlery-single-instance".to_string(),
        Some(d) => format!(r"Local\Owlery-single-instance:{}", scope_hash(d)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_read() {
        assert_eq!(parse_request(r#"{"v":1,"demande":"afficher"}"#), Ok(Incoming::Show { kind: None }));
        assert_eq!(
            parse_request(r#"{"v":1,"demande":"afficher","type":"revue"}"#),
            Ok(Incoming::Show { kind: Some(Kind::ReviewRequested) })
        );
        assert_eq!(parse_request(r#"{"v":1,"demande":"actualiser","inconnu":3}"#), Ok(Incoming::Refresh));
        assert_eq!(
            parse_request(r#"{"v":1,"demande":"etat","depot_url":"https://github.com/contoso/api.git"}"#),
            Ok(Incoming::Status { repo_url: Some("https://github.com/contoso/api.git".into()) })
        );
    }

    #[test]
    fn unknown_request_is_refused_with_a_message() {
        assert_eq!(parse_request(r#"{"v":1,"demande":"fusionner"}"#), Err("demande inconnue : fusionner".into()));
        assert!(parse_request(r#"{"v":1,"demande":"afficher","type":"tout"}"#).is_err());
        assert!(parse_request("pas du json").is_err());
        assert!(parse_request(r#"{"demande":"afficher"}"#).is_err());
        let (reply, forward) = handle(r#"{"v":1,"demande":"fusionner"}"#, &Snapshot::default());
        assert_eq!(forward, None);
        assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["ok"], false);
    }

    #[test]
    fn newer_version_is_refused() {
        assert_eq!(parse_request(r#"{"v":2,"demande":"afficher"}"#), Err("version".into()));
        assert_eq!(
            parse_reply(r#"{"v":1,"ok":false,"erreur":"version"}"#),
            Err("version du contrat non prise en charge".into())
        );
    }

    #[test]
    fn status_counts_reviews_of_the_asked_repository() {
        let s = Snapshot {
            waiting: 4,
            by_kind: [(Kind::ReviewRequested, 3), (Kind::Mention, 1)].into(),
            reviews_by_repo: [("github.com/contoso/api".to_string(), 2)].into(),
        };
        let v: Value = serde_json::from_str(&status_reply(&s, Some("git@github.com:contoso/api.git"))).unwrap();
        assert_eq!(v["en_attente"], 4);
        assert_eq!(v["par_type"]["revue"], 3);
        assert_eq!(v["par_type"]["ci"], 0);
        assert_eq!(v["depot"]["a_relire"], 2);
        let v: Value = serde_json::from_str(&status_reply(&s, Some("https://github.com/fabrikam/web"))).unwrap();
        assert_eq!(v["depot"]["a_relire"], 0);
        let v: Value = serde_json::from_str(&status_reply(&s, Some("C:\\local"))).unwrap();
        assert_eq!(v["ok"], false);
        let (_, forward) = handle(r#"{"v":1,"demande":"etat"}"#, &s);
        assert_eq!(forward, None);
    }

    #[test]
    fn fawkes_request_carries_dir_branch_and_prompt() {
        let line =
            fawkes_open(Path::new(r"D:\depots\api"), "contoso/api#138", Some("export-csv"), "CI en échec\nLien : …");
        assert!(!line.contains('\n'), "une seule ligne");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["demande"], "ouvrir");
        assert_eq!(v["dir"], r"D:\depots\api");
        assert_eq!(v["worktree"], "export-csv");
        assert_eq!(v["claude"], true);
        assert_eq!(v["consigne"], "CI en échec\nLien : …");
        assert!(
            serde_json::from_str::<Value>(&fawkes_open(Path::new("D:/x"), "t", None, ""))
                .unwrap()
                .get("worktree")
                .is_none()
        );
    }

    #[test]
    fn replies_are_read() {
        assert_eq!(parse_reply(r#"{"v":1,"ok":true}"#), Ok(()));
        assert_eq!(
            parse_reply(r#"{"v":1,"ok":false,"erreur":"dossier introuvable"}"#),
            Err("dossier introuvable".into())
        );
        assert_eq!(
            parse_reply(r#"{"ok":false,"error":"version"}"#),
            Err("version du contrat non prise en charge".into())
        );
        assert!(parse_reply("").is_err());
    }

    #[test]
    fn second_instance_name_depends_on_the_data_dir() {
        assert_eq!(pipe_name("owlery", "S-1-5-21-1", None), r"\\.\pipe\owlery-S-1-5-21-1");
        let a = pipe_name("owlery", "S-1-5-21-1", Some(Path::new(r"C:\Temp\Essai")));
        assert_eq!(a, pipe_name("owlery", "S-1-5-21-1", Some(Path::new("c:/temp/essai/"))));
        assert_ne!(a, pipe_name("owlery", "S-1-5-21-1", None));
        assert_ne!(instance_name(None), instance_name(Some(Path::new(r"C:\Temp\Essai"))));
    }
}
