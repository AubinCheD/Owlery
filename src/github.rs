//! Lecture de GitHub : recherche des PR à relire, de mes PR, des mentions et des tickets assignés, plus
//! l'état de la CI des PR. En lecture seule.
//!
//! Deux accès : le jeton du coffre (HTTP direct), sinon la CLI `gh` si elle est installée et connectée.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::model::{Ci, Forge, Item, Kind};

const API: &str = "https://api.github.com";
const USER_AGENT: &str = concat!("Owlery/", env!("CARGO_PKG_VERSION"));
/// Nombre maximal d'entrées lues par recherche.
const PER_PAGE: usize = 50;
/// Nombre maximal de PR dont on lit le détail (branche, CI) à chaque actualisation.
const MAX_DETAILS: usize = 30;

/// Erreur d'une lecture.
#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    /// Ni jeton, ni CLI `gh`.
    NoAccess,
    /// Jeton refusé, expiré ou sans les droits nécessaires.
    Unauthorized,
    /// Quota d'appels épuisé ; reprise possible à cette date (secondes Unix) si elle est connue.
    RateLimited {
        reset: Option<i64>,
    },
    Network(String),
    Http {
        status: u16,
        body: String,
    },
    Parse(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::NoAccess => {
                f.write_str("Aucun accès à GitHub : ajoutez un jeton, ou installez gh puis lancez `gh auth login`.")
            }
            FetchError::Unauthorized => {
                f.write_str("Accès refusé : le jeton est invalide, expiré ou n'a pas les droits nécessaires.")
            }
            FetchError::RateLimited { reset: Some(t) } => {
                let at = chrono::DateTime::from_timestamp(*t, 0)
                    .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
                    .unwrap_or_default();
                write!(f, "Quota d'appels GitHub épuisé : nouvelle lecture à {at}.")
            }
            FetchError::RateLimited { reset: None } => {
                f.write_str("Quota d'appels GitHub épuisé : nouvelle lecture plus tard.")
            }
            FetchError::Network(m) => write!(f, "Erreur réseau : {m}"),
            FetchError::Http { status, body } => write!(f, "GitHub a répondu {status} : {}", truncate(body, 300)),
            FetchError::Parse(m) => write!(f, "Réponse inattendue de GitHub : {m}"),
        }
    }
}

/// Réponse brute d'un appel.
#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub status: u16,
    pub body: String,
    /// En-têtes `x-ratelimit-remaining` et `x-ratelimit-reset`.
    pub remaining: Option<u32>,
    pub reset: Option<i64>,
}

/// Moyen d'appeler l'API (`path` sans l'hôte : `search/issues?q=…`).
pub trait Transport {
    fn get(&self, path: &str) -> Result<Reply, FetchError>;
    /// Nom affiché de l'accès (« jeton », « gh »).
    fn label(&self) -> &'static str;
}

/// HTTP direct avec le jeton du coffre.
pub struct TokenTransport {
    token: String,
    agent: ureq::Agent,
}

impl TokenTransport {
    pub fn new(token: String) -> Self {
        use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
        let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .proxy(ureq::Proxy::try_from_env())
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .user_agent(USER_AGENT)
            .build()
            .new_agent();
        TokenTransport { token, agent }
    }
}

impl Transport for TokenTransport {
    fn get(&self, path: &str) -> Result<Reply, FetchError> {
        let mut resp = self
            .agent
            .get(format!("{API}/{path}"))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .call()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
        let remaining = header("x-ratelimit-remaining").and_then(|v| v.parse().ok());
        let reset = header("x-ratelimit-reset").and_then(|v| v.parse().ok());
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .with_config()
            .limit(16 * 1024 * 1024)
            .lossy_utf8(true)
            .read_to_string()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        Ok(Reply { status, body, remaining, reset })
    }

    fn label(&self) -> &'static str {
        "jeton"
    }
}

/// La CLI `gh`, appelée à chaque lecture (son jeton reste chez elle).
pub struct GhTransport {
    exe: PathBuf,
}

impl GhTransport {
    /// `gh` trouvé dans le PATH ou à son emplacement d'installation habituel.
    pub fn find() -> Option<Self> {
        let names: &[&str] = if cfg!(windows) { &["gh.exe"] } else { &["gh"] };
        let mut dirs: Vec<PathBuf> =
            std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        if cfg!(windows)
            && let Some(pf) = std::env::var_os("ProgramFiles")
        {
            dirs.push(PathBuf::from(pf).join("GitHub CLI"));
        }
        dirs.iter()
            .flat_map(|d| names.iter().map(move |n| d.join(n)))
            .find(|p| p.is_file())
            .map(|exe| GhTransport { exe })
    }
}

impl Transport for GhTransport {
    fn get(&self, path: &str) -> Result<Reply, FetchError> {
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.args(["api", "-H", "Accept: application/vnd.github+json", path]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let out = cmd.output().map_err(|e| FetchError::Network(format!("gh : {e}")))?;
        let body = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            return Ok(Reply { status: 200, body, ..Reply::default() });
        }
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        Ok(Reply { status: gh_status(&err), body: if body.trim().is_empty() { err } else { body }, ..Reply::default() })
    }

    fn label(&self) -> &'static str {
        "gh"
    }
}

/// Statut HTTP lu dans le message d'erreur de `gh` (« HTTP 401: Bad credentials »), 401 s'il n'est pas connecté.
fn gh_status(stderr: &str) -> u16 {
    if let Some(i) = stderr.find("HTTP ")
        && let Some(code) = stderr[i + 5..].get(..3).and_then(|c| c.parse().ok())
    {
        return code;
    }
    if stderr.contains("gh auth login") { 401 } else { 500 }
}

fn check(reply: Reply) -> Result<Value, FetchError> {
    match reply.status {
        200..=299 => serde_json::from_str(&reply.body).map_err(|e| FetchError::Parse(e.to_string())),
        403 | 429 if reply.remaining == Some(0) || reply.body.to_lowercase().contains("rate limit") => {
            Err(FetchError::RateLimited { reset: reply.reset })
        }
        401 | 403 => Err(FetchError::Unauthorized),
        status => Err(FetchError::Http { status, body: truncate(&reply.body, 500) }),
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Lit tout ce qui attend l'utilisateur. Une erreur sur le détail d'une PR (branche, CI) n'arrête pas la
/// lecture : l'entrée est gardée sans ce détail.
pub fn fetch(t: &dyn Transport) -> Result<Vec<Item>, FetchError> {
    let mut items = Vec::new();
    let searches = [
        ("is:open is:pr review-requested:@me archived:false", Kind::ReviewRequested),
        ("is:open is:pr author:@me archived:false", Kind::MyPr),
        ("is:open mentions:@me archived:false", Kind::Mention),
        ("is:open is:issue assignee:@me archived:false", Kind::Assigned),
    ];
    for (query, kind) in searches {
        let path = format!("search/issues?q={}&sort=updated&order=desc&per_page={PER_PAGE}", encode(query));
        let value = check(t.get(&path)?)?;
        let found = value["items"].as_array().ok_or_else(|| FetchError::Parse("liste « items » absente".into()))?;
        items.extend(found.iter().filter_map(|v| parse_search_item(v, kind)));
    }
    for item in items.iter_mut().filter(|i| matches!(i.kind, Kind::ReviewRequested | Kind::MyPr)).take(MAX_DETAILS) {
        match pull_details(t, &item.repo, item.number.unwrap_or_default()) {
            Ok((branch, ci)) => {
                item.branch = branch;
                item.ci = ci;
            }
            Err(e @ FetchError::RateLimited { .. }) => return Err(e),
            Err(_) => {}
        }
        if item.kind == Kind::MyPr && item.ci == Some(Ci::Failure) {
            item.kind = Kind::CiFailed;
            item.id = Item::make_id(item.forge, &item.repo, item.number, item.kind);
        }
    }
    Ok(items)
}

/// Encodage d'une requête de recherche pour l'adresse.
fn encode(q: &str) -> String {
    q.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' | b'@' => {
                (b as char).to_string()
            }
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn parse_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp())
}

/// Début de la description, sur une ligne.
fn summary(body: &str) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 200 { format!("{}…", truncate(&flat, 200)) } else { flat }
}

/// Une entrée de `search/issues`.
fn parse_search_item(v: &Value, kind: Kind) -> Option<Item> {
    let number = v["number"].as_u64()?;
    let repo = v["repository_url"].as_str()?.rsplitn(3, '/').take(2).collect::<Vec<_>>();
    let repo = format!("{}/{}", repo.get(1)?, repo.first()?);
    Some(Item {
        id: Item::make_id(Forge::GitHub, &repo, Some(number), kind),
        forge: Forge::GitHub,
        kind,
        number: Some(number),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        url: v["html_url"].as_str()?.to_string(),
        branch: None,
        author: v["user"]["login"].as_str().unwrap_or_default().to_string(),
        updated_at: v["updated_at"].as_str().and_then(parse_time).unwrap_or(0),
        ci: None,
        summary: summary(v["body"].as_str().unwrap_or_default()),
        repo,
    })
}

/// Branche et état de la CI d'une PR (vérifications et statuts du dernier commit).
fn pull_details(t: &dyn Transport, repo: &str, number: u64) -> Result<(Option<String>, Option<Ci>), FetchError> {
    let pull = check(t.get(&format!("repos/{repo}/pulls/{number}"))?)?;
    let branch = pull["head"]["ref"].as_str().map(str::to_owned);
    let Some(sha) = pull["head"]["sha"].as_str() else { return Ok((branch, None)) };
    let runs = check(t.get(&format!("repos/{repo}/commits/{sha}/check-runs?per_page=100"))?)?;
    let status = check(t.get(&format!("repos/{repo}/commits/{sha}/status"))?)?;
    Ok((branch, ci_state(&runs, &status)))
}

/// Échec si une vérification ou un statut a échoué, en cours si l'un n'est pas fini, sinon succès ; rien
/// s'il n'y a aucune CI.
pub fn ci_state(runs: &Value, status: &Value) -> Option<Ci> {
    let mut states = Vec::new();
    for run in runs["check_runs"].as_array().into_iter().flatten() {
        states.push(match (run["status"].as_str(), run["conclusion"].as_str()) {
            (
                Some("completed"),
                Some("failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure"),
            ) => Ci::Failure,
            (Some("completed"), _) => Ci::Success,
            _ => Ci::Pending,
        });
    }
    if status["total_count"].as_u64().unwrap_or(0) > 0 {
        states.push(match status["state"].as_str() {
            Some("failure" | "error") => Ci::Failure,
            Some("pending") => Ci::Pending,
            _ => Ci::Success,
        });
    }
    [Ci::Failure, Ci::Pending, Ci::Success].into_iter().find(|s| states.contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Réponses enregistrées, par début de chemin.
    struct Fake(BTreeMap<&'static str, Reply>);

    impl Transport for Fake {
        fn get(&self, path: &str) -> Result<Reply, FetchError> {
            self.0
                .iter()
                .find(|(prefix, _)| path.starts_with(*prefix))
                .map(|(_, r)| r.clone())
                .ok_or_else(|| FetchError::Http { status: 404, body: path.into() })
        }
        fn label(&self) -> &'static str {
            "test"
        }
    }

    fn ok(body: &str) -> Reply {
        Reply { status: 200, body: body.into(), ..Reply::default() }
    }

    fn search(items: &str) -> Reply {
        ok(&format!(r#"{{"total_count":1,"items":[{items}]}}"#))
    }

    fn issue(repo: &str, n: u64, title: &str) -> String {
        format!(
            r#"{{"number":{n},"title":"{title}","html_url":"https://github.com/{repo}/pull/{n}",
            "repository_url":"https://api.github.com/repos/{repo}","user":{{"login":"camille"}},
            "updated_at":"2026-10-20T08:30:00Z","body":"Première ligne\n\nsuite"}}"#
        )
    }

    fn fake() -> Fake {
        let q = |s: &str| format!("search/issues?q={}", encode(s));
        let mut m = BTreeMap::new();
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        m.insert(
            leak(q("is:open is:pr review-requested:@me")),
            search(&issue("contoso/api", 142, "Ajoute la pagination")),
        );
        m.insert(leak(q("is:open is:pr author:@me")), search(&issue("contoso/api", 138, "Corrige l'export")));
        m.insert(leak(q("is:open mentions:@me")), search(&issue("fabrikam/web", 57, "Question sur le cache")));
        m.insert(leak(q("is:open is:issue assignee:@me")), search(""));
        m.insert("repos/contoso/api/pulls/142", ok(r#"{"head":{"ref":"pagination","sha":"aaa"}}"#));
        m.insert("repos/contoso/api/pulls/138", ok(r#"{"head":{"ref":"export","sha":"bbb"}}"#));
        m.insert(
            "repos/contoso/api/commits/aaa/check-runs",
            ok(r#"{"check_runs":[{"status":"completed","conclusion":"success"}]}"#),
        );
        m.insert("repos/contoso/api/commits/aaa/status", ok(r#"{"state":"pending","total_count":0}"#));
        m.insert(
            "repos/contoso/api/commits/bbb/check-runs",
            ok(r#"{"check_runs":[{"status":"completed","conclusion":"failure"}]}"#),
        );
        m.insert("repos/contoso/api/commits/bbb/status", ok(r#"{"state":"success","total_count":1}"#));
        Fake(m)
    }

    #[test]
    fn fetch_reads_every_list_and_the_ci_of_pull_requests() {
        let items = fetch(&fake()).unwrap();
        assert_eq!(items.len(), 3);
        let review = items.iter().find(|i| i.number == Some(142)).unwrap();
        assert_eq!(review.kind, Kind::ReviewRequested);
        assert_eq!(review.repo, "contoso/api");
        assert_eq!(review.branch.as_deref(), Some("pagination"));
        assert_eq!(review.ci, Some(Ci::Success));
        assert_eq!(review.author, "camille");
        assert_eq!(review.summary, "Première ligne suite");
        assert_eq!(review.updated_at, 1_792_485_000);
        let mine = items.iter().find(|i| i.number == Some(138)).unwrap();
        assert_eq!(mine.kind, Kind::CiFailed);
        assert_eq!(mine.id, "GitHub:contoso/api#138:CiFailed");
        assert_eq!(items.iter().find(|i| i.number == Some(57)).unwrap().kind, Kind::Mention);
    }

    #[test]
    fn a_missing_pull_detail_keeps_the_entry() {
        let mut f = fake();
        f.0.remove("repos/contoso/api/pulls/142");
        let items = fetch(&f).unwrap();
        let review = items.iter().find(|i| i.number == Some(142)).unwrap();
        assert_eq!(review.branch, None);
    }

    #[test]
    fn errors_are_classified() {
        let r = |status, remaining, body: &str| Reply { status, body: body.into(), remaining, reset: Some(5) };
        assert_eq!(check(r(401, None, "")), Err(FetchError::Unauthorized));
        assert_eq!(check(r(403, Some(0), "")), Err(FetchError::RateLimited { reset: Some(5) }));
        assert_eq!(check(r(403, None, "API rate limit exceeded")), Err(FetchError::RateLimited { reset: Some(5) }));
        assert_eq!(check(r(403, Some(10), "forbidden")), Err(FetchError::Unauthorized));
        assert!(matches!(check(r(500, None, "x")), Err(FetchError::Http { status: 500, .. })));
        assert!(matches!(check(r(200, None, "pas du json")), Err(FetchError::Parse(_))));
        let mut f = fake();
        f.0.insert("search", r(401, None, ""));
        assert_eq!(fetch(&f), Err(FetchError::Unauthorized));
    }

    #[test]
    fn gh_errors_give_a_status() {
        assert_eq!(gh_status("gh: Bad credentials (HTTP 401)"), 401);
        assert_eq!(gh_status("To get started with GitHub CLI, please run:  gh auth login"), 401);
        assert_eq!(gh_status("autre"), 500);
    }

    #[test]
    fn ci_state_prefers_failure_then_pending() {
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        let none = v(r#"{"check_runs":[]}"#);
        let no_status = v(r#"{"total_count":0}"#);
        assert_eq!(ci_state(&none, &no_status), None);
        let pending = v(r#"{"check_runs":[{"status":"in_progress"},{"status":"completed","conclusion":"success"}]}"#);
        assert_eq!(ci_state(&pending, &no_status), Some(Ci::Pending));
        assert_eq!(ci_state(&pending, &v(r#"{"total_count":1,"state":"error"}"#)), Some(Ci::Failure));
        let skipped = v(r#"{"check_runs":[{"status":"completed","conclusion":"skipped"}]}"#);
        assert_eq!(ci_state(&skipped, &no_status), Some(Ci::Success));
    }

    #[test]
    fn queries_are_encoded() {
        assert_eq!(encode("is:open author:@me"), "is:open+author:@me");
        assert_eq!(encode("a&b"), "a%26b");
    }
}

/// Accès à GitHub, dans l'ordre : le jeton du coffre, sinon `gh` s'il est installé.
pub fn access(store: &dyn crate::secrets::SecretStore) -> Result<Box<dyn Transport + Send>, FetchError> {
    access_with(store, GhTransport::find)
}

fn access_with(
    store: &dyn crate::secrets::SecretStore,
    find_gh: impl FnOnce() -> Option<GhTransport>,
) -> Result<Box<dyn Transport + Send>, FetchError> {
    if let Some(token) = store.get(crate::secrets::GITHUB_TOKEN).ok().flatten().filter(|t| !t.trim().is_empty()) {
        return Ok(Box::new(TokenTransport::new(token)));
    }
    find_gh().map(|gh| Box::new(gh) as Box<dyn Transport + Send>).ok_or(FetchError::NoAccess)
}

#[cfg(test)]
mod access_tests {
    use super::*;
    use crate::secrets::{GITHUB_TOKEN, MemoryStore, SecretStore};

    #[test]
    fn the_stored_token_comes_first_then_gh_then_nothing() {
        let store = MemoryStore::default();
        assert_eq!(access_with(&store, || None).err(), Some(FetchError::NoAccess));
        let gh = || Some(GhTransport { exe: PathBuf::from("gh.exe") });
        assert_eq!(access_with(&store, gh).unwrap().label(), "gh");
        store.set(GITHUB_TOKEN, "ghp_fictif").unwrap();
        assert_eq!(access_with(&store, gh).unwrap().label(), "jeton");
    }
}
