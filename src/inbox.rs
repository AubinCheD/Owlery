//! La boîte : entrées en cache, choix de l'utilisateur, fusion d'une lecture, tri, filtres et groupes.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, Duration, Local, NaiveTime, TimeZone, Weekday};
use serde::{Deserialize, Serialize};

use crate::model::{Forge, Item, Kind, Mark, Status};

/// Durée pendant laquelle le choix sur une entrée disparue de la forge est gardé (elle peut revenir).
const FORGOTTEN_AFTER_SECS: i64 = 30 * 24 * 3600;

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct Inbox {
    /// Dernière lecture de chaque forge, par identifiant.
    #[serde(default)]
    pub items: BTreeMap<String, Item>,
    /// Choix de l'utilisateur, par identifiant d'entrée.
    #[serde(default)]
    pub marks: BTreeMap<String, Mark>,
    /// Dernière fois qu'une entrée a été vue sur sa forge (pour oublier les choix trop anciens).
    #[serde(default)]
    pub last_seen: BTreeMap<String, i64>,
}

/// Regroupement de la liste.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GroupBy {
    #[default]
    Repo,
    Kind,
}

/// Filtres de la liste.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    /// Types affichés (vide : tous).
    pub kinds: BTreeSet<Kind>,
    pub forge: Option<Forge>,
    /// Texte cherché dans le dépôt et le titre (sans tenir compte de la casse).
    pub text: String,
    /// Seulement ce qui m'attend : non lus.
    pub waiting_only: bool,
    /// Montrer aussi les entrées terminées et reportées.
    pub show_hidden: bool,
}

/// Un groupe affiché : son titre, ses entrées triées et son nombre de non lus.
#[derive(Debug, PartialEq)]
pub struct Group<'a> {
    pub title: String,
    pub entries: Vec<(&'a Item, Status)>,
    pub unread: usize,
}

/// Quand une entrée reportée revient.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Snooze {
    /// Dans trois heures.
    Later,
    /// Demain à 9 h.
    Tomorrow,
    /// Lundi prochain à 9 h.
    Monday,
}

impl Snooze {
    pub const ALL: [Snooze; 3] = [Snooze::Later, Snooze::Tomorrow, Snooze::Monday];

    pub fn label(self) -> &'static str {
        match self {
            Snooze::Later => "Plus tard (3 h)",
            Snooze::Tomorrow => "Demain matin",
            Snooze::Monday => "Lundi matin",
        }
    }

    /// Date de retour, en secondes Unix, à partir de `now` (heure locale).
    pub fn until(self, now: i64) -> i64 {
        let now = Local.timestamp_opt(now, 0).single().unwrap_or_else(Local::now);
        let nine = NaiveTime::from_hms_opt(9, 0, 0).expect("heure valide");
        let morning = |days: i64| {
            let date = now.date_naive() + Duration::days(days);
            Local.from_local_datetime(&date.and_time(nine)).earliest().map(|d| d.timestamp())
        };
        let fallback = now.timestamp() + 24 * 3600;
        match self {
            Snooze::Later => now.timestamp() + 3 * 3600,
            Snooze::Tomorrow => morning(1).unwrap_or(fallback),
            Snooze::Monday => {
                let days = match now.weekday() {
                    Weekday::Mon => 7,
                    d => 7 - i64::from(d.num_days_from_monday()),
                };
                morning(days).unwrap_or(fallback)
            }
        }
    }
}

impl Inbox {
    /// Remplace les entrées d'une forge par une nouvelle lecture. Les choix sont gardés ; ceux des entrées
    /// absentes depuis plus de 30 jours sont oubliés.
    pub fn merge(&mut self, forge: Forge, fetched: Vec<Item>, now: i64) {
        self.items.retain(|_, item| item.forge != forge);
        for item in fetched {
            self.last_seen.insert(item.id.clone(), now);
            self.items.insert(item.id.clone(), item);
        }
        let last_seen = &self.last_seen;
        self.marks.retain(|id, _| last_seen.get(id).is_some_and(|t| now - t < FORGOTTEN_AFTER_SECS));
        let marks = &self.marks;
        let items = &self.items;
        self.last_seen
            .retain(|id, t| now - *t < FORGOTTEN_AFTER_SECS || marks.contains_key(id) || items.contains_key(id));
    }

    pub fn status(&self, item: &Item, now: i64) -> Status {
        self.marks.get(&item.id).map(|m| m.effective(item.updated_at, now)).unwrap_or(Status::Unread)
    }

    /// Enregistre un choix de l'utilisateur sur une entrée (sans effet si elle n'existe plus).
    pub fn set_status(&mut self, id: &str, status: Status) {
        let Some(item) = self.items.get(id) else { return };
        if status == Status::Unread {
            self.marks.remove(id);
        } else {
            self.marks.insert(id.to_string(), Mark { status, seen_updated_at: item.updated_at });
        }
    }

    /// Nombre de choses qui m'attendent (non lus).
    pub fn waiting(&self, now: i64) -> usize {
        self.items.values().filter(|i| self.status(i, now) == Status::Unread).count()
    }

    /// Liste filtrée, groupée et triée : non lus en tête, puis activité la plus récente ; les groupes qui ont
    /// des non lus d'abord, puis par activité la plus récente.
    pub fn view(&self, filter: &Filter, group_by: GroupBy, now: i64) -> Vec<Group<'_>> {
        let needle = filter.text.trim().to_lowercase();
        let mut groups: BTreeMap<String, Vec<(&Item, Status)>> = BTreeMap::new();
        for item in self.items.values() {
            let status = self.status(item, now);
            let hidden = matches!(status, Status::Done | Status::Snoozed { .. });
            if (hidden && !filter.show_hidden)
                || (filter.waiting_only && status != Status::Unread)
                || (!filter.kinds.is_empty() && !filter.kinds.contains(&item.kind))
                || filter.forge.is_some_and(|f| f != item.forge)
                || (!needle.is_empty()
                    && !item.repo.to_lowercase().contains(&needle)
                    && !item.title.to_lowercase().contains(&needle))
            {
                continue;
            }
            let title = match group_by {
                GroupBy::Repo => item.repo.clone(),
                GroupBy::Kind => item.kind.label().to_string(),
            };
            groups.entry(title).or_default().push((item, status));
        }
        let mut out: Vec<Group> = groups
            .into_iter()
            .map(|(title, mut entries)| {
                entries.sort_by(|(a, sa), (b, sb)| {
                    (*sb == Status::Unread).cmp(&(*sa == Status::Unread)).then(b.updated_at.cmp(&a.updated_at))
                });
                let unread = entries.iter().filter(|(_, s)| *s == Status::Unread).count();
                Group { title, entries, unread }
            })
            .collect();
        let latest = |g: &Group| g.entries.iter().map(|(i, _)| i.updated_at).max().unwrap_or(0);
        out.sort_by(|a, b| (b.unread > 0).cmp(&(a.unread > 0)).then(latest(b).cmp(&latest(a))));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(repo: &str, n: u64, kind: Kind, updated_at: i64) -> Item {
        Item {
            id: Item::make_id(Forge::GitHub, repo, Some(n), kind),
            forge: Forge::GitHub,
            kind,
            repo: repo.into(),
            number: Some(n),
            title: format!("Entrée {n}"),
            url: format!("https://github.com/{repo}/pull/{n}"),
            branch: None,
            author: "camille".into(),
            updated_at,
            ci: None,
            summary: String::new(),
        }
    }

    fn sample() -> Inbox {
        let mut inbox = Inbox::default();
        inbox.merge(
            Forge::GitHub,
            vec![
                item("contoso/api", 1, Kind::ReviewRequested, 100),
                item("contoso/api", 2, Kind::CiFailed, 300),
                item("fabrikam/web", 3, Kind::Mention, 200),
            ],
            1000,
        );
        inbox
    }

    #[test]
    fn unread_first_then_most_recent() {
        let mut inbox = sample();
        let id2 = inbox.items.values().find(|i| i.number == Some(2)).unwrap().id.clone();
        inbox.set_status(&id2, Status::Read);
        let view = inbox.view(&Filter::default(), GroupBy::Repo, 1000);
        assert_eq!(view.len(), 2);
        let api = view.iter().find(|g| g.title == "contoso/api").unwrap();
        assert_eq!(api.entries[0].0.number, Some(1));
        assert_eq!(api.entries[1].1, Status::Read);
        assert_eq!(api.unread, 1);
        assert_eq!(inbox.waiting(1000), 2);
    }

    #[test]
    fn done_and_snoozed_are_hidden_until_due() {
        let mut inbox = sample();
        let ids: Vec<String> = inbox.items.keys().cloned().collect();
        inbox.set_status(&ids[0], Status::Done);
        inbox.set_status(&ids[1], Status::Snoozed { until: 2000 });
        let count = |inbox: &Inbox, now| {
            inbox.view(&Filter::default(), GroupBy::Kind, now).iter().map(|g| g.entries.len()).sum::<usize>()
        };
        assert_eq!(count(&inbox, 1000), 1);
        assert_eq!(count(&inbox, 2000), 2);
        let all = Filter { show_hidden: true, ..Filter::default() };
        assert_eq!(inbox.view(&all, GroupBy::Kind, 1000).iter().map(|g| g.entries.len()).sum::<usize>(), 3);
    }

    #[test]
    fn filters_combine() {
        let inbox = sample();
        let f = Filter { text: "FABRIKAM".into(), ..Filter::default() };
        assert_eq!(inbox.view(&f, GroupBy::Repo, 1000)[0].title, "fabrikam/web");
        let f = Filter { kinds: [Kind::CiFailed].into(), ..Filter::default() };
        assert_eq!(inbox.view(&f, GroupBy::Kind, 1000)[0].title, "CI en échec");
        let f = Filter { forge: Some(Forge::GitLab), ..Filter::default() };
        assert!(inbox.view(&f, GroupBy::Repo, 1000).is_empty());
    }

    #[test]
    fn marks_survive_a_refresh_and_are_forgotten_after_a_month() {
        let mut inbox = sample();
        let id = inbox.items.keys().next().unwrap().clone();
        inbox.set_status(&id, Status::Done);
        inbox.merge(Forge::GitHub, vec![], 2000);
        assert!(inbox.items.is_empty());
        assert!(inbox.marks.contains_key(&id));
        inbox.merge(Forge::GitHub, vec![], 1000 + FORGOTTEN_AFTER_SECS);
        assert!(inbox.marks.is_empty());
    }

    #[test]
    fn merge_only_replaces_the_same_forge() {
        let mut inbox = sample();
        let mut other = item("northwind/app", 9, Kind::Assigned, 50);
        other.forge = Forge::GitLab;
        other.id = Item::make_id(Forge::GitLab, "northwind/app", Some(9), Kind::Assigned);
        inbox.merge(Forge::GitLab, vec![other], 1000);
        assert_eq!(inbox.items.len(), 4);
    }

    #[test]
    fn snooze_dates_are_in_the_future_at_nine() {
        let now = Local::now().timestamp();
        assert_eq!(Snooze::Later.until(now), now + 3 * 3600);
        for s in [Snooze::Tomorrow, Snooze::Monday] {
            let t = Local.timestamp_opt(s.until(now), 0).unwrap();
            assert!(t.timestamp() > now);
            assert_eq!(t.format("%H:%M").to_string(), "09:00");
        }
        assert_eq!(Local.timestamp_opt(Snooze::Monday.until(now), 0).unwrap().weekday(), Weekday::Mon);
    }
}
