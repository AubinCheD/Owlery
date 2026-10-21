//! Entrées de la boîte de réception et leur état local (lu, reporté, terminé).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Forge d'origine d'une entrée.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord, Default)]
pub enum Forge {
    #[default]
    GitHub,
    GitLab,
    AzureDevOps,
}

impl Forge {
    pub const ALL: [Forge; 3] = [Forge::GitHub, Forge::GitLab, Forge::AzureDevOps];

    pub fn label(self) -> &'static str {
        match self {
            Forge::GitHub => "GitHub",
            Forge::GitLab => "GitLab",
            Forge::AzureDevOps => "Azure DevOps",
        }
    }

    /// Hôte utilisé quand l'utilisateur n'en saisit pas.
    pub fn default_host(self) -> &'static str {
        match self {
            Forge::GitHub => "github.com",
            Forge::GitLab => "gitlab.com",
            Forge::AzureDevOps => "dev.azure.com",
        }
    }
}

/// Ce qui attend le développeur.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord, Default)]
pub enum Kind {
    /// PR où l'on est relecteur.
    ReviewRequested,
    /// CI en échec sur une de mes PR.
    CiFailed,
    /// Commentaire qui me cite.
    #[default]
    Mention,
    /// Ticket assigné.
    Assigned,
    /// Ma PR a bougé (commentaires, revue, conflits).
    MyPr,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::ReviewRequested, Kind::CiFailed, Kind::Mention, Kind::Assigned, Kind::MyPr];

    pub fn label(self) -> &'static str {
        match self {
            Kind::ReviewRequested => "Revue demandée",
            Kind::CiFailed => "CI en échec",
            Kind::Mention => "Mention",
            Kind::Assigned => "Ticket assigné",
            Kind::MyPr => "Ma PR",
        }
    }

    /// Un passage à ce type est une nouvelle en soi (CI devenue rouge, revue demandée) : l'entrée revient en non
    /// lu même sans autre activité.
    pub fn is_news(self) -> bool {
        matches!(self, Kind::CiFailed | Kind::ReviewRequested)
    }
}

/// État de la CI d'une PR.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ci {
    Success,
    Pending,
    Failure,
}

/// Vérification de CI en échec : son nom et sa page.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Check {
    pub name: String,
    pub url: String,
}

/// Commentaire montré dans le détail (lu à la demande, jamais enregistré).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Comment {
    pub author: String,
    /// Secondes Unix (0 si inconnu).
    pub at: i64,
    pub body: String,
}

/// Ce qu'il faut pour relire une entrée sur sa forge (commentaires, détail).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub enum Target {
    /// Entrée du premier jet, ou sans détail possible.
    #[default]
    Unknown,
    /// GitHub : PR (même numérotation que les tickets).
    Pull,
    /// GitHub : ticket.
    Issue,
    GitLab {
        project_id: u64,
        iid: u64,
        mr: bool,
    },
    Azure {
        org: String,
        project: String,
        repo_id: String,
        id: u64,
        pr: bool,
    },
}

/// Une entrée telle que lue sur la forge.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Item {
    /// Identifiant stable, indépendant du type (voir `make_id`).
    pub id: String,
    /// Compte qui l'a lue (vide pour les entrées du premier jet).
    #[serde(default)]
    pub account: String,
    pub forge: Forge,
    pub kind: Kind,
    /// `propriétaire/nom` (GitHub), chemin du projet (GitLab), `org/projet/dépôt` ou `org/projet` (Azure).
    pub repo: String,
    pub number: Option<u64>,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub author: String,
    /// Dernière activité, en secondes Unix.
    pub updated_at: i64,
    #[serde(default)]
    pub ci: Option<Ci>,
    /// Résumé court (début de la description).
    #[serde(default)]
    pub summary: String,
    /// Vérifications de CI en échec.
    #[serde(default)]
    pub checks: Vec<Check>,
    /// Nombre de commentaires connu de la forge (0 si inconnu).
    #[serde(default)]
    pub comments: u32,
    #[serde(default)]
    pub target: Target,
    /// Compteur des interactions (commentaires, revues) sur mes PR : quand il est connu, c'est lui, et non la
    /// date d'activité, qui dit si la PR « a bougé » (une poussée de ma part ne compte pas).
    #[serde(default)]
    pub activity: Option<u32>,
    /// La dernière activité connue est de moi (mon commentaire) : elle ne remet pas l'entrée en non lu.
    #[serde(default)]
    pub last_by_me: bool,
    /// Le détail (branche, CI, activité) n'a pas pu être lu : la fusion le reprend du cache. Jamais enregistré.
    #[serde(skip)]
    pub partial: bool,
    /// Champs écrits par une autre version, recopiés tels quels.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl Item {
    /// Identifiant stable : forge (et hôte s'il n'est pas celui par défaut), dépôt et numéro. Le type n'en fait
    /// pas partie : une PR qui passe de « Ma PR » à « CI en échec » reste la même entrée. GitLab et Azure DevOps
    /// numérotent PR et tickets à part : `!` pour une PR, `#` pour un ticket (GitHub : `#` pour les deux).
    pub fn make_id(forge: Forge, host: &str, repo: &str, number: Option<u64>, pull: bool) -> String {
        let host = host.trim();
        let mut id = if host.is_empty() || host == forge.default_host() {
            format!("{forge:?}:{repo}")
        } else {
            format!("{forge:?}@{host}:{repo}")
        };
        if let Some(n) = number {
            let sep = if pull && forge != Forge::GitHub { '!' } else { '#' };
            id.push_str(&format!("{sep}{n}"));
        }
        id
    }
}

/// État local d'une entrée, choisi par l'utilisateur.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum Status {
    #[default]
    Unread,
    Read,
    /// Masquée jusqu'à cette date (secondes Unix).
    Snoozed {
        until: i64,
    },
    Done,
}

/// Choix de l'utilisateur sur une entrée, et ce qu'il avait vu à ce moment-là.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Mark {
    pub status: Status,
    /// `updated_at` de l'entrée (ou l'heure d'ouverture) au moment du choix.
    pub seen_updated_at: i64,
    /// Type de l'entrée au moment du choix.
    #[serde(default)]
    pub seen_kind: Option<Kind>,
    /// Compteur d'activité au moment du choix (mes PR).
    #[serde(default)]
    pub seen_activity: Option<u32>,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl Mark {
    /// Le choix `status` sur `item`, tel que vu maintenant (`seen_at` : heure d'ouverture, si elle est plus
    /// récente que la dernière activité connue).
    pub fn new(status: Status, item: &Item, seen_at: Option<i64>) -> Self {
        Mark {
            status,
            seen_updated_at: item.updated_at.max(seen_at.unwrap_or(i64::MIN)),
            seen_kind: Some(item.kind),
            seen_activity: item.activity,
            unknown: BTreeMap::new(),
        }
    }

    /// État effectif de l'entrée à l'instant `now`. Une entrée lue ou terminée redevient non lue quand :
    /// - elle passe à un type qui est une nouvelle (CI devenue rouge, revue demandée) ;
    /// - son compteur d'activité augmente (mes PR), ou sinon sa date d'activité avance ;
    /// - sauf si cette dernière activité est la mienne.
    pub fn effective(&self, item: &Item, now: i64) -> Status {
        match self.status {
            Status::Snoozed { until } if now >= until => Status::Unread,
            Status::Read | Status::Done => {
                let kind_news = self.seen_kind.is_some_and(|k| k != item.kind) && item.kind.is_news();
                let moved = match (item.activity, self.seen_activity) {
                    (Some(a), Some(seen)) => a > seen,
                    _ => item.updated_at > self.seen_updated_at,
                };
                if kind_news || (moved && !item.last_by_me) { Status::Unread } else { self.status }
            }
            s => s,
        }
    }

    /// Absorbe une activité qui est la mienne : ce qui a été vu avance jusqu'à l'état actuel de l'entrée.
    pub fn absorb(&mut self, item: &Item) {
        self.seen_updated_at = self.seen_updated_at.max(item.updated_at);
        if item.activity.is_some() {
            self.seen_activity = item.activity;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: Kind, updated_at: i64) -> Item {
        Item { id: "x".into(), kind, updated_at, ..Item::default() }
    }

    #[test]
    fn new_activity_brings_a_read_or_done_entry_back() {
        let seen = item(Kind::Mention, 100);
        let read = Mark::new(Status::Read, &seen, None);
        assert_eq!(read.effective(&item(Kind::Mention, 100), 500), Status::Read);
        assert_eq!(read.effective(&item(Kind::Mention, 101), 500), Status::Unread);
        let done = Mark::new(Status::Done, &seen, None);
        assert_eq!(done.effective(&item(Kind::Mention, 100), 500), Status::Done);
        assert_eq!(done.effective(&item(Kind::Mention, 200), 500), Status::Unread);
    }

    #[test]
    fn opening_counts_as_seen_until_the_opening_time() {
        let m = Mark::new(Status::Read, &item(Kind::Mention, 100), Some(400));
        assert_eq!(m.effective(&item(Kind::Mention, 300), 500), Status::Read);
        assert_eq!(m.effective(&item(Kind::Mention, 401), 500), Status::Unread);
    }

    #[test]
    fn snooze_ends_at_its_date() {
        let m = Mark { status: Status::Snoozed { until: 1000 }, ..Mark::default() };
        assert_eq!(m.effective(&item(Kind::Mention, 0), 999), Status::Snoozed { until: 1000 });
        assert_eq!(m.effective(&item(Kind::Mention, 0), 1000), Status::Unread);
    }

    #[test]
    fn ci_turning_red_is_news_but_turning_green_is_not() {
        let m = Mark::new(Status::Done, &item(Kind::MyPr, 100), None);
        assert_eq!(m.effective(&item(Kind::CiFailed, 100), 500), Status::Unread);
        let m = Mark::new(Status::Done, &item(Kind::CiFailed, 100), None);
        assert_eq!(m.effective(&item(Kind::MyPr, 100), 500), Status::Done);
    }

    #[test]
    fn my_prs_move_with_their_activity_not_with_my_pushes() {
        let mine = |updated_at, activity| Item { activity: Some(activity), ..item(Kind::MyPr, updated_at) };
        let m = Mark::new(Status::Read, &mine(100, 2), None);
        // Une poussée : la date avance, pas le compteur.
        assert_eq!(m.effective(&mine(300, 2), 500), Status::Read);
        assert_eq!(m.effective(&mine(300, 3), 500), Status::Unread);
    }

    #[test]
    fn my_own_last_activity_does_not_bring_it_back() {
        let m = Mark::new(Status::Read, &item(Kind::Mention, 100), None);
        let mut answered = item(Kind::Mention, 300);
        answered.last_by_me = true;
        assert_eq!(m.effective(&answered, 500), Status::Read);
        let mut m = m;
        m.absorb(&answered);
        answered.last_by_me = false;
        assert_eq!(m.effective(&answered, 500), Status::Read);
    }

    #[test]
    fn ids_do_not_depend_on_the_kind() {
        assert_eq!(
            Item::make_id(Forge::GitHub, "github.com", "contoso/api", Some(142), true),
            "GitHub:contoso/api#142"
        );
        assert_eq!(Item::make_id(Forge::GitHub, "", "contoso/api", Some(142), false), "GitHub:contoso/api#142");
        assert_eq!(Item::make_id(Forge::GitLab, "gitlab.com", "contoso/api", Some(4), true), "GitLab:contoso/api!4");
        assert_eq!(Item::make_id(Forge::GitLab, "gitlab.com", "contoso/api", Some(4), false), "GitLab:contoso/api#4");
        assert_eq!(
            Item::make_id(Forge::GitHub, "ghe.contoso.test", "contoso/api", Some(1), true),
            "GitHub@ghe.contoso.test:contoso/api#1"
        );
    }
}
