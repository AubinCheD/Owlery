//! Entrées de la boîte de réception et leur état local (lu, reporté, terminé).

use serde::{Deserialize, Serialize};

/// Forge d'origine d'une entrée.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Forge {
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
}

/// Ce qui attend le développeur.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// PR où l'on est relecteur.
    ReviewRequested,
    /// CI en échec sur une de mes PR.
    CiFailed,
    /// Commentaire qui me cite.
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
}

/// État de la CI d'une PR.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ci {
    Success,
    Pending,
    Failure,
}

/// Une entrée telle que lue sur la forge.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Item {
    /// Identifiant stable : forge, dépôt, numéro et type (`GitHub:contoso/api#142:ReviewRequested`).
    pub id: String,
    pub forge: Forge,
    pub kind: Kind,
    /// `propriétaire/nom`.
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
}

impl Item {
    pub fn make_id(forge: Forge, repo: &str, number: Option<u64>, kind: Kind) -> String {
        match number {
            Some(n) => format!("{forge:?}:{repo}#{n}:{kind:?}"),
            None => format!("{forge:?}:{repo}:{kind:?}"),
        }
    }
}

/// État local d'une entrée, choisi par l'utilisateur.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum Status {
    #[default]
    Unread,
    Read,
    /// Masquée jusqu'à cette date (secondes Unix).
    Snoozed { until: i64 },
    Done,
}

/// Choix de l'utilisateur sur une entrée, et l'activité qu'il avait vue à ce moment-là.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub struct Mark {
    pub status: Status,
    /// `updated_at` de l'entrée au moment du choix : une activité plus récente la remet en non lu.
    pub seen_updated_at: i64,
}

impl Mark {
    /// État effectif à l'instant `now` pour une entrée dont la dernière activité est `updated_at`.
    pub fn effective(&self, updated_at: i64, now: i64) -> Status {
        match self.status {
            Status::Snoozed { until } if now >= until => Status::Unread,
            Status::Read | Status::Done if updated_at > self.seen_updated_at => Status::Unread,
            s => s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_activity_brings_a_read_or_done_entry_back() {
        let read = Mark { status: Status::Read, seen_updated_at: 100 };
        assert_eq!(read.effective(100, 500), Status::Read);
        assert_eq!(read.effective(101, 500), Status::Unread);
        let done = Mark { status: Status::Done, seen_updated_at: 100 };
        assert_eq!(done.effective(100, 500), Status::Done);
        assert_eq!(done.effective(200, 500), Status::Unread);
    }

    #[test]
    fn snooze_ends_at_its_date() {
        let m = Mark { status: Status::Snoozed { until: 1000 }, seen_updated_at: 0 };
        assert_eq!(m.effective(0, 999), Status::Snoozed { until: 1000 });
        assert_eq!(m.effective(0, 1000), Status::Unread);
    }

    #[test]
    fn ids_are_stable() {
        assert_eq!(
            Item::make_id(Forge::GitHub, "contoso/api", Some(142), Kind::ReviewRequested),
            "GitHub:contoso/api#142:ReviewRequested"
        );
    }
}
