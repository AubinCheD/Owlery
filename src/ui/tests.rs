//! Tests d'interface (egui_kittest) : l'application complète, avec des forges, un coffre et un Fawkes factices.
//! Les lectures tournent en arrière-plan : on fait avancer l'interface jusqu'à la condition attendue.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::{AccountDraft, App, FawkesLink, StartOptions};
use crate::demo::{self, DemoFetcher};
use crate::forges::{Account, FetchError, Fetched, Fetcher, Identity, Seen};
use crate::model::{Comment, Forge, Item, Kind, Status, Target};
use crate::secrets::{MemoryStore, SecretStore};
use crate::state::{self, AppState, Loaded};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("owlery-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Fawkes factice : présent ou non, il garde les demandes reçues.
#[derive(Default)]
struct FakeFawkes {
    up: bool,
    sent: Mutex<Vec<String>>,
    /// Réponse de Fawkes (par défaut : acceptée).
    reply: Option<String>,
}

impl FawkesLink for FakeFawkes {
    fn available(&self) -> bool {
        self.up
    }
    fn send(&self, line: &str) -> Result<String, String> {
        self.sent.lock().unwrap().push(line.to_string());
        Ok(self.reply.clone().unwrap_or_else(|| r#"{"v":1,"ok":true}"#.into()))
    }
    fn find_exe(&self, _configured: &str) -> Option<PathBuf> {
        Some(PathBuf::from("fawkes.exe"))
    }
    fn start(&self, _exe: &std::path::Path) -> Result<(), String> {
        Ok(())
    }
}

struct Setup {
    dir: PathBuf,
    state: AppState,
    store: Arc<MemoryStore>,
    fawkes: Arc<FakeFawkes>,
    fetcher: Arc<dyn Fetcher>,
    fetch: bool,
    writable: bool,
}

impl Setup {
    /// Les comptes et entrées de la démonstration, lus au démarrage.
    fn demo(name: &str) -> Self {
        let mut state = AppState::default();
        state.settings.accounts = demo::accounts();
        Setup {
            dir: temp(name),
            state,
            store: Arc::default(),
            fawkes: Arc::default(),
            fetcher: Arc::new(DemoFetcher),
            fetch: true,
            writable: true,
        }
    }

    fn empty(name: &str) -> Self {
        Setup {
            dir: temp(name),
            state: AppState::default(),
            store: Arc::default(),
            fawkes: Arc::default(),
            fetcher: Arc::new(DemoFetcher),
            fetch: false,
            writable: true,
        }
    }

    fn start(self) -> Harness<'static, App> {
        let mut opts = StartOptions::for_tests(self.dir, self.store, self.fetcher);
        opts.fawkes = self.fawkes;
        opts.fetch_on_start = self.fetch;
        opts.writable = self.writable;
        let loaded = Loaded { state: self.state, problems: Vec::new(), writable: self.writable };
        let mut h = Harness::builder().with_size([1300.0, 850.0]).build_eframe(move |cc| App::new(cc, opts, loaded));
        h.step();
        h
    }
}

fn wait_until(h: &mut Harness<'static, App>, what: &str, cond: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond(h.state()) {
        assert!(Instant::now() < deadline, "attente dépassée : {what}");
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    h.step();
}

fn loaded(h: &mut Harness<'static, App>) {
    wait_until(h, "entrées lues", |a| a.st.inbox.items.len() == 7 && a.running.is_empty());
}

fn id_of(app: &App, number: u64) -> String {
    app.st.inbox.items.values().find(|i| i.number == Some(number)).unwrap().id.clone()
}

#[test]
fn no_account_shows_the_welcome_and_opens_accounts() {
    let mut h = Setup::empty("accueil").start();
    h.get_by_label("Aucun compte pour l'instant");
    h.get_by_label_contains("Ajouter un compte").click();
    h.step();
    h.step();
    assert!(h.state().accounts_open);
    h.get_by_label("➕ Ajouter un compte");
}

#[test]
fn clicking_a_title_selects_its_row() {
    let mut h = Setup::demo("clic").start();
    loaded(&mut h);
    h.get_by_label_contains("Corrige l'export CSV").click();
    h.step();
    h.step();
    let id = id_of(h.state(), 138);
    assert_eq!(h.state().selected.as_deref(), Some(id.as_str()));
    // Le détail montre la vérification en échec.
    h.get_by_label("✖ tests (windows)");
}

#[test]
fn keyboard_done_then_undo() {
    let mut h = Setup::demo("clavier").start();
    loaded(&mut h);
    h.key_press(Key::ArrowDown);
    h.step();
    let first = h.state().selected.clone().expect("une entrée choisie au clavier");
    h.key_press(Key::E);
    h.step();
    assert_eq!(h.state().st.inbox.marks[&first].status, Status::Done);
    assert_ne!(h.state().selected.as_deref(), Some(first.as_str()), "la sélection passe à la voisine");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.step();
    assert!(!h.state().st.inbox.marks.contains_key(&first));
}

#[test]
fn mark_all_read_then_undo() {
    let mut h = Setup::demo("tout-lu").start();
    loaded(&mut h);
    let waiting = |a: &App| a.st.inbox.waiting(&a.st.settings.rules, crate::app::now());
    assert_eq!(waiting(h.state()), 6, "« Ma PR » arrive lue");
    h.key_press_modifiers(Modifiers::SHIFT, Key::R);
    h.step();
    assert_eq!(waiting(h.state()), 0);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.step();
    assert_eq!(waiting(h.state()), 6);
}

#[test]
fn muted_repositories_and_bots_leave_the_list_and_the_count() {
    let mut h = Setup::demo("ignorer").start();
    loaded(&mut h);
    h.state_mut().mute_repo("northwind/app");
    h.state_mut().st.settings.rules.hide_bots = true;
    h.step();
    let a = h.state();
    assert_eq!(a.st.inbox.waiting(&a.st.settings.rules, crate::app::now()), 4);
    assert!(h.query_by_label_contains("Erreur à l'export").is_none());
    assert!(h.query_by_label_contains("Bump serde").is_none());
}

#[test]
fn adding_an_account_verifies_then_stores_the_token() {
    let setup = Setup::empty("compte");
    let store = setup.store.clone();
    let mut h = setup.start();
    h.state_mut().accounts_open = true;
    h.state_mut().draft = Some(AccountDraft {
        forge: Forge::GitLab,
        host: "gitlab.contoso.test".into(),
        orgs: String::new(),
        token: " glpat-fictif ".into(),
        use_gh: false,
        checking: None,
        error: None,
    });
    h.run_steps(4);
    h.get_by_label("Vérifier et ajouter").click();
    wait_until(&mut h, "compte ajouté", |a| !a.st.settings.accounts.is_empty());
    let account = h.state().st.settings.accounts[0].clone();
    assert_eq!(account.forge, Forge::GitLab);
    assert_eq!(account.host, "gitlab.contoso.test");
    assert_eq!(account.username, "camille");
    assert_eq!(store.get(&account.token_key()).unwrap().as_deref(), Some("glpat-fictif"));
    assert!(h.state().draft.is_none(), "le jeton saisi ne reste pas en mémoire");
}

#[test]
fn removing_an_account_deletes_its_token_and_entries() {
    let setup = Setup::demo("retrait");
    let store = setup.store.clone();
    store.set(&format!("account:{}", demo::GITLAB), "glpat-fictif").unwrap();
    let mut h = setup.start();
    loaded(&mut h);
    h.state_mut().accounts_open = true;
    h.run_steps(4);
    // Les lignes du tableau : GitHub, GitLab, Azure ; le deuxième bouton « Retirer » est celui de GitLab.
    let buttons: Vec<_> = h.get_all_by_label("Retirer").collect();
    buttons[1].click();
    h.step();
    h.step();
    let a = h.state();
    assert!(a.st.settings.accounts.iter().all(|x| x.id != demo::GITLAB));
    assert!(a.st.inbox.items.values().all(|i| i.account != demo::GITLAB));
    assert_eq!(store.get(&format!("account:{}", demo::GITLAB)).unwrap(), None);
}

#[test]
fn fawkes_absent_opens_the_dialog_and_copy_closes_it() {
    let mut setup = Setup::demo("fawkes-absent");
    setup.state.settings.repo_dirs.insert("contoso/api".into(), setup.dir.display().to_string());
    let mut h = setup.start();
    loaded(&mut h);
    let id = id_of(h.state(), 138);
    h.state_mut().select(Some(id));
    h.step();
    h.get_by_label("✨ Corriger avec Claude").click();
    h.step();
    h.step();
    assert!(h.state().fawkes_dialog.is_some());
    h.get_by_label("Copier le contexte").click();
    h.step();
    h.step();
    assert!(h.state().fawkes_dialog.is_none());
    assert!(h.state().toasts.iter().any(|(t, _)| t.contains("rien n'a été envoyé")));
}

#[test]
fn fawkes_present_receives_the_request() {
    let mut setup = Setup::demo("fawkes-present");
    setup.state.settings.repo_dirs.insert("contoso/api".into(), setup.dir.display().to_string());
    setup.fawkes = Arc::new(FakeFawkes { up: true, ..FakeFawkes::default() });
    let fawkes = setup.fawkes.clone();
    let mut h = setup.start();
    loaded(&mut h);
    let id = id_of(h.state(), 138);
    h.state_mut().select(Some(id));
    h.step();
    h.get_by_label("✨ Corriger avec Claude").click();
    wait_until(&mut h, "réponse de Fawkes", |a| {
        a.fawkes_job.is_none() && a.toasts.iter().any(|(t, _)| t.contains("Fawkes"))
    });
    let sent = fawkes.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    let v: serde_json::Value = serde_json::from_str(&sent[0]).unwrap();
    assert_eq!(v["demande"], "ouvrir");
    assert_eq!(v["worktree"], "export-csv");
    assert!(v["consigne"].as_str().unwrap().contains("CI en échec"));
}

#[test]
fn choices_survive_a_restart() {
    let setup = Setup::demo("redemarrage");
    let dir = setup.dir.clone();
    let mut h = setup.start();
    loaded(&mut h);
    let id = id_of(h.state(), 57);
    h.state_mut().set_status(&id, Status::Done);
    h.state_mut().st.view.waiting_only = true;
    h.state_mut().changed();
    wait_until(&mut h, "état enregistré", App::is_saved);
    drop(h);
    let back = state::load(&dir);
    assert_eq!(back.state.inbox.marks[&id].status, Status::Done);
    assert!(back.state.view.waiting_only);
    assert_eq!(back.state.settings.accounts.len(), 3);
}

#[test]
fn the_snapshot_counts_reviews_by_repository() {
    let mut h = Setup::demo("etat").start();
    loaded(&mut h);
    h.step();
    let snap = h.state().snapshot_for_tests();
    assert_eq!(snap.waiting, 6);
    assert_eq!(snap.reviews_by_repo.get("github.com/contoso/api"), Some(&1));
    assert_eq!(snap.reviews_by_repo.get("dev.azure.com/fabrikam/ventes web/api"), Some(&1));
}

#[test]
fn the_palette_filters_on_a_view() {
    let mut h = Setup::demo("palette").start();
    loaded(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.step();
    assert!(h.state().palette.is_some());
    h.get_all_by_label("CI en échec").last().expect("entrée de la palette").click();
    h.step();
    h.step();
    assert!(h.state().palette.is_none());
    assert_eq!(h.state().st.view.kinds, [crate::model::Kind::CiFailed].into());
}

/// Forge factice programmable : des réponses dans l'ordre (la dernière se répète), un délai, ou une panique.
#[derive(Default)]
struct Scripted {
    replies: Mutex<Vec<Result<Fetched, FetchError>>>,
    calls: std::sync::atomic::AtomicUsize,
    delay: Duration,
    panics: bool,
}

impl Scripted {
    fn new(replies: Vec<Result<Fetched, FetchError>>) -> Self {
        Scripted { replies: Mutex::new(replies), ..Scripted::default() }
    }
}

impl Fetcher for Scripted {
    fn gh_available(&self) -> bool {
        false
    }
    fn verify(&self, _account: &Account, _token: &str) -> Result<Identity, FetchError> {
        Ok(Identity::default())
    }
    fn fetch(&self, account: &Account, _seen: &Seen) -> Result<Fetched, FetchError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert!(!self.panics, "panique voulue dans la lecture");
        std::thread::sleep(self.delay);
        let mut replies = self.replies.lock().unwrap();
        let reply = if replies.len() > 1 { replies.remove(0) } else { replies[0].clone() };
        reply.map(|mut f| {
            f.items.retain(|i| i.account == account.id);
            f
        })
    }
    fn comments(&self, _account: &Account, _item: &Item) -> Result<Vec<Comment>, FetchError> {
        Ok(Vec::new())
    }
}

fn demo_fetched() -> Fetched {
    Fetched { items: demo::items(), ..Fetched::default() }
}

fn calls(f: &Scripted) -> usize {
    f.calls.load(std::sync::atomic::Ordering::SeqCst)
}

#[test]
fn fetch_thread_panic_ends_the_wait() {
    let mut setup = Setup::demo("panique");
    setup.fetcher = Arc::new(Scripted { panics: true, ..Scripted::default() });
    let mut h = setup.start();
    wait_until(&mut h, "fin de la lecture", |a| a.running.is_empty());
    let a = h.state();
    assert!(a.status.values().all(|s| s.error.as_ref().is_some_and(|e| e.to_string().contains("interrompue"))));
    h.get_by_label("La lecture a échoué");
}

#[test]
fn a_refresh_asked_during_a_read_follows_it() {
    let mut setup = Setup::demo("pendant");
    let fetcher = Arc::new(Scripted { delay: Duration::from_millis(200), ..Scripted::new(vec![Ok(demo_fetched())]) });
    setup.fetcher = fetcher.clone();
    let mut h = setup.start();
    assert!(!h.state().running.is_empty(), "la lecture du démarrage est en cours");
    // Un jeton enregistré pendant la lecture demande une nouvelle lecture : elle suit la première.
    let ctx = h.ctx.clone();
    h.state_mut().refresh(&ctx);
    wait_until(&mut h, "seconde lecture", |a| a.running.is_empty() && calls(&fetcher) >= 6);
    assert_eq!(calls(&fetcher), 6, "3 comptes, deux fois");
}

#[test]
fn a_failed_account_keeps_its_cache() {
    let mut setup = Setup::demo("echec");
    let ok = || Ok(demo_fetched());
    let fetcher = Arc::new(Scripted::new(vec![ok(), ok(), ok(), Err(FetchError::Unauthorized)]));
    setup.fetcher = fetcher.clone();
    let mut h = setup.start();
    loaded(&mut h);
    let ctx = h.ctx.clone();
    h.state_mut().refresh(&ctx);
    wait_until(&mut h, "lecture en échec", |a| a.running.is_empty() && a.status.values().all(|s| s.error.is_some()));
    assert_eq!(h.state().st.inbox.items.len(), 7, "rien n'est effacé");
    assert!(super::status_summary(h.state()).2, "l'échec est dit");
}

#[test]
fn an_error_without_cache_is_not_an_empty_inbox() {
    let mut setup = Setup::demo("erreur-vide");
    setup.fetcher = Arc::new(Scripted::new(vec![Err(FetchError::Network("hôte injoignable".into()))]));
    let mut h = setup.start();
    wait_until(&mut h, "fin de la lecture", |a| a.running.is_empty() && !a.status.is_empty());
    h.get_by_label("La lecture a échoué");
    assert!(h.query_by_label("Rien ne vous attend 🎉").is_none());
}

#[test]
fn selection_scrolls_with_the_keyboard() {
    let mut setup = Setup::demo("defilement");
    let many: Vec<Item> = (1..=80)
        .map(|n| Item {
            id: Item::make_id(Forge::GitHub, "", "contoso/api", Some(n), false),
            account: demo::GITHUB.into(),
            kind: Kind::Mention,
            repo: "contoso/api".into(),
            number: Some(n),
            title: format!("Entrée numéro {n}"),
            url: format!("https://github.com/contoso/api/issues/{n}"),
            updated_at: 1_000_000 - n as i64,
            target: Target::Issue,
            ..Item::default()
        })
        .collect();
    setup.fetcher = Arc::new(Scripted::new(vec![Ok(Fetched { items: many, ..Fetched::default() })]));
    let mut h = setup.start();
    wait_until(&mut h, "entrées lues", |a| a.st.inbox.items.len() == 80 && a.running.is_empty());
    for _ in 0..70 {
        h.key_press(Key::ArrowDown);
        h.step();
    }
    h.run_steps(3);
    let selected = h.state().st.inbox.items[h.state().selected.as_ref().unwrap()].title.clone();
    assert_eq!(selected, "Entrée numéro 70");
    // La ligne de la liste (à gauche ; le titre du détail est à droite).
    let rect = h
        .get_all_by_label_contains("Entrée numéro 70")
        .map(|n| n.rect())
        .find(|r| r.min.x < 800.0)
        .expect("la ligne de la liste");
    assert!(rect.min.y > 0.0 && rect.max.y < 850.0, "la ligne choisie est à l'écran : {rect:?}");
}

#[test]
fn fawkes_error_is_shown_and_the_context_copied() {
    let mut setup = Setup::demo("fawkes-refus");
    setup.state.settings.repo_dirs.insert("contoso/api".into(), setup.dir.display().to_string());
    setup.fawkes = Arc::new(FakeFawkes {
        up: true,
        reply: Some(r#"{"v":1,"ok":false,"erreur":"branche inconnue"}"#.into()),
        ..FakeFawkes::default()
    });
    let mut h = setup.start();
    loaded(&mut h);
    let id = id_of(h.state(), 138);
    h.state_mut().select(Some(id));
    h.step();
    h.get_by_label("✨ Corriger avec Claude").click();
    wait_until(&mut h, "réponse de Fawkes", |a| a.fawkes_job.is_none() && !a.toasts.is_empty());
    assert!(h.state().toasts.iter().any(|(t, _)| t.contains("branche inconnue") && t.contains("Contexte copié")));
}

#[test]
fn the_first_draft_token_is_taken_over_then_forgotten_after_saving() {
    let setup = Setup::empty("jeton-0-1");
    let store = setup.store.clone();
    store.set(crate::forges::FIRST_DRAFT_TOKEN, "ghp_fictif").unwrap();
    let mut h = setup.start();
    assert_eq!(h.state().st.settings.accounts.len(), 1);
    wait_until(&mut h, "état enregistré", App::is_saved);
    assert_eq!(store.get(crate::forges::FIRST_DRAFT_TOKEN).unwrap(), None);
    let account = &h.state().st.settings.accounts[0];
    assert_eq!(store.get(&account.token_key()).unwrap().as_deref(), Some("ghp_fictif"));
}

#[test]
fn nothing_is_taken_over_when_the_state_cannot_be_saved() {
    let mut setup = Setup::empty("jeton-lecture-seule");
    setup.writable = false;
    let store = setup.store.clone();
    store.set(crate::forges::FIRST_DRAFT_TOKEN, "ghp_fictif").unwrap();
    let h = setup.start();
    assert!(h.state().st.settings.accounts.is_empty());
    assert_eq!(store.get(crate::forges::FIRST_DRAFT_TOKEN).unwrap().as_deref(), Some("ghp_fictif"));
}

#[test]
fn a_save_failure_shows_one_banner() {
    let setup = Setup::demo("enregistrement");
    // Un fichier à la place du dossier d'état : l'écriture échoue.
    std::fs::remove_dir_all(&setup.dir).unwrap();
    std::fs::write(&setup.dir, "pas un dossier").unwrap();
    let mut h = setup.start();
    loaded(&mut h);
    h.run_steps(20);
    let a = h.state();
    assert!(a.save_error.as_ref().is_some_and(|e| e.contains("nouvel essai dans 30 s")));
    assert!(a.toasts.iter().all(|(t, _)| !t.contains("Enregistrement")), "pas de toast par essai");
    let _ = std::fs::remove_file(&a.dir);
}

#[test]
fn orphan_entries_leave_when_their_account_is_gone() {
    let mut h = Setup::demo("orphelines").start();
    loaded(&mut h);
    h.state_mut().st.settings.accounts.retain(|a| a.id != demo::GITLAB);
    let ctx = h.ctx.clone();
    h.state_mut().refresh(&ctx);
    wait_until(&mut h, "lecture", |a| a.running.is_empty());
    assert!(h.state().st.inbox.items.values().all(|i| i.account != demo::GITLAB));
}
