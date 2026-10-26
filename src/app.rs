//! L'application : état, lectures en arrière-plan (une par compte), commandes, notifications, liens avec
//! Windows et les autres applications. L'affichage est dans `ui`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::contract::{self, Incoming, Snapshot};
use crate::forges::{self, Account, FetchError, Fetched, Fetcher, Identity, Seen};
use crate::inbox::{GroupBy, Rules, Snooze, Undo};
use crate::model::{Comment, Item, Kind, Status, Target};
use crate::secrets::SecretStore;
use crate::shortcuts::{Command, Focus, Shortcuts};
use crate::state::{self, AppState, ThemeChoice, WindowState};

/// Demande à Fawkes en cours : la réponse attendue, et le contexte à copier si elle échoue.
pub struct FawkesJob {
    pub rx: Receiver<Result<String, String>>,
    pub context: String,
}

/// Recherches coûteuses, gardées un instant (voir `App::lookups`).
#[derive(Clone, Debug, Default)]
pub struct Lookups {
    pub fawkes_up: bool,
    pub fawkes_exe: Option<PathBuf>,
    pub dragontree: Option<PathBuf>,
}

/// Commentaires en cours de lecture : entrée, état vu, réponse attendue.
type CommentJob = (String, i64, Receiver<Result<Vec<Comment>, FetchError>>);

/// Nombre de changements qu'on peut annuler.
const UNDO_DEPTH: usize = 50;
/// Attente du canal de Fawkes après l'avoir démarré.
const FAWKES_START_WAIT: Duration = Duration::from_secs(20);

/// Lien vers Fawkes par le contrat local (réel : canal nommé ; tests : faux).
pub trait FawkesLink: Send + Sync {
    /// Fawkes écoute-t-il sur son canal ?
    fn available(&self) -> bool;
    /// Envoie une demande (une ligne JSON) et renvoie la réponse.
    fn send(&self, line: &str) -> Result<String, String>;
    /// `fawkes.exe` : celui des réglages, sinon celui du PATH.
    fn find_exe(&self, configured: &str) -> Option<PathBuf>;
    /// Démarre Fawkes, sans argument (il ne connaît pas encore de demande en ligne de commande).
    fn start(&self, exe: &Path) -> Result<(), String>;
}

/// Notifications Windows (réel : zone de notification ; tests : rien).
pub trait Notifier: Send + Sync {
    fn show(&self, title: &str, body: &str);
    fn hide(&self);
}

/// Pas de Fawkes (tests, autres systèmes).
pub struct NoFawkes;

impl FawkesLink for NoFawkes {
    fn available(&self) -> bool {
        false
    }
    fn send(&self, _line: &str) -> Result<String, String> {
        Err("Fawkes n'est pas joignable".into())
    }
    fn find_exe(&self, _configured: &str) -> Option<PathBuf> {
        None
    }
    fn start(&self, _exe: &Path) -> Result<(), String> {
        Err("Fawkes n'est pas disponible".into())
    }
}

/// Pas de notifications (tests).
pub struct NoNotifier;

impl Notifier for NoNotifier {
    fn show(&self, _title: &str, _body: &str) {}
    fn hide(&self) {}
}

/// Dépendances de l'application, fournies par `main` (ou par les tests).
pub struct StartOptions {
    pub dir: PathBuf,
    pub store: Arc<dyn SecretStore>,
    pub fetcher: Arc<dyn Fetcher>,
    pub demo: bool,
    pub capture: Option<PathBuf>,
    /// Demandes reçues sur le canal d'Owlery.
    pub incoming: Option<Receiver<Incoming>>,
    /// Ce que l'interface publie pour répondre au canal sans l'attendre.
    pub snapshot: Arc<Mutex<Snapshot>>,
    pub fawkes: Arc<dyn FawkesLink>,
    pub notifier: Arc<dyn Notifier>,
    /// Faux si l'état ne doit pas être écrit (illisible, ou tenu par une autre instance).
    pub writable: bool,
    /// Lancer une lecture au démarrage.
    pub fetch_on_start: bool,
    /// Les comptes communs de la suite EyrieSuite : `None` en démonstration et dans les tests.
    pub accounts: Option<suite_accounts::Store>,
}

impl StartOptions {
    /// Options d'essai : dossier donné, rien de Windows, pas de lecture au démarrage.
    pub fn for_tests(dir: PathBuf, store: Arc<dyn SecretStore>, fetcher: Arc<dyn Fetcher>) -> Self {
        StartOptions {
            dir,
            store,
            fetcher,
            demo: false,
            capture: None,
            incoming: None,
            snapshot: Arc::new(Mutex::new(Snapshot::default())),
            fawkes: Arc::new(NoFawkes),
            notifier: Arc::new(NoNotifier),
            writable: true,
            fetch_on_start: false,
            accounts: None,
        }
    }
}

/// Relecture des comptes de la suite (une autre application a pu les changer) : au plus toutes les 2 s.
const SUITE_CHECK_EVERY: Duration = Duration::from_secs(2);
/// Début du message d'un fichier des comptes illisible, dans les problèmes affichés.
const SUITE_PROBLEM: &str = "Comptes de la suite : ";

/// État de la dernière lecture d'un compte.
#[derive(Debug, Clone, Default)]
pub struct AccountStatus {
    pub last_ok: Option<i64>,
    pub error: Option<FetchError>,
    pub notes: Vec<String>,
    /// Pas de lecture automatique avant (quota épuisé), en secondes Unix.
    pub hold_until: Option<i64>,
}

/// Commentaires d'une entrée, lus à la demande.
#[derive(Debug, Clone, PartialEq)]
pub enum CommentsState {
    Loading,
    Loaded(Vec<Comment>),
    Failed(String),
}

/// Dialogue d'ajout d'un compte.
pub struct AccountDraft {
    pub forge: crate::model::Forge,
    pub host: String,
    pub orgs: String,
    pub token: String,
    pub use_gh: bool,
    pub checking: Option<Receiver<Result<Identity, FetchError>>>,
    pub error: Option<String>,
}

/// *Corriger avec Claude* quand Fawkes n'écoute pas : démarrer Fawkes, copier le contexte, ou annuler.
pub struct FawkesDialog {
    pub id: String,
    pub line: String,
    pub context: String,
}

pub struct App {
    pub(crate) dir: PathBuf,
    pub(crate) st: AppState,
    pub(crate) writable: bool,
    pub(crate) shortcuts: Shortcuts,
    pub(crate) store: Arc<dyn SecretStore>,
    pub(crate) fetcher: Arc<dyn Fetcher>,
    pub(crate) demo: bool,
    pub(crate) capture: Option<PathBuf>,
    pub(crate) gh_available: bool,

    pub(crate) selected: Option<String>,
    pub(crate) scroll_to_selected: bool,
    pub(crate) focus_search: bool,

    fetch_tx: Sender<(String, Result<Fetched, FetchError>)>,
    fetch_rx: Receiver<(String, Result<Fetched, FetchError>)>,
    pub(crate) running: BTreeSet<String>,
    pub(crate) status: BTreeMap<String, AccountStatus>,
    last_cycle: Option<Instant>,
    refresh_pending: bool,
    /// Aucun compte à lire : l'écran d'accueil propose d'en ajouter un.
    pub(crate) no_access: bool,
    pending_arrivals: Vec<Item>,

    pub(crate) comments: BTreeMap<String, (i64, CommentsState)>,
    comment_job: Option<CommentJob>,

    pub(crate) found_repos: BTreeMap<String, PathBuf>,
    scan_job: Option<Receiver<BTreeMap<String, PathBuf>>>,
    pub(crate) scanning: bool,

    pub(crate) undo: Vec<Vec<Undo>>,
    pub(crate) problems: Vec<String>,
    pub(crate) toasts: Vec<(String, Instant)>,
    dirty: bool,
    last_save: Option<Instant>,
    /// Le jeton du premier jet a été repris : son ancienne entrée du coffre part après le premier enregistrement.
    forget_first_draft: bool,
    /// Fawkes écoute-t-il, et où sont `dragontree.exe` et `fawkes.exe` : vérifié au plus toutes les 2 s, pas à
    /// chaque image.
    lookups: Option<(Instant, Lookups)>,
    pub(crate) save_error: Option<String>,

    pub(crate) accounts_open: bool,
    pub(crate) draft: Option<AccountDraft>,
    pub(crate) settings_open: bool,
    pub(crate) capturing: Option<(Command, usize)>,
    pub(crate) palette: Option<(String, usize)>,
    pub(crate) fawkes_dialog: Option<FawkesDialog>,
    pub(crate) fawkes_job: Option<FawkesJob>,

    incoming: Option<Receiver<Incoming>>,
    snapshot: Arc<Mutex<Snapshot>>,
    pub(crate) fawkes: Arc<dyn FawkesLink>,
    notifier: Arc<dyn Notifier>,
    revision: u64,
    published: u64,
    title_shown: Option<usize>,
    window_known: bool,
    focused: bool,
    /// Embarqué par un hôte (voir `set_hosted`).
    pub(crate) hosted: bool,
    /// Les comptes communs de la suite (`StartOptions::accounts`) et leur dernière lecture.
    suite: Option<suite_accounts::Store>,
    suite_accounts: Vec<Account>,
    suite_stamp: Option<std::time::SystemTime>,
    pub(crate) suite_check: Option<Instant>,
    /// Posé par l'hôte : les comptes se règlent chez lui (voir `set_accounts_link`).
    accounts_link: Option<Box<dyn Fn()>>,
    /// Posé par l'hôte : les réglages s'ouvrent chez lui, sur la page d'Owlery (voir `set_settings_link`).
    settings_link: Option<Box<dyn Fn()>>,
}

pub(crate) fn now() -> i64 {
    chrono::Local::now().timestamp()
}

impl App {
    /// Embarqué par un hôte (la suite) : la fenêtre n'est pas la sienne. Sa taille et sa position ne
    /// sont pas enregistrées dans l'état (sinon, lancé seul ensuite, il s'ouvrirait à la taille de la
    /// suite). Faux par défaut : rien ne change pour l'exécutable autonome.
    pub fn set_hosted(&mut self, hosted: bool) {
        self.hosted = hosted;
    }

    /// Embarqué par un hôte qui règle les comptes de toute la suite (Eyrie) : « Comptes… » et « Ajouter un compte »
    /// appellent `open` au lieu d'ouvrir la fenêtre Comptes d'Owlery.
    pub fn set_accounts_link(&mut self, open: Box<dyn Fn()>) {
        self.accounts_link = Some(open);
    }

    /// Embarqué par un hôte qui montre les réglages d'Owlery dans les siens : « Réglages » appelle `open` (la page
    /// Owlery des réglages de l'hôte) au lieu d'ouvrir sa fenêtre.
    pub fn set_settings_link(&mut self, open: Box<dyn Fn()>) {
        self.settings_link = Some(open);
    }

    /// Ses réglages, dessinés dans un `Ui` de l'hôte (la page Owlery de ses réglages).
    pub fn settings_ui(&mut self, ui: &mut egui::Ui) {
        crate::ui::settings::panel(self, ui);
    }

    /// Sa page chez l'hôte quittée (fenêtre fermée, autre page) : un raccourci en cours de saisie est abandonné.
    pub fn settings_left(&mut self) {
        self.capturing = None;
    }

    /// Les réglages se dessinent chez l'hôte (lien posé).
    pub(crate) fn settings_in_host(&self) -> bool {
        self.settings_link.is_some()
    }

    pub(crate) fn toggle_settings(&mut self) {
        match &self.settings_link {
            Some(open) => open(),
            None => self.settings_open = !self.settings_open,
        }
    }

    /// Ouvre les comptes : ceux de l'hôte s'il les règle, sinon la fenêtre d'Owlery.
    pub(crate) fn open_accounts(&mut self) {
        match &self.accounts_link {
            Some(open) => open(),
            None => self.accounts_open = true,
        }
    }

    pub(crate) fn toggle_accounts(&mut self) {
        match &self.accounts_link {
            Some(open) => open(),
            None => self.accounts_open = !self.accounts_open,
        }
    }

    /// Relit les comptes de la suite.
    /// Relit les comptes de la suite. Un fichier illisible n'est pas « aucun compte » : la dernière liste reste, la
    /// date n'avance pas (il sera relu) et l'erreur s'affiche. Vrai si la liste a été lue.
    pub(crate) fn load_suite(&mut self) -> bool {
        let Some(suite) = &self.suite else { return false };
        self.suite_check = Some(Instant::now() + SUITE_CHECK_EVERY);
        // La date avant la liste : une écriture qui tombe entre les deux sera relue la fois suivante.
        let stamp = suite.stamp();
        let read = suite.try_list();
        self.problems.retain(|p| !p.starts_with(SUITE_PROBLEM));
        match read {
            Ok(list) => {
                self.suite_accounts = list.iter().filter_map(Account::from_suite).collect();
                self.suite_stamp = stamp;
                true
            }
            Err(e) => {
                self.problems.push(format!("{SUITE_PROBLEM}{e}"));
                false
            }
        }
    }

    /// Comptes ajoutés ou retirés par une autre application de la suite : les nouveaux sont lus, les entrées des
    /// disparus retirées.
    fn sync_suite(&mut self, ctx: &egui::Context) {
        let Some(suite) = &self.suite else { return };
        if self.suite_check.is_some_and(|t| Instant::now() < t) {
            return;
        }
        self.suite_check = Some(Instant::now() + SUITE_CHECK_EVERY);
        if suite.stamp() == self.suite_stamp {
            return;
        }
        let before: BTreeSet<String> = self.suite_accounts.iter().map(|a| a.id.clone()).collect();
        if !self.load_suite() {
            return;
        }
        let after: BTreeSet<String> = self.suite_accounts.iter().map(|a| a.id.clone()).collect();
        for gone in before.difference(&after) {
            self.st.inbox.remove_account(gone);
            self.status.remove(gone);
            self.changed();
        }
        if after.difference(&before).next().is_some() {
            self.refresh(ctx);
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>, opts: StartOptions, loaded: state::Loaded) -> Self {
        let (fetch_tx, fetch_rx) = channel();
        let mut st = loaded.state;
        let mut problems = loaded.problems;
        let mut toasts = Vec::new();
        let mut dirty = false;
        let mut forget_first_draft = false;
        if opts.demo {
            if st.settings.accounts.is_empty() {
                st.settings.accounts = crate::demo::accounts();
                dirty = true;
            }
        } else if opts.writable {
            // Seulement si l'état peut être enregistré : sinon le compte repris serait perdu au lancement suivant.
            match forges::migrate_first_draft_token(opts.store.as_ref(), &mut st.settings.accounts) {
                Ok(true) => {
                    dirty = true;
                    forget_first_draft = true;
                    toasts.push((
                        "Le jeton GitHub du premier jet a été repris dans un compte".to_string(),
                        Instant::now(),
                    ));
                }
                Ok(false) => {}
                Err(e) => problems.push(format!("Reprise du jeton GitHub du premier jet impossible : {e}")),
            }
            // Les comptes à jeton deviennent ceux de toute la suite (Eyrie, DragonTree).
            if let Some(suite) = &opts.accounts {
                match forges::migrate_to_suite(suite, opts.store.as_ref(), &mut st.settings.accounts) {
                    Ok(moved) => dirty |= moved,
                    Err(e) => problems.push(format!("Reprise des comptes dans la suite impossible : {e}")),
                }
            }
        }
        if !opts.writable && !problems.iter().any(|p| p.contains("rien ne sera enregistré")) {
            problems.push("Rien ne sera enregistré pendant cette session (voir le message ci-dessus).".into());
        }
        let mut app = App {
            dir: opts.dir,
            shortcuts: Shortcuts::from_settings(&st.settings.shortcuts),
            st,
            writable: opts.writable,
            store: opts.store,
            gh_available: opts.fetcher.gh_available(),
            fetcher: opts.fetcher,
            demo: opts.demo,
            capture: opts.capture,
            selected: None,
            scroll_to_selected: false,
            focus_search: false,
            fetch_tx,
            fetch_rx,
            running: BTreeSet::new(),
            status: BTreeMap::new(),
            last_cycle: None,
            refresh_pending: false,
            no_access: false,
            pending_arrivals: Vec::new(),
            comments: BTreeMap::new(),
            comment_job: None,
            found_repos: BTreeMap::new(),
            scan_job: None,
            scanning: false,
            undo: Vec::new(),
            problems,
            toasts,
            dirty,
            last_save: None,
            forget_first_draft,
            lookups: None,
            save_error: None,
            accounts_open: false,
            draft: None,
            settings_open: false,
            capturing: None,
            palette: None,
            fawkes_dialog: None,
            fawkes_job: None,
            incoming: opts.incoming,
            snapshot: opts.snapshot,
            fawkes: opts.fawkes,
            notifier: opts.notifier,
            revision: 1,
            published: 0,
            title_shown: None,
            window_known: false,
            focused: true,
            hosted: false,
            suite: opts.accounts,
            suite_accounts: Vec::new(),
            suite_stamp: None,
            suite_check: None,
            accounts_link: None,
            settings_link: None,
        };
        app.load_suite();
        apply_theme(&cc.egui_ctx, app.st.settings.theme);
        app.scan_repos();
        if opts.fetch_on_start {
            app.refresh(&cc.egui_ctx);
        } else {
            app.no_access = app.accounts().is_empty();
        }
        if app.demo {
            app.selected = app.visible_ids().into_iter().next();
        }
        app
    }

    pub(crate) fn toast(&mut self, msg: impl Into<String>) {
        self.toasts.push((msg.into(), Instant::now()));
    }

    pub(crate) fn changed(&mut self) {
        self.dirty = true;
        self.revision += 1;
    }

    pub(crate) fn rules(&self) -> &Rules {
        &self.st.settings.rules
    }

    /// Comptes lus : ceux de la suite, ceux des réglages (`gh`), et « GitHub par gh » s'il n'y a pas de compte
    /// GitHub.
    pub(crate) fn accounts(&self) -> Vec<Account> {
        let mut configured = self.st.settings.accounts.clone();
        configured.extend(self.suite_accounts.iter().cloned());
        forges::accounts_to_read(&configured, self.gh_available)
    }

    /// La liste des comptes est-elle sûre ? Faux quand les comptes de la suite n'ont pas pu être lus : la liste
    /// peut alors en manquer.
    pub(crate) fn accounts_known(&self) -> bool {
        !self.problems.iter().any(|p| p.starts_with(SUITE_PROBLEM))
    }

    /// Le compte vient-il de la suite (et non des réglages d'Owlery) ?
    pub(crate) fn is_suite_account(&self, id: &str) -> bool {
        self.suite_accounts.iter().any(|a| a.id == id)
    }

    pub(crate) fn suite(&self) -> Option<&suite_accounts::Store> {
        self.suite.as_ref()
    }

    pub(crate) fn account_of(&self, item: &Item) -> Option<Account> {
        let accounts = self.accounts();
        accounts
            .iter()
            .find(|a| a.id == item.account)
            .or_else(|| accounts.iter().find(|a| item.account.is_empty() && a.forge == item.forge))
            .cloned()
    }

    pub(crate) fn host_of(&self, item: &Item) -> String {
        self.account_of(item).map(|a| a.host).unwrap_or_else(|| item.forge.default_host().to_string())
    }

    // --- Lecture -----------------------------------------------------------------------------------------

    pub(crate) fn refresh(&mut self, ctx: &egui::Context) {
        if !self.running.is_empty() {
            // Une lecture est en cours : la demande est gardée, une nouvelle lecture suivra.
            self.refresh_pending = true;
            return;
        }
        self.refresh_pending = false;
        self.last_cycle = Some(Instant::now());
        self.gh_available = self.fetcher.gh_available();
        let accounts = self.accounts();
        self.no_access = accounts.is_empty();
        // Entrées des comptes qui ne sont plus lus (compte « gh » remplacé, gh désinstallé) : retirées, sinon
        // elles attendraient pour toujours.
        let ids: BTreeSet<String> = accounts.iter().map(|a| a.id.clone()).collect();
        if self.st.inbox.retain_accounts(&ids) > 0 {
            self.changed();
        }
        let now = now();
        for account in accounts {
            if self.status.get(&account.id).and_then(|s| s.hold_until).is_some_and(|t| now < t) {
                continue;
            }
            let seen: Seen = self
                .st
                .inbox
                .items
                .values()
                .filter(|i| i.account == account.id)
                .filter_map(|i| {
                    let m = self.st.inbox.marks.get(&i.id)?;
                    matches!(m.status, Status::Read | Status::Done).then(|| (i.id.clone(), m.seen_updated_at))
                })
                .collect();
            let (tx, fetcher, ctx) = (self.fetch_tx.clone(), self.fetcher.clone(), ctx.clone());
            self.running.insert(account.id.clone());
            std::thread::spawn(move || {
                // Une panique dans la lecture ne doit pas laisser l'interface attendre pour toujours.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fetcher.fetch(&account, &seen)))
                    .unwrap_or_else(|_| Err(FetchError::Network("lecture interrompue par une erreur interne".into())));
                let _ = tx.send((account.id.clone(), result));
                ctx.request_repaint();
            });
        }
    }

    fn poll_fetches(&mut self, ctx: &egui::Context) {
        let now = now();
        while let Ok((id, result)) = self.fetch_rx.try_recv() {
            self.running.remove(&id);
            let Some(account) = self.accounts().into_iter().find(|a| a.id == id) else { continue };
            let status = self.status.entry(id.clone()).or_default();
            match result {
                Ok(fetched) => {
                    status.last_ok = Some(now);
                    status.error = None;
                    status.hold_until = None;
                    status.notes = fetched.notes;
                    let merged = self.st.inbox.merge_with(&id, account.forge, fetched.items, !fetched.incomplete, now);
                    // Nom appris pendant la lecture (compte repris du premier jet) : pour le titre du compte.
                    if let (Some(me), Some(a)) = (fetched.me, self.st.settings.accounts.iter_mut().find(|a| a.id == id))
                        && a.username.is_empty()
                        && !me.is_empty()
                    {
                        a.username = me;
                    }
                    let s = &self.st.settings;
                    self.pending_arrivals.extend(crate::notify::arrivals(
                        &merged.before_unread,
                        merged.first_fetch,
                        &self.st.inbox,
                        &id,
                        &s.notify_kinds,
                        &s.rules,
                        now,
                    ));
                    self.changed();
                }
                Err(e) => {
                    if let FetchError::RateLimited { reset } = e {
                        status.hold_until = Some(reset.unwrap_or(now + 15 * 60));
                    }
                    // Rien n'est effacé : les entrées en cache du compte restent affichées.
                    status.error = Some(e);
                }
            }
        }
        if self.running.is_empty() {
            if self.demo && self.selected.is_none() {
                // Démonstration : une entrée choisie d'emblée, pour montrer le détail.
                self.selected = self.visible_ids().into_iter().next();
            }
            if !self.pending_arrivals.is_empty() {
                let arrivals = std::mem::take(&mut self.pending_arrivals);
                self.notify(&arrivals, &[]);
            }
            if self.refresh_pending {
                self.refresh(ctx);
            }
        }
    }

    fn auto_refresh(&mut self, ctx: &egui::Context) {
        let minutes = self.st.settings.refresh_minutes;
        if minutes == 0 || !self.running.is_empty() {
            return;
        }
        if self.last_cycle.is_none_or(|t| t.elapsed() >= Duration::from_secs(u64::from(minutes) * 60)) {
            self.refresh(ctx);
        }
    }

    /// Reports arrivés à échéance : ils reviennent en non lu, avec une notification « Rappel ».
    fn due_snoozes(&mut self) {
        let due = self.st.inbox.take_due_snoozes(now());
        if due.is_empty() {
            return;
        }
        // Les dépôts ignorés et les robots masqués ne rappellent rien non plus.
        let rules = self.rules().clone();
        let items: Vec<Item> =
            due.iter().filter_map(|id| self.st.inbox.items.get(id)).filter(|i| rules.shows(i)).cloned().collect();
        self.changed();
        self.notify(&[], &items);
    }

    fn notify(&mut self, arrivals: &[Item], reminders: &[Item]) {
        if !self.st.settings.notify || self.focused {
            return;
        }
        if let Some((title, body)) = crate::notify::message(arrivals, reminders) {
            self.notifier.show(&title, &body);
        }
    }

    // --- Commentaires, dépôts locaux --------------------------------------------------------------------

    /// Lit les derniers commentaires de l'entrée sélectionnée, s'ils ne sont pas déjà en mémoire pour son état.
    fn ensure_comments(&mut self, ctx: &egui::Context) {
        if let Some((id, at, rx)) = &self.comment_job {
            match rx.try_recv() {
                Ok(result) => {
                    let state = match result {
                        Ok(list) => CommentsState::Loaded(list),
                        Err(e) => CommentsState::Failed(e.to_string()),
                    };
                    self.comments.insert(id.clone(), (*at, state));
                    self.comment_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.comments.insert(id.clone(), (*at, CommentsState::Failed("lecture interrompue".into())));
                    self.comment_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
            }
        }
        let Some(item) = self.selected.as_ref().and_then(|id| self.st.inbox.items.get(id)).cloned() else { return };
        let wanted = !matches!(item.target, Target::Unknown)
            && (item.comments > 0 || matches!(item.target, Target::Azure { .. }) || self.demo);
        if !wanted || self.comments.get(&item.id).is_some_and(|(at, _)| *at == item.updated_at) {
            return;
        }
        let Some(account) = self.account_of(&item) else { return };
        let (tx, rx) = channel();
        let (fetcher, ctx) = (self.fetcher.clone(), ctx.clone());
        let for_thread = item.clone();
        std::thread::spawn(move || {
            let _ = tx.send(fetcher.comments(&account, &for_thread));
            ctx.request_repaint();
        });
        self.comments.insert(item.id.clone(), (item.updated_at, CommentsState::Loading));
        self.comment_job = Some((item.id, item.updated_at, rx));
    }

    /// Parcourt les dossiers racines des réglages, en arrière-plan.
    pub(crate) fn scan_repos(&mut self) {
        let roots: Vec<PathBuf> =
            self.st.settings.repo_roots.iter().filter(|r| !r.trim().is_empty()).map(PathBuf::from).collect();
        if roots.is_empty() {
            self.found_repos.clear();
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::localrepo::scan(&roots, 3));
        });
        self.scan_job = Some(rx);
        self.scanning = true;
    }

    fn poll_scan(&mut self) {
        let Some(rx) = &self.scan_job else { return };
        match rx.try_recv() {
            Ok(found) => self.found_repos = found,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            // Parcours interrompu : on garde ce qui était connu.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
        }
        self.scan_job = None;
        self.scanning = false;
    }

    /// Dossier local d'une entrée : celui choisi à la main, sinon celui trouvé dans les dossiers racines.
    pub(crate) fn local_dir(&self, item: &Item) -> Option<PathBuf> {
        crate::localrepo::local_dir(item, &self.host_of(item), &self.st.settings.repo_dirs, &self.found_repos)
    }

    // --- Corriger avec Claude -------------------------------------------------------------------------

    pub(crate) fn fix_with_claude(&mut self, ctx: &egui::Context, id: &str) {
        let Some(item) = self.st.inbox.items.get(id).cloned() else { return };
        let Some(dir) = self.local_dir(&item) else {
            self.toast("Choisissez d'abord le dossier local du dépôt");
            return;
        };
        let context = crate::actions::claude_context(&item);
        let title = match item.number {
            Some(n) => format!("{} #{n}", item.repo),
            None => item.repo.clone(),
        };
        let line = contract::fawkes_open(&dir, &title, item.branch.as_deref(), &context);
        if self.lookups().fawkes_up {
            self.send_to_fawkes(ctx, line, context, false);
        } else {
            self.fawkes_dialog = Some(FawkesDialog { id: id.to_string(), line, context });
        }
    }

    /// Envoie la demande à Fawkes en arrière-plan ; `start` : le démarrer d'abord et attendre son canal.
    pub(crate) fn send_to_fawkes(&mut self, ctx: &egui::Context, line: String, context: String, start: bool) {
        let exe = if start {
            match self.fawkes.find_exe(&self.st.settings.fawkes_exe) {
                Some(exe) => Some(exe),
                None => {
                    self.toast("fawkes.exe introuvable : indiquez son chemin dans les réglages");
                    return;
                }
            }
        } else {
            None
        };
        let (tx, rx) = channel();
        let (fawkes, ctx) = (self.fawkes.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = (|| {
                if let Some(exe) = exe {
                    fawkes.start(&exe)?;
                    let deadline = Instant::now() + FAWKES_START_WAIT;
                    while !fawkes.available() {
                        if Instant::now() > deadline {
                            return Err("Fawkes ne reçoit pas encore de demandes (pas de canal après 20 s)".to_string());
                        }
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
                fawkes.send(&line)
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.fawkes_job = Some(FawkesJob { rx, context });
        self.lookups = None;
    }

    fn poll_fawkes(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.fawkes_job else { return };
        let result = match job.rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("demande interrompue (erreur interne)".to_string()),
        };
        let context = std::mem::take(&mut self.fawkes_job.as_mut().expect("tâche en cours").context);
        self.fawkes_job = None;
        match result.and_then(|reply| contract::parse_reply(&reply)) {
            Ok(()) => self.toast("Fawkes ouvre une session Claude (consigne collée, pas envoyée)"),
            Err(e) => {
                // Le contexte de l'entrée demandée, pas celui de l'entrée sélectionnée entre-temps.
                ctx.copy_text(context);
                self.toast(format!("Fawkes : {e}. Contexte copié dans le presse-papiers à la place."));
            }
        }
    }

    /// Fawkes écoute-t-il, où sont DragonTree et Fawkes : vérifié au plus toutes les 2 s (le détail est redessiné
    /// à chaque mouvement de la souris).
    pub(crate) fn lookups(&mut self) -> Lookups {
        if let Some((at, l)) = &self.lookups
            && at.elapsed() < Duration::from_secs(2)
        {
            return l.clone();
        }
        let l = Lookups {
            fawkes_up: self.fawkes.available(),
            fawkes_exe: self.fawkes.find_exe(&self.st.settings.fawkes_exe),
            dragontree: crate::actions::find_dragontree(&self.st.settings.dragontree_exe),
        };
        self.lookups = Some((Instant::now(), l.clone()));
        l
    }

    // --- Entrées --------------------------------------------------------------------------------------

    pub(crate) fn visible_ids(&self) -> Vec<String> {
        let view = self.st.inbox.view(&self.st.view, self.rules(), self.st.settings.group_by, now());
        view.iter().flat_map(|g| g.entries.iter().map(|(i, _)| i.id.clone())).collect()
    }

    pub(crate) fn select(&mut self, id: Option<String>) {
        self.selected = id;
        self.scroll_to_selected = true;
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
        self.select(Some(ids[next].clone()));
    }

    /// Change l'état d'entrées (annulable d'un coup). Une entrée sélectionnée qui quitte la liste passe la
    /// sélection à sa voisine.
    pub(crate) fn set_status_many(&mut self, ids: &[String], status: Status, seen_at: Option<i64>) {
        if let Some(sel) = self.selected.clone()
            && ids.contains(&sel)
            && !self.st.inbox.would_show(&self.st.view, self.rules(), &sel, status)
        {
            let visible = self.visible_ids();
            let pos = visible.iter().position(|i| *i == sel);
            let next = pos.and_then(|p| {
                visible[p + 1..].iter().chain(visible[..p].iter().rev()).find(|i| !ids.contains(i)).cloned()
            });
            self.select(next);
        }
        let undos: Vec<Undo> = ids.iter().filter_map(|id| self.st.inbox.set_status(id, status, seen_at)).collect();
        if undos.is_empty() {
            return;
        }
        self.undo.push(undos);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.changed();
    }

    pub(crate) fn set_status(&mut self, id: &str, status: Status) {
        self.set_status_many(&[id.to_string()], status, None);
    }

    pub(crate) fn undo_last(&mut self) {
        let Some(group) = self.undo.pop() else {
            self.toast("Rien à annuler");
            return;
        };
        let n = group.len();
        for u in group.into_iter().rev() {
            self.st.inbox.restore(u);
        }
        self.changed();
        self.toast(if n == 1 { "Annulé".to_string() } else { format!("Annulé ({n} entrées)") });
    }

    /// Marque lues les entrées non lues de la liste affichée (ou d'un groupe).
    pub(crate) fn mark_read(&mut self, ids: Vec<String>) {
        let unread: Vec<String> = ids
            .into_iter()
            .filter(|id| self.st.inbox.items.get(id).is_some_and(|i| self.st.inbox.status(i, now()) == Status::Unread))
            .collect();
        if unread.is_empty() {
            return;
        }
        let n = unread.len();
        self.set_status_many(&unread, Status::Read, None);
        if n > 1 {
            let undo = self.shortcuts.label(Command::Undo);
            self.toast(format!("{n} entrées marquées lues ({undo} pour annuler)"));
        }
    }

    pub(crate) fn open(&mut self, id: &str) {
        let Some(url) = self.st.inbox.items.get(id).map(|i| i.url.clone()) else { return };
        match crate::actions::open_url(&url) {
            Ok(()) => {
                if self.st.inbox.items.get(id).is_some_and(|i| self.st.inbox.status(i, now()) == Status::Unread) {
                    // Ouverte maintenant : ce qui s'est passé avant est vu.
                    self.set_status_many(&[id.to_string()], Status::Read, Some(now()));
                }
            }
            Err(e) => self.toast(e),
        }
    }

    pub(crate) fn mute_repo(&mut self, repo: &str) {
        self.st.settings.rules.muted_repos.insert(repo.to_string());
        if self.selected.as_ref().and_then(|id| self.st.inbox.items.get(id)).is_some_and(|i| i.repo == repo) {
            self.select(None);
        }
        self.changed();
        self.toast(format!("Dépôt ignoré : {repo} (réglages pour le rétablir)"));
    }

    pub(crate) fn run(&mut self, ctx: &egui::Context, cmd: Command) {
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
            Command::MarkAllRead => {
                let ids = self.visible_ids();
                self.mark_read(ids);
            }
            Command::Undo => self.undo_last(),
            Command::Find => self.focus_search = true,
            Command::ToggleGroupBy => {
                self.st.settings.group_by = match self.st.settings.group_by {
                    GroupBy::Repo => GroupBy::Kind,
                    GroupBy::Kind => GroupBy::Repo,
                };
                self.changed();
            }
            Command::WaitingOnly => {
                self.st.view.waiting_only = !self.st.view.waiting_only;
                self.changed();
            }
            Command::Palette => {
                self.palette = if self.palette.is_some() { None } else { Some((String::new(), 0)) };
            }
            Command::Accounts => self.toggle_accounts(),
            Command::Settings => self.toggle_settings(),
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.capturing.is_some() {
            return;
        }
        let modal = self.palette.is_some() || self.draft.is_some() || self.fawkes_dialog.is_some();
        let focus = if modal {
            Focus::Modal
        } else if ctx.text_edit_focused() {
            Focus::Text
        } else if ctx.memory(|m| m.focused().is_some()) {
            Focus::Widget
        } else {
            Focus::None
        };
        let cmds = ctx.input_mut(|i| self.shortcuts.take_commands(&mut i.events, focus));
        for cmd in cmds {
            if !modal || cmd == Command::Palette && self.palette.is_some() {
                self.run(ctx, cmd);
            }
        }
    }

    // --- Demandes reçues, publication, fenêtre -------------------------------------------------------

    fn poll_incoming(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.incoming else { return };
        let requests: Vec<Incoming> = rx.try_iter().collect();
        for req in requests {
            match req {
                Incoming::Show { kind } => {
                    if let Some(k) = kind {
                        self.st.view.kinds = [k].into();
                        self.st.view.show_hidden = false;
                        self.changed();
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    #[cfg(windows)]
                    crate::win::bring_to_front();
                }
                Incoming::Refresh => self.refresh(ctx),
                Incoming::Status { .. } => {}
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn snapshot_for_tests(&self) -> Snapshot {
        self.snapshot.lock().unwrap().clone()
    }

    /// Tout est-il enregistré ?
    #[cfg(test)]
    pub(crate) fn is_saved(&self) -> bool {
        !self.dirty
    }

    /// Compteurs publiés pour le canal (`etat`) et le titre de la fenêtre.
    fn publish(&mut self, ctx: &egui::Context) {
        if self.published == self.revision {
            return;
        }
        self.published = self.revision;
        let now = now();
        let rules = self.rules().clone();
        let mut snap = Snapshot::default();
        for item in self.st.inbox.items.values() {
            if !rules.shows(item) || self.st.inbox.status(item, now) != Status::Unread {
                continue;
            }
            snap.waiting += 1;
            *snap.by_kind.entry(item.kind).or_default() += 1;
            if item.kind == Kind::ReviewRequested {
                let key = crate::localrepo::item_key(item, &self.host_of(item));
                *snap.reviews_by_repo.entry(key).or_default() += 1;
            }
        }
        let waiting = snap.waiting;
        if let Ok(mut s) = self.snapshot.lock() {
            *s = snap;
        }
        if self.title_shown != Some(waiting) {
            self.title_shown = Some(waiting);
            let title = if waiting == 0 { "Owlery".to_string() } else { format!("Owlery — {waiting} en attente") };
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
    }

    /// Taille, position et agrandissement gardés pour le prochain lancement.
    fn track_window(&mut self, ctx: &egui::Context) {
        let (inner, outer, maximized, minimized, focused) = ctx.input(|i| {
            let v = i.viewport();
            (v.inner_rect, v.outer_rect, v.maximized.unwrap_or(false), v.minimized.unwrap_or(false), v.focused)
        });
        if let Some(f) = focused {
            if f && !self.focused {
                // De retour dans Owlery : la notification n'a plus lieu d'être.
                self.notifier.hide();
            }
            self.focused = f;
        }
        if minimized || self.hosted {
            return;
        }
        let mut w = self.st.window.clone();
        w.maximized = maximized;
        if !maximized {
            if let Some(r) = inner {
                w.size = [r.width().round(), r.height().round()];
            }
            if let Some(r) = outer {
                w.pos = Some([r.min.x.round(), r.min.y.round()]);
            }
        }
        if w != self.st.window && w.size[0] > 100.0 {
            self.st.window = w;
            self.dirty = true;
        }
    }

    /// Enregistre l'état s'il a changé : au plus une fois par seconde, et après un échec une nouvelle tentative
    /// toutes les 30 s (un seul bandeau, pas un message par essai).
    fn save(&mut self, force: bool) {
        if !self.dirty || !self.writable {
            return;
        }
        let wait = if self.save_error.is_some() { Duration::from_secs(30) } else { Duration::from_secs(1) };
        if !force && self.last_save.is_some_and(|t| t.elapsed() < wait) {
            return;
        }
        self.last_save = Some(Instant::now());
        self.st.settings.shortcuts = self.shortcuts.to_settings();
        match state::save(&self.dir, &self.st) {
            Ok(()) => {
                self.dirty = false;
                self.save_error = None;
                // L'état porte maintenant le compte repris : l'ancienne entrée du coffre peut partir.
                if std::mem::take(&mut self.forget_first_draft)
                    && let Err(e) = forges::forget_first_draft_token(self.store.as_ref())
                {
                    self.problems.push(format!("Ancien jeton du premier jet non supprimé du coffre : {e}"));
                }
            }
            Err(e) => {
                self.save_error = Some(format!("Enregistrement de l'état impossible ({e}) : nouvel essai dans 30 s."))
            }
        }
    }

    /// Fenêtre principale (Windows), pour la ramener au premier plan depuis le canal ou une notification.
    fn learn_window(&mut self, frame: &eframe::Frame) {
        if self.window_known {
            return;
        }
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = frame.window_handle() else { return };
        self.window_known = true;
        #[cfg(windows)]
        if let RawWindowHandle::Win32(h) = handle.as_raw() {
            crate::win::set_main_window(h.hwnd.get());
        }
        #[cfg(not(windows))]
        let _ = handle;
    }
}

pub(crate) fn apply_theme(ctx: &egui::Context, theme: ThemeChoice) {
    ctx.set_theme(match theme {
        ThemeChoice::System => egui::ThemePreference::System,
        ThemeChoice::Dark => egui::ThemePreference::Dark,
        ThemeChoice::Light => egui::ThemePreference::Light,
    });
}

/// Fenêtre à la taille et à la position gardées.
pub fn viewport(window: &WindowState) -> egui::ViewportBuilder {
    let size = crate::icon::owl(64);
    let mut v = egui::ViewportBuilder::default()
        .with_title("Owlery")
        .with_app_id("owlery")
        .with_inner_size(window.size)
        .with_min_inner_size([640.0, 360.0])
        .with_maximized(window.maximized)
        .with_icon(egui::IconData { rgba: size, width: 64, height: 64 });
    if let Some(pos) = window.pos {
        v = v.with_position(pos);
    }
    v
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.learn_window(frame);
        self.sync_suite(ctx);
        self.poll_fetches(ctx);
        self.poll_scan();
        self.poll_fawkes(ctx);
        self.poll_incoming(ctx);
        self.ensure_comments(ctx);
        self.due_snoozes();
        self.auto_refresh(ctx);
        self.track_window(ctx);
        self.publish(ctx);
        crate::capture::poll(ctx, self.capture.as_ref());
        self.save(false);
        // Réveil léger : lecture périodique, fin des reports, demandes du canal.
        ctx.request_repaint_after(Duration::from_secs(if self.incoming.is_some() { 1 } else { 30 }));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        crate::ui::draw(self, ui);
        self.save(false);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save(true);
        self.notifier.hide();
    }
}
