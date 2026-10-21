//! Le détail de l'entrée sélectionnée : résumé, CI en échec, derniers commentaires, actions (navigateur,
//! états, report, DragonTree, Claude dans Fawkes).

use eframe::egui::{self, RichText};

use crate::actions;
use crate::app::{App, CommentsState, now};
use crate::inbox::Snooze;
use crate::model::Status;
use crate::shortcuts::Command;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let now = now();
    let Some(item) = app.selected.as_ref().and_then(|id| app.st.inbox.items.get(id)).cloned() else {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| ui.label(RichText::new("Choisissez une entrée.").weak()));
        return;
    };
    let ctx = ui.ctx().clone();
    let status = app.st.inbox.status(&item, now);
    let account = app.account_of(&item).map(|a| a.title()).unwrap_or_else(|| item.forge.label().to_string());
    ui.label(RichText::new(format!("{} · {account}", item.kind.label())).weak());
    ui.heading(&item.title);
    let number = item.number.map(|n| format!(" #{n}")).unwrap_or_default();
    ui.label(format!("{}{number}", item.repo));
    egui::Grid::new("detail").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        if !item.author.is_empty() {
            ui.label("Auteur");
            ui.label(&item.author);
            ui.end_row();
        }
        if let Some(b) = &item.branch {
            ui.label("Branche");
            ui.label(b);
            ui.end_row();
        }
        if let Some(ci) = item.ci {
            ui.label("CI");
            let (text, color) = super::ci_label(ci);
            ui.label(RichText::new(text).color(color));
            ui.end_row();
        }
        ui.label("Activité");
        ui.label(format!("{} ({})", super::ago(item.updated_at, now), super::date(item.updated_at)));
        ui.end_row();
        ui.label("État");
        ui.label(super::status_label(status));
        ui.end_row();
    });
    if !item.checks.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new("Vérifications en échec").strong());
        for c in &item.checks {
            let resp = ui.link(format!("✖ {}", c.name));
            if resp.on_hover_text(&c.url).clicked()
                && let Err(e) = actions::open_url(&c.url)
            {
                app.toast(e);
            }
        }
    }
    if !item.summary.is_empty() {
        ui.add_space(6.0);
        ui.label(&item.summary);
    }
    comments(app, ui, &item.id);

    ui.add_space(10.0);
    ui.separator();
    let id = item.id.clone();
    if ui.button("🌐 Ouvrir dans le navigateur").on_hover_text(app.shortcuts.label(Command::Open)).clicked() {
        app.open(&id);
    }
    ui.horizontal(|ui| {
        if status == Status::Unread {
            if ui.button("Marquer lu").on_hover_text(app.shortcuts.label(Command::MarkRead)).clicked() {
                app.set_status(&id, Status::Read);
            }
        } else if ui.button("Marquer non lu").on_hover_text(app.shortcuts.label(Command::MarkUnread)).clicked() {
            app.set_status(&id, Status::Unread);
        }
        if ui.button("✔ Terminé").on_hover_text(app.shortcuts.label(Command::MarkDone)).clicked() {
            app.set_status(&id, Status::Done);
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Me le rappeler :");
        for s in Snooze::ALL {
            if ui.small_button(s.label()).clicked() {
                app.set_status(&id, Status::Snoozed { until: s.until(now) });
            }
        }
    });
    ui.add_space(10.0);
    ui.separator();
    local_actions(app, ui, &ctx, &item);
}

fn comments(app: &App, ui: &mut egui::Ui, id: &str) {
    let Some((_, state)) = app.comments.get(id) else { return };
    ui.add_space(8.0);
    ui.label(RichText::new("Derniers commentaires").strong());
    match state {
        CommentsState::Loading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Lecture…").weak());
            });
        }
        CommentsState::Failed(e) => {
            ui.label(RichText::new(format!("Illisibles : {e}")).weak());
        }
        CommentsState::Loaded(list) if list.is_empty() => {
            ui.label(RichText::new("Aucun commentaire.").weak());
        }
        CommentsState::Loaded(list) => {
            for c in list {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let when = if c.at > 0 { format!(" · {}", super::ago(c.at, now())) } else { String::new() };
                    ui.label(RichText::new(format!("{}{when}", c.author)).small().weak());
                    ui.label(&c.body);
                });
            }
        }
    }
}

/// Dossier local du dépôt, DragonTree et Claude.
fn local_actions(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, item: &crate::model::Item) {
    let manual = app.st.settings.repo_dirs.get(&item.repo).cloned();
    let dir = app.local_dir(item);
    ui.horizontal_wrapped(|ui| {
        ui.label("Dossier local :");
        let shown = dir.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "non trouvé".into());
        ui.label(RichText::new(shown).monospace());
        if manual.is_none() && dir.is_some() {
            ui.label(RichText::new("(trouvé)").weak()).on_hover_text("Trouvé dans les dossiers racines des réglages");
        }
        if ui.small_button("Choisir…").clicked()
            && let Some(d) = rfd::FileDialog::new().set_title(format!("Dossier local de {}", item.repo)).pick_folder()
        {
            app.st.settings.repo_dirs.insert(item.repo.clone(), d.display().to_string());
            app.changed();
        }
        if manual.is_some() && ui.small_button("Oublier").on_hover_text("Revenir au dossier trouvé tout seul").clicked()
        {
            app.st.settings.repo_dirs.remove(&item.repo);
            app.changed();
        }
    });
    let lookups = app.lookups();
    let dragontree = lookups.dragontree.clone();
    let hint = if dragontree.is_none() {
        "dragontree.exe introuvable : indiquez son chemin dans les réglages"
    } else if dir.is_none() {
        "Choisissez d'abord le dossier local du dépôt"
    } else {
        "Ouvre le dossier du dépôt dans DragonTree"
    };
    let can_dt = dir.is_some() && dragontree.is_some();
    if ui
        .add_enabled(can_dt, egui::Button::new("🌳 Ouvrir dans DragonTree"))
        .on_hover_text(hint)
        .on_disabled_hover_text(hint)
        .clicked()
        && let (Some(exe), Some(d)) = (&dragontree, &dir)
        && let Err(e) = actions::open_in_dragontree(exe, d)
    {
        app.toast(e);
    }
    let claude_hint = if dir.is_none() {
        "Choisissez d'abord le dossier local du dépôt".to_string()
    } else if lookups.fawkes_up {
        "Fawkes ouvre une session Claude dans un worktree de la branche ; le contexte est collé, pas envoyé".to_string()
    } else {
        "Fawkes n'écoute pas encore : vous pourrez le démarrer, ou copier le contexte".to_string()
    };
    let busy = app.fawkes_job.is_some();
    ui.horizontal(|ui| {
        let button = egui::Button::new("✨ Corriger avec Claude");
        if ui
            .add_enabled(dir.is_some() && !busy, button)
            .on_hover_text(&claude_hint)
            .on_disabled_hover_text(&claude_hint)
            .clicked()
        {
            app.fix_with_claude(ctx, &item.id);
        }
        if busy {
            ui.spinner();
        }
    });
    if ui
        .button("📋 Copier le contexte pour Claude")
        .on_hover_text("Lien, branche et CI en échec, à coller dans une session Claude")
        .clicked()
    {
        ctx.copy_text(actions::claude_context(item));
        app.toast("Contexte copié : rien n'a été envoyé");
    }
    if ui
        .small_button(format!("Ignorer {}", item.repo))
        .on_hover_text("Ne plus montrer ce dépôt (réglages pour le rétablir)")
        .clicked()
    {
        app.mute_repo(&item.repo.clone());
    }
}

/// *Corriger avec Claude* alors que Fawkes n'écoute pas : rien n'est lancé sans le choix de l'utilisateur.
pub fn fawkes_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(dialog) = &app.fawkes_dialog else { return };
    let (line, context) = (dialog.line.clone(), dialog.context.clone());
    let exe = app.lookups().fawkes_exe;
    let mut choice = None;
    egui::Modal::new(egui::Id::unique("fawkes")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.heading("Corriger avec Claude");
        ui.label("Fawkes n'est pas ouvert, ou ne reçoit pas encore de demandes.");
        ui.add_space(6.0);
        match &exe {
            Some(p) => ui.label(RichText::new(format!("Fawkes : {}", p.display())).weak()),
            None => ui.label(RichText::new("fawkes.exe introuvable (PATH, ou chemin dans les réglages).").weak()),
        };
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(exe.is_some(), egui::Button::new("Démarrer Fawkes et envoyer")).clicked() {
                choice = Some(1);
            }
            if ui.button("Copier le contexte").clicked() {
                choice = Some(2);
            }
            if ui.button("Annuler").clicked() {
                choice = Some(0);
            }
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        choice = Some(0);
    }
    match choice {
        Some(1) => {
            app.fawkes_dialog = None;
            app.send_to_fawkes(ctx, line, context, true);
        }
        Some(2) => {
            app.fawkes_dialog = None;
            ctx.copy_text(context);
            app.toast("Contexte copié : rien n'a été envoyé");
        }
        Some(_) => app.fawkes_dialog = None,
        None => {}
    }
}
