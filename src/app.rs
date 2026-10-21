//! L'application : barre d'outils, filtres, liste groupée, détail et actions, dialogues (jeton, réglages,
//! palette).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use crate::actions;
use crate::github::{self, FetchError};
use crate::inbox::{Filter, GroupBy, Snooze};
use crate::model::{Ci, Forge, Item, Kind, Status};
use crate::secrets::{self, GITHUB_TOKEN, SecretStore};
use crate::shortcuts::{Binding, Command, Shortcuts};
use crate::state::{self, AppState, ThemeChoice};

/// Page de création d'un jeton à grain fin.
const TOKEN_PAGE: &str = "https://github.com/settings/personal-access-tokens/new";
/// Page d'installation de la CLI gh.
const GH_PAGE: &str = "https://cli.github.com/";

type FetchResult = Result<(Vec<Item>, &'static str), FetchError>;

pub struct App {
    dir: PathBuf,
    st: AppState,
    shortcuts: Shortcuts,
    store: Arc<dyn SecretStore>,
    /// Entrées fictives, aucune lecture réelle (`OWLERY_DEMO=1`).
    demo: bool,
    filter: Filter,
    selected: Option<String>,
    fetching: Option<Receiver<FetchResult>>,
    last_fetch: Option<Instant>,
    /// Pas de nouvelle lecture automatique avant (quota épuisé), en secondes Unix.
    hold_until: Option<i64>,
    status_line: String,
    /// La dernière lecture a échoué faute d'accès : la liste affiche les deux façons d'en donner un.
    no_access: bool,
    problems: Vec<String>,
    toasts: Vec<(String, Instant)>,
    dirty: bool,
    token_dialog: Option<TokenDialog>,
    has_token: bool,
    settings_open: bool,
    /// Raccourci en cours de saisie dans les réglages.
    capturing: Option<(Command, usize)>,
    palette: Option<Palette>,
    focus_search: bool,
    capture: Option<PathBuf>,
}

#[derive(Default)]
struct TokenDialog {
    input: String,
    error: Option<String>,
}

#[derive(Default)]
struct Palette {
    query: String,
    selected: usize,
}

fn now() -> i64 {
    chrono::Local::now().timestamp()
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        dir: PathBuf,
        loaded: state::Loaded,
        store: Arc<dyn SecretStore>,
        demo: bool,
    ) -> Self {
        let mut st = loaded.state;
        if demo {
            st.inbox.merge(Forge::GitHub, crate::demo::items(now()), now());
        }
        let has_token = matches!(store.get(GITHUB_TOKEN), Ok(Some(_)));
        let mut app = App {
            dir,
            shortcuts: Shortcuts::from_settings(&st.settings.shortcuts),
            st,
            store,
            demo,
            filter: Filter::default(),
            selected: None,
            fetching: None,
            last_fetch: None,
            hold_until: None,
            status_line: String::new(),
            no_access: false,
            problems: loaded.problems,
            toasts: Vec::new(),
            dirty: demo,
            token_dialog: None,
            has_token,
            settings_open: false,
            capturing: None,
            palette: None,
            focus_search: false,
            capture: crate::capture::target(),
        };
        apply_theme(&cc.egui_ctx, app.st.settings.theme);
        if demo {
            app.status_line = "Démonstration : entrées fictives, aucune lecture de GitHub".into();
            app.last_fetch = Some(Instant::now());
            app.selected = app.visible_ids().into_iter().next();
        } else {
            app.refresh(&cc.egui_ctx);
        }
        app
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toasts.push((msg.into(), Instant::now()));
    }

    // --- Lecture -----------------------------------------------------------------------------------------

    fn refresh(&mut self, ctx: &egui::Context) {
        if self.fetching.is_some() {
            return;
        }
        if self.demo {
            self.st.inbox.merge(Forge::GitHub, crate::demo::items(now()), now());
            self.dirty = true;
            self.toast("Démonstration : entrées fictives rechargées");
            return;
        }
        let (tx, rx) = channel();
        let store = self.store.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result =
                github::access(store.as_ref()).and_then(|t| github::fetch(t.as_ref()).map(|items| (items, t.label())));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.fetching = Some(rx);
        self.last_fetch = Some(Instant::now());
        self.status_line = "Lecture de GitHub…".into();
    }

    fn poll_fetch(&mut self) {
        let Some(rx) = &self.fetching else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.fetching = None;
        let at = chrono::Local::now().format("%H:%M");
        match result {
            Ok((items, via)) => {
                let n = items.len();
                self.st.inbox.merge(Forge::GitHub, items, now());
                self.dirty = true;
                self.no_access = false;
                self.hold_until = None;
                self.status_line = format!("GitHub lu à {at} ({n} entrée(s), accès : {via})");
            }
            Err(e) => {
                // Rien n'est effacé : la liste en cache reste affichée.
                self.no_access = e == FetchError::NoAccess;
                if let FetchError::RateLimited { reset } = e {
                    self.hold_until = Some(reset.unwrap_or(now() + 15 * 60));
                }
                self.status_line = if self.no_access {
                    "Aucun accès à GitHub".into()
                } else {
                    format!("Lecture de {at} en échec : {e}")
                };
            }
        }
    }

    fn auto_refresh(&mut self, ctx: &egui::Context) {
        let minutes = self.st.settings.refresh_minutes;
        if minutes == 0 || self.demo || self.fetching.is_some() || self.hold_until.is_some_and(|t| now() < t) {
            return;
        }
        if self.last_fetch.is_none_or(|t| t.elapsed() >= Duration::from_secs(u64::from(minutes) * 60)) {
            self.refresh(ctx);
        }
    }

    // --- Actions sur les entrées ---------------------------------------------------------------------

    fn visible_ids(&self) -> Vec<String> {
        let view = self.st.inbox.view(&self.filter, self.st.settings.group_by, now());
        view.iter().flat_map(|g| g.entries.iter().map(|(i, _)| i.id.clone())).collect()
    }

    fn select_step(&mut self, step: isize) {
        let ids = self.visible_ids();
        if ids.is_empty() {
            return;
        }
        let pos = self.selected.as_ref().and_then(|s| ids.iter().position(|i| i == s));
        let next = match pos {
            None => 0,
            Some(p) => (p as isize + step).clamp(0, ids.len() as isize - 1) as usize,
        };
        self.selected = Some(ids[next].clone());
    }

    fn set_status(&mut self, id: &str, status: Status) {
        // Une entrée qui quitte la liste : la sélection passe à la suivante.
        let hides = matches!(status, Status::Done | Status::Snoozed { .. }) && !self.filter.show_hidden;
        if hides && self.selected.as_deref() == Some(id) {
            let ids = self.visible_ids();
            let pos = ids.iter().position(|i| i == id);
            self.selected =
                pos.and_then(|p| ids.get(p + 1).or_else(|| p.checked_sub(1).and_then(|q| ids.get(q)))).cloned();
        }
        self.st.inbox.set_status(id, status);
        self.dirty = true;
    }

    fn open(&mut self, id: &str) {
        let Some(url) = self.st.inbox.items.get(id).map(|i| i.url.clone()) else { return };
        match actions::open_url(&url) {
            Ok(()) => {
                if self.st.inbox.status(&self.st.inbox.items[id], now()) == Status::Unread {
                    self.set_status(id, Status::Read);
                }
            }
            Err(e) => self.toast(e),
        }
    }

    fn run(&mut self, ctx: &egui::Context, cmd: Command) {
        let sel = self.selected.clone();
        match cmd {
            Command::Refresh => self.refresh(ctx),
            Command::Next => self.select_step(1),
            Command::Prev => self.select_step(-1),
            Command::Open => {
                if let Some(id) = sel {
                    self.open(&id);
                }
            }
            Command::MarkRead => {
                if let Some(id) = sel {
                    self.set_status(&id, Status::Read);
                }
            }
            Command::MarkUnread => {
                if let Some(id) = sel {
                    self.set_status(&id, Status::Unread);
                }
            }
            Command::MarkDone => {
                if let Some(id) = sel {
                    self.set_status(&id, Status::Done);
                }
            }
            Command::SnoozeTomorrow => {
                if let Some(id) = sel {
                    self.set_status(&id, Status::Snoozed { until: Snooze::Tomorrow.until(now()) });
                }
            }
            Command::Find => self.focus_search = true,
            Command::ToggleGroupBy => {
                self.st.settings.group_by = match self.st.settings.group_by {
                    GroupBy::Repo => GroupBy::Kind,
                    GroupBy::Kind => GroupBy::Repo,
                };
                self.dirty = true;
            }
            Command::WaitingOnly => self.filter.waiting_only = !self.filter.waiting_only,
            Command::Palette => self.palette = Some(Palette::default()),
            Command::Settings => self.settings_open = !self.settings_open,
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.capturing.is_some() {
            return;
        }
        let typing = ctx.egui_wants_keyboard_input();
        let modal = self.palette.is_some() || self.token_dialog.is_some();
        let cmds = ctx.input_mut(|i| self.shortcuts.take_commands(&mut i.events, typing || modal));
        for cmd in cmds {
            if !modal || cmd == Command::Palette {
                self.run(ctx, cmd);
            }
        }
    }

    fn save(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        if let Err(e) = state::save(&self.dir, &self.st) {
            self.dirty = true;
            self.toast(format!("Enregistrement de l'état impossible : {e}"));
        }
    }

    // --- Affichage -----------------------------------------------------------------------------------

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            let waiting = self.st.inbox.waiting(now());
            ui.heading("Boîte de réception");
            if waiting > 0 {
                ui.label(
                    RichText::new(format!(" {waiting} "))
                        .strong()
                        .color(Color32::WHITE)
                        .background_color(ui.visuals().selection.bg_fill),
                );
            }
            ui.separator();
            let busy = self.fetching.is_some();
            if ui
                .add_enabled(!busy, egui::Button::new("⟳ Actualiser"))
                .on_hover_text(self.shortcuts.label(Command::Refresh))
                .clicked()
            {
                self.refresh(&ctx);
            }
            if busy {
                ui.spinner();
            }
            ui.add(egui::Label::new(RichText::new(&self.status_line).weak()).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙ Réglages").on_hover_text(self.shortcuts.label(Command::Settings)).clicked() {
                    self.settings_open = !self.settings_open;
                }
                let label = if self.has_token { "🔑 Jeton GitHub ✔" } else { "🔑 Jeton GitHub…" };
                if ui.button(label).clicked() {
                    self.token_dialog = Some(TokenDialog::default());
                }
            });
        });
    }

    fn filters(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.filter.text)
                    .hint_text("Chercher (dépôt, titre)")
                    .desired_width(200.0),
            );
            if std::mem::take(&mut self.focus_search) {
                search.request_focus();
            }
            ui.separator();
            for kind in Kind::ALL {
                let on = self.filter.kinds.contains(&kind);
                if ui.selectable_label(on, kind.label()).clicked() {
                    if on {
                        self.filter.kinds.remove(&kind);
                    } else {
                        self.filter.kinds.insert(kind);
                    }
                }
            }
            ui.separator();
            egui::ComboBox::from_id_salt("forge")
                .selected_text(self.filter.forge.map_or("Toutes les forges", Forge::label))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter.forge, None, "Toutes les forges");
                    for f in Forge::ALL {
                        ui.selectable_value(&mut self.filter.forge, Some(f), f.label());
                    }
                });
            ui.checkbox(&mut self.filter.waiting_only, "Seulement ce qui m'attend");
            ui.checkbox(&mut self.filter.show_hidden, "Terminés et reportés");
            ui.separator();
            ui.label("Grouper :");
            let before = self.st.settings.group_by;
            ui.selectable_value(&mut self.st.settings.group_by, GroupBy::Repo, "par dépôt");
            ui.selectable_value(&mut self.st.settings.group_by, GroupBy::Kind, "par type");
            if before != self.st.settings.group_by {
                self.dirty = true;
            }
        });
    }

    fn list(&mut self, ui: &mut egui::Ui) {
        let now = now();
        if self.st.inbox.items.is_empty() {
            self.empty_state(ui);
            return;
        }
        let mut clicked = None;
        let mut opened = None;
        let mut actions: Vec<(String, Status)> = Vec::new();
        let view = self.st.inbox.view(&self.filter, self.st.settings.group_by, now);
        if view.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| ui.label("Rien ne correspond aux filtres."));
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for group in &view {
                let title =
                    if group.unread > 0 { format!("{}  ({})", group.title, group.unread) } else { group.title.clone() };
                egui::CollapsingHeader::new(RichText::new(title).strong())
                    .id_salt(&group.title)
                    .default_open(true)
                    .show(ui, |ui| {
                        for (item, status) in &group.entries {
                            let selected = self.selected.as_deref() == Some(item.id.as_str());
                            let resp = row(ui, item, *status, selected, now, self.st.settings.group_by);
                            if resp.clicked() {
                                clicked = Some(item.id.clone());
                            }
                            if resp.double_clicked() {
                                opened = Some(item.id.clone());
                            }
                            resp.context_menu(|ui| {
                                if ui.button("Ouvrir dans le navigateur").clicked() {
                                    opened = Some(item.id.clone());
                                    ui.close();
                                }
                                if ui.button("Marquer lu").clicked() {
                                    actions.push((item.id.clone(), Status::Read));
                                    ui.close();
                                }
                                if ui.button("Marquer non lu").clicked() {
                                    actions.push((item.id.clone(), Status::Unread));
                                    ui.close();
                                }
                                ui.menu_button("Me le rappeler", |ui| {
                                    for s in Snooze::ALL {
                                        if ui.button(s.label()).clicked() {
                                            actions.push((item.id.clone(), Status::Snoozed { until: s.until(now) }));
                                            ui.close();
                                        }
                                    }
                                });
                                if ui.button("Marquer terminé").clicked() {
                                    actions.push((item.id.clone(), Status::Done));
                                    ui.close();
                                }
                            });
                            if selected && resp.gained_focus() {
                                resp.scroll_to_me(None);
                            }
                        }
                    });
            }
        });
        drop(view);
        if let Some(id) = clicked {
            self.selected = Some(id);
        }
        if let Some(id) = opened {
            self.open(&id);
        }
        for (id, s) in actions {
            self.set_status(&id, s);
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            if self.no_access {
                ui.heading("Owlery n'a pas encore accès à GitHub");
                ui.add_space(8.0);
                ui.label("Deux façons de lui en donner un, en lecture seule :");
                ui.add_space(8.0);
                if ui.button("🔑 Ajouter un jeton GitHub").clicked() {
                    self.token_dialog = Some(TokenDialog::default());
                }
                ui.add_space(4.0);
                ui.label("ou");
                ui.add_space(4.0);
                if ui.button("Installer gh, puis lancer `gh auth login`").clicked()
                    && let Err(e) = actions::open_url(GH_PAGE)
                {
                    self.toast(e);
                }
            } else if self.fetching.is_some() {
                ui.spinner();
                ui.label("Lecture de GitHub…");
            } else {
                ui.heading("Rien ne vous attend 🎉");
            }
        });
    }

    fn detail(&mut self, ui: &mut egui::Ui) {
        let now = now();
        let Some(item) = self.selected.as_ref().and_then(|id| self.st.inbox.items.get(id)).cloned() else {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Choisissez une entrée.").weak()));
            return;
        };
        let status = self.st.inbox.status(&item, now);
        ui.label(RichText::new(format!("{} · {}", item.kind.label(), item.forge.label())).weak());
        ui.heading(&item.title);
        let number = item.number.map(|n| format!("#{n}")).unwrap_or_default();
        ui.label(format!("{} {number}", item.repo));
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
                let (text, color) = ci_label(ci);
                ui.label(RichText::new(text).color(color));
                ui.end_row();
            }
            ui.label("Activité");
            ui.label(format!("{} ({})", ago(item.updated_at, now), date(item.updated_at)));
            ui.end_row();
            ui.label("État");
            ui.label(status_label(status));
            ui.end_row();
        });
        if !item.summary.is_empty() {
            ui.add_space(6.0);
            ui.label(&item.summary);
        }
        ui.add_space(10.0);
        ui.separator();
        let id = item.id.clone();
        if ui.button("🌐 Ouvrir dans le navigateur").on_hover_text(self.shortcuts.label(Command::Open)).clicked() {
            self.open(&id);
        }
        ui.horizontal(|ui| {
            if status == Status::Unread {
                if ui.button("Marquer lu").on_hover_text(self.shortcuts.label(Command::MarkRead)).clicked() {
                    self.set_status(&id, Status::Read);
                }
            } else if ui.button("Marquer non lu").on_hover_text(self.shortcuts.label(Command::MarkUnread)).clicked() {
                self.set_status(&id, Status::Unread);
            }
            if ui.button("✔ Terminé").on_hover_text(self.shortcuts.label(Command::MarkDone)).clicked() {
                self.set_status(&id, Status::Done);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Me le rappeler :");
            for s in Snooze::ALL {
                if ui.small_button(s.label()).clicked() {
                    self.set_status(&id, Status::Snoozed { until: s.until(now) });
                }
            }
        });
        ui.add_space(10.0);
        ui.separator();
        self.local_actions(ui, &item);
    }

    /// Dossier local du dépôt, DragonTree et Claude.
    fn local_actions(&mut self, ui: &mut egui::Ui, item: &Item) {
        let dir = self.st.settings.repo_dirs.get(&item.repo).cloned();
        ui.horizontal(|ui| {
            ui.label("Dossier local :");
            ui.label(RichText::new(dir.as_deref().unwrap_or("non choisi")).monospace());
            if ui.small_button("Choisir…").clicked()
                && let Some(d) =
                    rfd::FileDialog::new().set_title(format!("Dossier local de {}", item.repo)).pick_folder()
            {
                self.st.settings.repo_dirs.insert(item.repo.clone(), d.display().to_string());
                self.dirty = true;
            }
        });
        let dragontree = actions::find_dragontree(&self.st.settings.dragontree_exe);
        let can_dt = dir.is_some() && dragontree.is_some();
        let hint = if dragontree.is_none() {
            "dragontree.exe introuvable : indiquez son chemin dans les réglages"
        } else if dir.is_none() {
            "Choisissez d'abord le dossier local du dépôt"
        } else {
            "Ouvre le dossier du dépôt dans DragonTree"
        };
        if ui
            .add_enabled(can_dt, egui::Button::new("🌳 Ouvrir dans DragonTree"))
            .on_hover_text(hint)
            .on_disabled_hover_text(hint)
            .clicked()
            && let (Some(exe), Some(d)) = (dragontree, dir)
            && let Err(e) = actions::open_in_dragontree(&exe, std::path::Path::new(&d))
        {
            self.toast(e);
        }
        let fawkes_hint = "Fawkes ne reçoit pas encore de demandes (contrat local à venir, voir docs/contrat-local.md)";
        ui.add_enabled(false, egui::Button::new("✨ Corriger avec Claude")).on_disabled_hover_text(fawkes_hint);
        if ui
            .button("📋 Copier le contexte pour Claude")
            .on_hover_text("Lien, branche et échec de CI, à coller dans une session Claude")
            .clicked()
        {
            ui.ctx().copy_text(actions::claude_context(item));
            self.toast("Contexte copié : rien n'a été envoyé");
        }
    }

    fn token_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.token_dialog else { return };
        let mut close = false;
        let mut save = None;
        let mut remove = false;
        egui::Modal::new(egui::Id::unique("token")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading("Jeton GitHub");
            ui.label("Owlery lit GitHub en lecture seule. Créez un jeton à grain fin avec, en lecture seule :");
            ui.label("  • Metadata, Pull requests, Issues, Commit statuses, Checks (dépôts concernés) ;");
            ui.label("  • rien en écriture.");
            ui.label("Un jeton classique marche aussi, avec la portée « repo » (ou « public_repo »).");
            if ui.link("Créer un jeton sur GitHub").clicked() {
                let _ = actions::open_url(TOKEN_PAGE);
            }
            ui.add_space(6.0);
            ui.label("Le jeton est rangé dans le Gestionnaire d'identifiants de Windows (service « Owlery »), jamais ailleurs, et n'est plus jamais affiché.");
            ui.add(egui::TextEdit::singleline(&mut dialog.input).password(true).hint_text("Coller le jeton").desired_width(f32::INFINITY));
            if let Some(e) = &dialog.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Enregistrer").clicked() {
                    save = Some(std::mem::take(&mut dialog.input));
                }
                if ui.button("Annuler").clicked() {
                    close = true;
                }
                if self.has_token && ui.button("Retirer le jeton").clicked() {
                    remove = true;
                }
            });
        });
        if let Some(token) = save {
            match secrets::store_token(self.store.as_ref(), GITHUB_TOKEN, &token) {
                Ok(()) => {
                    self.has_token = true;
                    self.token_dialog = None;
                    self.toast("Jeton enregistré dans le coffre de Windows");
                    self.refresh(ctx);
                }
                Err(e) => {
                    if let Some(d) = &mut self.token_dialog {
                        d.error = Some(format!("Enregistrement impossible : {e}"));
                    }
                }
            }
        } else if remove {
            match self.store.delete(GITHUB_TOKEN) {
                Ok(()) => {
                    self.has_token = false;
                    self.token_dialog = None;
                    self.toast("Jeton retiré du coffre");
                }
                Err(e) => {
                    if let Some(d) = &mut self.token_dialog {
                        d.error = Some(format!("Suppression impossible : {e}"));
                    }
                }
            }
        } else if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.token_dialog = None;
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        // Saisie d'un raccourci : la prochaine combinaison est prise (Échap annule).
        if let Some((cmd, slot)) = self.capturing {
            let got = ctx.input_mut(|i| {
                let mut found = None;
                i.events.retain(|ev| {
                    if found.is_none()
                        && let Some(b) = Binding::from_event(ev)
                    {
                        found = Some(b);
                        return false;
                    }
                    true
                });
                found
            });
            if let Some(b) = got {
                self.capturing = None;
                if b.key != egui::Key::Escape {
                    let conflicts = self.shortcuts.conflicts(cmd, &b);
                    if !conflicts.is_empty() {
                        let names: Vec<&str> = conflicts.iter().map(|c| c.label()).collect();
                        self.toast(format!("{} servait déjà à : {} (gardé aussi)", b.label(), names.join(", ")));
                    }
                    self.shortcuts.set(cmd, slot, Some(b));
                    self.st.settings.shortcuts = self.shortcuts.to_settings();
                    self.dirty = true;
                }
            }
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("Réglages").open(&mut open).default_width(520.0).show(ctx, |ui| {
            egui::Grid::new("settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Lecture périodique");
                ui.horizontal(|ui| {
                    changed |= ui
                        .add(egui::DragValue::new(&mut self.st.settings.refresh_minutes).range(0..=240).suffix(" min"))
                        .changed();
                    ui.label(RichText::new("0 : seulement à la demande").weak());
                });
                ui.end_row();
                ui.label("Thème");
                ui.horizontal(|ui| {
                    for (t, label) in
                        [(ThemeChoice::System, "Système"), (ThemeChoice::Dark, "Sombre"), (ThemeChoice::Light, "Clair")]
                    {
                        if ui.selectable_value(&mut self.st.settings.theme, t, label).changed() {
                            apply_theme(ui.ctx(), t);
                            changed = true;
                        }
                    }
                });
                ui.end_row();
                ui.label("dragontree.exe");
                ui.horizontal(|ui| {
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut self.st.settings.dragontree_exe)
                                .hint_text("cherché dans le PATH"),
                        )
                        .changed();
                    if ui.small_button("Choisir…").clicked()
                        && let Some(p) = rfd::FileDialog::new().add_filter("Programme", &["exe"]).pick_file()
                    {
                        self.st.settings.dragontree_exe = p.display().to_string();
                        changed = true;
                    }
                });
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.collapsing("Raccourcis", |ui| {
                egui::Grid::new("shortcuts").num_columns(4).striped(true).show(ui, |ui| {
                    for cmd in Command::ALL {
                        ui.label(cmd.label());
                        let slots = self.shortcuts.get(cmd);
                        for (slot, b) in slots.iter().enumerate() {
                            let waiting = self.capturing == Some((cmd, slot));
                            let text = if waiting {
                                "Appuyez…".to_string()
                            } else {
                                b.map(|b| b.label()).unwrap_or_else(|| "—".into())
                            };
                            let resp = ui.button(text).on_hover_text("Clic : changer · clic droit : effacer");
                            if resp.clicked() {
                                self.capturing = Some((cmd, slot));
                            }
                            if resp.secondary_clicked() {
                                self.shortcuts.set(cmd, slot, None);
                                changed = true;
                            }
                        }
                        if ui.small_button("Défaut").clicked() {
                            self.shortcuts.reset(cmd);
                            changed = true;
                        }
                        ui.end_row();
                    }
                });
            });
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Dossier de l'état : {}", self.dir.display())).weak());
        });
        if changed {
            self.st.settings.shortcuts = self.shortcuts.to_settings();
            self.dirty = true;
        }
        if !open {
            self.settings_open = false;
            self.capturing = None;
        }
    }

    fn palette_window(&mut self, ctx: &egui::Context) {
        let Some(p) = &mut self.palette else { return };
        let entries: Vec<(String, PaletteAction)> = palette_entries(&self.shortcuts)
            .into_iter()
            .filter(|(label, _)| label.to_lowercase().contains(&p.query.to_lowercase()))
            .collect();
        p.selected = p.selected.min(entries.len().saturating_sub(1));
        let (up, down, enter, escape) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if down {
            p.selected = (p.selected + 1).min(entries.len().saturating_sub(1));
        }
        if up {
            p.selected = p.selected.saturating_sub(1);
        }
        let mut chosen = None;
        egui::Modal::new(egui::Id::unique("palette")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.add(egui::TextEdit::singleline(&mut p.query).hint_text("Commande…").desired_width(f32::INFINITY))
                .request_focus();
            for (i, (label, action)) in entries.iter().enumerate() {
                if ui.selectable_label(i == p.selected, label).clicked() {
                    chosen = Some(*action);
                }
            }
        });
        if enter {
            chosen = entries.get(p.selected).map(|(_, a)| *a);
        }
        if chosen.is_some() || escape {
            self.palette = None;
        }
        match chosen {
            Some(PaletteAction::Run(cmd)) => self.run(ctx, cmd),
            Some(PaletteAction::Show(kind)) => {
                self.filter = Filter::default();
                if let Some(k) = kind {
                    self.filter.kinds.insert(k);
                }
            }
            None => {}
        }
    }

    fn banners(&mut self, ui: &mut egui::Ui) {
        let mut dismiss = None;
        for (i, p) in self.problems.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.colored_label(ui.visuals().warn_fg_color, format!("⚠ {p}"));
                if ui.small_button("Compris").clicked() {
                    dismiss = Some(i);
                }
            });
        }
        if let Some(i) = dismiss {
            self.problems.remove(i);
        }
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|(_, t)| t.elapsed() < Duration::from_secs(5));
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::unique("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -12.0]).show(
            ctx,
            |ui| {
                for (msg, _) in &self.toasts {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(msg));
                }
            },
        );
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

#[derive(Clone, Copy)]
enum PaletteAction {
    Run(Command),
    /// Aller à la boîte de réception, filtrée sur un type (ou tout).
    Show(Option<Kind>),
}

fn palette_entries(shortcuts: &Shortcuts) -> Vec<(String, PaletteAction)> {
    let mut v = vec![
        ("Aller à la boîte de réception".to_string(), PaletteAction::Show(None)),
        ("PR à relire".to_string(), PaletteAction::Show(Some(Kind::ReviewRequested))),
        ("CI en échec".to_string(), PaletteAction::Show(Some(Kind::CiFailed))),
        ("Mentions".to_string(), PaletteAction::Show(Some(Kind::Mention))),
        ("Tickets assignés".to_string(), PaletteAction::Show(Some(Kind::Assigned))),
    ];
    for cmd in Command::ALL.into_iter().filter(|c| *c != Command::Palette) {
        let key = shortcuts.label(cmd);
        let label = if key.is_empty() { cmd.label().to_string() } else { format!("{}    {key}", cmd.label()) };
        v.push((label, PaletteAction::Run(cmd)));
    }
    v
}

fn apply_theme(ctx: &egui::Context, theme: ThemeChoice) {
    ctx.set_theme(match theme {
        ThemeChoice::System => egui::ThemePreference::System,
        ThemeChoice::Dark => egui::ThemePreference::Dark,
        ThemeChoice::Light => egui::ThemePreference::Light,
    });
}

fn ci_label(ci: Ci) -> (&'static str, Color32) {
    match ci {
        Ci::Success => ("✔ réussie", Color32::from_rgb(0x3f, 0xb9, 0x50)),
        Ci::Pending => ("⟳ en cours", Color32::from_rgb(0xd2, 0x99, 0x22)),
        Ci::Failure => ("✖ en échec", Color32::from_rgb(0xf8, 0x51, 0x49)),
    }
}

fn status_label(s: Status) -> String {
    match s {
        Status::Unread => "non lu".into(),
        Status::Read => "lu".into(),
        Status::Done => "terminé".into(),
        Status::Snoozed { until } => format!("reporté jusqu'au {}", date(until)),
    }
}

fn date(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%d/%m %H:%M").to_string())
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

/// Une ligne de la liste : pastille non lu, type ou dépôt, titre, CI, ancienneté.
fn row(ui: &mut egui::Ui, item: &Item, status: Status, selected: bool, now: i64, group_by: GroupBy) -> egui::Response {
    let unread = status == Status::Unread;
    let resp = ui
        .horizontal(|ui| {
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
            let mut title = RichText::new(&item.title);
            if unread {
                title = title.strong();
            }
            if matches!(status, Status::Done | Status::Snoozed { .. }) {
                title = title.weak();
            }
            ui.label(title);
            if let Some(n) = item.number {
                ui.label(RichText::new(format!("#{n}")).weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(ago(item.updated_at, now)).small().weak());
                if let Some(ci) = item.ci {
                    let (text, color) = ci_label(ci);
                    ui.label(RichText::new(&text[..text.find(' ').unwrap_or(text.len())]).color(color))
                        .on_hover_text(text);
                }
                if let Status::Snoozed { until } = status {
                    ui.label(RichText::new(format!("⏰ {}", date(until))).small().weak());
                }
            });
        })
        .response;
    let resp = resp.interact(egui::Sense::click());
    if selected {
        ui.painter().rect_filled(resp.rect, 3.0, ui.visuals().selection.bg_fill.gamma_multiply(0.25));
    } else if resp.hovered() {
        ui.painter().rect_filled(resp.rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(0.4));
    }
    resp
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_fetch();
        self.auto_refresh(ctx);
        crate::capture::poll(ctx, self.capture.as_ref());
        self.save();
        // Réveil léger : lecture périodique et fin des reports.
        ctx.request_repaint_after(Duration::from_secs(30));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(4.0);
            self.toolbar(ui);
            self.filters(ui);
            self.banners(ui);
            ui.add_space(2.0);
        });
        egui::Panel::right("detail").default_size(360.0).resizable(true).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.detail(ui));
        });
        egui::CentralPanel::default().show(ui, |ui| self.list(ui));
        self.token_window(&ctx);
        self.settings_window(&ctx);
        self.palette_window(&ctx);
        self.toasts(&ctx);
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_read_naturally() {
        assert_eq!(ago(100, 110), "à l'instant");
        assert_eq!(ago(0, 12 * 60), "il y a 12 min");
        assert_eq!(ago(0, 3 * 3600 + 5), "il y a 3 h");
        assert_eq!(ago(0, 2 * 86400), "il y a 2 j");
        assert_eq!(ago(500, 100), "à l'instant");
    }

    #[test]
    fn palette_offers_the_spec_entries() {
        let labels: Vec<String> =
            palette_entries(&Shortcuts::from_settings(&Default::default())).into_iter().map(|(l, _)| l).collect();
        for want in ["Aller à la boîte de réception", "PR à relire", "CI en échec"] {
            assert!(labels.iter().any(|l| l == want), "{want}");
        }
        assert!(labels.iter().any(|l| l.starts_with("Actualiser") && l.ends_with("F5")));
    }
}
