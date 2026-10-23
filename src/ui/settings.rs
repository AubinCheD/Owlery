//! Réglages : lecture, thème, notifications, dépôts ignorés et robots, dossiers des dépôts, programmes
//! (DragonTree, Fawkes), raccourcis.

use eframe::egui::{self, RichText};

use crate::app::{App, apply_theme};
use crate::model::Kind;
use crate::shortcuts::{Binding, Command};
use crate::state::ThemeChoice;

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.settings_open {
        app.capturing = None;
        return;
    }
    capture_shortcut(app, ctx);
    let mut open = true;
    let mut changed = false;
    let mut rescan = false;
    egui::Window::new("Réglages").open(&mut open).default_width(560.0).vscroll(true).show(ctx, |ui| {
        // Embarqué, le thème est celui de l'hôte.
        let hosted = app.hosted;
        let s = &mut app.st.settings;
        egui::Grid::new("settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Lecture périodique");
            ui.horizontal(|ui| {
                changed |= ui.add(egui::DragValue::new(&mut s.refresh_minutes).range(0..=240).suffix(" min")).changed();
                ui.label(RichText::new("0 : seulement à la demande").weak());
            });
            ui.end_row();
            if !hosted {
                ui.label("Thème");
                ui.horizontal(|ui| {
                    for (t, label) in
                        [(ThemeChoice::System, "Système"), (ThemeChoice::Dark, "Sombre"), (ThemeChoice::Light, "Clair")]
                    {
                        if ui.selectable_value(&mut s.theme, t, label).changed() {
                            apply_theme(ui.ctx(), t);
                            changed = true;
                        }
                    }
                });
                ui.end_row();
            }
        });

        ui.add_space(8.0);
        ui.label(RichText::new("Notifications Windows").strong());
        changed |= ui.checkbox(&mut s.notify, "Prévenir quand Owlery n'est pas au premier plan").changed();
        ui.add_enabled_ui(s.notify, |ui| {
            ui.horizontal_wrapped(|ui| {
                for k in Kind::ALL {
                    let mut on = s.notify_kinds.contains(&k);
                    if ui.checkbox(&mut on, k.label()).changed() {
                        if on {
                            s.notify_kinds.insert(k);
                        } else {
                            s.notify_kinds.remove(&k);
                        }
                        changed = true;
                    }
                }
            });
            ui.label(RichText::new("Rien au premier chargement d'un compte ; un rappel quand un report arrive à échéance.").weak());
        });

        ui.add_space(8.0);
        ui.label(RichText::new("Tri").strong());
        changed |= ui.checkbox(&mut s.rules.hide_bots, "Masquer les entrées des robots (dependabot[bot], renovate…)").changed();
        if s.rules.muted_repos.is_empty() {
            ui.label(RichText::new("Aucun dépôt ignoré (clic droit sur un dépôt dans la liste pour l'ignorer).").weak());
        }
        let mut unmute = None;
        for repo in &s.rules.muted_repos {
            ui.horizontal(|ui| {
                ui.label(format!("Ignoré : {repo}"));
                if ui.small_button("Rétablir").clicked() {
                    unmute = Some(repo.clone());
                }
            });
        }
        if let Some(r) = unmute {
            s.rules.muted_repos.remove(&r);
            changed = true;
        }

        ui.add_space(8.0);
        ui.label(RichText::new("Dossiers des dépôts").strong());
        ui.label(RichText::new("Owlery y cherche les dépôts (profondeur 3) pour DragonTree et Claude ; un dossier choisi à la main dans le détail l'emporte.").weak());
        let mut remove_root = None;
        for (i, root) in s.repo_roots.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(root).monospace());
                if ui.small_button("Retirer").clicked() {
                    remove_root = Some(i);
                }
            });
        }
        if let Some(i) = remove_root {
            s.repo_roots.remove(i);
            changed = true;
            rescan = true;
        }
        ui.horizontal(|ui| {
            if ui.button("Ajouter un dossier…").clicked()
                && let Some(d) = rfd::FileDialog::new().set_title("Dossier où chercher les dépôts").pick_folder() {
                    s.repo_roots.push(d.display().to_string());
                    changed = true;
                    rescan = true;
                }
            if ui.button("Parcourir maintenant").clicked() {
                rescan = true;
            }
        });

        ui.add_space(8.0);
        ui.label(RichText::new("Programmes").strong());
        egui::Grid::new("programs").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            for (label, path) in [("dragontree.exe", &mut s.dragontree_exe), ("fawkes.exe", &mut s.fawkes_exe)] {
                ui.label(label);
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::TextEdit::singleline(path).hint_text("cherché dans le PATH").desired_width(300.0)).changed();
                    if ui.small_button("Choisir…").clicked()
                        && let Some(p) = rfd::FileDialog::new().add_filter("Programme", &["exe"]).pick_file() {
                            *path = p.display().to_string();
                            changed = true;
                        }
                });
                ui.end_row();
            }
        });

        ui.add_space(8.0);
        ui.collapsing("Raccourcis", |ui| shortcuts_grid(app, ui, &mut changed));
        ui.add_space(8.0);
        ui.label(RichText::new(format!("Dossier de l'état : {}", app.dir.display())).weak());
        if app.scanning {
            ui.label(RichText::new("Parcours des dossiers…").weak());
        } else if !app.found_repos.is_empty() {
            ui.label(RichText::new(format!("{} dépôt(s) trouvé(s) dans les dossiers", app.found_repos.len())).weak());
        }
    });
    if changed {
        app.changed();
    }
    if rescan {
        app.scan_repos();
    }
    if !open {
        app.settings_open = false;
        app.capturing = None;
    }
}

fn shortcuts_grid(app: &mut App, ui: &mut egui::Ui, changed: &mut bool) {
    egui::Grid::new("shortcuts").num_columns(4).striped(true).show(ui, |ui| {
        for cmd in Command::ALL {
            ui.label(cmd.label());
            let slots = app.shortcuts.get(cmd);
            for (slot, b) in slots.iter().enumerate() {
                let waiting = app.capturing == Some((cmd, slot));
                let text = if waiting {
                    "Appuyez… (Échap : annuler)".to_string()
                } else {
                    b.map(|b| b.label()).unwrap_or_else(|| "—".into())
                };
                let resp = ui.button(text).on_hover_text("Clic : changer · clic droit : effacer");
                if resp.clicked() {
                    app.capturing = Some((cmd, slot));
                }
                if resp.secondary_clicked() {
                    app.shortcuts.assign(cmd, slot, None);
                    *changed = true;
                }
            }
            if ui.small_button("Défaut").clicked() {
                app.shortcuts.reset(cmd);
                *changed = true;
            }
            ui.end_row();
        }
    });
}

/// Saisie d'un raccourci : la prochaine combinaison est prise (Échap ou un clic ailleurs annule). Une
/// combinaison déjà prise est retirée de l'autre commande, et c'est dit.
fn capture_shortcut(app: &mut App, ctx: &egui::Context) {
    let Some((cmd, slot)) = app.capturing else { return };
    let clicked_elsewhere = ctx.input(|i| i.pointer.any_pressed());
    let got = ctx.input_mut(|i| {
        let mut found = None;
        i.events.retain(|ev| {
            if found.is_none()
                && let Some((b, _)) = Binding::from_event(ev)
            {
                found = Some(b);
                return false;
            }
            true
        });
        found
    });
    match got {
        Some(b) if b.key == egui::Key::Escape => app.capturing = None,
        Some(b) => {
            app.capturing = None;
            let taken = app.shortcuts.assign(cmd, slot, Some(b));
            if !taken.is_empty() {
                let names: Vec<&str> = taken.iter().map(|c| c.label()).collect();
                app.toast(format!("{} retiré de : {}", b.label(), names.join(", ")));
            }
            app.changed();
        }
        // La saisie commence au relâchement du clic : un nouvel appui, n'importe où, l'annule.
        None if clicked_elsewhere => app.capturing = None,
        None => {}
    }
}
