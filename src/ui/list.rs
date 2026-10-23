//! La liste : groupes repliables, lignes (sélection, ouverture, menu), écran d'accueil.

use eframe::egui::{self, RichText, text::LayoutJob};

use crate::app::{App, now};
use crate::inbox::{GroupBy, Snooze};
use crate::model::{Item, Status};

/// Ce qu'un clic dans la liste demande ; appliqué après l'affichage (la liste emprunte la boîte).
enum Action {
    Select(String),
    Open(String),
    Status(String, Status),
    MarkRead(Vec<String>),
    Mute(String),
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    if app.st.inbox.items.is_empty() {
        empty_state(app, ui);
        return;
    }
    let now = now();
    let group_by = app.st.settings.group_by;
    let mut actions = Vec::new();
    {
        let view = app.st.inbox.view(&app.st.view, app.rules(), group_by, now);
        if view.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| ui.label("Rien ne correspond aux filtres."));
        }
        let scroll = std::mem::take(&mut app.scroll_to_selected);
        let selected = app.selected.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for group in &view {
                let title =
                    if group.unread > 0 { format!("{}  ({})", group.title, group.unread) } else { group.title.clone() };
                let holds_selection = selected.as_ref().is_some_and(|s| group.entries.iter().any(|(i, _)| &i.id == s));
                let mut header =
                    egui::CollapsingHeader::new(RichText::new(title).strong()).id_salt(&group.title).default_open(true);
                if scroll && holds_selection {
                    // L'entrée choisie au clavier doit se voir : son groupe s'ouvre.
                    header = header.open(Some(true));
                }
                let shown = header.show(ui, |ui| {
                    for (item, status) in &group.entries {
                        let is_selected = selected.as_deref() == Some(item.id.as_str());
                        let resp = row(ui, item, *status, is_selected, now, group_by);
                        if is_selected && scroll {
                            resp.scroll_to_me(Some(egui::Align::Center));
                        }
                        if resp.clicked() {
                            actions.push(Action::Select(item.id.clone()));
                        }
                        if resp.double_clicked() {
                            actions.push(Action::Open(item.id.clone()));
                        }
                        resp.context_menu(|ui| item_menu(ui, item, now, &mut actions));
                    }
                });
                let ids: Vec<String> = group.entries.iter().map(|(i, _)| i.id.clone()).collect();
                shown.header_response.context_menu(|ui| {
                    if ui.button("Tout marquer lu").clicked() {
                        actions.push(Action::MarkRead(ids.clone()));
                        ui.close();
                    }
                    if group_by == GroupBy::Repo && ui.button("Ignorer ce dépôt").clicked() {
                        actions.push(Action::Mute(group.title.clone()));
                        ui.close();
                    }
                });
            }
        });
    }
    for a in actions {
        match a {
            Action::Select(id) => app.selected = Some(id),
            Action::Open(id) => app.open(&id),
            Action::Status(id, s) => app.set_status(&id, s),
            Action::MarkRead(ids) => app.mark_read(ids),
            Action::Mute(repo) => app.mute_repo(&repo),
        }
    }
}

fn item_menu(ui: &mut egui::Ui, item: &Item, now: i64, actions: &mut Vec<Action>) {
    let choices = [
        ("Ouvrir dans le navigateur", Action::Open(item.id.clone())),
        ("Marquer lu", Action::Status(item.id.clone(), Status::Read)),
        ("Marquer non lu", Action::Status(item.id.clone(), Status::Unread)),
    ];
    for (label, action) in choices {
        if ui.button(label).clicked() {
            actions.push(action);
            ui.close();
        }
    }
    ui.menu_button("Me le rappeler", |ui| {
        for s in Snooze::ALL {
            if ui.button(s.label()).clicked() {
                actions.push(Action::Status(item.id.clone(), Status::Snoozed { until: s.until(now) }));
                ui.close();
            }
        }
    });
    if ui.button("Marquer terminé").clicked() {
        actions.push(Action::Status(item.id.clone(), Status::Done));
        ui.close();
    }
    ui.separator();
    if ui.button(format!("Ignorer {}", item.repo)).clicked() {
        actions.push(Action::Mute(item.repo.clone()));
        ui.close();
    }
}

/// Une ligne : pastille non lu, type ou dépôt, titre (tronqué) et numéro, puis à droite CI, report, ancienneté.
/// Le fond est peint sous le texte ; un clic n'importe où sur la ligne la sélectionne.
fn row(ui: &mut egui::Ui, item: &Item, status: Status, selected: bool, now: i64, group_by: GroupBy) -> egui::Response {
    let unread = status == Status::Unread;
    let background = ui.painter().add(egui::Shape::Noop);
    let inner = ui.horizontal(|ui| {
        ui.style_mut().interaction.selectable_labels = false;
        ui.set_width(ui.available_width());
        let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        if unread {
            ui.painter().circle_filled(dot.center(), 4.0, ui.visuals().selection.bg_fill);
        }
        let tag = match group_by {
            GroupBy::Repo => item.kind.label().to_string(),
            GroupBy::Kind => item.repo.clone(),
        };
        ui.label(RichText::new(tag).small().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(super::ago(item.updated_at, now)).small().weak());
            if let Some(ci) = item.ci {
                let (text, color) = super::ci_label(ci);
                ui.label(RichText::new(&text[..text.find(' ').unwrap_or(text.len())]).color(color)).on_hover_text(text);
            }
            if let Status::Snoozed { until } = status {
                ui.label(RichText::new(format!("⏰ {}", super::date(until))).small().weak());
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let style = ui.style().clone();
                let mut job = LayoutJob::default();
                let mut title = RichText::new(&item.title);
                if unread {
                    title = title.strong();
                }
                if matches!(status, Status::Done | Status::Snoozed { .. }) {
                    title = title.weak();
                }
                title.append_to(&mut job, &style, egui::FontSelection::Default, egui::Align::Center);
                if let Some(n) = item.number {
                    RichText::new(format!("  #{n}")).weak().append_to(
                        &mut job,
                        &style,
                        egui::FontSelection::Default,
                        egui::Align::Center,
                    );
                }
                ui.add(egui::Label::new(job).truncate());
            });
        });
    });
    let rect = inner.response.rect.expand2(egui::vec2(3.0, 1.0));
    let resp = ui.interact(rect, ui.scope_id().with(("ligne", &item.id)), egui::Sense::click());
    let fill = if selected {
        Some(ui.visuals().selection.bg_fill.gamma_multiply(0.35))
    } else if resp.hovered() {
        Some(ui.visuals().widgets.hovered.weak_bg_fill)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().set(background, egui::Shape::rect_filled(rect, 3.0, fill));
    }
    resp
}

/// Rien en cache : pas de compte, une lecture en cours, des erreurs, ou rien qui attende.
fn empty_state(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(40.0);
    let ctx = ui.ctx().clone();
    ui.vertical_centered(|ui| {
        if app.no_access {
            ui.heading("Aucun compte pour l'instant");
            ui.add_space(8.0);
            ui.label("Owlery lit vos forges en lecture seule. Deux façons de commencer :");
            ui.add_space(8.0);
            if ui.button("👤 Ajouter un compte (GitHub, GitLab, Azure DevOps)").clicked() {
                app.open_accounts();
            }
            ui.add_space(4.0);
            ui.label("ou, pour GitHub :");
            ui.add_space(4.0);
            if ui.button("Installer gh, puis lancer `gh auth login`").clicked()
                && let Err(e) = crate::actions::open_url("https://cli.github.com/")
            {
                app.toast(e);
            }
            return;
        }
        if !app.running.is_empty() {
            ui.spinner();
            ui.label("Lecture des forges…");
            return;
        }
        let (text, detail, failed) = super::status_summary(app);
        if failed {
            ui.heading("La lecture a échoué");
            ui.add_space(6.0);
            ui.label(if detail.is_empty() { text } else { detail });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("⟳ Réessayer").clicked() {
                    app.refresh(&ctx);
                }
                if ui.button("👤 Comptes…").clicked() {
                    app.open_accounts();
                }
            });
        } else {
            ui.heading("Rien ne vous attend 🎉");
        }
    });
}
