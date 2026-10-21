//! Règles des notifications Windows : quelles entrées méritent une bulle après une lecture, et son
//! texte. L'affichage (zone de notification) est dans `win::tray`.

use std::collections::BTreeSet;

use crate::inbox::{Inbox, Rules};
use crate::model::{Item, Kind, Status};

/// Longueurs maximales des champs de la bulle (zéro final compris : 64 et 256 caractères UTF-16).
pub const TITLE_MAX: usize = 63;
pub const BODY_MAX: usize = 255;

/// Types notifiés par défaut : tout ce qui attend une action, sauf « Ma PR » (trop bavard).
pub fn default_kinds() -> BTreeSet<Kind> {
    [Kind::ReviewRequested, Kind::CiFailed, Kind::Mention, Kind::Assigned].into()
}

/// Entrées d'un compte devenues non lues par une lecture : nouvelles, ou revenues par une activité.
/// Rien au premier chargement du compte (sinon une avalanche), rien pour les types décochés, les dépôts
/// ignorés et les robots masqués.
pub fn arrivals(
    before_unread: &BTreeSet<String>,
    first_fetch: bool,
    after: &Inbox,
    account: &str,
    kinds: &BTreeSet<Kind>,
    rules: &Rules,
    now: i64,
) -> Vec<Item> {
    if first_fetch {
        return Vec::new();
    }
    let mut out: Vec<Item> = after
        .items
        .values()
        .filter(|i| i.account == account && kinds.contains(&i.kind) && rules.shows(i))
        .filter(|i| after.status(i, now) == Status::Unread && !before_unread.contains(&i.id))
        .cloned()
        .collect();
    out.sort_by_key(|i| std::cmp::Reverse(i.updated_at));
    out
}

/// Titre et texte de la bulle, `None` s'il n'y a rien à dire.
pub fn message(items: &[Item], reminders: &[Item]) -> Option<(String, String)> {
    let (n, r) = (items.len(), reminders.len());
    let title = match (n, r) {
        (0, 0) => return None,
        (1, 0) => format!("{} · {}", items[0].kind.label(), items[0].repo),
        (_, 0) => format!("{n} nouvelles entrées"),
        (0, 1) => format!("Rappel · {}", reminders[0].repo),
        (0, _) => format!("{r} rappels"),
        (1, 1) => "1 nouvelle entrée, 1 rappel".to_string(),
        (1, _) => format!("1 nouvelle entrée, {r} rappels"),
        (_, 1) => format!("{n} nouvelles entrées, 1 rappel"),
        _ => format!("{n} nouvelles entrées, {r} rappels"),
    };
    let lines: Vec<String> = reminders
        .iter()
        .map(|i| format!("Rappel : {}", line(i)))
        .chain(items.iter().map(|i| if n + r == 1 { i.title.clone() } else { line(i) }))
        .collect();
    Some((cut(&title, TITLE_MAX), body(&lines)))
}

fn line(i: &Item) -> String {
    match i.number {
        Some(n) => format!("{} #{n} {}", i.repo, i.title),
        None => format!("{} {}", i.repo, i.title),
    }
}

fn cut(s: &str, max: usize) -> String {
    if s.encode_utf16().count() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        if out.encode_utf16().count() + c.len_utf16() + 1 > max {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// Lignes jointes dans la limite de la bulle ; celles qui ne tiennent pas deviennent « et k autres ».
fn body(lines: &[String]) -> String {
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        let rest = lines.len() - i;
        let more = if rest > 1 { format!("\net {} autres", rest - 1) } else { String::new() };
        let sep = if out.is_empty() { "" } else { "\n" };
        let candidate = format!("{out}{sep}{l}");
        let room_for_more = if rest > 1 { more.encode_utf16().count() } else { 0 };
        if candidate.encode_utf16().count() + room_for_more <= BODY_MAX {
            out = candidate;
            continue;
        }
        if out.is_empty() {
            return cut(l, BODY_MAX);
        }
        out.push_str(&format!("\net {rest} autre{}", if rest > 1 { "s" } else { "" }));
        return out;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Forge, Mark};

    fn item(n: u64, kind: Kind, account: &str, updated_at: i64) -> Item {
        Item {
            id: Item::make_id(Forge::GitHub, "", "contoso/api", Some(n), true),
            account: account.into(),
            kind,
            repo: "contoso/api".into(),
            number: Some(n),
            title: format!("Entrée {n}"),
            updated_at,
            ..Item::default()
        }
    }

    fn inbox(items: Vec<Item>) -> Inbox {
        let mut inbox = Inbox::default();
        inbox.merge("a", Forge::GitHub, items, 1000);
        inbox
    }

    #[test]
    fn no_notification_on_the_first_fetch_of_an_account() {
        let after = inbox(vec![item(1, Kind::Mention, "a", 10)]);
        assert!(arrivals(&BTreeSet::new(), true, &after, "a", &default_kinds(), &Rules::default(), 1000).is_empty());
    }

    #[test]
    fn only_newly_unread_entries_of_checked_kinds() {
        let after = inbox(vec![
            item(1, Kind::Mention, "a", 10),
            item(2, Kind::MyPr, "a", 10),
            item(3, Kind::Assigned, "a", 10),
        ]);
        let before: BTreeSet<String> = [after.items.values().find(|i| i.number == Some(3)).unwrap().id.clone()].into();
        let got = arrivals(&before, false, &after, "a", &default_kinds(), &Rules::default(), 1000);
        assert_eq!(got.iter().map(|i| i.number.unwrap()).collect::<Vec<_>>(), [1]);
        let muted = Rules { muted_repos: ["contoso/api".to_string()].into(), hide_bots: false };
        assert!(arrivals(&before, false, &after, "a", &default_kinds(), &muted, 1000).is_empty());
        assert!(arrivals(&before, false, &after, "autre", &default_kinds(), &Rules::default(), 1000).is_empty());
    }

    #[test]
    fn reactivated_entry_notifies_again() {
        let mut after = inbox(vec![item(1, Kind::Mention, "a", 10)]);
        let id = after.items.keys().next().unwrap().clone();
        after.marks.insert(id.clone(), Mark { status: Status::Done, seen_updated_at: 5, ..Mark::default() });
        // Terminée avant la lecture (donc pas « non lue »), activité plus récente : elle revient.
        let got = arrivals(&BTreeSet::new(), false, &after, "a", &default_kinds(), &Rules::default(), 1000);
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn message_summarises_many_entries() {
        let one = [item(1, Kind::ReviewRequested, "a", 1)];
        assert_eq!(message(&one, &[]), Some(("Revue demandée · contoso/api".into(), "Entrée 1".into())));
        assert_eq!(message(&[], &[]), None);
        let many: Vec<Item> = (1..=40).map(|n| item(n, Kind::Mention, "a", n as i64)).collect();
        let (title, body) = message(&many, &one).unwrap();
        assert_eq!(title, "40 nouvelles entrées, 1 rappel");
        assert!(body.starts_with("Rappel : contoso/api #1 Entrée 1\ncontoso/api #1 Entrée 1"));
        assert!(body.encode_utf16().count() <= BODY_MAX, "{}", body.len());
        assert!(body.contains("autres"));
        let mut long = item(1, Kind::Mention, "a", 1);
        long.repo = "fabrikam/un-depot-au-nom-vraiment-tres-long-pour-une-bulle-windows".into();
        let (title, _) = message(&[long], &[]).unwrap();
        assert!(title.encode_utf16().count() <= TITLE_MAX && title.ends_with('…'));
    }
}
