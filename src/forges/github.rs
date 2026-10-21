//! Lecture de GitHub (github.com ou GitHub Enterprise) : recherche des PR à relire, de mes PR, des
//! mentions et des tickets assignés, plus la branche et la CI des PR. En lecture seule.
//!
//! Deux accès : le jeton du coffre (HTTP direct), ou la CLI `gh`, appelée à chaque lecture (son jeton reste
//! chez elle).

use std::io::Read as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::http::{Http, Reply, Request};
use super::{
    Account, FetchError, Fetched, Identity, LAST_COMMENTS, MAX_DETAILS, PER_PAGE, Seen, check, comment, dedupe,
    parse_time, summary,
};
use crate::model::{Check, Ci, Comment, Forge, Item, Kind, Target};

/// Durée maximale d'un appel à `gh`.
const GH_TIMEOUT: Duration = Duration::from_secs(60);
/// Nombre maximal d'entrées dont on vérifie si la dernière activité est la mienne, à chaque lecture.
const MAX_SELF_CHECKS: usize = 10;

/// Moyen d'appeler l'API (`path` sans l'hôte : `search/issues?q=…`).
pub trait Transport: Send {
    fn get(&self, path: &str) -> Result<Reply, FetchError>;
    /// Nom affiché de l'accès (« jeton », « gh »).
    fn label(&self) -> &'static str;
}

/// Adresse de l'API : `api.github.com`, ou `/api/v3` de GitHub Enterprise.
pub fn api_base(host: &str) -> String {
    if host == "github.com" { "https://api.github.com".into() } else { format!("https://{host}/api/v3") }
}

/// HTTP direct avec le jeton du coffre.
pub struct TokenTransport {
    http: Arc<dyn Http>,
    base: String,
    token: String,
}

impl TokenTransport {
    pub fn new(http: Arc<dyn Http>, host: &str, token: String) -> Self {
        TokenTransport { http, base: api_base(host), token }
    }
}

impl Transport for TokenTransport {
    fn get(&self, path: &str) -> Result<Reply, FetchError> {
        let req = Request::get(format!("{}/{path}", self.base))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        self.http.send(&req)
    }

    fn label(&self) -> &'static str {
        "jeton"
    }
}

/// La CLI `gh`.
pub struct GhTransport {
    exe: PathBuf,
    host: String,
}

impl GhTransport {
    /// `gh` trouvé dans le PATH ou à son emplacement d'installation habituel.
    pub fn find(host: &str) -> Option<Self> {
        let name = if cfg!(windows) { "gh.exe" } else { "gh" };
        let mut dirs: Vec<PathBuf> =
            std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        if cfg!(windows)
            && let Some(pf) = std::env::var_os("ProgramFiles")
        {
            dirs.push(PathBuf::from(pf).join("GitHub CLI"));
        }
        let exe = dirs.iter().map(|d| d.join(name)).find(|p| p.is_file())?;
        Some(GhTransport { exe, host: host.to_string() })
    }
}

impl Transport for GhTransport {
    fn get(&self, path: &str) -> Result<Reply, FetchError> {
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.arg("api");
        if !self.host.is_empty() && self.host != "github.com" {
            cmd.args(["--hostname", &self.host]);
        }
        cmd.args(["-H", "Accept: application/vnd.github+json", path]);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| FetchError::Network(format!("gh : {e}")))?;
        // Sorties lues à part : une grosse réponse remplirait le tube et bloquerait `gh`.
        let read_all = |mut r: Box<dyn std::io::Read + Send>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = r.read_to_end(&mut buf);
                buf
            })
        };
        let out = read_all(Box::new(child.stdout.take().expect("sortie capturée")));
        let err = read_all(Box::new(child.stderr.take().expect("erreurs capturées")));
        let deadline = Instant::now() + GH_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(30)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(FetchError::Network(format!("gh ne répond pas après {} s", GH_TIMEOUT.as_secs())));
                }
                Err(e) => return Err(FetchError::Network(format!("gh : {e}"))),
            }
        };
        let body = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
        if status.success() {
            return Ok(Reply::ok(&body));
        }
        let stderr = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
        Ok(Reply::status(gh_status(&stderr), if body.trim().is_empty() { &stderr } else { &body }))
    }

    fn label(&self) -> &'static str {
        "gh"
    }
}

/// Statut HTTP lu dans le message d'erreur de `gh` (« HTTP 401: Bad credentials »), 401 s'il n'est pas connecté.
fn gh_status(stderr: &str) -> u16 {
    if let Some(code) = stderr.find("HTTP ").and_then(|i| stderr[i + 5..].get(..3)).and_then(|c| c.parse().ok()) {
        return code;
    }
    if stderr.contains("gh auth login") { 401 } else { 500 }
}

pub fn verify(t: &dyn Transport) -> Result<Identity, FetchError> {
    let user = check(t.get("user")?)?;
    let login = user["login"].as_str().ok_or_else(|| FetchError::Parse("« login » absent".into()))?;
    Ok(Identity {
        username: login.to_string(),
        display_name: user["name"].as_str().unwrap_or_default().to_string(),
        orgs: Vec::new(),
    })
}

/// Recherches, dans l'ordre. La recherche exige `is:pr` ou `is:issue` : les mentions sont donc lues deux fois.
const SEARCHES: [(&str, Kind, &str); 5] = [
    ("is:open is:pr review-requested:@me archived:false", Kind::ReviewRequested, "PR à relire"),
    ("is:open is:pr author:@me archived:false", Kind::MyPr, "Mes PR"),
    ("is:open is:pr mentions:@me archived:false", Kind::Mention, "Mentions (PR)"),
    ("is:open is:issue mentions:@me archived:false", Kind::Mention, "Mentions (tickets)"),
    ("is:open is:issue assignee:@me archived:false", Kind::Assigned, "Tickets assignés"),
];

/// Lit tout ce qui attend l'utilisateur. Une recherche en échec fait échouer la lecture (le cache du compte
/// est alors gardé tel quel) ; une erreur sur le détail d'une PR n'arrête pas la lecture : l'entrée est marquée
/// incomplète, et la fusion reprend son détail du cache.
pub fn fetch(t: &dyn Transport, account: &Account, seen: &Seen) -> Result<Fetched, FetchError> {
    let mut items = Vec::new();
    let mut notes = Vec::new();
    for (query, kind, label) in SEARCHES {
        let path = format!("search/issues?q={}&sort=updated&order=desc&per_page={PER_PAGE}", encode(query));
        let value = check(t.get(&path)?)?;
        let found = value["items"].as_array().ok_or_else(|| FetchError::Parse("liste « items » absente".into()))?;
        let total = value["total_count"].as_u64().unwrap_or(0) as usize;
        if total > found.len() {
            notes.push(format!("{label} : {} affichées sur {total}", found.len()));
        } else if value["incomplete_results"].as_bool() == Some(true) {
            notes.push(format!("{label} : résultats incomplets (délai de recherche de GitHub dépassé)"));
        }
        items.extend(found.iter().filter_map(|v| parse_search_item(v, kind, account)));
    }
    let mut items = dedupe(items);
    let me = if account.username.is_empty() { verify(t)?.username } else { account.username.clone() };

    // Détail : mes PR d'abord (leur CI et leur activité comptent), puis les PR à relire, chacune avec son budget.
    let mut detail_error = None;
    for kind in [Kind::MyPr, Kind::ReviewRequested] {
        for item in items.iter_mut().filter(|i| i.kind == kind).take(MAX_DETAILS) {
            match pull_details(t, item, &me) {
                Ok(()) => {}
                Err(e @ FetchError::RateLimited { .. }) => return Err(e),
                Err(e) => {
                    item.partial = true;
                    detail_error.get_or_insert(e);
                }
            }
        }
    }
    if let Some(e) = detail_error {
        notes.push(format!("Détail de certaines PR indisponible (branche, CI gardées du cache) : {e}"));
    }

    // Activité de l'utilisateur lui-même : une entrée lue qui a bougé reste lue si le dernier commentaire est le sien.
    let candidates: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| seen.get(&i.id).is_some_and(|s| i.updated_at > *s) && i.comments > 0)
        .map(|(n, _)| n)
        .take(MAX_SELF_CHECKS)
        .collect();
    for n in candidates {
        match comments(t, &items[n]) {
            Ok(list) => items[n].last_by_me = list.last().is_some_and(|c| is_mine(c, &me, items[n].updated_at)),
            Err(e @ FetchError::RateLimited { .. }) => return Err(e),
            Err(_) => {}
        }
    }
    Ok(Fetched { items, notes })
}

/// Le commentaire est-il de moi, et assez récent pour être la dernière activité ?
pub(crate) fn is_mine(c: &Comment, me: &str, updated_at: i64) -> bool {
    !me.is_empty() && c.author.eq_ignore_ascii_case(me) && c.at + 120 >= updated_at
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

/// Une entrée de `search/issues`.
fn parse_search_item(v: &Value, kind: Kind, account: &Account) -> Option<Item> {
    let number = v["number"].as_u64()?;
    let mut parts = v["repository_url"].as_str()?.rsplitn(3, '/');
    let (name, owner) = (parts.next()?, parts.next()?);
    let repo = format!("{owner}/{name}");
    let pull = v.get("pull_request").is_some_and(|p| !p.is_null());
    Some(Item {
        id: Item::make_id(Forge::GitHub, &account.host, &repo, Some(number), pull),
        account: account.id.clone(),
        forge: Forge::GitHub,
        kind,
        number: Some(number),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        url: v["html_url"].as_str()?.to_string(),
        author: v["user"]["login"].as_str().unwrap_or_default().to_string(),
        updated_at: v["updated_at"].as_str().and_then(parse_time).unwrap_or(0),
        summary: summary(v["body"].as_str().unwrap_or_default()),
        comments: v["comments"].as_u64().unwrap_or(0) as u32,
        target: if pull { Target::Pull } else { Target::Issue },
        repo,
        ..Item::default()
    })
}

/// Branche, CI et vérifications en échec d'une PR ; pour mes PR, le compteur d'activité (commentaires et revues
/// des autres) et le passage à « CI en échec ».
fn pull_details(t: &dyn Transport, item: &mut Item, me: &str) -> Result<(), FetchError> {
    let (repo, number) = (item.repo.clone(), item.number.unwrap_or_default());
    let pull = check(t.get(&format!("repos/{repo}/pulls/{number}"))?)?;
    item.branch = pull["head"]["ref"].as_str().map(str::to_owned);
    if let Some(sha) = pull["head"]["sha"].as_str() {
        let runs = check(t.get(&format!("repos/{repo}/commits/{sha}/check-runs?per_page=100"))?)?;
        let status = check(t.get(&format!("repos/{repo}/commits/{sha}/status"))?)?;
        item.ci = ci_state(&runs, &status);
        item.checks = failing_checks(&runs, &status);
    }
    if item.kind == Kind::MyPr {
        let reviews = check(t.get(&format!("repos/{repo}/pulls/{number}/reviews?per_page=100"))?)?;
        let by_others = reviews
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["user"]["login"].as_str().is_some_and(|l| !l.eq_ignore_ascii_case(me)))
            .count() as u32;
        let comments = pull["comments"].as_u64().unwrap_or(0) + pull["review_comments"].as_u64().unwrap_or(0);
        item.activity = Some(comments as u32 + by_others);
        if item.ci == Some(Ci::Failure) {
            item.kind = Kind::CiFailed;
        }
    }
    Ok(())
}

const FAILED: [&str; 5] = ["failure", "timed_out", "cancelled", "action_required", "startup_failure"];

/// Échec si une vérification ou un statut a échoué, en cours si l'un n'est pas fini, sinon succès ; rien
/// s'il n'y a aucune CI.
pub fn ci_state(runs: &Value, status: &Value) -> Option<Ci> {
    let mut states = Vec::new();
    for run in runs["check_runs"].as_array().into_iter().flatten() {
        states.push(match (run["status"].as_str(), run["conclusion"].as_str()) {
            (Some("completed"), Some(c)) if FAILED.contains(&c) => Ci::Failure,
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

/// Vérifications et statuts en échec, avec leur page.
fn failing_checks(runs: &Value, status: &Value) -> Vec<Check> {
    let mut out = Vec::new();
    for run in runs["check_runs"].as_array().into_iter().flatten() {
        if run["status"].as_str() == Some("completed")
            && run["conclusion"].as_str().is_some_and(|c| FAILED.contains(&c))
        {
            let url = run["html_url"].as_str().or_else(|| run["details_url"].as_str()).unwrap_or_default();
            out.push(Check { name: run["name"].as_str().unwrap_or("vérification").into(), url: url.into() });
        }
    }
    for s in status["statuses"].as_array().into_iter().flatten() {
        if matches!(s["state"].as_str(), Some("failure" | "error")) {
            out.push(Check {
                name: s["context"].as_str().unwrap_or("statut").into(),
                url: s["target_url"].as_str().unwrap_or_default().into(),
            });
        }
    }
    out
}

/// Derniers commentaires d'une PR ou d'un ticket : la dernière page des commentaires (et l'avant-dernière si
/// elle ne suffit pas).
pub fn comments(t: &dyn Transport, item: &Item) -> Result<Vec<Comment>, FetchError> {
    let (Some(number), n) = (item.number, item.comments as usize) else { return Ok(Vec::new()) };
    if n == 0 || matches!(item.target, Target::Unknown) {
        return Ok(Vec::new());
    }
    let per = LAST_COMMENTS;
    let last = n.div_ceil(per);
    let page = |p: usize| -> Result<Vec<Comment>, FetchError> {
        let v = check(t.get(&format!("repos/{}/issues/{number}/comments?per_page={per}&page={p}", item.repo))?)?;
        Ok(v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                Some(comment(
                    c["user"]["login"].as_str().unwrap_or_default(),
                    c["created_at"].as_str().and_then(parse_time),
                    c["body"].as_str()?,
                ))
            })
            .collect())
    };
    let mut out = page(last)?;
    if out.len() < per && last > 1 {
        let mut before = page(last - 1)?;
        before.append(&mut out);
        out = before;
    }
    let skip = out.len().saturating_sub(per);
    Ok(out.split_off(skip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forges::Auth;
    use std::collections::BTreeMap;

    /// Réponses enregistrées, par début de chemin (le plus long l'emporte).
    struct Fake(BTreeMap<String, Reply>);

    impl Transport for Fake {
        fn get(&self, path: &str) -> Result<Reply, FetchError> {
            Ok(self
                .0
                .iter()
                .filter(|(prefix, _)| path.starts_with(prefix.as_str()))
                .max_by_key(|(prefix, _)| prefix.len())
                .map(|(_, r)| r.clone())
                .unwrap_or_else(|| Reply::status(404, path)))
        }
        fn label(&self) -> &'static str {
            "test"
        }
    }

    fn account() -> Account {
        let mut a = Account::new(Forge::GitHub, "", vec![], Auth::Token);
        a.id = "gh1".into();
        a.username = "camille".into();
        a
    }

    fn search(items: &str) -> Reply {
        let total = usize::from(!items.is_empty());
        Reply::ok(&format!(r#"{{"total_count":{total},"incomplete_results":false,"items":[{items}]}}"#))
    }

    fn issue(repo: &str, n: u64, title: &str, pull: bool, comments: u32) -> String {
        let pr = if pull { r#","pull_request":{"url":"x"}"# } else { "" };
        format!(
            r#"{{"number":{n},"title":"{title}","html_url":"https://github.com/{repo}/pull/{n}",
            "repository_url":"https://api.github.com/repos/{repo}","user":{{"login":"dominique"}},
            "updated_at":"2026-10-20T08:30:00Z","body":"Première ligne\n\nsuite","comments":{comments}{pr}}}"#
        )
    }

    fn fake() -> Fake {
        let q = |s: &str| format!("search/issues?q={}", encode(s));
        let mut m = BTreeMap::new();
        m.insert(q(SEARCHES[0].0), search(&issue("contoso/api", 142, "Ajoute la pagination", true, 0)));
        m.insert(q(SEARCHES[1].0), search(&issue("contoso/api", 138, "Corrige l'export", true, 2)));
        // La PR à relire me mentionne aussi : une seule entrée.
        m.insert(q(SEARCHES[2].0), search(&issue("contoso/api", 142, "Ajoute la pagination", true, 0)));
        m.insert(q(SEARCHES[3].0), search(&issue("fabrikam/web", 57, "Question sur le cache", false, 4)));
        m.insert(q(SEARCHES[4].0), search(""));
        m.insert("repos/contoso/api/pulls/142".into(), Reply::ok(r#"{"head":{"ref":"pagination","sha":"aaa"}}"#));
        m.insert(
            "repos/contoso/api/pulls/138".into(),
            Reply::ok(r#"{"head":{"ref":"export","sha":"bbb"},"comments":2,"review_comments":3}"#),
        );
        m.insert(
            "repos/contoso/api/pulls/138/reviews".into(),
            Reply::ok(r#"[{"user":{"login":"dominique"}},{"user":{"login":"Camille"}}]"#),
        );
        m.insert(
            "repos/contoso/api/commits/aaa/check-runs".into(),
            Reply::ok(r#"{"check_runs":[{"status":"completed","conclusion":"success"}]}"#),
        );
        m.insert("repos/contoso/api/commits/aaa/status".into(), Reply::ok(r#"{"state":"pending","total_count":0}"#));
        m.insert(
            "repos/contoso/api/commits/bbb/check-runs".into(),
            Reply::ok(r#"{"check_runs":[{"name":"tests","status":"completed","conclusion":"failure","html_url":"https://github.com/contoso/api/runs/9"}]}"#),
        );
        m.insert(
            "repos/contoso/api/commits/bbb/status".into(),
            Reply::ok(r#"{"state":"success","total_count":1,"statuses":[]}"#),
        );
        Fake(m)
    }

    #[test]
    fn fetch_reads_every_list_and_the_ci_of_pull_requests() {
        let got = fetch(&fake(), &account(), &Seen::new()).unwrap();
        let items = got.items;
        assert_eq!(items.len(), 3, "{:?}", items.iter().map(|i| &i.id).collect::<Vec<_>>());
        let review = items.iter().find(|i| i.number == Some(142)).unwrap();
        assert_eq!(review.kind, Kind::ReviewRequested, "la revue gagne sur la mention");
        assert_eq!(review.repo, "contoso/api");
        assert_eq!(review.branch.as_deref(), Some("pagination"));
        assert_eq!(review.ci, Some(Ci::Success));
        assert_eq!(review.summary, "Première ligne suite");
        assert_eq!(review.updated_at, 1_792_485_000);
        assert_eq!(review.account, "gh1");
        let mine = items.iter().find(|i| i.number == Some(138)).unwrap();
        assert_eq!(mine.kind, Kind::CiFailed);
        assert_eq!(mine.id, "GitHub:contoso/api#138");
        assert_eq!(
            mine.checks,
            vec![Check { name: "tests".into(), url: "https://github.com/contoso/api/runs/9".into() }]
        );
        // 2 commentaires + 3 commentaires de revue + 1 revue d'un autre (la mienne ne compte pas).
        assert_eq!(mine.activity, Some(6));
        let mention = items.iter().find(|i| i.number == Some(57)).unwrap();
        assert_eq!(mention.kind, Kind::Mention);
        assert_eq!(mention.target, Target::Issue);
        assert!(got.notes.is_empty(), "{:?}", got.notes);
    }

    #[test]
    fn a_missing_pull_detail_keeps_the_entry_and_says_so() {
        let mut f = fake();
        f.0.remove("repos/contoso/api/pulls/142");
        let got = fetch(&f, &account(), &Seen::new()).unwrap();
        let review = got.items.iter().find(|i| i.number == Some(142)).unwrap();
        assert_eq!(review.branch, None);
        assert!(review.partial);
        assert_eq!(got.notes.len(), 1);
    }

    #[test]
    fn truncated_lists_are_reported() {
        let mut f = fake();
        let q = format!("search/issues?q={}", encode(SEARCHES[0].0));
        f.0.insert(
            q,
            Reply::ok(&format!(r#"{{"total_count":80,"items":[{}]}}"#, issue("contoso/api", 142, "t", true, 0))),
        );
        let got = fetch(&f, &account(), &Seen::new()).unwrap();
        assert_eq!(got.notes, vec!["PR à relire : 1 affichées sur 80".to_string()]);
    }

    #[test]
    fn my_own_last_comment_is_detected_for_entries_already_seen() {
        let mut f = fake();
        f.0.insert(
            "repos/fabrikam/web/issues/57/comments".into(),
            Reply::ok(
                r#"[{"user":{"login":"alex"},"created_at":"2026-10-20T07:00:00Z","body":"question"},
                         {"user":{"login":"camille"},"created_at":"2026-10-20T08:29:30Z","body":"réponse"}]"#,
            ),
        );
        let seen = Seen::from([("GitHub:fabrikam/web#57".to_string(), 0)]);
        let got = fetch(&f, &account(), &seen).unwrap();
        assert!(got.items.iter().find(|i| i.number == Some(57)).unwrap().last_by_me);
        // Sans choix enregistré, rien n'est vérifié (pas d'appel inutile).
        let got = fetch(&f, &account(), &Seen::new()).unwrap();
        assert!(!got.items.iter().find(|i| i.number == Some(57)).unwrap().last_by_me);
    }

    #[test]
    fn github_last_comments_use_the_last_page() {
        let mut f = fake();
        f.0.insert(
            "repos/fabrikam/web/issues/57/comments?per_page=3&page=2".into(),
            Reply::ok(r#"[{"user":{"login":"sam"},"created_at":"2026-10-20T08:00:00Z","body":"quatre"}]"#),
        );
        f.0.insert(
            "repos/fabrikam/web/issues/57/comments?per_page=3&page=1".into(),
            Reply::ok(r#"[{"user":{"login":"a"},"body":"un"},{"user":{"login":"b"},"body":"deux"},{"user":{"login":"c"},"body":"trois"}]"#),
        );
        let item = Item {
            repo: "fabrikam/web".into(),
            number: Some(57),
            comments: 4,
            target: Target::Issue,
            ..Item::default()
        };
        let c = comments(&f, &item).unwrap();
        assert_eq!(c.iter().map(|c| c.body.as_str()).collect::<Vec<_>>(), ["deux", "trois", "quatre"]);
        assert_eq!(c[2].author, "sam");
    }

    #[test]
    fn enterprise_host_uses_api_v3() {
        assert_eq!(api_base("github.com"), "https://api.github.com");
        assert_eq!(api_base("ghe.contoso.test"), "https://ghe.contoso.test/api/v3");
        let mut a = account();
        a.host = "ghe.contoso.test".into();
        let got = fetch(&fake(), &a, &Seen::new()).unwrap();
        assert!(got.items.iter().all(|i| i.id.starts_with("GitHub@ghe.contoso.test:")));
    }

    #[test]
    fn missing_field_skips_the_entry() {
        let mut f = fake();
        f.0.insert(
            format!("search/issues?q={}", encode(SEARCHES[4].0)),
            search(r#"{"number":3,"title":"sans dépôt"}"#),
        );
        let got = fetch(&f, &account(), &Seen::new()).unwrap();
        assert_eq!(got.items.len(), 3);
    }

    #[test]
    fn errors_stop_the_read() {
        let mut f = fake();
        f.0.insert("search".into(), Reply::status(401, ""));
        // Les routes plus précises gagnent : on retire celles des recherches.
        f.0.retain(|k, _| !k.starts_with("search/"));
        assert_eq!(fetch(&f, &account(), &Seen::new()).err(), Some(FetchError::Unauthorized));
        let mut f = fake();
        f.0.insert(
            "repos/contoso/api/pulls/138".into(),
            Reply { status: 403, remaining: Some(0), reset: Some(9), ..Reply::default() },
        );
        assert_eq!(fetch(&f, &account(), &Seen::new()).err(), Some(FetchError::RateLimited { reset: Some(9) }));
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
        let failed_status = v(
            r#"{"total_count":1,"state":"error","statuses":[{"context":"ci/ext","state":"error","target_url":"https://ci.test/1"}]}"#,
        );
        assert_eq!(ci_state(&pending, &failed_status), Some(Ci::Failure));
        assert_eq!(
            failing_checks(&none, &failed_status),
            vec![Check { name: "ci/ext".into(), url: "https://ci.test/1".into() }]
        );
        let skipped = v(r#"{"check_runs":[{"status":"completed","conclusion":"skipped"}]}"#);
        assert_eq!(ci_state(&skipped, &no_status), Some(Ci::Success));
    }

    #[test]
    fn queries_are_encoded() {
        assert_eq!(encode("is:open author:@me"), "is:open+author:@me");
        assert_eq!(encode("a&b"), "a%26b");
    }
}
