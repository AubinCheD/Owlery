//! Palette de commandes (Ctrl+P) : vues (boîte de réception, PR à relire, CI en échec…), commandes avec leur
//! raccourci, et *Aller au dépôt…*.

use std::collections::BTreeSet;

use eframe::egui;

use crate::app::App;
use crate::inbox::Filter;
use crate::model::Kind;
use crate::shortcuts::{Command, Shortcuts};

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteAction {
    Run(Command),
    /// Aller à la boîte de réception, filtrée sur un type (ou tout).
    Show(Option<Kind>),
    /// Seulement ce dépôt.
    Repo(String),
}

pub fn entries(shortcuts: &Shortcuts, repos: &BTreeSet<String>) -> Vec<(String, PaletteAction)> {
    let mut v = vec![
        ("Aller à la boîte de réception".to_string(), PaletteAction::Show(None)),
        ("PR à relire".to_string(), PaletteAction::Show(Some(Kind::ReviewRequested))),
        ("CI en échec".to_string(), PaletteAction::Show(Some(Kind::CiFailed))),
        ("Mentions".to_string(), PaletteAction::Show(Some(Kind::Mention))),
        ("Tickets assignés".to_string(), PaletteAction::Show(Some(Kind::Assigned))),
        ("Mes PR".to_string(), PaletteAction::Show(Some(Kind::MyPr))),
    ];
    for cmd in Command::ALL.into_iter().filter(|c| *c != Command::Palette) {
        let key = shortcuts.label(cmd);
        let label = if key.is_empty() { cmd.label().to_string() } else { format!("{}    {key}", cmd.label()) };
        v.push((label, PaletteAction::Run(cmd)));
    }
    for repo in repos {
        v.push((format!("Aller au dépôt {repo}"), PaletteAction::Repo(repo.clone())));
    }
    v
}

/// Les entrées qui contiennent tous les mots cherchés (sans tenir compte de la casse).
pub fn matching(all: Vec<(String, PaletteAction)>, query: &str) -> Vec<(String, PaletteAction)> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    all.into_iter().filter(|(label, _)| words.iter().all(|w| label.to_lowercase().contains(w))).collect()
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some((query, selected)) = &mut app.palette else { return };
    let repos: BTreeSet<String> = app.st.inbox.items.values().map(|i| i.repo.clone()).collect();
    let found = matching(entries(&app.shortcuts, &repos), query);
    *selected = (*selected).min(found.len().saturating_sub(1));
    let (up, down, enter, escape) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if down {
        *selected = (*selected + 1).min(found.len().saturating_sub(1));
    }
    if up {
        *selected = selected.saturating_sub(1);
    }
    let mut chosen = None;
    egui::Modal::new(egui::Id::unique("palette")).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.add(egui::TextEdit::singleline(query).hint_text("Commande, vue ou dépôt…").desired_width(f32::INFINITY))
            .request_focus();
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (i, (label, action)) in found.iter().enumerate() {
                let resp = ui.selectable_label(i == *selected, label);
                if i == *selected && (up || down) {
                    resp.scroll_to_me(None);
                }
                if resp.clicked() {
                    chosen = Some(action.clone());
                }
            }
        });
    });
    if enter {
        chosen = found.get(*selected).map(|(_, a)| a.clone());
    }
    if chosen.is_some() || escape {
        app.palette = None;
    }
    match chosen {
        Some(PaletteAction::Run(cmd)) => app.run(ctx, cmd),
        Some(PaletteAction::Show(kind)) => {
            app.st.view = Filter { kinds: kind.into_iter().collect(), ..Filter::default() };
            app.changed();
        }
        Some(PaletteAction::Repo(repo)) => {
            app.st.view = Filter { text: repo, ..Filter::default() };
            app.changed();
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_offers_the_spec_entries_and_repositories() {
        let repos: BTreeSet<String> = ["contoso/api".to_string()].into();
        let all = entries(&Shortcuts::from_settings(&Default::default()), &repos);
        let labels: Vec<&String> = all.iter().map(|(l, _)| l).collect();
        for want in ["Aller à la boîte de réception", "PR à relire", "CI en échec"] {
            assert!(labels.iter().any(|l| *l == want), "{want}");
        }
        assert!(labels.iter().any(|l| l.starts_with("Actualiser") && l.ends_with("F5")));
        let found = matching(all, "dépôt API");
        assert_eq!(found, vec![("Aller au dépôt contoso/api".to_string(), PaletteAction::Repo("contoso/api".into()))]);
    }
}
