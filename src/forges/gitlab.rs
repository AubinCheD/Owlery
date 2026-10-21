//! Lecture de GitLab (gitlab.com ou instance) : MR où je suis relecteur, mes MR et leur pipeline,
//! à-faire « mentionné », tickets assignés. API v4, jeton `read_api`.

use serde_json::Value;

use super::http::{Http, Request, encode};
use super::{
    Account, FetchError, Fetched, Identity, LAST_COMMENTS, MAX_DETAILS, PER_PAGE, Seen, check, comment, dedupe,
    parse_time, summary,
};
use crate::model::{Check, Ci, Comment, Forge, Item, Kind, Target};

pub fn api_base(host: &str) -> String {
    format!("https://{host}/api/v4")
}

fn get(http: &dyn Http, token: &str, url: &str) -> Result<Value, FetchError> {
    // `Authorization: Bearer`, accepté par GitLab pour les jetons personnels, et que le client ne
    // transmet pas lors d'une redirection vers un autre hôte.
    let req = Request::get(url).header("Authorization", format!("Bearer {token}")).header("Accept", "application/json");
    check(http.send(&req)?)
}

pub fn verify(http: &dyn Http, account: &Account, token: &str) -> Result<Identity, FetchError> {
    let user = get(http, token, &format!("{}/user", api_base(&account.host)))?;
    let username = user["username"].as_str().ok_or_else(|| FetchError::Parse("« username » absent".into()))?;
    Ok(Identity { username: username.to_string(), display_name: user["name"].as_str().unwrap_or_default().to_string() })
}

/// Nombre maximal d'entrées dont on vérifie si la dernière note est la mienne, à chaque lecture.
const MAX_SELF_CHECKS: usize = 10;

pub fn fetch(http: &dyn Http, account: &Account, token: &str, seen: &Seen) -> Result<Fetched, FetchError> {
    let base = api_base(&account.host);
    let me =
        if account.username.is_empty() { verify(http, account, token)?.username } else { account.username.clone() };
    let mut items = Vec::new();
    let mut notes = Vec::new();
    let mut read_list = |label: &str, url: String| -> Result<Vec<Value>, FetchError> {
        let v = get(http, token, &url)?;
        let found = v.as_array().cloned().ok_or_else(|| FetchError::Parse("liste attendue".into()))?;
        if found.len() >= PER_PAGE {
            notes.push(format!("{label} : les {PER_PAGE} plus récentes seulement"));
        }
        Ok(found)
    };

    let reviews = read_list(
        "MR à relire",
        format!(
            "{base}/merge_requests?scope=all&state=opened&reviewer_username={}&order_by=updated_at&per_page={PER_PAGE}",
            encode(&me, false)
        ),
    )?;
    items.extend(reviews.iter().filter_map(|v| parse_merge_request(v, Kind::ReviewRequested, account)));
    let mine = read_list(
        "Mes MR",
        format!("{base}/merge_requests?scope=created_by_me&state=opened&order_by=updated_at&per_page={PER_PAGE}"),
    )?;
    items.extend(mine.iter().filter_map(|v| parse_merge_request(v, Kind::MyPr, account)));
    let todos = read_list("À-faire", format!("{base}/todos?state=pending&per_page={PER_PAGE}"))?;
    items.extend(todos.iter().filter_map(|v| parse_todo(v, account)));
    let issues = read_list(
        "Tickets assignés",
        format!("{base}/issues?scope=assigned_to_me&state=opened&order_by=updated_at&per_page={PER_PAGE}"),
    )?;
    items.extend(issues.iter().filter_map(|v| parse_issue(v, account)));
    let mut items = dedupe(items);

    // Pipeline de mes MR ; leur compteur d'activité est celui des notes.
    let mut detail_error = None;
    for item in items.iter_mut().filter(|i| i.kind == Kind::MyPr).take(MAX_DETAILS) {
        item.activity = Some(item.comments);
        match pipeline(http, token, &base, &item.target) {
            Ok((ci, checks)) => {
                item.ci = ci;
                item.checks = checks;
                if ci == Some(Ci::Failure) {
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
    if let Some(e) = detail_error {
        notes.push(format!("Pipeline de certaines MR indisponible (gardé du cache) : {e}"));
    }

    // Une entrée lue qui a bougé reste lue si la dernière note est la mienne.
    let candidates: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| seen.get(&i.id).is_some_and(|s| i.updated_at > *s))
        .map(|(n, _)| n)
        .take(MAX_SELF_CHECKS)
        .collect();
    for n in candidates {
        match comments(http, account, token, &items[n]) {
            Ok(list) => {
                items[n].last_by_me = list.last().is_some_and(|c| super::github::is_mine(c, &me, items[n].updated_at));
            }
            Err(e @ FetchError::RateLimited { .. }) => return Err(e),
            Err(_) => {}
        }
    }
    Ok(Fetched { items, notes, me: Some(me), ..Fetched::default() })
}

/// `contoso/api!42` ou `contoso/api#12` → `contoso/api`.
fn repo_of(reference: &str) -> Option<String> {
    let cut = reference.rfind(['!', '#'])?;
    let repo = &reference[..cut];
    (!repo.is_empty()).then(|| repo.to_string())
}

fn base_item(v: &Value, kind: Kind, account: &Account, repo: String, number: u64, target: Target) -> Item {
    Item {
        id: Item::make_id(Forge::GitLab, &account.host, &repo, Some(number), target.is_pull()),
        account: account.id.clone(),
        forge: Forge::GitLab,
        kind,
        number: Some(number),
        title: v["title"].as_str().unwrap_or_default().to_string(),
        url: v["web_url"].as_str().unwrap_or_default().to_string(),
        branch: v["source_branch"].as_str().map(str::to_owned),
        author: v["author"]["username"].as_str().unwrap_or_default().to_string(),
        updated_at: v["updated_at"].as_str().and_then(parse_time).unwrap_or(0),
        ci: None,
        summary: summary(v["description"].as_str().unwrap_or_default()),
        checks: Vec::new(),
        comments: v["user_notes_count"].as_u64().unwrap_or(0) as u32,
        target,
        repo,
        ..Item::default()
    }
}

fn parse_merge_request(v: &Value, kind: Kind, account: &Account) -> Option<Item> {
    let iid = v["iid"].as_u64()?;
    let project_id = v["project_id"].as_u64()?;
    let repo = repo_of(v["references"]["full"].as_str()?)?;
    v["web_url"].as_str()?;
    Some(base_item(v, kind, account, repo, iid, Target::GitLab { project_id, iid, mr: true }))
}

fn parse_issue(v: &Value, account: &Account) -> Option<Item> {
    let iid = v["iid"].as_u64()?;
    let project_id = v["project_id"].as_u64()?;
    let repo = repo_of(v["references"]["full"].as_str()?)?;
    v["web_url"].as_str()?;
    let mut item = base_item(v, Kind::Assigned, account, repo, iid, Target::GitLab { project_id, iid, mr: false });
    item.branch = None;
    Some(item)
}

/// Un à-faire « mentionné » ou « cité directement » ; les autres (assigné, revue…) sont déjà lus par ailleurs.
fn parse_todo(v: &Value, account: &Account) -> Option<Item> {
    if !matches!(v["action_name"].as_str()?, "mentioned" | "directly_addressed") {
        return None;
    }
    let mr = match v["target_type"].as_str()? {
        "MergeRequest" => true,
        "Issue" => false,
        _ => return None,
    };
    let target = &v["target"];
    let iid = target["iid"].as_u64()?;
    let project_id = v["project"]["id"].as_u64().or_else(|| target["project_id"].as_u64())?;
    let repo = v["project"]["path_with_namespace"].as_str()?.to_string();
    let mut item = base_item(target, Kind::Mention, account, repo, iid, Target::GitLab { project_id, iid, mr });
    item.url = v["target_url"].as_str().or_else(|| target["web_url"].as_str())?.to_string();
    item.updated_at =
        v["updated_at"].as_str().or_else(|| v["created_at"].as_str()).and_then(parse_time).unwrap_or(item.updated_at);
    if let Some(who) = v["author"]["username"].as_str() {
        item.author = who.to_string();
    }
    if let Some(body) = v["body"].as_str().filter(|b| !b.trim().is_empty()) {
        item.summary = summary(body);
    }
    Some(item)
}

/// État du pipeline de tête d'une MR, et ses travaux en échec.
fn pipeline(http: &dyn Http, token: &str, base: &str, target: &Target) -> Result<(Option<Ci>, Vec<Check>), FetchError> {
    let Target::GitLab { project_id, iid, .. } = target else { return Ok((None, Vec::new())) };
    let mr = get(http, token, &format!("{base}/projects/{project_id}/merge_requests/{iid}"))?;
    let pipeline = &mr["head_pipeline"];
    let ci = pipeline["status"].as_str().and_then(pipeline_state);
    let mut checks = Vec::new();
    if ci == Some(Ci::Failure) {
        if let Some(pid) = pipeline["id"].as_u64() {
            let jobs = get(
                http,
                token,
                &format!("{base}/projects/{project_id}/pipelines/{pid}/jobs?scope[]=failed&per_page=20"),
            )?;
            for job in jobs.as_array().into_iter().flatten() {
                let (Some(name), Some(url)) = (job["name"].as_str(), job["web_url"].as_str()) else { continue };
                checks.push(Check { name: name.into(), url: url.into() });
            }
        }
        if checks.is_empty()
            && let Some(url) = pipeline["web_url"].as_str()
        {
            checks.push(Check { name: "Pipeline".into(), url: url.into() });
        }
    }
    Ok((ci, checks))
}

pub fn pipeline_state(status: &str) -> Option<Ci> {
    match status {
        "success" => Some(Ci::Success),
        "failed" | "canceled" => Some(Ci::Failure),
        "created" | "waiting_for_resource" | "preparing" | "pending" | "running" | "scheduled" => Some(Ci::Pending),
        // « skipped », « manual » : rien à signaler.
        _ => None,
    }
}

/// Derniers commentaires d'une MR ou d'un ticket (les notes du système, « a ajouté un commit », sont sautées).
pub fn comments(http: &dyn Http, account: &Account, token: &str, item: &Item) -> Result<Vec<Comment>, FetchError> {
    let Target::GitLab { project_id, iid, mr } = &item.target else { return Ok(Vec::new()) };
    let what = if *mr { "merge_requests" } else { "issues" };
    let notes = get(
        http,
        token,
        &format!(
            "{}/projects/{project_id}/{what}/{iid}/notes?sort=desc&order_by=created_at&per_page=20",
            api_base(&account.host)
        ),
    )?;
    let mut out: Vec<Comment> = notes
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| !n["system"].as_bool().unwrap_or(false))
        .filter_map(|n| {
            let body = n["body"].as_str()?;
            Some(comment(
                n["author"]["username"].as_str().unwrap_or_default(),
                n["created_at"].as_str().and_then(parse_time),
                body,
            ))
        })
        .take(LAST_COMMENTS)
        .collect();
    out.reverse();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forges::Auth;
    use crate::forges::http::{FakeHttp, Reply};

    const USER: &str = include_str!("testdata/gitlab_user.json");
    const REVIEWS: &str = include_str!("testdata/gitlab_reviews.json");
    const MINE: &str = include_str!("testdata/gitlab_mine.json");
    const MR_DETAIL: &str = include_str!("testdata/gitlab_mr_detail.json");
    const JOBS: &str = include_str!("testdata/gitlab_jobs.json");
    const TODOS: &str = include_str!("testdata/gitlab_todos.json");
    const ISSUES: &str = include_str!("testdata/gitlab_issues.json");
    const NOTES: &str = include_str!("testdata/gitlab_notes.json");

    fn account(host: &str) -> Account {
        let mut a = Account::new(Forge::GitLab, host, vec![], Auth::Token);
        a.id = "gl1".into();
        a
    }

    fn fake(host: &str) -> FakeHttp {
        let base = api_base(host);
        let f = FakeHttp::new();
        f.route(&format!("{base}/user"), Reply::ok(USER));
        f.route(&format!("{base}/merge_requests?scope=all"), Reply::ok(REVIEWS));
        f.route(&format!("{base}/merge_requests?scope=created_by_me"), Reply::ok(MINE));
        f.route(&format!("{base}/projects/7/merge_requests/31"), Reply::ok(MR_DETAIL));
        f.route(&format!("{base}/projects/7/pipelines/900/jobs"), Reply::ok(JOBS));
        f.route(&format!("{base}/todos"), Reply::ok(TODOS));
        f.route(&format!("{base}/issues"), Reply::ok(ISSUES));
        f
    }

    #[test]
    fn gitlab_reads_reviews_mine_todos_and_issues() {
        let f = fake("gitlab.com");
        let items = fetch(&f, &account("gitlab.com"), "glpat-fictif", &Seen::new()).unwrap().items;
        let by = |n: u64, mr: bool| items.iter().find(|i| i.number == Some(n) && i.target.is_pull() == mr).unwrap();
        let review = by(42, true);
        assert_eq!(review.kind, Kind::ReviewRequested);
        assert_eq!(review.repo, "contoso/api");
        assert_eq!(review.branch.as_deref(), Some("pagination"));
        assert_eq!(review.author, "dominique");
        assert_eq!(review.id, "GitLab:contoso/api!42");
        assert_eq!(review.account, "gl1");
        assert_eq!(by(12, false).kind, Kind::Assigned);
        let mention = by(5, false);
        assert_eq!(mention.kind, Kind::Mention);
        assert_eq!(mention.repo, "fabrikam/web/site");
        assert_eq!(mention.summary, "@camille peux-tu regarder ?");
        // Le relecteur est cherché sous son nom, lu une fois par /user.
        assert!(f.urls().iter().any(|u| u.contains("reviewer_username=camille")));
        // L'à-faire « assigné » n'est pas une mention.
        assert_eq!(items.iter().filter(|i| i.kind == Kind::Mention).count(), 1);
    }

    #[test]
    fn gitlab_failed_pipeline_is_ci_failed() {
        let items = fetch(&fake("gitlab.com"), &account("gitlab.com"), "t", &Seen::new()).unwrap().items;
        let mine = items.iter().find(|i| i.number == Some(31)).unwrap();
        assert_eq!(mine.kind, Kind::CiFailed);
        assert_eq!(mine.ci, Some(Ci::Failure));
        assert_eq!(
            mine.checks,
            vec![Check { name: "tests".into(), url: "https://gitlab.com/contoso/api/-/jobs/5001".into() }]
        );
    }

    #[test]
    fn gitlab_rate_limit() {
        let f = fake("gitlab.com");
        f.route(
            &format!("{}/todos", api_base("gitlab.com")),
            Reply { status: 429, reset: Some(77), ..Reply::default() },
        );
        assert_eq!(
            fetch(&f, &account("gitlab.com"), "t", &Seen::new()),
            Err(FetchError::RateLimited { reset: Some(77) })
        );
    }

    #[test]
    fn gitlab_self_hosted_host() {
        let f = fake("gitlab.contoso.test");
        let items = fetch(&f, &account("gitlab.contoso.test"), "t", &Seen::new()).unwrap().items;
        assert!(f.urls().iter().all(|u| u.starts_with("https://gitlab.contoso.test/api/v4/")));
        assert!(items[0].id.starts_with("GitLab@gitlab.contoso.test:"));
    }

    #[test]
    fn gitlab_missing_field_skips_the_entry() {
        let f = fake("gitlab.com");
        f.route(&format!("{}/issues", api_base("gitlab.com")), Reply::ok(r#"[{"iid":3,"title":"sans projet"}]"#));
        let items = fetch(&f, &account("gitlab.com"), "t", &Seen::new()).unwrap().items;
        assert!(items.iter().all(|i| i.kind != Kind::Assigned));
        assert!(!items.is_empty());
    }

    #[test]
    fn gitlab_notes_skip_system_notes() {
        let f = fake("gitlab.com");
        f.route(&format!("{}/projects/7/merge_requests/42/notes", api_base("gitlab.com")), Reply::ok(NOTES));
        let items = fetch(&f, &account("gitlab.com"), "t", &Seen::new()).unwrap().items;
        let review = items.iter().find(|i| i.number == Some(42)).unwrap();
        let notes = comments(&f, &account("gitlab.com"), "t", review).unwrap();
        assert_eq!(
            notes.iter().map(|c| c.body.as_str()).collect::<Vec<_>>(),
            ["Premier avis", "Deuxième avis", "Dernier avis"]
        );
    }

    #[test]
    fn pipeline_states() {
        assert_eq!(pipeline_state("running"), Some(Ci::Pending));
        assert_eq!(pipeline_state("manual"), None);
        assert_eq!(pipeline_state("canceled"), Some(Ci::Failure));
        assert_eq!(repo_of("group/sub/proj!3").as_deref(), Some("group/sub/proj"));
        assert_eq!(repo_of("!3"), None);
    }
}
