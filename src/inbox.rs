//! La boîte : entrées en cache, choix de l'utilisateur, fusion d'une lecture, tri, filtres et groupes.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, Duration, Local, NaiveTime, TimeZone, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Forge, Item, Kind, Mark, Status};

/// Durée pendant laquelle le choix sur une entrée disparue de la forge est gardé (elle peut revenir).
const FORGOTTEN_AFTER_SECS: i64 = 30 * 24 * 3600;

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct Inbox {
    /// Dernière lecture de chaque compte, par identifiant.
    #[serde(default)]
    pub items: BTreeMap<String, Item>,
    /// Choix de l'utilisateur, par identifiant d'entrée.
    #[serde(default)]
    pub marks: BTreeMap<String, Mark>,
    /// Dernière fois qu'une entrée a été vue sur sa forge (pour oublier les choix trop anciens).
    #[serde(default)]
    pub last_seen: BTreeMap<String, i64>,
    /// Comptes lus au moins une fois : la première lecture d'un compte ne notifie rien (sinon une avalanche).
    #[serde(default)]
    pub fetched: BTreeSet<String>,
    /// Entrées et choix enregistrés par une autre version et illisibles par celle-ci : gardés tels quels et
    /// réécrits à l'enregistrement (voir `state`).
    #[serde(skip)]
    pub unreadable: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// Regroupement de la liste.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GroupBy {
    #[default]
    Repo,
    Kind,
}

/// Filtres de la liste (gardés entre deux lancements, sauf le texte cherché).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Filter {
    /// Types affichés (vide : tous).
    pub kinds: BTreeSet<Kind>,
    pub forge: Option<Forge>,
    /// Compte d'où viennent les entrées affichées (aucun : tous les comptes).
    pub account: Option<String>,
    /// Texte cherché dans le dépôt, le titre et l'auteur (sans tenir compte de la casse).
    #[serde(skip)]
    pub text: String,
    /// Seulement ce qui m'attend : non lus.
    pub waiting_only: bool,
    /// Montrer aussi les entrées terminées et reportées.
    pub show_hidden: bool,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// Règles permanentes des réglages : dépôts ignorés, robots masqués. Elles valent aussi pour le compteur et
/// les notifications.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Rules {
    pub muted_repos: BTreeSet<String>,
    pub hide_bots: bool,
}

impl Rules {
    pub fn shows(&self, item: &Item) -> bool {
        !self.muted_repos.contains(&item.repo) && !(self.hide_bots && is_bot(&item.author))
    }
}

/// Auteur automatique : `dependabot[bot]`, `renovate-bot`, `github-actions`…
pub fn is_bot(author: &str) -> bool {
    let a = author.trim().to_ascii_lowercase();
    a.ends_with("[bot]")
        || a.ends_with("-bot")
        || a.ends_with("_bot")
        || matches!(a.as_str(), "dependabot" | "renovate" | "github-actions")
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

/// Ce que la fusion d'une lecture a changé : de quoi décider des notifications.
#[derive(Debug, Default, PartialEq)]
pub struct Merged {
    /// Le compte n'avait encore rien en cache (premier chargement).
    pub first_fetch: bool,
    /// Entrées du compte qui étaient non lues avant la lecture.
    pub before_unread: BTreeSet<String>,
}

/// Un choix à défaire : l'entrée et son choix précédent (aucun : non lue).
#[derive(Debug, Clone, PartialEq)]
pub struct Undo {
    pub id: String,
    pub previous: Option<Mark>,
}

impl Inbox {
    /// Remplace les entrées d'un compte par une nouvelle lecture. Les entrées du premier jet (sans compte) sont
    /// rattachées au premier compte de la même forge qui est lu. Les choix sont gardés ; ceux des entrées
    /// absentes depuis plus de 30 jours sont oubliés. Une de mes PR vue pour la première fois est marquée lue
    /// (elle n'est pas une nouvelle) ; une activité qui est la mienne est absorbée.
    pub fn merge(&mut self, account: &str, forge: Forge, fetched: Vec<Item>, now: i64) -> Merged {
        self.merge_with(account, forge, fetched, true, now)
    }

    /// `merge`, pour une lecture qui peut être incomplète (`complete` faux : la forge a dit ne pas tout
    /// renvoyer) : les entrées du compte qu'elle n'a pas renvoyées sont alors gardées.
    pub fn merge_with(&mut self, account: &str, forge: Forge, fetched: Vec<Item>, complete: bool, now: i64) -> Merged {
        let belongs = |i: &Item| i.account == account || (i.account.is_empty() && i.forge == forge);
        // Premier chargement : jamais lu, et rien en cache (les entrées du premier jet comptent comme lues).
        let first_fetch = !self.fetched.contains(account) && !self.items.values().any(belongs);
        self.fetched.insert(account.to_string());
        let before_unread: BTreeSet<String> = self
            .items
            .values()
            .filter(|i| belongs(i) && self.status(i, now) == Status::Unread)
            .map(|i| i.id.clone())
            .collect();
        let known: BTreeSet<String> = self.items.keys().cloned().collect();
        let mut previous: BTreeMap<String, Item> = BTreeMap::new();
        self.items.retain(|id, item| {
            if belongs(item) {
                previous.insert(id.clone(), item.clone());
            }
            !belongs(item)
        });
        let returned: BTreeSet<String> = fetched.iter().map(|i| i.id.clone()).collect();
        for mut item in fetched {
            if item.partial {
                // Détail illisible cette fois : celui du cache, plutôt qu'une CI qui disparaît.
                if let Some(old) = previous.get(&item.id) {
                    item.branch = item.branch.take().or_else(|| old.branch.clone());
                    item.ci = old.ci;
                    item.checks = old.checks.clone();
                    item.activity = item.activity.or(old.activity);
                    item.updated_at = item.updated_at.max(old.updated_at);
                    if old.kind == Kind::CiFailed && item.kind == Kind::MyPr {
                        item.kind = Kind::CiFailed;
                    }
                }
                item.partial = false;
            }
            self.last_seen.insert(item.id.clone(), now);
            if let Some(mark) = self.marks.get_mut(&item.id) {
                if matches!(mark.status, Status::Read | Status::Done) {
                    if item.last_by_me {
                        mark.absorb(&item);
                    }
                    // Passé à un type qui n'est pas une nouvelle (CI redevenue verte) : c'est vu, pour qu'un
                    // nouveau passage au rouge le soit à nouveau.
                    if mark.seen_kind.is_some_and(|k| k != item.kind) && !item.kind.is_news() {
                        mark.seen_kind = Some(item.kind);
                    }
                }
            } else if item.kind == Kind::MyPr && !known.contains(&item.id) {
                self.marks.insert(item.id.clone(), Mark::new(Status::Read, &item, None));
            }
            self.items.insert(item.id.clone(), item);
        }
        if !complete {
            // Lecture incomplète : ce qu'elle n'a pas renvoyé reste en cache, plutôt que de disparaître et de
            // revenir « nouveau » à la lecture suivante.
            for (id, old) in previous {
                if !returned.contains(&id) {
                    self.items.insert(id, old);
                }
            }
        }
        let (last_seen, items) = (&self.last_seen, &self.items);
        // Un choix est gardé tant que son entrée est en cache, et 30 jours après sa disparition de la forge.
        self.marks.retain(|id, _| {
            items.contains_key(id) || last_seen.get(id).is_some_and(|t| now - t < FORGOTTEN_AFTER_SECS)
        });
        let (marks, items) = (&self.marks, &self.items);
        self.last_seen
            .retain(|id, t| now - *t < FORGOTTEN_AFTER_SECS || marks.contains_key(id) || items.contains_key(id));
        Merged { first_fetch, before_unread }
    }

    /// Retire les entrées d'un compte supprimé (leurs choix suivent la règle des 30 jours).
    pub fn remove_account(&mut self, account: &str) {
        self.items.retain(|_, i| i.account != account);
        self.fetched.remove(account);
    }

    /// Retire les entrées des comptes qui ne sont plus lus (compte « gh » remplacé par un compte GitHub, gh
    /// désinstallé…) : sinon elles resteraient en attente pour toujours. Les entrées du premier jet (sans compte)
    /// attendent d'être rattachées. Renvoie le nombre d'entrées retirées.
    pub fn retain_accounts(&mut self, accounts: &BTreeSet<String>) -> usize {
        let before = self.items.len();
        self.items.retain(|_, i| i.account.is_empty() || accounts.contains(&i.account));
        self.fetched.retain(|a| accounts.contains(a));
        before - self.items.len()
    }

    pub fn status(&self, item: &Item, now: i64) -> Status {
        self.marks.get(&item.id).map(|m| m.effective(item, now)).unwrap_or(Status::Unread)
    }

    /// Enregistre un choix (`seen_at` : heure d'ouverture dans le navigateur, le cas échéant). Renvoie de quoi le
    /// défaire ; rien si l'entrée n'existe plus.
    pub fn set_status(&mut self, id: &str, status: Status, seen_at: Option<i64>) -> Option<Undo> {
        let item = self.items.get(id)?;
        let previous = self.marks.get(id).cloned();
        if status == Status::Unread {
            self.marks.remove(id);
        } else {
            let mark = Mark::new(status, item, seen_at);
            self.marks.insert(id.to_string(), mark);
        }
        Some(Undo { id: id.to_string(), previous })
    }

    /// Défait un choix.
    pub fn restore(&mut self, undo: Undo) {
        match undo.previous {
            Some(m) => self.marks.insert(undo.id, m),
            None => self.marks.remove(&undo.id),
        };
    }

    /// Reports arrivés à échéance : ils redeviennent non lus (choix retiré). Renvoie leurs identifiants.
    pub fn take_due_snoozes(&mut self, now: i64) -> Vec<String> {
        let due: Vec<String> = self
            .marks
            .iter()
            .filter(|(id, m)| {
                matches!(m.status, Status::Snoozed { until } if until <= now) && self.items.contains_key(*id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &due {
            self.marks.remove(id);
        }
        due
    }

    /// Nombre de choses qui m'attendent (non lues, hors dépôts ignorés et robots masqués).
    pub fn waiting(&self, rules: &Rules, now: i64) -> usize {
        self.items.values().filter(|i| rules.shows(i) && self.status(i, now) == Status::Unread).count()
    }

    /// Liste filtrée, groupée et triée : non lus en tête, puis activité la plus récente ; les groupes qui ont
    /// des non lus d'abord, puis par activité la plus récente.
    pub fn view(&self, filter: &Filter, rules: &Rules, group_by: GroupBy, now: i64) -> Vec<Group<'_>> {
        let needle = filter.text.trim().to_lowercase();
        let mut groups: BTreeMap<String, Vec<(&Item, Status)>> = BTreeMap::new();
        for item in self.items.values() {
            let status = self.status(item, now);
            if !rules.shows(item) || !passes(filter, &needle, item, status) {
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

    /// L'entrée resterait-elle affichée avec ce statut ? (pour déplacer la sélection quand elle disparaît)
    pub fn would_show(&self, filter: &Filter, rules: &Rules, id: &str, status: Status) -> bool {
        self.items
            .get(id)
            .is_some_and(|i| rules.shows(i) && passes(filter, &filter.text.trim().to_lowercase(), i, status))
    }
}

fn passes(filter: &Filter, needle: &str, item: &Item, status: Status) -> bool {
    let hidden = matches!(status, Status::Done | Status::Snoozed { .. });
    !((hidden && !filter.show_hidden)
        || (filter.waiting_only && status != Status::Unread)
        || (!filter.kinds.is_empty() && !filter.kinds.contains(&item.kind))
        || filter.forge.is_some_and(|f| f != item.forge)
        // Une entrée d'avant les comptes (sans compte) reste visible : la prochaine lecture lui en donne un.
        || filter.account.as_ref().is_some_and(|a| !item.account.is_empty() && &item.account != a)
        || (!needle.is_empty()
            && !item.repo.to_lowercase().contains(needle)
            && !item.title.to_lowercase().contains(needle)
            && !item.author.to_lowercase().contains(needle)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(repo: &str, n: u64, kind: Kind, updated_at: i64) -> Item {
        Item {
            id: Item::make_id(Forge::GitHub, "", repo, Some(n), true),
            account: "a".into(),
            forge: Forge::GitHub,
            kind,
            repo: repo.into(),
            number: Some(n),
            title: format!("Entrée {n}"),
            url: format!("https://github.com/{repo}/pull/{n}"),
            author: "camille".into(),
            updated_at,
            ..Item::default()
        }
    }

    fn sample() -> Inbox {
        let mut inbox = Inbox::default();
        inbox.merge(
            "a",
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

    fn id(inbox: &Inbox, n: u64) -> String {
        inbox.items.values().find(|i| i.number == Some(n)).unwrap().id.clone()
    }

    fn count(inbox: &Inbox, filter: &Filter, now: i64) -> usize {
        inbox.view(filter, &Rules::default(), GroupBy::Kind, now).iter().map(|g| g.entries.len()).sum()
    }

    #[test]
    fn unread_first_then_most_recent() {
        let mut inbox = sample();
        inbox.set_status(&id(&inbox, 2), Status::Read, None);
        let view = inbox.view(&Filter::default(), &Rules::default(), GroupBy::Repo, 1000);
        assert_eq!(view.len(), 2);
        let api = view.iter().find(|g| g.title == "contoso/api").unwrap();
        assert_eq!(api.entries[0].0.number, Some(1));
        assert_eq!(api.entries[1].1, Status::Read);
        assert_eq!(api.unread, 1);
        assert_eq!(inbox.waiting(&Rules::default(), 1000), 2);
    }

    #[test]
    fn done_and_snoozed_are_hidden_until_due() {
        let mut inbox = sample();
        inbox.set_status(&id(&inbox, 1), Status::Done, None);
        inbox.set_status(&id(&inbox, 2), Status::Snoozed { until: 2000 }, None);
        assert_eq!(count(&inbox, &Filter::default(), 1000), 1);
        assert_eq!(count(&inbox, &Filter::default(), 2000), 2);
        let all = Filter { show_hidden: true, ..Filter::default() };
        assert_eq!(count(&inbox, &all, 1000), 3);
    }

    #[test]
    fn due_snoozes_become_unread_once() {
        let mut inbox = sample();
        inbox.set_status(&id(&inbox, 2), Status::Snoozed { until: 2000 }, None);
        assert!(inbox.take_due_snoozes(1999).is_empty());
        assert_eq!(inbox.take_due_snoozes(2000), vec![id(&inbox, 2)]);
        assert!(inbox.take_due_snoozes(3000).is_empty());
        assert_eq!(inbox.status(&inbox.items[&id(&inbox, 2)], 3000), Status::Unread);
    }

    #[test]
    fn filters_combine() {
        let inbox = sample();
        let rules = Rules::default();
        let f = Filter { text: "FABRIKAM".into(), ..Filter::default() };
        assert_eq!(inbox.view(&f, &rules, GroupBy::Repo, 1000)[0].title, "fabrikam/web");
        let f = Filter { text: "Camille".into(), ..Filter::default() };
        assert_eq!(count(&inbox, &f, 1000), 3);
        let f = Filter { kinds: [Kind::CiFailed].into(), ..Filter::default() };
        assert_eq!(inbox.view(&f, &rules, GroupBy::Kind, 1000)[0].title, "CI en échec");
        let f = Filter { forge: Some(Forge::GitLab), ..Filter::default() };
        assert!(inbox.view(&f, &rules, GroupBy::Repo, 1000).is_empty());
    }

    /// Deux comptes de la même forge : le filtre par compte les sépare, là où celui des forges ne peut pas.
    #[test]
    fn the_account_filter_keeps_one_account() {
        let mut inbox = sample();
        let pro = Item { account: "b".into(), ..item("northwind/app", 9, Kind::Mention, 400) };
        inbox.merge("b", Forge::GitHub, vec![pro], 1000);
        let only = |account: &str| Filter { account: Some(account.into()), ..Filter::default() };
        assert_eq!(count(&inbox, &Filter::default(), 1000), 4);
        assert_eq!(count(&inbox, &only("a"), 1000), 3);
        assert_eq!(count(&inbox, &only("b"), 1000), 1);
        // Une entrée d'avant les comptes reste visible, quel que soit le compte choisi.
        let old = Item { account: String::new(), ..item("contoso/old", 4, Kind::Mention, 50) };
        inbox.items.insert(old.id.clone(), old);
        assert_eq!(count(&inbox, &only("b"), 1000), 2);
    }

    #[test]
    fn muted_repositories_are_hidden_and_not_counted() {
        let inbox = sample();
        let rules = Rules { muted_repos: ["contoso/api".to_string()].into(), hide_bots: false };
        assert_eq!(inbox.waiting(&rules, 1000), 1);
        assert_eq!(inbox.view(&Filter::default(), &rules, GroupBy::Repo, 1000).len(), 1);
    }

    #[test]
    fn bots_are_recognised() {
        for bot in ["dependabot[bot]", "renovate-bot", "github-actions", "Ci_Bot"] {
            assert!(is_bot(bot), "{bot}");
        }
        for human in ["camille", "robotique", "abbot"] {
            assert!(!is_bot(human), "{human}");
        }
        let mut inbox = sample();
        let mut bot = item("contoso/api", 9, Kind::ReviewRequested, 50);
        bot.author = "dependabot[bot]".into();
        let mut items: Vec<Item> = inbox.items.values().cloned().collect();
        items.push(bot);
        inbox.merge("a", Forge::GitHub, items, 1000);
        assert_eq!(inbox.waiting(&Rules { hide_bots: true, ..Rules::default() }, 1000), 3);
        assert_eq!(inbox.waiting(&Rules::default(), 1000), 4);
    }

    #[test]
    fn marks_survive_a_refresh_and_are_forgotten_after_a_month() {
        let mut inbox = sample();
        let id = id(&inbox, 1);
        inbox.set_status(&id, Status::Done, None);
        inbox.merge("a", Forge::GitHub, vec![], 2000);
        assert!(inbox.items.is_empty());
        assert!(inbox.marks.contains_key(&id));
        inbox.merge("a", Forge::GitHub, vec![], 1000 + FORGOTTEN_AFTER_SECS);
        assert!(inbox.marks.is_empty());
    }

    #[test]
    fn merge_replaces_only_the_account_read() {
        let mut inbox = sample();
        let mut other = item("northwind/app", 9, Kind::Assigned, 50);
        other.account = "b".into();
        other.id = Item::make_id(Forge::GitHub, "ghe.contoso.test", "northwind/app", Some(9), false);
        let merged = inbox.merge("b", Forge::GitHub, vec![other], 1000);
        assert!(merged.first_fetch);
        assert_eq!(inbox.items.len(), 4);
        let merged = inbox.merge("a", Forge::GitHub, vec![], 1000);
        assert!(!merged.first_fetch);
        assert_eq!(merged.before_unread.len(), 3);
        assert_eq!(inbox.items.len(), 1);
    }

    #[test]
    fn first_draft_items_are_adopted_by_the_first_github_account() {
        let mut inbox = sample();
        for i in inbox.items.values_mut() {
            i.account.clear();
        }
        let merged = inbox.merge("gh", Forge::GitHub, vec![item("contoso/api", 1, Kind::ReviewRequested, 100)], 1000);
        assert!(!merged.first_fetch, "le cache du premier jet compte");
        assert_eq!(inbox.items.len(), 1);
        // Une autre forge ne les adopte pas.
        let mut inbox = sample();
        for i in inbox.items.values_mut() {
            i.account.clear();
        }
        inbox.merge("gl", Forge::GitLab, vec![], 1000);
        assert_eq!(inbox.items.len(), 3);
    }

    #[test]
    fn a_new_pr_of_mine_is_not_news() {
        let mut inbox = Inbox::default();
        let mut mine = item("contoso/api", 5, Kind::MyPr, 100);
        mine.activity = Some(1);
        inbox.merge("a", Forge::GitHub, vec![mine.clone()], 1000);
        assert_eq!(inbox.status(&inbox.items[&mine.id], 1000), Status::Read);
        mine.activity = Some(2);
        inbox.merge("a", Forge::GitHub, vec![mine.clone()], 1100);
        assert_eq!(inbox.status(&inbox.items[&mine.id], 1100), Status::Unread);
        // Devenue rouge sans autre activité : c'est une nouvelle.
        let mut inbox = Inbox::default();
        mine.activity = Some(1);
        inbox.merge("a", Forge::GitHub, vec![mine.clone()], 1000);
        mine.kind = Kind::CiFailed;
        inbox.merge("a", Forge::GitHub, vec![mine.clone()], 1100);
        assert_eq!(inbox.status(&inbox.items[&mine.id], 1100), Status::Unread);
    }

    #[test]
    fn my_own_activity_is_absorbed() {
        let mut inbox = sample();
        let id1 = id(&inbox, 1);
        inbox.set_status(&id1, Status::Read, None);
        let mut answered = inbox.items[&id1].clone();
        answered.updated_at = 500;
        answered.last_by_me = true;
        let mut items: Vec<Item> = inbox.items.values().filter(|i| i.id != id1).cloned().collect();
        items.push(answered.clone());
        inbox.merge("a", Forge::GitHub, items.clone(), 1000);
        assert_eq!(inbox.marks[&id1].seen_updated_at, 500);
        // La lecture suivante ne sait plus que c'était moi : l'entrée reste lue quand même.
        answered.last_by_me = false;
        items.retain(|i| i.id != id1);
        items.push(answered);
        inbox.merge("a", Forge::GitHub, items, 1100);
        assert_eq!(inbox.status(&inbox.items[&id1], 1100), Status::Read);
    }

    #[test]
    fn a_quiet_account_notifies_its_first_arrival() {
        let mut inbox = Inbox::default();
        assert!(inbox.merge("calme", Forge::GitLab, vec![], 1000).first_fetch);
        assert!(!inbox.merge("calme", Forge::GitLab, vec![], 1100).first_fetch, "déjà lu, même vide");
        let mut first = item("northwind/app", 1, Kind::ReviewRequested, 1100);
        first.account = "calme".into();
        assert!(!inbox.merge("calme", Forge::GitLab, vec![first], 1200).first_fetch);
    }

    #[test]
    fn ci_turning_red_again_is_news() {
        let mut inbox = Inbox::default();
        let mut pr = item("contoso/api", 7, Kind::CiFailed, 100);
        pr.activity = Some(1);
        inbox.merge("a", Forge::GitHub, vec![pr.clone()], 1000);
        inbox.set_status(&pr.id, Status::Done, None);
        // Je pousse un correctif : la CI repasse au vert.
        pr.kind = Kind::MyPr;
        inbox.merge("a", Forge::GitHub, vec![pr.clone()], 1100);
        assert_eq!(inbox.status(&inbox.items[&pr.id], 1100), Status::Done);
        // Puis de nouveau au rouge : elle revient.
        pr.kind = Kind::CiFailed;
        inbox.merge("a", Forge::GitHub, vec![pr.clone()], 1200);
        assert_eq!(inbox.status(&inbox.items[&pr.id], 1200), Status::Unread);
    }

    #[test]
    fn orphan_entries_leave_and_choices_stay_while_cached() {
        let mut inbox = sample();
        let id1 = id(&inbox, 1);
        inbox.set_status(&id1, Status::Done, None);
        // Le compte « a » n'est plus lu depuis longtemps ; un autre l'est.
        inbox.merge("b", Forge::GitLab, vec![], 1000 + FORGOTTEN_AFTER_SECS + 10);
        assert_eq!(inbox.marks[&id1].status, Status::Done, "l'entrée est encore en cache : son choix reste");
        assert_eq!(inbox.retain_accounts(&["b".to_string()].into()), 3);
        assert!(inbox.items.is_empty());
        assert!(!inbox.fetched.contains("a"));
    }

    #[test]
    fn incomplete_reads_keep_what_they_did_not_return() {
        let mut inbox = sample();
        let kept: Vec<Item> = inbox.items.values().filter(|i| i.number == Some(1)).cloned().collect();
        inbox.merge_with("a", Forge::GitHub, kept.clone(), false, 1100);
        assert_eq!(inbox.items.len(), 3);
        inbox.merge_with("a", Forge::GitHub, kept, true, 1200);
        assert_eq!(inbox.items.len(), 1);
    }

    #[test]
    fn partial_detail_comes_from_the_cache() {
        let mut inbox = Inbox::default();
        let mut red = item("contoso/api", 7, Kind::CiFailed, 100);
        red.branch = Some("export".into());
        red.ci = Some(crate::model::Ci::Failure);
        red.activity = Some(3);
        inbox.merge("a", Forge::GitHub, vec![red.clone()], 1000);
        // Lecture suivante : le détail a échoué, la recherche la donne comme « Ma PR » sans CI.
        let partial = Item { kind: Kind::MyPr, partial: true, ..item("contoso/api", 7, Kind::MyPr, 100) };
        inbox.merge("a", Forge::GitHub, vec![partial], 1100);
        let kept = &inbox.items[&red.id];
        assert_eq!(kept.kind, Kind::CiFailed);
        assert_eq!(kept.ci, red.ci);
        assert_eq!(kept.branch.as_deref(), Some("export"));
        assert_eq!(kept.activity, Some(3));
        assert!(!kept.partial);
    }

    #[test]
    fn undo_restores_the_previous_mark() {
        let mut inbox = sample();
        let id1 = id(&inbox, 1);
        let first = inbox.set_status(&id1, Status::Read, None).unwrap();
        let second = inbox.set_status(&id1, Status::Done, None).unwrap();
        inbox.restore(second);
        assert_eq!(inbox.marks[&id1].status, Status::Read);
        inbox.restore(first);
        assert!(!inbox.marks.contains_key(&id1));
        assert!(inbox.set_status("absente", Status::Done, None).is_none());
    }

    #[test]
    fn would_show_follows_the_filter() {
        let inbox = sample();
        let waiting = Filter { waiting_only: true, ..Filter::default() };
        let id1 = id(&inbox, 1);
        assert!(inbox.would_show(&waiting, &Rules::default(), &id1, Status::Unread));
        assert!(!inbox.would_show(&waiting, &Rules::default(), &id1, Status::Read));
        assert!(inbox.would_show(&Filter::default(), &Rules::default(), &id1, Status::Read));
        assert!(!inbox.would_show(&Filter::default(), &Rules::default(), &id1, Status::Done));
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
