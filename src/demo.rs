//! Démonstration (`OWLERY_DEMO=1`) : des comptes et des entrées fictifs, servis par un faux `Fetcher`, pour
//! essayer l'interface et la capturer sans compte réel. Rien n'est lu sur le réseau ni écrit dans le coffre.

use chrono::{Duration, Local, NaiveTime, TimeZone};

use crate::forges::{Account, Auth, FetchError, Fetched, Fetcher, Identity, Seen};
use crate::model::{Check, Ci, Comment, Forge, Item, Kind, Target};

pub const GITHUB: &str = "demo-github";
pub const GITLAB: &str = "demo-gitlab";
pub const AZURE: &str = "demo-azure";

/// Comptes fictifs de la démonstration.
pub fn accounts() -> Vec<Account> {
    let mk = |id: &str, forge: Forge, orgs: Vec<String>, user: &str| {
        let mut a = Account::new(forge, "", orgs, Auth::Token);
        a.id = id.into();
        a.username = user.into();
        a.display_name = "Camille Martin".into();
        a
    };
    vec![
        mk(GITHUB, Forge::GitHub, vec![], "camille"),
        mk(GITLAB, Forge::GitLab, vec![], "camille"),
        mk(AZURE, Forge::AzureDevOps, vec!["fabrikam".into()], ""),
    ]
}

/// Heure de référence : 8 h aujourd'hui (hier si l'on est avant 8 h). Elle ne bouge pas d'un lancement à l'autre
/// dans la journée : les choix faits en démonstration tiennent.
fn base() -> i64 {
    let now = Local::now();
    let eight = NaiveTime::from_hms_opt(8, 0, 0).expect("heure valide");
    let mut day = now.date_naive();
    if now.time() < eight {
        day -= Duration::days(1);
    }
    Local.from_local_datetime(&day.and_time(eight)).earliest().map(|d| d.timestamp()).unwrap_or_else(|| now.timestamp())
}

pub fn items() -> Vec<Item> {
    let base = base();
    let gh = |n: u64, kind: Kind, title: &str, repo: &str, ago_min: i64, pull: bool| Item {
        id: Item::make_id(Forge::GitHub, "", repo, Some(n), pull),
        account: GITHUB.into(),
        forge: Forge::GitHub,
        kind,
        repo: repo.into(),
        number: Some(n),
        title: title.into(),
        url: format!("https://github.com/{repo}/{}/{n}", if pull { "pull" } else { "issues" }),
        author: "dominique".into(),
        updated_at: base - ago_min * 60,
        summary: "Exemple fictif : la description de la PR ou du ticket apparaît ici, sur une ligne.".into(),
        comments: 3,
        target: if pull { Target::Pull } else { Target::Issue },
        ..Item::default()
    };
    let mut review = gh(142, Kind::ReviewRequested, "Ajoute la pagination", "contoso/api", 12, true);
    review.branch = Some("pagination".into());
    review.ci = Some(Ci::Success);
    let mut failed = gh(138, Kind::CiFailed, "Corrige l'export CSV", "contoso/api", 40, true);
    failed.author = "camille".into();
    failed.branch = Some("export-csv".into());
    failed.ci = Some(Ci::Failure);
    failed.activity = Some(2);
    failed.checks =
        vec![Check { name: "tests (windows)".into(), url: "https://github.com/contoso/api/actions/runs/1".into() }];
    let mut mine = gh(131, Kind::MyPr, "Met à jour les dépendances", "contoso/api", 300, true);
    mine.author = "camille".into();
    mine.branch = Some("deps".into());
    mine.ci = Some(Ci::Pending);
    mine.activity = Some(1);
    let mention = gh(57, Kind::Mention, "Question sur le cache des images", "fabrikam/web", 90, false);
    let mut bot = gh(61, Kind::ReviewRequested, "Bump serde from 1.0.1 to 1.0.2", "fabrikam/web", 1500, true);
    bot.author = "dependabot[bot]".into();
    bot.ci = Some(Ci::Success);
    let assigned = Item {
        id: Item::make_id(Forge::GitLab, "", "northwind/app", Some(12), false),
        account: GITLAB.into(),
        forge: Forge::GitLab,
        kind: Kind::Assigned,
        repo: "northwind/app".into(),
        number: Some(12),
        title: "Erreur à l'export".into(),
        url: "https://gitlab.com/northwind/app/-/issues/12".into(),
        author: "sam".into(),
        updated_at: base - 2900 * 60,
        summary: "Plantage quand on exporte une commande sans lignes.".into(),
        target: Target::GitLab { project_id: 9, iid: 12, mr: false },
        ..Item::default()
    };
    let azure = Item {
        id: Item::make_id(Forge::AzureDevOps, "", "fabrikam/Ventes Web/api", Some(81), true),
        account: AZURE.into(),
        forge: Forge::AzureDevOps,
        kind: Kind::ReviewRequested,
        repo: "fabrikam/Ventes Web/api".into(),
        number: Some(81),
        title: "Ajoute le calcul des remises".into(),
        url: "https://dev.azure.com/fabrikam/Ventes%20Web/_git/api/pullrequest/81".into(),
        branch: Some("feature/remises".into()),
        author: "Alex Moreau".into(),
        updated_at: base - 200 * 60,
        ci: Some(Ci::Success),
        target: Target::Azure {
            org: "fabrikam".into(),
            project: "Ventes Web".into(),
            repo_id: "r1".into(),
            id: 81,
            pr: true,
        },
        ..Item::default()
    };
    vec![review, failed, mine, mention, bot, assigned, azure]
}

/// Faux accès aux forges : les entrées fictives du compte demandé.
pub struct DemoFetcher;

impl Fetcher for DemoFetcher {
    fn gh_available(&self) -> bool {
        false
    }

    fn verify(&self, _account: &Account, _token: &str) -> Result<Identity, FetchError> {
        Ok(Identity { username: "camille".into(), display_name: "Camille Martin".into(), orgs: Vec::new() })
    }

    fn fetch(&self, account: &Account, _seen: &Seen) -> Result<Fetched, FetchError> {
        Ok(Fetched { items: items().into_iter().filter(|i| i.account == account.id).collect(), notes: Vec::new() })
    }

    fn comments(&self, _account: &Account, item: &Item) -> Result<Vec<Comment>, FetchError> {
        let at = item.updated_at;
        Ok(vec![
            Comment { author: "sam".into(), at: at - 7200, body: "Premier avis : ça me semble bien.".into() },
            Comment { author: "alex".into(), at: at - 3600, body: "Il manque un test pour le cas vide.".into() },
            Comment { author: "dominique".into(), at, body: "Corrigé, peux-tu relire ?".into() },
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_entries_belong_to_the_demo_accounts_and_stay_put() {
        let accounts = accounts();
        let items = items();
        assert!(items.iter().all(|i| accounts.iter().any(|a| a.id == i.account && a.forge == i.forge)));
        assert_eq!(items, super::items(), "mêmes dates d'un appel à l'autre");
        assert!(items.iter().all(|i| i.updated_at <= Local::now().timestamp()));
        let got = DemoFetcher.fetch(&accounts[0], &Seen::new()).unwrap();
        assert_eq!(got.items.len(), 5);
    }
}
