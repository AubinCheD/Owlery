//! Raccourcis clavier réglables, deux par commande, comme dans Fawkes. Enregistrés dans les réglages
//! sous forme de texte (« Ctrl+R »).

use std::collections::BTreeMap;

use eframe::egui::{Event, Key, Modifiers};

/// Ce qu'un raccourci déclenche.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Command {
    Refresh,
    Next,
    Prev,
    Open,
    MarkRead,
    MarkUnread,
    MarkDone,
    SnoozeTomorrow,
    Find,
    ToggleGroupBy,
    WaitingOnly,
    Palette,
    Settings,
}

impl Command {
    pub const ALL: [Command; 13] = [
        Command::Refresh,
        Command::Next,
        Command::Prev,
        Command::Open,
        Command::MarkRead,
        Command::MarkUnread,
        Command::MarkDone,
        Command::SnoozeTomorrow,
        Command::Find,
        Command::ToggleGroupBy,
        Command::WaitingOnly,
        Command::Palette,
        Command::Settings,
    ];

    /// Identifiant enregistré dans les réglages.
    pub fn id(self) -> &'static str {
        match self {
            Command::Refresh => "refresh",
            Command::Next => "next",
            Command::Prev => "prev",
            Command::Open => "open",
            Command::MarkRead => "mark_read",
            Command::MarkUnread => "mark_unread",
            Command::MarkDone => "mark_done",
            Command::SnoozeTomorrow => "snooze_tomorrow",
            Command::Find => "find",
            Command::ToggleGroupBy => "toggle_group_by",
            Command::WaitingOnly => "waiting_only",
            Command::Palette => "palette",
            Command::Settings => "settings",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Command::Refresh => "Actualiser",
            Command::Next => "Entrée suivante",
            Command::Prev => "Entrée précédente",
            Command::Open => "Ouvrir dans le navigateur",
            Command::MarkRead => "Marquer lu",
            Command::MarkUnread => "Marquer non lu",
            Command::MarkDone => "Marquer terminé",
            Command::SnoozeTomorrow => "Me le rappeler demain",
            Command::Find => "Chercher",
            Command::ToggleGroupBy => "Grouper par dépôt / par type",
            Command::WaitingOnly => "Seulement ce qui m'attend",
            Command::Palette => "Palette de commandes",
            Command::Settings => "Réglages",
        }
    }

    pub fn defaults(self) -> [Option<Binding>; 2] {
        let k = |ctrl, shift, key| Some(Binding { ctrl, shift, alt: false, key });
        match self {
            Command::Refresh => [k(false, false, Key::F5), k(true, false, Key::R)],
            Command::Next => [k(false, false, Key::ArrowDown), k(false, false, Key::J)],
            Command::Prev => [k(false, false, Key::ArrowUp), k(false, false, Key::K)],
            Command::Open => [k(false, false, Key::Enter), k(false, false, Key::O)],
            Command::MarkRead => [k(false, false, Key::R), None],
            Command::MarkUnread => [k(false, false, Key::U), None],
            Command::MarkDone => [k(false, false, Key::E), k(false, false, Key::Delete)],
            Command::SnoozeTomorrow => [k(false, false, Key::H), None],
            Command::Find => [k(true, false, Key::F), None],
            Command::ToggleGroupBy => [k(true, false, Key::G), None],
            Command::WaitingOnly => [k(true, false, Key::W), None],
            Command::Palette => [k(true, false, Key::P), k(true, true, Key::P)],
            Command::Settings => [k(true, false, Key::Comma), None],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl Binding {
    /// « Ctrl+Maj+P », « Entrée », « F5 ».
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for (on, name) in [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Maj")] {
            if on {
                parts.push(name.into());
            }
        }
        parts.push(
            match self.key {
                Key::Enter => "Entrée",
                Key::Escape => "Échap",
                Key::Delete => "Suppr",
                Key::Space => "Espace",
                Key::ArrowUp => "↑",
                Key::ArrowDown => "↓",
                Key::ArrowLeft => "←",
                Key::ArrowRight => "→",
                Key::Comma => ",",
                k => k.name(),
            }
            .to_string(),
        );
        parts.join("+")
    }

    /// Forme enregistrée : « Ctrl+Shift+ArrowDown » (noms egui des touches).
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
            if on {
                s.push_str(name);
            }
        }
        s.push_str(self.key.name());
        s
    }

    pub fn from_text(text: &str) -> Option<Binding> {
        let mut b = Binding { ctrl: false, shift: false, alt: false, key: Key::A };
        let mut parts: Vec<&str> = text.split('+').collect();
        let key = parts.pop()?;
        for p in parts {
            match p {
                "Ctrl" => b.ctrl = true,
                "Shift" => b.shift = true,
                "Alt" => b.alt = true,
                _ => return None,
            }
        }
        b.key = Key::from_name(key)?;
        Some(b)
    }

    pub fn from_event(ev: &Event) -> Option<Binding> {
        match ev {
            Event::Key { key, pressed: true, repeat: _, modifiers, .. } => Some(Self::with(modifiers, *key)),
            _ => None,
        }
    }

    fn with(m: &Modifiers, key: Key) -> Self {
        Binding { ctrl: m.ctrl || m.command, shift: m.shift, alt: m.alt, key }
    }

    /// Une touche sans Ctrl ni Alt se tape dans un champ de texte : elle n'agit pas pendant la saisie.
    pub fn types_text(&self) -> bool {
        !self.ctrl && !self.alt && !matches!(self.key, Key::F1 | Key::F2 | Key::F3 | Key::F4 | Key::F5 | Key::Escape)
    }
}

/// Raccourcis en vigueur : les défauts, remplacés par ceux réglés.
#[derive(Clone, Debug, PartialEq)]
pub struct Shortcuts(BTreeMap<Command, [Option<Binding>; 2]>);

impl Shortcuts {
    pub fn from_settings(saved: &BTreeMap<String, Vec<String>>) -> Self {
        let mut map = BTreeMap::new();
        for cmd in Command::ALL {
            let slots = match saved.get(cmd.id()) {
                Some(list) => {
                    let mut slots = [None, None];
                    for (slot, text) in slots.iter_mut().zip(list) {
                        *slot = Binding::from_text(text);
                    }
                    slots
                }
                None => cmd.defaults(),
            };
            map.insert(cmd, slots);
        }
        Shortcuts(map)
    }

    /// Ce qui diffère des défauts, à enregistrer.
    pub fn to_settings(&self) -> BTreeMap<String, Vec<String>> {
        self.0
            .iter()
            .filter(|(cmd, slots)| **slots != cmd.defaults())
            .map(|(cmd, slots)| {
                (cmd.id().to_string(), slots.iter().map(|b| b.map(|b| b.to_text()).unwrap_or_default()).collect())
            })
            .collect()
    }

    pub fn get(&self, cmd: Command) -> [Option<Binding>; 2] {
        self.0.get(&cmd).copied().unwrap_or([None, None])
    }

    pub fn set(&mut self, cmd: Command, slot: usize, binding: Option<Binding>) {
        if let Some(slots) = self.0.get_mut(&cmd) {
            slots[slot.min(1)] = binding;
        }
    }

    pub fn reset(&mut self, cmd: Command) {
        self.0.insert(cmd, cmd.defaults());
    }

    /// Premier raccourci, pour l'afficher à côté d'une commande.
    pub fn label(&self, cmd: Command) -> String {
        self.get(cmd).iter().flatten().next().map(Binding::label).unwrap_or_default()
    }

    /// Autres commandes qui utilisent déjà cette combinaison.
    pub fn conflicts(&self, cmd: Command, binding: &Binding) -> Vec<Command> {
        self.0.iter().filter(|(c, slots)| **c != cmd && slots.contains(&Some(*binding))).map(|(c, _)| *c).collect()
    }

    /// Commandes déclenchées par les événements ; ceux qui en déclenchent une sont retirés. Pendant une
    /// saisie (`typing`), les touches qui écrivent du texte sont laissées au champ.
    pub fn take_commands(&self, events: &mut Vec<Event>, typing: bool) -> Vec<Command> {
        let mut out = Vec::new();
        events.retain(|ev| {
            let Some(b) = Binding::from_event(ev) else { return true };
            if typing && b.types_text() {
                return true;
            }
            match self.0.iter().find(|(_, slots)| slots.contains(&Some(b))) {
                Some((cmd, _)) => {
                    out.push(*cmd);
                    false
                }
                None => true,
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, ctrl: bool) -> Event {
        let modifiers = Modifiers { ctrl, ..Modifiers::default() };
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    #[test]
    fn defaults_are_distinct() {
        let s = Shortcuts::from_settings(&BTreeMap::new());
        for cmd in Command::ALL {
            for b in s.get(cmd).iter().flatten() {
                assert!(s.conflicts(cmd, b).is_empty(), "{cmd:?} {}", b.label());
            }
        }
    }

    #[test]
    fn saved_shortcuts_round_trip_and_only_changes_are_saved() {
        let mut s = Shortcuts::from_settings(&BTreeMap::new());
        assert!(s.to_settings().is_empty());
        s.set(Command::Refresh, 1, Some(Binding { ctrl: true, shift: true, alt: false, key: Key::ArrowDown }));
        let saved = s.to_settings();
        assert_eq!(saved["refresh"], vec!["F5".to_string(), "Ctrl+Shift+Down".to_string()]);
        assert_eq!(Shortcuts::from_settings(&saved), s);
        s.reset(Command::Refresh);
        assert!(s.to_settings().is_empty());
    }

    #[test]
    fn labels_read_like_windows() {
        assert_eq!(Binding { ctrl: true, shift: true, alt: false, key: Key::P }.label(), "Ctrl+Maj+P");
        assert_eq!(Binding { ctrl: false, shift: false, alt: false, key: Key::Enter }.label(), "Entrée");
    }

    #[test]
    fn letters_stay_in_text_fields_while_typing() {
        let s = Shortcuts::from_settings(&BTreeMap::new());
        let mut events = vec![key(Key::R, false), key(Key::F, true)];
        assert_eq!(s.take_commands(&mut events, true), vec![Command::Find]);
        assert_eq!(events.len(), 1);
        let mut events = vec![key(Key::R, false)];
        assert_eq!(s.take_commands(&mut events, false), vec![Command::MarkRead]);
        assert!(events.is_empty());
    }
}
