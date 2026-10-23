//! Affichage : barre d'outils et filtres, liste groupée, détail, dialogues (comptes, réglages, palette, Fawkes).

pub mod accounts;
pub mod detail;
pub mod list;
pub mod palette;
pub mod settings;

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use crate::app::{App, now};
use crate::inbox::GroupBy;
use crate::model::{Ci, Forge, Kind, Status};
use crate::shortcuts::Command;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    egui::Panel::top("toolbar").show(ui, |ui| {
        ui.add_space(4.0);
        toolbar(app, ui);
        filters(app, ui);
        banners(app, ui);
        ui.add_space(2.0);
    });
    egui::Panel::right("detail").default_size(380.0).resizable(true).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| detail::show(app, ui));
    });
    egui::CentralPanel::default().show(ui, |ui| list::show(app, ui));
    accounts::window(app, &ctx);
    settings::window(app, &ctx);
    palette::window(app, &ctx);
    detail::fawkes_dialog(app, &ctx);
    toasts(app, &ctx);
}

/// Résumé de la dernière lecture, et le détail (erreurs et remarques de chaque compte) en infobulle.
pub(crate) fn status_summary(app: &App) -> (String, String, bool) {
    if !app.running.is_empty() {
        let n = app.running.len();
        return (if n == 1 { "Lecture…".into() } else { format!("Lecture de {n} comptes…") }, String::new(), false);
    }
    let accounts = app.accounts();
    if accounts.is_empty() {
        return ("Aucun compte".into(), String::new(), false);
    }
    let mut detail = Vec::new();
    let mut failed = Vec::new();
    let mut last = None;
    let mut read = 0;
    for a in &accounts {
        let Some(s) = app.status.get(&a.id) else { continue };
        if let Some(e) = &s.error {
            failed.push(a.title());
            detail.push(format!("{} : {e}", a.title()));
        }
        for n in &s.notes {
            detail.push(format!("{} : {n}", a.title()));
        }
        last = last.max(s.last_ok);
        if s.last_ok.is_some() {
            read += 1;
        }
    }
    let mut text = match last {
        Some(t) if accounts.len() > 1 => format!("Lu à {} ({read} comptes sur {})", clock(t), accounts.len()),
        Some(t) => format!("Lu à {}", clock(t)),
        None if failed.is_empty() => "Pas encore lu".into(),
        None => String::new(),
    };
    if !failed.is_empty() {
        if !text.is_empty() {
            text.push_str(" · ");
        }
        text.push_str(&match failed.as_slice() {
            [one] => {
                format!("échec : {}", detail.iter().find(|d| d.starts_with(one.as_str())).cloned().unwrap_or_default())
            }
            many => format!("{} comptes en échec", many.len()),
        });
    } else if !detail.is_empty() {
        text.push_str(&format!(" · {} remarque(s)", detail.len()));
    }
    (text, detail.join("\n"), !failed.is_empty())
}

fn toolbar(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        ui.heading("Boîte de réception");
        let waiting = app.st.inbox.waiting(app.rules(), now());
        if waiting > 0 {
            ui.label(
                RichText::new(format!(" {waiting} "))
                    .strong()
                    .color(Color32::WHITE)
                    .background_color(ui.visuals().selection.bg_fill),
            );
        }
        ui.separator();
        // Les boutons de droite d'abord : le message d'état prend la place qui reste, tronqué.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("⚙ Réglages").on_hover_text(app.shortcuts.label(Command::Settings)).clicked() {
                app.toggle_settings();
            }
            if ui.button("👤 Comptes…").clicked() {
                app.toggle_accounts();
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let busy = !app.running.is_empty();
                if ui
                    .add_enabled(!busy, egui::Button::new("⟳ Actualiser"))
                    .on_hover_text(app.shortcuts.label(Command::Refresh))
                    .clicked()
                {
                    app.refresh(&ctx);
                }
                if busy {
                    ui.spinner();
                }
                let (text, detail, failed) = status_summary(app);
                let color = if failed { ui.visuals().warn_fg_color } else { ui.visuals().weak_text_color() };
                let resp = ui.add(egui::Label::new(RichText::new(text).color(color)).truncate());
                if !detail.is_empty() {
                    resp.on_hover_text(detail);
                }
            });
        });
    });
}

fn filters(app: &mut App, ui: &mut egui::Ui) {
    let before = app.st.view.clone();
    let group_before = app.st.settings.group_by;
    ui.horizontal_wrapped(|ui| {
        let view = &mut app.st.view;
        let search = ui.add(
            egui::TextEdit::singleline(&mut view.text)
                .hint_text("Chercher (dépôt, titre, auteur)")
                .desired_width(200.0),
        );
        if std::mem::take(&mut app.focus_search) {
            search.request_focus();
        }
        ui.separator();
        for kind in Kind::ALL {
            let on = view.kinds.contains(&kind);
            if ui.selectable_label(on, kind.label()).clicked() {
                if on {
                    view.kinds.remove(&kind);
                } else {
                    view.kinds.insert(kind);
                }
            }
        }
        ui.separator();
        let forge = egui::ComboBox::from_id_salt("forge")
            .selected_text(view.forge.map_or("Toutes les forges", Forge::label))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut view.forge, None, "Toutes les forges");
                for f in Forge::ALL {
                    ui.selectable_value(&mut view.forge, Some(f), f.label());
                }
            });
        // Nom lu par les lecteurs d'écran : la liste n'a pas de libellé visible.
        forge.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Forge"));
        ui.checkbox(&mut view.waiting_only, "Seulement ce qui m'attend");
        ui.checkbox(&mut view.show_hidden, "Terminés et reportés");
        ui.separator();
        ui.label("Grouper :");
        ui.selectable_value(&mut app.st.settings.group_by, GroupBy::Repo, "par dépôt");
        ui.selectable_value(&mut app.st.settings.group_by, GroupBy::Kind, "par type");
    });
    let mut text_only = before.clone();
    text_only.text = app.st.view.text.clone();
    if text_only != app.st.view || group_before != app.st.settings.group_by {
        app.changed();
    }
}

fn banners(app: &mut App, ui: &mut egui::Ui) {
    let mut dismiss = None;
    for (i, p) in app.problems.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(ui.visuals().warn_fg_color, format!("⚠ {p}"));
            if ui.small_button("Compris").clicked() {
                dismiss = Some(i);
            }
        });
    }
    if let Some(i) = dismiss {
        app.problems.remove(i);
    }
    if let Some(e) = &app.save_error {
        ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
    }
}

fn toasts(app: &mut App, ctx: &egui::Context) {
    app.toasts.retain(|(_, t)| t.elapsed() < Duration::from_secs(6));
    if app.toasts.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::unique("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -12.0]).show(ctx, |ui| {
        for (msg, _) in &app.toasts {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(420.0);
                ui.label(msg);
            });
        }
    });
    let next = app.toasts.iter().map(|(_, t)| Duration::from_secs(6).saturating_sub(t.elapsed())).min();
    ctx.request_repaint_after(next.unwrap_or(Duration::from_millis(500)).max(Duration::from_millis(100)));
    let _ = Instant::now();
}

pub(crate) fn ci_label(ci: Ci) -> (&'static str, Color32) {
    match ci {
        Ci::Success => ("✔ réussie", Color32::from_rgb(0x3f, 0xb9, 0x50)),
        Ci::Pending => ("⟳ en cours", Color32::from_rgb(0xd2, 0x99, 0x22)),
        Ci::Failure => ("✖ en échec", Color32::from_rgb(0xf8, 0x51, 0x49)),
    }
}

pub(crate) fn status_label(s: Status) -> String {
    match s {
        Status::Unread => "non lu".into(),
        Status::Read => "lu".into(),
        Status::Done => "terminé".into(),
        Status::Snoozed { until } => format!("reporté jusqu'au {}", date(until)),
    }
}

pub(crate) fn date(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%d/%m %H:%M").to_string())
        .unwrap_or_default()
}

fn clock(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

/// « à l'instant », « il y a 12 min », « il y a 3 h », « il y a 2 j ».
pub fn ago(t: i64, now: i64) -> String {
    let s = (now - t).max(0);
    match s {
        0..60 => "à l'instant".into(),
        60..3600 => format!("il y a {} min", s / 60),
        3600..86400 => format!("il y a {} h", s / 3600),
        _ => format!("il y a {} j", s / 86400),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod helpers_tests {
    use super::*;

    #[test]
    fn ages_read_naturally() {
        assert_eq!(ago(100, 110), "à l'instant");
        assert_eq!(ago(0, 12 * 60), "il y a 12 min");
        assert_eq!(ago(0, 3 * 3600 + 5), "il y a 3 h");
        assert_eq!(ago(0, 2 * 86400), "il y a 2 j");
        assert_eq!(ago(500, 100), "à l'instant");
    }
}
