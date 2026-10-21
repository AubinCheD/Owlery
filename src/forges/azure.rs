//! Lecture d'Azure DevOps, organisation par organisation : PR où je suis relecteur (sans avoir encore
//! voté), mes PR et leur build, éléments de travail assignés et mentions récentes. API 7.1 pour
//! Azure DevOps Services (dev.azure.com), 5.0 pour Azure DevOps Server, comme DragonTree. Jeton :
//! Code, Work Items et Build en lecture.

use serde_json::{Value, json};

use super::http::{Http, Request, encode};
use super::{
    Account, FetchError, Fetched, Identity, LAST_COMMENTS, MAX_DETAILS, PER_PAGE, Seen, check, comment, dedupe,
    parse_time, summary,
};
use crate::model::{Check, Ci, Comment, Forge, Item, Kind, Target};

/// Identité renvoyée par `connectionData` à un appel anonyme (jeton refusé sans erreur HTTP).
const ANONYMOUS_ID: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

fn is_services(host: &str) -> bool {
    host == "dev.azure.com"
}

fn api_version(host: &str) -> &'static str {
    if is_services(host) { "7.1" } else { "5.0" }
}

/// `https://dev.azure.com/<org>` (Services) ou `https://<hôte>/<collection>` (Server).
pub fn org_base(host: &str, org: &str) -> String {
    format!("https://{host}/{}", encode(org.trim().trim_matches('/'), true))
}

fn send(http: &dyn Http, token: &str, req: Request) -> Result<Value, FetchError> {
    use base64::Engine as _;
    let basic = base64::engine::general_purpose::STANDARD.encode(format!(":{token}"));
    let req = req
        .header("Authorization", format!("Basic {basic}"))
        .header("Accept", "application/json")
        // Un 401 plutôt qu'une redirection vers la page de connexion.
        .header("X-TFS-FedAuthRedirect", "Suppress");
    let reply = http.send(&req)?;
    // Jeton invalide ou expiré : Azure renvoie parfois une page de connexion HTML en 203, ou en 200.
    let start = reply.body.trim_start_matches('\u{feff}').trim_start();
    if reply.status == 203
        || ((200..300).contains(&reply.status) && !(start.starts_with('{') || start.starts_with('[')))
    {
        return Err(FetchError::Unauthorized);
    }
    check(reply)
}

fn get(http: &dyn Http, token: &str, url: &str) -> Result<Value, FetchError> {
    send(http, token, Request::get(url))
}

fn orgs(account: &Account) -> Result<Vec<&str>, FetchError> {
    let orgs: Vec<&str> = account.orgs.iter().map(|o| o.trim().trim_matches('/')).filter(|o| !o.is_empty()).collect();
    if orgs.is_empty() {
        return Err(FetchError::Config("indiquez au moins une organisation Azure DevOps".into()));
    }
    Ok(orgs)
}

/// Identifiant de l'utilisateur du jeton dans une organisation.
fn me(http: &dyn Http, token: &str, host: &str, org: &str) -> Result<(String, String), FetchError> {
    let data = get(http, token, &format!("{}/_apis/connectionData", org_base(host, org)))?;
    let user = &data["authenticatedUser"];
    let id = user["id"].as_str().ok_or_else(|| FetchError::Parse("« authenticatedUser.id » absent".into()))?;
    if id == ANONYMOUS_ID {
        return Err(FetchError::Unauthorized);
    }
    Ok((id.to_string(), user["providerDisplayName"].as_str().unwrap_or_default().to_string()))
}

/// Vérifie le jeton sur chaque organisation indiquée.
pub fn verify(http: &dyn Http, account: &Account, token: &str) -> Result<Identity, FetchError> {
    let mut name = String::new();
    for org in orgs(account)? {
        let (_, display) = me(http, token, &account.host, org).map_err(|e| in_org(org, e))?;
        name = display;
    }
    Ok(Identity { username: name.clone(), display_name: name })
}

/// L'erreur d'une organisation, nommée : une organisation en échec arrête la lecture du compte (son
/// cache est gardé) plutôt que d'effacer en silence les entrées de cette organisation.
fn in_org(org: &str, e: FetchError) -> FetchError {
    match e {
        FetchError::Http { status, body } => {
            FetchError::Http { status, body: format!("organisation « {org} » : {body}") }
        }
        FetchError::Parse(m) => FetchError::Parse(format!("organisation « {org} » : {m}")),
        FetchError::Network(m) => FetchError::Network(format!("organisation « {org} » : {m}")),
        FetchError::Unauthorized => FetchError::Config(format!(
            "organisation « {org} » : jeton refusé (invalide, expiré, ou sans accès à cette organisation)"
        )),
        other => other,
    }
}

pub fn fetch(http: &dyn Http, account: &Account, token: &str, _seen: &Seen) -> Result<Fetched, FetchError> {
    let mut items = Vec::new();
    let mut notes = Vec::new();
    for org in orgs(account)? {
        items.extend(fetch_org(http, account, token, org, &mut notes).map_err(|e| in_org(org, e))?);
    }
    Ok(Fetched { items: dedupe(items), notes, ..Fetched::default() })
}

fn fetch_org(
    http: &dyn Http,
    account: &Account,
    token: &str,
    org: &str,
    notes: &mut Vec<String>,
) -> Result<Vec<Item>, FetchError> {
    let host = &account.host;
    let v = api_version(host);
    let base = org_base(host, org);
    let (me, _) = me(http, token, host, org)?;

    let mut prs = Vec::new();
    for (criteria, kind) in [("reviewerId", Kind::ReviewRequested), ("creatorId", Kind::MyPr)] {
        let query =
            format!("searchCriteria.{criteria}={me}&searchCriteria.status=active&$top={PER_PAGE}&api-version={v}");
        for pr in pull_requests(http, token, &base, &query)? {
            // Une revue déjà faite (vote donné) n'attend plus l'utilisateur.
            if kind == Kind::ReviewRequested && my_vote(&pr, &me) != 0 {
                continue;
            }
            if let Some(mut item) = parse_pull_request(&pr, kind, account, org) {
                if kind == Kind::MyPr {
                    // Les votes des autres (approuvé, à revoir…) comptent dans l'activité de mes PR.
                    item.activity = Some(votes_by_others(&pr, &me));
                }
                prs.push(item);
            }
        }
    }
    // Détail : mes PR d'abord, puis les revues, chacune avec son budget.
    let mut detail_error = None;
    for kind in [Kind::MyPr, Kind::ReviewRequested] {
        for item in prs.iter_mut().filter(|i| i.kind == kind).take(MAX_DETAILS) {
            match pull_details(http, token, &base, v, &item.target, &me) {
                Ok(d) => {
                    item.updated_at = item.updated_at.max(d.activity_at);
                    item.last_by_me = d.last_by_me;
                    if kind == Kind::MyPr {
                        item.activity = Some(item.activity.unwrap_or(0) + d.others_comments);
                    }
                    match d.build {
                        Ok((ci, checks)) => {
                            item.ci = ci;
                            item.checks = checks;
                            if kind == Kind::MyPr && ci == Some(Ci::Failure) {
                                item.kind = Kind::CiFailed;
                            }
                        }
                        Err(e @ FetchError::RateLimited { .. }) => return Err(e),
                        Err(e) => {
                            item.partial = true;
                            detail_error.get_or_insert(e);
                        }
                    }
                }
                Err(e @ FetchError::RateLimited { .. }) => return Err(e),
                Err(e) => {
                    item.partial = true;
                    detail_error.get_or_insert(e);
                }
            }
        }
    }
    if let Some(e) = detail_error {
        notes.push(format!("{org} : détail de certaines PR indisponible (droit Build du jeton ?) : {e}"));
    }

    let mut items = prs;
    items.extend(work_items(http, token, account, org, notes)?);
    Ok(items)
}

/// Ce que les fils et la build d'une PR apprennent.
struct PrDetail {
    /// Dernière activité (fils, votes et poussées compris).
    activity_at: i64,
    /// Commentaires de texte des autres (compteur d'activité de mes PR).
    others_comments: u32,
    /// Le dernier commentaire de texte est le mien.
    last_by_me: bool,
    /// État de la build ; une erreur ici (jeton sans le droit Build) n'empêche pas le reste.
    build: Result<(Option<Ci>, Vec<Check>), FetchError>,
}

/// PR de toute l'organisation ; repli projet par projet si le serveur ne connaît pas la liste globale.
fn pull_requests(http: &dyn Http, token: &str, base: &str, query: &str) -> Result<Vec<Value>, FetchError> {
    match get(http, token, &format!("{base}/_apis/git/pullrequests?{query}")) {
        Ok(v) => Ok(values(&v)),
        Err(FetchError::Http { status: 404, .. }) => {
            let v = query.rsplit("api-version=").next().unwrap_or("5.0");
            let projects = get(http, token, &format!("{base}/_apis/projects?$top=200&api-version={v}"))?;
            let mut out = Vec::new();
            for p in values(&projects) {
                let Some(name) = p["name"].as_str() else { continue };
                let found =
                    get(http, token, &format!("{base}/{}/_apis/git/pullrequests?{query}", encode(name, false)))?;
                out.extend(values(&found));
            }
            Ok(out)
        }
        Err(e) => Err(e),
    }
}

fn values(v: &Value) -> Vec<Value> {
    v["value"].as_array().cloned().unwrap_or_default()
}

/// Relecteurs autres que moi qui ont voté.
fn votes_by_others(pr: &Value, me: &str) -> u32 {
    pr["reviewers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| {
            r["vote"].as_i64().unwrap_or(0) != 0 && !r["id"].as_str().is_some_and(|id| id.eq_ignore_ascii_case(me))
        })
        .count() as u32
}

fn my_vote(pr: &Value, me: &str) -> i64 {
    pr["reviewers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["id"].as_str().is_some_and(|id| id.eq_ignore_ascii_case(me)))
        .and_then(|r| r["vote"].as_i64())
        .unwrap_or(0)
}

fn parse_pull_request(pr: &Value, kind: Kind, account: &Account, org: &str) -> Option<Item> {
    let id = pr["pullRequestId"].as_u64()?;
    let repo_name = pr["repository"]["name"].as_str()?;
    let repo_id = pr["repository"]["id"].as_str()?;
    let project = pr["repository"]["project"]["name"].as_str()?;
    let repo = format!("{org}/{project}/{repo_name}");
    let url = format!(
        "{}/{}/_git/{}/pullrequest/{id}",
        org_base(&account.host, org),
        encode(project, false),
        encode(repo_name, false)
    );
    Some(Item {
        id: Item::make_id(Forge::AzureDevOps, &account.host, &repo, Some(id), true),
        account: account.id.clone(),
        forge: Forge::AzureDevOps,
        kind,
        number: Some(id),
        title: pr["title"].as_str().unwrap_or_default().to_string(),
        url,
        branch: pr["sourceRefName"].as_str().map(|r| r.trim_start_matches("refs/heads/").to_string()),
        author: pr["createdBy"]["displayName"].as_str().unwrap_or_default().to_string(),
        updated_at: pr["creationDate"].as_str().and_then(parse_time).unwrap_or(0),
        summary: summary(pr["description"].as_str().unwrap_or_default()),
        target: Target::Azure { org: org.into(), project: project.into(), repo_id: repo_id.into(), id, pr: true },
        repo,
        ..Item::default()
    })
}

/// Dernière activité (fils de discussion, votes et poussées compris), et build de la PR.
fn pull_details(
    http: &dyn Http,
    token: &str,
    base: &str,
    v: &str,
    target: &Target,
    me: &str,
) -> Result<PrDetail, FetchError> {
    let Target::Azure { project, repo_id, id, .. } = target else {
        return Ok(PrDetail { activity_at: 0, others_comments: 0, last_by_me: false, build: Ok((None, Vec::new())) });
    };
    let project = encode(project, false);
    let threads = values(&get(
        http,
        token,
        &format!("{base}/{project}/_apis/git/repositories/{repo_id}/pullRequests/{id}/threads?api-version={v}"),
    )?);
    let activity_at = threads
        .iter()
        .flat_map(|t| {
            let own = t["lastUpdatedDate"].as_str().and_then(parse_time);
            let comments = t["comments"].as_array().cloned().unwrap_or_default();
            own.into_iter()
                .chain(comments.into_iter().filter_map(|c| c["lastUpdatedDate"].as_str().and_then(parse_time)))
        })
        .max()
        .unwrap_or(0);
    let mut texts: Vec<(i64, bool)> = text_comments(&threads)
        .map(|c| {
            (
                c["publishedDate"].as_str().and_then(parse_time).unwrap_or(0),
                c["author"]["id"].as_str().is_some_and(|a| a.eq_ignore_ascii_case(me)),
            )
        })
        .collect();
    texts.sort();
    let others_comments = texts.iter().filter(|(_, mine)| !mine).count() as u32;
    let last_by_me = texts.last().is_some_and(|(at, mine)| *mine && at + 120 >= activity_at);
    let build = get(
        http,
        token,
        &format!("{base}/{project}/_apis/build/builds?branchName=refs/pull/{id}/merge&$top=1&queryOrder=queueTimeDescending&api-version={v}"),
    )
    .map(|builds| match values(&builds).first() {
        Some(b) => {
            let ci = build_state(b["status"].as_str().unwrap_or_default(), b["result"].as_str().unwrap_or_default());
            let mut checks = Vec::new();
            if ci == Some(Ci::Failure) {
                let name = b["definition"]["name"].as_str().unwrap_or("Build").to_string();
                if let Some(url) = b["_links"]["web"]["href"].as_str() {
                    checks.push(Check { name, url: url.into() });
                }
            }
            (ci, checks)
        }
        None => (None, Vec::new()),
    });
    Ok(PrDetail { activity_at, others_comments, last_by_me, build })
}

/// Commentaires de texte (pas ceux du système : votes, poussées) des fils non supprimés.
fn text_comments(threads: &[Value]) -> impl Iterator<Item = Value> + '_ {
    threads
        .iter()
        .filter(|t| !t["isDeleted"].as_bool().unwrap_or(false))
        .flat_map(|t| t["comments"].as_array().cloned().unwrap_or_default())
        .filter(|c| c["commentType"].as_str() == Some("text") && !c["isDeleted"].as_bool().unwrap_or(false))
}

pub fn build_state(status: &str, result: &str) -> Option<Ci> {
    match (status, result) {
        ("completed", "succeeded" | "partiallySucceeded") => Some(Ci::Success),
        ("completed", "failed" | "canceled") => Some(Ci::Failure),
        ("completed", _) => None,
        ("inProgress" | "notStarted" | "postponed" | "cancelling", _) => Some(Ci::Pending),
        _ => None,
    }
}

/// Éléments de travail assignés (non fermés) et mentions récentes, par WIQL puis lecture groupée.
fn work_items(
    http: &dyn Http,
    token: &str,
    account: &Account,
    org: &str,
    notes: &mut Vec<String>,
) -> Result<Vec<Item>, FetchError> {
    let host = &account.host;
    let v = api_version(host);
    let base = org_base(host, org);
    let wiql = |query: &str| {
        let body = json!({ "query": query }).to_string();
        send(http, token, Request::post_json(format!("{base}/_apis/wit/wiql?$top={PER_PAGE}&api-version={v}"), body))
    };
    let ids_of = |v: &Value| -> Vec<u64> {
        v["workItems"].as_array().into_iter().flatten().filter_map(|w| w["id"].as_u64()).take(PER_PAGE).collect()
    };
    let assigned = ids_of(&wiql(
        "SELECT [System.Id] FROM WorkItems WHERE [System.AssignedTo] = @Me \
         AND [System.State] NOT IN ('Closed', 'Done', 'Removed', 'Completed', 'Cut') ORDER BY [System.ChangedDate] DESC",
    )?);
    // `@RecentMentions` (30 derniers jours) n'existe pas sur les anciens serveurs : une requête refusée
    // (400) n'empêche pas le reste.
    let mentioned = match wiql(
        "SELECT [System.Id] FROM WorkItems WHERE [System.Id] IN (@RecentMentions) ORDER BY [System.ChangedDate] DESC",
    ) {
        Ok(v) => ids_of(&v),
        Err(FetchError::Http { status: 400, .. }) => {
            notes.push(format!("{org} : mentions récentes non prises en charge par ce serveur"));
            Vec::new()
        }
        Err(e) => return Err(e),
    };
    let all: Vec<u64> = assigned
        .iter()
        .chain(mentioned.iter())
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if all.is_empty() {
        return Ok(Vec::new());
    }
    let ids = all.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
    let fields =
        "System.Title,System.TeamProject,System.ChangedDate,System.CreatedBy,System.Description,System.WorkItemType";
    let found = get(http, token, &format!("{base}/_apis/wit/workitems?ids={ids}&fields={fields}&api-version={v}"))?;
    let mut out = Vec::new();
    for w in values(&found) {
        let Some(id) = w["id"].as_u64() else { continue };
        let f = &w["fields"];
        let Some(project) = f["System.TeamProject"].as_str() else { continue };
        let repo = format!("{org}/{project}");
        let author = match &f["System.CreatedBy"] {
            Value::String(s) => s.split('<').next().unwrap_or_default().trim().to_string(),
            v => v["displayName"].as_str().unwrap_or_default().to_string(),
        };
        let title = match f["System.WorkItemType"].as_str() {
            Some(t) => format!("{t} : {}", f["System.Title"].as_str().unwrap_or_default()),
            None => f["System.Title"].as_str().unwrap_or_default().to_string(),
        };
        for (list, kind) in [(&assigned, Kind::Assigned), (&mentioned, Kind::Mention)] {
            if !list.contains(&id) {
                continue;
            }
            out.push(Item {
                id: Item::make_id(Forge::AzureDevOps, host, &repo, Some(id), false),
                account: account.id.clone(),
                forge: Forge::AzureDevOps,
                kind,
                number: Some(id),
                title: title.clone(),
                url: format!("{base}/{}/_workitems/edit/{id}", encode(project, false)),
                author: author.clone(),
                updated_at: f["System.ChangedDate"].as_str().and_then(parse_time).unwrap_or(0),
                summary: summary(f["System.Description"].as_str().unwrap_or_default()),
                target: Target::Azure {
                    org: org.into(),
                    project: project.into(),
                    repo_id: String::new(),
                    id,
                    pr: false,
                },
                repo: repo.clone(),
                ..Item::default()
            });
        }
    }
    Ok(out)
}

/// Derniers commentaires : fils de la PR (commentaires de texte), ou commentaires de l'élément de travail
/// (Azure DevOps Services seulement).
pub fn comments(http: &dyn Http, account: &Account, token: &str, item: &Item) -> Result<Vec<Comment>, FetchError> {
    let Target::Azure { org, project, repo_id, id, pr } = &item.target else { return Ok(Vec::new()) };
    let host = &account.host;
    let base = org_base(host, org);
    let project = encode(project, false);
    if *pr {
        let threads = get(
            http,
            token,
            &format!(
                "{base}/{project}/_apis/git/repositories/{repo_id}/pullRequests/{id}/threads?api-version={}",
                api_version(host)
            ),
        )?;
        let mut all: Vec<Comment> = values(&threads)
            .iter()
            .filter(|t| !t["isDeleted"].as_bool().unwrap_or(false))
            .flat_map(|t| t["comments"].as_array().cloned().unwrap_or_default())
            .filter(|c| c["commentType"].as_str() == Some("text") && !c["isDeleted"].as_bool().unwrap_or(false))
            .filter_map(|c| {
                Some(comment(
                    c["author"]["displayName"].as_str().unwrap_or_default(),
                    c["publishedDate"].as_str().and_then(parse_time),
                    c["content"].as_str()?,
                ))
            })
            .collect();
        all.sort_by_key(|c| c.at);
        let skip = all.len().saturating_sub(LAST_COMMENTS);
        return Ok(all.split_off(skip));
    }
    if !is_services(host) {
        return Ok(Vec::new());
    }
    let found = get(
        http,
        token,
        &format!(
            "{base}/{project}/_apis/wit/workItems/{id}/comments?$top={LAST_COMMENTS}&order=desc&api-version=7.1-preview.4"
        ),
    )?;
    let mut out: Vec<Comment> = found["comments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some(comment(
                c["createdBy"]["displayName"].as_str().unwrap_or_default(),
                c["createdDate"].as_str().and_then(parse_time),
                c["text"].as_str()?,
            ))
        })
        .collect();
    out.reverse();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forges::Auth;
    use crate::forges::http::{FakeHttp, Method, Reply};

    const CONNECTION: &str = include_str!("testdata/azure_connection_data.json");
    const REVIEWS: &str = include_str!("testdata/azure_reviews.json");
    const MINE: &str = include_str!("testdata/azure_mine.json");
    const THREADS: &str = include_str!("testdata/azure_threads.json");
    const BUILD_FAILED: &str = include_str!("testdata/azure_build_failed.json");
    const WIQL_ASSIGNED: &str = include_str!("testdata/azure_wiql_assigned.json");
    const WORKITEMS: &str = include_str!("testdata/azure_workitems.json");

    const B: &str = "https://dev.azure.com/fabrikam";

    fn account(orgs: &[&str]) -> Account {
        let mut a = Account::new(Forge::AzureDevOps, "", orgs.iter().map(|o| o.to_string()).collect(), Auth::Token);
        a.id = "az1".into();
        a
    }

    fn fake() -> FakeHttp {
        let f = FakeHttp::new();
        f.route(&format!("{B}/_apis/connectionData"), Reply::ok(CONNECTION));
        f.route(&format!("{B}/_apis/git/pullrequests?searchCriteria.reviewerId"), Reply::ok(REVIEWS));
        f.route(&format!("{B}/_apis/git/pullrequests?searchCriteria.creatorId"), Reply::ok(MINE));
        f.route(&format!("{B}/Ventes%20Web/_apis/git/repositories/"), Reply::ok(THREADS));
        f.route(&format!("{B}/Ventes%20Web/_apis/build/builds?branchName=refs/pull/77/"), Reply::ok(BUILD_FAILED));
        f.route(&format!("{B}/Ventes%20Web/_apis/build/builds"), Reply::ok(r#"{"count":0,"value":[]}"#));
        f.route(&format!("{B}/_apis/wit/wiql"), Reply::ok(WIQL_ASSIGNED));
        f.route(&format!("{B}/_apis/wit/workitems"), Reply::ok(WORKITEMS));
        f
    }

    #[test]
    fn azure_reads_reviews_mine_and_work_items() {
        let f = fake();
        let items = fetch(&f, &account(&["fabrikam"]), "pat-fictif", &Seen::new()).unwrap().items;
        let review = items.iter().find(|i| i.number == Some(81)).unwrap();
        assert_eq!(review.kind, Kind::ReviewRequested);
        assert_eq!(review.repo, "fabrikam/Ventes Web/api");
        assert_eq!(review.url, "https://dev.azure.com/fabrikam/Ventes%20Web/_git/api/pullrequest/81");
        assert_eq!(review.branch.as_deref(), Some("feature/pagination"));
        assert_eq!(review.author, "Dominique Petit");
        // Activité : le dernier fil (2026-10-20T09:00) est plus récent que la création.
        assert_eq!(review.updated_at, parse_time("2026-10-20T09:00:00Z").unwrap());
        // Déjà approuvée par moi : n'attend plus.
        assert!(items.iter().all(|i| i.number != Some(82)));
        let wi = items.iter().find(|i| i.number == Some(1203)).unwrap();
        assert_eq!(wi.kind, Kind::Assigned);
        assert_eq!(wi.repo, "fabrikam/Ventes Web");
        assert_eq!(wi.title, "Bug : Erreur à l'export");
        assert_eq!(wi.url, "https://dev.azure.com/fabrikam/Ventes%20Web/_workitems/edit/1203");
        assert_eq!(wi.summary, "Plantage quand on exporte");
        // La requête WIQL est un POST avec la requête en JSON, et l'autorisation en Basic.
        let seen = f.seen.lock().unwrap();
        let post = seen.iter().find(|r| r.method == Method::Post).unwrap();
        assert!(post.body.as_deref().unwrap().contains("@Me"));
        assert!(post.headers.iter().any(|(k, v)| k == "Authorization" && v.starts_with("Basic ")));
    }

    #[test]
    fn azure_failed_build_is_ci_failed() {
        let items = fetch(&fake(), &account(&["fabrikam"]), "t", &Seen::new()).unwrap().items;
        let mine = items.iter().find(|i| i.number == Some(77)).unwrap();
        assert_eq!(mine.kind, Kind::CiFailed);
        assert_eq!(
            mine.checks,
            vec![Check {
                name: "api-ci".into(),
                url: "https://dev.azure.com/fabrikam/Ventes%20Web/_build/results?buildId=501".into()
            }]
        );
    }

    #[test]
    fn azure_203_is_unauthorized() {
        let f = fake();
        f.route(&format!("{B}/_apis/connectionData"), Reply::status(203, "<html>connexion</html>"));
        assert!(
            matches!(fetch(&f, &account(&["fabrikam"]), "t", &Seen::new()), Err(FetchError::Config(m)) if m.contains("fabrikam"))
        );
        let f = fake();
        f.route(&format!("{B}/_apis/connectionData"), Reply::ok("<html>connexion</html>"));
        assert!(matches!(fetch(&f, &account(&["fabrikam"]), "t", &Seen::new()), Err(FetchError::Config(_))));
        let f = fake();
        f.route(
            &format!("{B}/_apis/connectionData"),
            Reply::ok(r#"{"authenticatedUser":{"id":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"}}"#),
        );
        assert!(fetch(&f, &account(&["fabrikam"]), "t", &Seen::new()).is_err());
    }

    #[test]
    fn azure_failing_org_is_named() {
        let f = fake();
        f.route("https://dev.azure.com/northwind/", Reply::status(500, "panne"));
        let err = fetch(&f, &account(&["fabrikam", "northwind"]), "t", &Seen::new()).unwrap_err();
        assert!(err.to_string().contains("northwind"), "{err}");
        assert_eq!(
            fetch(&fake(), &account(&[]), "t", &Seen::new()),
            Err(FetchError::Config("indiquez au moins une organisation Azure DevOps".into()))
        );
    }

    #[test]
    fn azure_falls_back_to_projects_when_the_global_list_is_missing() {
        let f = fake();
        f.route(&format!("{B}/_apis/git/pullrequests"), Reply::status(404, ""));
        f.route(&format!("{B}/_apis/projects"), Reply::ok(r#"{"value":[{"name":"Ventes Web"}]}"#));
        f.route(&format!("{B}/Ventes%20Web/_apis/git/pullrequests?searchCriteria.reviewerId"), Reply::ok(REVIEWS));
        f.route(&format!("{B}/Ventes%20Web/_apis/git/pullrequests?searchCriteria.creatorId"), Reply::ok(MINE));
        let items = fetch(&f, &account(&["fabrikam"]), "t", &Seen::new()).unwrap().items;
        assert!(items.iter().any(|i| i.number == Some(81)));
    }

    #[test]
    fn azure_server_uses_api_5_and_its_host() {
        let mut a = account(&["DefaultCollection"]);
        a.host = "tfs.contoso.test".into();
        let f = FakeHttp::new();
        f.route("https://tfs.contoso.test/DefaultCollection/_apis/connectionData", Reply::ok(CONNECTION));
        f.route("https://tfs.contoso.test/DefaultCollection/_apis/git/pullrequests", Reply::ok(r#"{"value":[]}"#));
        f.route("https://tfs.contoso.test/DefaultCollection/_apis/wit/wiql", Reply::ok(r#"{"workItems":[]}"#));
        fetch(&f, &a, "t", &Seen::new()).unwrap();
        assert!(f.urls().iter().filter(|u| u.contains("api-version")).all(|u| u.contains("api-version=5.0")));
    }

    #[test]
    fn azure_threads_keep_text_comments() {
        let f = fake();
        let items = fetch(&f, &account(&["fabrikam"]), "t", &Seen::new()).unwrap().items;
        let review = items.iter().find(|i| i.number == Some(81)).unwrap();
        let c = comments(&f, &account(&["fabrikam"]), "t", review).unwrap();
        assert_eq!(
            c.iter().map(|c| c.body.as_str()).collect::<Vec<_>>(),
            ["Deuxième remarque", "Troisième remarque", "Quatrième remarque"]
        );
    }

    #[test]
    fn azure_missing_field_skips_the_entry() {
        let f = fake();
        f.route(
            &format!("{B}/_apis/git/pullrequests?searchCriteria.reviewerId"),
            Reply::ok(r#"{"value":[{"pullRequestId":90,"title":"sans dépôt"}]}"#),
        );
        let items = fetch(&f, &account(&["fabrikam"]), "t", &Seen::new()).unwrap().items;
        assert!(items.iter().all(|i| i.number != Some(90)));
        assert!(items.iter().any(|i| i.number == Some(77)), "le reste est lu");
    }

    #[test]
    fn build_states() {
        assert_eq!(build_state("completed", "partiallySucceeded"), Some(Ci::Success));
        assert_eq!(build_state("inProgress", ""), Some(Ci::Pending));
        assert_eq!(build_state("completed", "none"), None);
    }
}
