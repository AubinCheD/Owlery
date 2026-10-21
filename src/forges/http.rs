//! Transport HTTP des forges : un trait, le client réel (ureq, magasin de certificats de Windows) et
//! un faux pour les tests (réponses enregistrées par préfixe d'adresse).

use std::time::Duration;

use super::FetchError;

const USER_AGENT: &str = concat!("Owlery/", env!("CARGO_PKG_VERSION"));
/// Taille maximale d'un corps de réponse lu en mémoire.
const BODY_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// Une requête. Les en-têtes peuvent porter un jeton : une requête n'est jamais affichée ni journalisée.
#[derive(Clone, PartialEq)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl std::fmt::Debug for Request {
    /// Sans les en-têtes : ils portent le jeton.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} {}", self.method, self.url)
    }
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Request { method: Method::Get, url: url.into(), headers: Vec::new(), body: None }
    }

    pub fn post_json(url: impl Into<String>, body: String) -> Self {
        Request {
            method: Method::Post,
            url: url.into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: Some(body),
        }
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }
}

/// Réponse lue entièrement, avec les en-têtes de quota utiles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: String,
    /// Appels restants (`x-ratelimit-remaining` de GitHub, `RateLimit-Remaining` de GitLab).
    pub remaining: Option<u32>,
    /// Fin de la période de quota, en secondes Unix.
    pub reset: Option<i64>,
    /// `Retry-After`, en secondes.
    pub retry_after: Option<i64>,
}

impl Reply {
    pub fn ok(body: &str) -> Self {
        Reply { status: 200, body: body.into(), ..Reply::default() }
    }

    pub fn status(status: u16, body: &str) -> Self {
        Reply { status, body: body.into(), ..Reply::default() }
    }
}

pub trait Http: Send + Sync {
    fn send(&self, req: &Request) -> Result<Reply, FetchError>;
}

/// Client réel : TLS natif (magasin de certificats de Windows, autorités d'entreprise comprises),
/// proxy de l'environnement, délais bornés, statuts non 2xx rendus comme des données.
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl Default for UreqHttp {
    fn default() -> Self {
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
        UreqHttp { agent }
    }
}

impl Http for UreqHttp {
    fn send(&self, req: &Request) -> Result<Reply, FetchError> {
        let result = match req.method {
            Method::Get => {
                let mut r = self.agent.get(&req.url);
                for (k, v) in &req.headers {
                    r = r.header(k, v);
                }
                r.call()
            }
            Method::Post => {
                let mut r = self.agent.post(&req.url);
                for (k, v) in &req.headers {
                    r = r.header(k, v);
                }
                r.send(req.body.as_deref().unwrap_or_default())
            }
        };
        // Le message d'erreur d'ureq ne contient ni les en-têtes ni le jeton.
        let mut resp = result.map_err(|e| FetchError::Network(e.to_string()))?;
        let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
        let remaining =
            header("x-ratelimit-remaining").or_else(|| header("ratelimit-remaining")).and_then(|v| v.parse().ok());
        let reset = header("x-ratelimit-reset").or_else(|| header("ratelimit-reset")).and_then(|v| v.parse().ok());
        let retry_after = header("retry-after").and_then(|v| v.trim().parse().ok());
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT)
            .lossy_utf8(true)
            .read_to_string()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        Ok(Reply { status, body, remaining, reset, retry_after })
    }
}

/// Faux transport : la première route dont le préfixe correspond à l'adresse répond ; sinon 404. Garde
/// les requêtes reçues.
#[derive(Default)]
pub struct FakeHttp {
    routes: std::sync::Mutex<Vec<(String, Reply)>>,
    pub seen: std::sync::Mutex<Vec<Request>>,
}

impl FakeHttp {
    pub fn new() -> Self {
        FakeHttp::default()
    }

    /// Ajoute une route ; une route ajoutée plus tard et plus précise l'emporte (les plus longues d'abord).
    pub fn route(&self, prefix: &str, reply: Reply) -> &Self {
        let mut routes = self.routes.lock().unwrap();
        routes.retain(|(p, _)| p != prefix);
        routes.push((prefix.to_string(), reply));
        routes.sort_by_key(|r| std::cmp::Reverse(r.0.len()));
        self
    }

    pub fn urls(&self) -> Vec<String> {
        self.seen.lock().unwrap().iter().map(|r| r.url.clone()).collect()
    }
}

impl Http for FakeHttp {
    fn send(&self, req: &Request) -> Result<Reply, FetchError> {
        self.seen.lock().unwrap().push(req.clone());
        let routes = self.routes.lock().unwrap();
        Ok(routes
            .iter()
            .find(|(p, _)| req.url.starts_with(p.as_str()))
            .map(|(_, r)| r.clone())
            .unwrap_or_else(|| Reply::status(404, &format!("pas de route pour {}", req.url))))
    }
}

/// Encode un segment d'adresse (`%20` pour l'espace) ; garde `/` si `keep_slash`.
pub fn encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') || (keep_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_never_show_their_headers() {
        let r = Request::get("https://api.github.com/user").header("Authorization", "Bearer ghp_fictif");
        assert_eq!(format!("{r:?}"), "Get https://api.github.com/user");
    }

    #[test]
    fn fake_http_prefers_the_longest_prefix() {
        let fake = FakeHttp::new();
        fake.route("https://a.test/x", Reply::ok("court"));
        fake.route("https://a.test/x/y", Reply::ok("long"));
        assert_eq!(fake.send(&Request::get("https://a.test/x/y?z")).unwrap().body, "long");
        assert_eq!(fake.send(&Request::get("https://a.test/x/w")).unwrap().body, "court");
        assert_eq!(fake.send(&Request::get("https://b.test")).unwrap().status, 404);
        assert_eq!(fake.urls().len(), 3);
    }

    #[test]
    fn segments_are_encoded() {
        assert_eq!(encode("Mon Projet/api", true), "Mon%20Projet/api");
        assert_eq!(encode("group/sub", false), "group%2Fsub");
    }
}
