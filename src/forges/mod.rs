//! Forges : comptes, lecture de ce qui attend l'utilisateur, erreurs. En lecture seule.
//!
//! Chaque compte est lu séparément (`Fetcher::fetch`) ; le jeton vient du coffre au moment de la
//! lecture et n'est gardé nulle part ailleurs.

pub mod azure;
pub mod github;
pub mod gitlab;
pub mod http;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Comment, Forge, Item, Kind, Target};
use crate::secrets::SecretStore;
use http::{Http, Reply};

/// Nombre maximal d'entrées lues par liste.
pub(crate) const PER_PAGE: usize = 50;
/// Nombre maximal de PR dont on lit le détail (branche, CI, activité) à chaque lecture.
pub(crate) const MAX_DETAILS: usize = 30;
/// Nombre de commentaires montrés dans le détail.
pub const LAST_COMMENTS: usize = 3;

/// Comment Owlery accède au compte.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Auth {
    /// Jeton saisi dans Owlery, rangé dans le coffre.
    #[default]
    Token,
    /// CLI `gh`, déjà connectée (GitHub seulement).
    Gh,
}

/// Compte enregistré dans les réglages (le jeton est dans le coffre, jamais ici).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Account {
    pub id: String,
    pub forge: Forge,
    /// Hôte normalisé : `github.com`, `ghe.contoso.test`, `gitlab.com`, `dev.azure.com`…
    pub host: String,
    /// Organisations (Azure DevOps Services) ou collections (Azure DevOps Server).
    #[serde(default)]
    pub orgs: Vec<String>,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// Identifiant du compte implicite « GitHub par gh ».
pub const GH_ACCOUNT_ID: &str = "gh";

impl Account {
    pub fn new(forge: Forge, host: &str, orgs: Vec<String>, auth: Auth) -> Self {
        Account {
            id: new_account_id(),
            forge,
            host: normalize_host(host, forge),
            orgs,
            auth,
            username: String::new(),
            display_name: String::new(),
            unknown: BTreeMap::new(),
        }
    }

    /// Le compte implicite : GitHub par la CLI `gh`, quand aucun compte GitHub n'est configuré.
    pub fn gh_implicit() -> Self {
        Account { id: GH_ACCOUNT_ID.into(), ..Account::new(Forge::GitHub, "", Vec::new(), Auth::Gh) }
    }

    /// Clé du jeton dans le coffre.
    pub fn token_key(&self) -> String {
        format!("account:{}", self.id)
    }

    /// « GitHub · camille », « GitLab · camille (gitlab.contoso.test) », « Azure DevOps · fabrikam ».
    pub fn title(&self) -> String {
        let who = match (self.forge, self.auth) {
            (_, Auth::Gh) if self.username.is_empty() => "via gh".to_string(),
            (Forge::AzureDevOps, _) if self.username.is_empty() => self.orgs.join(", "),
            _ if !self.display_name.is_empty() && self.forge == Forge::AzureDevOps => self.display_name.clone(),
            _ if !self.username.is_empty() => self.username.clone(),
            _ => "compte".into(),
        };
        let mut title = format!("{} · {who}", self.forge.label());
        if self.host != self.forge.default_host() {
            title.push_str(&format!(" ({})", self.host));
        }
        title
    }
}

/// Forge d'un compte de la suite (`suite_accounts::forge`).
pub fn suite_forge(forge: Forge) -> &'static str {
    match forge {
        Forge::GitHub => suite_accounts::forge::GITHUB,
        Forge::GitLab => suite_accounts::forge::GITLAB,
        Forge::AzureDevOps => suite_accounts::forge::AZURE,
    }
}

impl Account {
    /// Le compte tel que la suite le range (son jeton est à part, dans le coffre).
    pub fn to_suite(&self) -> suite_accounts::Account {
        suite_accounts::Account {
            id: self.id.clone(),
            forge: suite_forge(self.forge).into(),
            host: self.host.clone(),
            username: self.username.clone(),
            display_name: self.display_name.clone(),
            orgs: self.orgs.clone(),
            ..Default::default()
        }
    }

    /// Un compte de la suite, toujours lu par jeton ; forge inconnue d'Owlery : `None`.
    pub fn from_suite(a: &suite_accounts::Account) -> Option<Account> {
        let forge = Forge::ALL.into_iter().find(|f| suite_forge(*f) == a.forge)?;
        Some(Account {
            id: a.id.clone(),
            forge,
            host: normalize_host(&a.host, forge),
            orgs: a.orgs.clone(),
            auth: Auth::Token,
            username: a.username.clone(),
            display_name: a.display_name.clone(),
            unknown: BTreeMap::new(),
        })
    }
}

/// Range les comptes à jeton dans les comptes communs de la suite, sous leur identifiant (une connexion déjà
/// connue de la suite garde le sien) ; ne restent dans Owlery que ses comptes `gh`. Rien n'est retiré tant
/// que tous ne sont pas repris. Vrai si un compte a été repris.
pub fn migrate_to_suite(
    suite: &suite_accounts::Store,
    store: &dyn SecretStore,
    accounts: &mut Vec<Account>,
) -> Result<bool, String> {
    for a in accounts.iter().filter(|a| a.auth == Auth::Token) {
        let token = store.get(&a.token_key())?;
        suite.adopt(a.to_suite(), token)?;
    }
    let before = accounts.len();
    accounts.retain(|a| a.auth != Auth::Token);
    Ok(accounts.len() != before)
}

/// Nouvel identifiant de compte : 16 caractères hexadécimaux.
pub fn new_account_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.write_u32(std::process::id());
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    format!("{:016x}", h.finish())
}

/// Hôte saisi par l'utilisateur, sans schéma, chemin, identifiant ni majuscules ; celui de la forge
/// s'il est vide. `*.visualstudio.com` devient `dev.azure.com`.
pub fn normalize_host(host: &str, forge: Forge) -> String {
    let mut h = host.trim();
    if let Some(i) = h.find("://") {
        h = &h[i + 3..];
    }
    let h = h.split(['/', '?', '#']).next().unwrap_or_default();
    let h = h.rsplit('@').next().unwrap_or_default().trim().to_ascii_lowercase();
    if h.is_empty() || (forge == Forge::AzureDevOps && h.ends_with(".visualstudio.com")) {
        forge.default_host().to_string()
    } else if forge == Forge::GitHub && h == "api.github.com" {
        "github.com".into()
    } else {
        h
    }
}

/// Comptes lus : ceux des réglages, plus « GitHub par gh » si aucun compte GitHub n'est configuré et que
/// `gh` est installé.
pub fn accounts_to_read(configured: &[Account], gh_available: bool) -> Vec<Account> {
    let mut out = configured.to_vec();
    if gh_available && !configured.iter().any(|a| a.forge == Forge::GitHub) {
        out.push(Account::gh_implicit());
    }
    out
}

/// Clé du jeton GitHub du premier jet (0.1.0), avant les comptes.
pub const FIRST_DRAFT_TOKEN: &str = "github:token";

/// Reprend le jeton GitHub du premier jet dans un compte : le jeton est copié sous la clé du nouveau compte.
/// L'ancienne entrée n'est **pas** supprimée ici : seulement une fois l'état (qui porte le compte) enregistré
/// (`forget_first_draft_token`), sinon un arrêt entre les deux perdrait le compte. Vrai si un compte a été créé.
pub fn migrate_first_draft_token(store: &dyn SecretStore, accounts: &mut Vec<Account>) -> Result<bool, String> {
    let Some(token) = store.get(FIRST_DRAFT_TOKEN)?.filter(|t| !t.trim().is_empty()) else { return Ok(false) };
    if accounts.iter().any(|a| a.forge == Forge::GitHub && a.auth == Auth::Token) {
        // Déjà repris, et l'état qui porte le compte a été enregistré : l'ancienne entrée peut partir.
        store.delete(FIRST_DRAFT_TOKEN)?;
        return Ok(false);
    }
    let account = Account::new(Forge::GitHub, "github.com", Vec::new(), Auth::Token);
    store.set(&account.token_key(), &token)?;
    accounts.push(account);
    Ok(true)
}

/// Supprime le jeton du premier jet, une fois l'état qui porte son compte enregistré.
pub fn forget_first_draft_token(store: &dyn SecretStore) -> Result<(), String> {
    store.delete(FIRST_DRAFT_TOKEN)
}

/// Erreur d'une lecture.
#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    /// Aucun compte, et pas de CLI `gh`.
    NoAccess,
    /// Le jeton du compte n'est plus dans le coffre.
    MissingToken,
    /// Erreur du Gestionnaire d'identifiants.
    Store(String),
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
    /// Réglage du compte incomplet (organisation manquante…).
    Config(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::NoAccess => {
                f.write_str("Aucun compte : ajoutez-en un, ou installez gh puis lancez `gh auth login`.")
            }
            FetchError::MissingToken => {
                f.write_str("Jeton introuvable dans le coffre : saisissez-le à nouveau (Comptes).")
            }
            FetchError::Store(m) => write!(f, "Gestionnaire d'identifiants de Windows : {m}"),
            FetchError::Unauthorized => {
                f.write_str("Accès refusé : le jeton est invalide, expiré ou n'a pas les droits nécessaires.")
            }
            FetchError::RateLimited { reset: Some(t) } => {
                let at = chrono::DateTime::from_timestamp(*t, 0)
                    .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
                    .unwrap_or_default();
                write!(f, "Quota d'appels épuisé : nouvelle lecture à {at}.")
            }
            FetchError::RateLimited { reset: None } => {
                f.write_str("Quota d'appels épuisé : nouvelle lecture plus tard.")
            }
            FetchError::Network(m) => write!(f, "Erreur réseau : {m}"),
            FetchError::Http { status, body } => write!(f, "Le serveur a répondu {status} : {}", truncate(body, 300)),
            FetchError::Parse(m) => write!(f, "Réponse inattendue : {m}"),
            FetchError::Config(m) => write!(f, "Compte incomplet : {m}"),
        }
    }
}

/// Ce qu'une lecture a trouvé, et ce qu'il faut en dire sans que ce soit une erreur (listes tronquées,
/// détail indisponible…).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fetched {
    pub items: Vec<Item>,
    pub notes: Vec<String>,
    /// La forge a dit ne pas tout renvoyer : ce qui manque reste en cache.
    pub incomplete: bool,
    /// Nom de l'utilisateur du jeton, appris pendant la lecture (compte repris du premier jet, `gh`).
    pub me: Option<String>,
}

/// Entrées lues ou terminées → ce qui en a été vu (`Mark::seen_updated_at`).
pub type Seen = BTreeMap<String, i64>;

/// Identité renvoyée par la vérification d'un jeton.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Identity {
    pub username: String,
    pub display_name: String,
}

/// Ce qu'Owlery demande aux forges (réel : `ForgeFetcher` ; tests : réponses fictives).
pub trait Fetcher: Send + Sync {
    /// La CLI `gh` est-elle installée ?
    fn gh_available(&self) -> bool;
    /// Vérifie un jeton saisi, avant qu'il ne soit enregistré.
    fn verify(&self, account: &Account, token: &str) -> Result<Identity, FetchError>;
    /// Tout ce qui attend l'utilisateur sur ce compte. `seen` : entrées lues ou terminées et ce qui en a été vu,
    /// pour reconnaître une activité qui est la mienne.
    fn fetch(&self, account: &Account, seen: &Seen) -> Result<Fetched, FetchError>;
    /// Derniers commentaires d'une entrée (au plus `LAST_COMMENTS`, du plus ancien au plus récent).
    fn comments(&self, account: &Account, item: &Item) -> Result<Vec<Comment>, FetchError>;
}

/// Les vraies forges : jeton du coffre, HTTP ou `gh`.
pub struct ForgeFetcher {
    store: Arc<dyn SecretStore>,
    http: Arc<dyn Http>,
}

impl ForgeFetcher {
    pub fn new(store: Arc<dyn SecretStore>, http: Arc<dyn Http>) -> Self {
        ForgeFetcher { store, http }
    }

    fn token(&self, account: &Account) -> Result<String, FetchError> {
        self.store
            .get(&account.token_key())
            .map_err(FetchError::Store)?
            .filter(|t| !t.trim().is_empty())
            .ok_or(FetchError::MissingToken)
    }

    fn github(&self, account: &Account, token: Option<String>) -> Result<Box<dyn github::Transport>, FetchError> {
        match account.auth {
            Auth::Gh => github::GhTransport::find(&account.host).map(|t| Box::new(t) as _).ok_or(FetchError::NoAccess),
            Auth::Token => {
                let token = match token {
                    Some(t) => t,
                    None => self.token(account)?,
                };
                Ok(Box::new(github::TokenTransport::new(self.http.clone(), &account.host, token)))
            }
        }
    }
}

impl Fetcher for ForgeFetcher {
    fn gh_available(&self) -> bool {
        github::GhTransport::find("github.com").is_some()
    }

    fn verify(&self, account: &Account, token: &str) -> Result<Identity, FetchError> {
        let token = token.trim().to_string();
        match account.forge {
            Forge::GitHub => github::verify(self.github(account, Some(token))?.as_ref()),
            Forge::GitLab => gitlab::verify(self.http.as_ref(), account, &token),
            Forge::AzureDevOps => azure::verify(self.http.as_ref(), account, &token),
        }
    }

    fn fetch(&self, account: &Account, seen: &Seen) -> Result<Fetched, FetchError> {
        match account.forge {
            Forge::GitHub => github::fetch(self.github(account, None)?.as_ref(), account, seen),
            Forge::GitLab => gitlab::fetch(self.http.as_ref(), account, &self.token(account)?, seen),
            Forge::AzureDevOps => azure::fetch(self.http.as_ref(), account, &self.token(account)?, seen),
        }
    }

    fn comments(&self, account: &Account, item: &Item) -> Result<Vec<Comment>, FetchError> {
        match account.forge {
            Forge::GitHub => github::comments(self.github(account, None)?.as_ref(), item),
            Forge::GitLab => gitlab::comments(self.http.as_ref(), account, &self.token(account)?, item),
            Forge::AzureDevOps => azure::comments(self.http.as_ref(), account, &self.token(account)?, item),
        }
    }
}

/// Statut d'une réponse JSON : 2xx → le JSON ; 429, ou 403 avec quota épuisé → `RateLimited` ;
/// 401/403 → `Unauthorized` ; autre → `Http` (corps tronqué).
pub(crate) fn check(reply: Reply) -> Result<Value, FetchError> {
    match reply.status {
        200..=299 => serde_json::from_str(&reply.body).map_err(|e| FetchError::Parse(e.to_string())),
        429 => Err(FetchError::RateLimited { reset: reset_of(&reply) }),
        403 if reply.remaining == Some(0) || reply.body.to_lowercase().contains("rate limit") => {
            Err(FetchError::RateLimited { reset: reset_of(&reply) })
        }
        401 | 403 => Err(FetchError::Unauthorized),
        status => Err(FetchError::Http { status, body: truncate(&reply.body, 500) }),
    }
}

fn reset_of(reply: &Reply) -> Option<i64> {
    reply.reset.or_else(|| reply.retry_after.map(|s| chrono::Local::now().timestamp() + s.max(0)))
}

pub(crate) fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Date ISO 8601 (`2026-10-20T08:30:00Z`, fraction et décalage acceptés) en secondes Unix.
pub(crate) fn parse_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s.trim()).ok().map(|d| d.timestamp())
}

/// Début d'un texte, sur une ligne (balises HTML retirées), 200 caractères au plus.
pub(crate) fn summary(body: &str) -> String {
    let text = strip_tags(body);
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 200 { format!("{}…", truncate(&flat, 200)) } else { flat }
}

/// Retire les balises HTML (descriptions et commentaires d'Azure DevOps), décode les entités courantes.
pub(crate) fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Une même PR ou un même ticket n'apparaît qu'une fois par compte (même identifiant), avec le type le plus
/// utile : revue demandée, CI en échec, ma PR, ticket assigné, mention.
pub(crate) fn dedupe(items: Vec<Item>) -> Vec<Item> {
    fn rank(k: Kind) -> u8 {
        match k {
            Kind::ReviewRequested => 0,
            Kind::CiFailed => 1,
            Kind::MyPr => 2,
            Kind::Assigned => 3,
            Kind::Mention => 4,
        }
    }
    let mut best: BTreeMap<String, Item> = BTreeMap::new();
    for item in items {
        match best.get(&item.id) {
            Some(kept) if rank(kept.kind) <= rank(item.kind) => {}
            _ => {
                best.insert(item.id.clone(), item);
            }
        }
    }
    let mut out: Vec<Item> = best.into_values().collect();
    out.sort_by_key(|i| std::cmp::Reverse(i.updated_at));
    out
}

/// Commentaire tronqué pour le détail.
pub(crate) fn comment(author: &str, at: Option<i64>, body: &str) -> Comment {
    let text = strip_tags(body);
    let text = text.trim();
    let body = if text.chars().count() > 600 { format!("{}…", truncate(text, 600)) } else { text.to_string() };
    Comment { author: author.to_string(), at: at.unwrap_or(0), body }
}

impl Target {
    pub fn is_pull(&self) -> bool {
        match self {
            Target::Pull => true,
            Target::GitLab { mr, .. } => *mr,
            Target::Azure { pr, .. } => *pr,
            Target::Unknown | Target::Issue => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryStore;

    #[test]
    fn implicit_gh_account_only_without_github_account() {
        assert!(accounts_to_read(&[], false).is_empty());
        let read = accounts_to_read(&[], true);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].id, GH_ACCOUNT_ID);
        assert_eq!(read[0].auth, Auth::Gh);
        let gitlab = Account::new(Forge::GitLab, "", vec![], Auth::Token);
        assert_eq!(accounts_to_read(std::slice::from_ref(&gitlab), true).len(), 2);
        let github = Account::new(Forge::GitHub, "", vec![], Auth::Token);
        assert_eq!(accounts_to_read(&[github], true).len(), 1);
    }

    #[test]
    fn first_draft_token_moves_to_an_account() {
        let store = MemoryStore::default();
        let mut accounts = Vec::new();
        assert!(!migrate_first_draft_token(&store, &mut accounts).unwrap());
        store.set(FIRST_DRAFT_TOKEN, "ghp_fictif").unwrap();
        assert!(migrate_first_draft_token(&store, &mut accounts).unwrap());
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].host, "github.com");
        assert_eq!(store.get(&accounts[0].token_key()).unwrap().as_deref(), Some("ghp_fictif"));
        assert_eq!(
            store.get(FIRST_DRAFT_TOKEN).unwrap().as_deref(),
            Some("ghp_fictif"),
            "gardé tant que l'état n'est pas enregistré"
        );
        forget_first_draft_token(&store).unwrap();
        assert_eq!(store.get(FIRST_DRAFT_TOKEN).unwrap(), None);
        // Une ancienne entrée revenue (suppression ratée) ne crée pas un second compte.
        store.set(FIRST_DRAFT_TOKEN, "ghp_fictif").unwrap();
        assert!(!migrate_first_draft_token(&store, &mut accounts).unwrap());
        assert_eq!(accounts.len(), 1);
        assert_eq!(store.get(FIRST_DRAFT_TOKEN).unwrap(), None);
    }

    #[test]
    fn hosts_are_normalised() {
        assert_eq!(normalize_host(" https://GHE.Contoso.test/org ", Forge::GitHub), "ghe.contoso.test");
        assert_eq!(normalize_host("", Forge::GitLab), "gitlab.com");
        assert_eq!(normalize_host("fabrikam.visualstudio.com", Forge::AzureDevOps), "dev.azure.com");
        assert_eq!(normalize_host("api.github.com", Forge::GitHub), "github.com");
        assert_eq!(normalize_host("camille@gitlab.contoso.test", Forge::GitLab), "gitlab.contoso.test");
    }

    #[test]
    fn titles_name_the_account() {
        let mut a = Account::new(Forge::GitLab, "gitlab.contoso.test", vec![], Auth::Token);
        a.username = "camille".into();
        assert_eq!(a.title(), "GitLab · camille (gitlab.contoso.test)");
        assert_eq!(Account::gh_implicit().title(), "GitHub · via gh");
        let az = Account::new(Forge::AzureDevOps, "", vec!["fabrikam".into()], Auth::Token);
        assert_eq!(az.title(), "Azure DevOps · fabrikam");
    }

    #[test]
    fn errors_are_classified() {
        let r = |status, remaining, body: &str| Reply {
            status,
            body: body.into(),
            remaining,
            reset: Some(5),
            retry_after: None,
        };
        assert_eq!(check(r(401, None, "")), Err(FetchError::Unauthorized));
        assert_eq!(check(r(403, Some(0), "")), Err(FetchError::RateLimited { reset: Some(5) }));
        assert_eq!(check(r(403, None, "API rate limit exceeded")), Err(FetchError::RateLimited { reset: Some(5) }));
        assert_eq!(check(r(429, None, "")), Err(FetchError::RateLimited { reset: Some(5) }));
        assert_eq!(check(r(403, Some(10), "forbidden")), Err(FetchError::Unauthorized));
        assert!(matches!(check(r(500, None, "x")), Err(FetchError::Http { status: 500, .. })));
        assert!(matches!(check(r(200, None, "pas du json")), Err(FetchError::Parse(_))));
        let retry = Reply { status: 429, retry_after: Some(60), ..Reply::default() };
        let Err(FetchError::RateLimited { reset: Some(t) }) = check(retry) else { panic!() };
        assert!(t > chrono::Local::now().timestamp());
    }

    #[test]
    fn html_is_flattened() {
        assert_eq!(
            summary("<div>Bonjour&nbsp;<b>Camille</b></div><p>suite &amp; fin</p>"),
            "Bonjour Camille suite & fin"
        );
        assert_eq!(comment("a", None, "<p>x</p>").body, "x");
    }
}
