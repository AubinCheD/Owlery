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
    MarkAllRead,
    Undo,
    Find,
    ToggleGroupBy,
    WaitingOnly,
    Palette,
    Accounts,
    Settings,
}

impl Command {
    pub const ALL: [Command; 16] = [
        Command::Refresh,
        Command::Next,
        Command::Prev,
        Command::Open,
        Command::MarkRead,
        Command::MarkUnread,
        Command::MarkDone,
        Command::SnoozeTomorrow,
        Command::MarkAllRead,
        Command::Undo,
        Command::Find,
        Command::ToggleGroupBy,
        Command::WaitingOnly,
        Command::Palette,
        Command::Accounts,
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
            Command::MarkAllRead => "mark_all_read",
            Command::Undo => "undo",
            Command::Find => "find",
            Command::ToggleGroupBy => "toggle_group_by",
            Command::WaitingOnly => "waiting_only",
            Command::Palette => "palette",
            Command::Accounts => "accounts",
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
            Command::MarkAllRead => "Tout marquer lu (liste affichée)",
            Command::Undo => "Annuler",
            Command::Find => "Chercher",
            Command::ToggleGroupBy => "Grouper par dépôt / par type",
            Command::WaitingOnly => "Seulement ce qui m'attend",
            Command::Palette => "Palette de commandes",
            Command::Accounts => "Comptes",
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
            Command::MarkAllRead => [k(false, true, Key::R), None],
            Command::Undo => [k(true, false, Key::Z), None],
            Command::Find => [k(true, false, Key::F), None],
            Command::ToggleGroupBy => [k(true, false, Key::G), None],
            Command::WaitingOnly => [k(true, false, Key::W), None],
            Command::Palette => [k(true, false, Key::P), k(true, true, Key::P)],
            Command::Accounts => [None, None],
            Command::Settings => [k(true, false, Key::Comma), None],
        }
    }

    /// Commande qu'on peut répéter en gardant la touche enfoncée (se déplacer) ; les autres (terminer, reporter)
    /// n'agissent qu'une fois par appui.
    fn repeats(self) -> bool {
        matches!(self, Command::Next | Command::Prev)
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
    /// « Ctrl+Maj+P », « Entrée », « Page suiv. ».
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
                Key::Insert => "Inser",
                Key::Backspace => "Retour arrière",
                Key::Space => "Espace",
                Key::Tab => "Tab",
                Key::Home => "Origine",
                Key::End => "Fin",
                Key::PageUp => "Page préc.",
                Key::PageDown => "Page suiv.",
                Key::ArrowUp => "Flèche haut",
                Key::ArrowDown => "Flèche bas",
                Key::ArrowLeft => "Flèche gauche",
                Key::ArrowRight => "Flèche droite",
                Key::Comma => ",",
                Key::Period => ".",
                Key::Minus => "-",
                Key::Plus => "+",
                Key::Slash => "/",
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

    /// La combinaison d'un appui de touche, et s'il s'agit d'une répétition.
    pub fn from_event(ev: &Event) -> Option<(Binding, bool)> {
        match ev {
            Event::Key { key, pressed: true, repeat, modifiers, .. } => Some((Self::with(modifiers, *key), *repeat)),
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

    /// Ctrl+Z, Ctrl+Y, Ctrl+A, Ctrl+C, Ctrl+V, Ctrl+X agissent sur le champ de texte qui a le focus.
    fn edits_text(&self) -> bool {
        self.ctrl && !self.alt && matches!(self.key, Key::Z | Key::Y | Key::A | Key::C | Key::V | Key::X)
    }

    /// Entrée et Espace actionnent le bouton qui a le focus : ils lui restent.
    fn activates_widget(&self) -> bool {
        !self.ctrl && !self.alt && matches!(self.key, Key::Enter | Key::Space)
    }
}

/// Ce qui a le focus clavier quand un raccourci arrive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    /// Rien : tous les raccourcis agissent.
    None,
    /// Un champ de texte : les touches qui écrivent lui restent.
    Text,
    /// Un autre élément (bouton, case) : Entrée et Espace lui restent.
    Widget,
    /// Une fenêtre modale : seuls Ctrl/Alt agissent, et l'interface décide lesquels passent.
    Modal,
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

    /// Donne la combinaison à la commande ; elle est retirée des autres commandes qui l'avaient (renvoyées).
    pub fn assign(&mut self, cmd: Command, slot: usize, binding: Option<Binding>) -> Vec<Command> {
        let mut taken_from = Vec::new();
        if let Some(b) = binding {
            for (other, slots) in self.0.iter_mut() {
                if *other == cmd {
                    continue;
                }
                for s in slots.iter_mut() {
                    if *s == Some(b) {
                        *s = None;
                        taken_from.push(*other);
                    }
                }
            }
        }
        if let Some(slots) = self.0.get_mut(&cmd) {
            slots[slot.min(1)] = binding;
        }
        taken_from
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

    /// Commandes déclenchées par les événements ; ceux qui en déclenchent une sont retirés. Selon le focus,
    /// certaines touches sont laissées à l'élément qui l'a.
    pub fn take_commands(&self, events: &mut Vec<Event>, focus: Focus) -> Vec<Command> {
        let mut out = Vec::new();
        events.retain(|ev| {
            let Some((b, repeat)) = Binding::from_event(ev) else { return true };
            let left = match focus {
                Focus::None => false,
                Focus::Text | Focus::Modal => b.types_text() || b.edits_text(),
                Focus::Widget => b.activates_widget(),
            };
            if left {
                return true;
            }
            match self.0.iter().find(|(_, slots)| slots.contains(&Some(b))) {
                Some((cmd, _)) if repeat && !cmd.repeats() => false,
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

    fn repeated(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: true, modifiers: Modifiers::default() }
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
        s.assign(Command::Refresh, 1, Some(Binding { ctrl: true, shift: true, alt: false, key: Key::ArrowDown }));
        let saved = s.to_settings();
        assert_eq!(saved["refresh"], vec!["F5".to_string(), "Ctrl+Shift+Down".to_string()]);
        assert_eq!(Shortcuts::from_settings(&saved), s);
        s.reset(Command::Refresh);
        assert!(s.to_settings().is_empty());
    }

    #[test]
    fn a_combination_moves_instead_of_being_shared() {
        let mut s = Shortcuts::from_settings(&BTreeMap::new());
        let r = Binding { ctrl: false, shift: false, alt: false, key: Key::R };
        assert_eq!(s.assign(Command::Refresh, 0, Some(r)), vec![Command::MarkRead]);
        assert_eq!(s.get(Command::MarkRead), [None, None]);
        let mut events = vec![key(Key::R, false)];
        assert_eq!(s.take_commands(&mut events, Focus::None), vec![Command::Refresh]);
    }

    #[test]
    fn labels_read_like_windows() {
        assert_eq!(Binding { ctrl: true, shift: true, alt: false, key: Key::P }.label(), "Ctrl+Maj+P");
        assert_eq!(Binding { ctrl: false, shift: false, alt: false, key: Key::Enter }.label(), "Entrée");
        assert_eq!(Binding { ctrl: false, shift: false, alt: false, key: Key::PageDown }.label(), "Page suiv.");
        assert_eq!(Binding { ctrl: false, shift: false, alt: false, key: Key::Backspace }.label(), "Retour arrière");
    }

    #[test]
    fn letters_stay_in_text_fields_while_typing() {
        let s = Shortcuts::from_settings(&BTreeMap::new());
        let mut events = vec![key(Key::R, false), key(Key::F, true)];
        assert_eq!(s.take_commands(&mut events, Focus::Text), vec![Command::Find]);
        assert_eq!(events.len(), 1);
        let mut events = vec![key(Key::R, false)];
        assert_eq!(s.take_commands(&mut events, Focus::None), vec![Command::MarkRead]);
        assert!(events.is_empty());
    }

    #[test]
    fn undo_in_a_text_field_stays_with_the_field() {
        let s = Shortcuts::from_settings(&BTreeMap::new());
        let mut events = vec![key(Key::Z, true)];
        assert!(s.take_commands(&mut events, Focus::Text).is_empty());
        assert_eq!(events.len(), 1);
        assert_eq!(s.take_commands(&mut events, Focus::None), vec![Command::Undo]);
    }

    #[test]
    fn a_focused_button_keeps_enter_but_not_letters() {
        let s = Shortcuts::from_settings(&BTreeMap::new());
        let mut events = vec![key(Key::Enter, false), key(Key::E, false)];
        assert_eq!(s.take_commands(&mut events, Focus::Widget), vec![Command::MarkDone]);
        assert_eq!(events.len(), 1, "Entrée reste au bouton");
    }

    #[test]
    fn held_keys_only_repeat_moves() {
        let s = Shortcuts::from_settings(&BTreeMap::new());
        let mut events = vec![key(Key::E, false), repeated(Key::E), repeated(Key::E), repeated(Key::J)];
        assert_eq!(s.take_commands(&mut events, Focus::None), vec![Command::MarkDone, Command::Next]);
        assert!(events.is_empty(), "les répétitions ignorées ne vont pas ailleurs");
    }
}
