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
use crate::model::{Forge, Status};
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
}

impl FawkesLink for FakeFawkes {
    fn available(&self) -> bool {
        self.up
    }
    fn send(&self, line: &str) -> Result<String, String> {
        self.sent.lock().unwrap().push(line.to_string());
        Ok(r#"{"v":1,"ok":true}"#.into())
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
    fetch: bool,
}

impl Setup {
    /// Les comptes et entrées de la démonstration, lus au démarrage.
    fn demo(name: &str) -> Self {
        let mut state = AppState::default();
        state.settings.accounts = demo::accounts();
        Setup { dir: temp(name), state, store: Arc::default(), fawkes: Arc::default(), fetch: true }
    }

    fn empty(name: &str) -> Self {
        Setup {
            dir: temp(name),
            state: AppState::default(),
            store: Arc::default(),
            fawkes: Arc::default(),
            fetch: false,
        }
    }

    fn start(self) -> Harness<'static, App> {
        let mut opts = StartOptions::for_tests(self.dir, self.store, Arc::new(DemoFetcher));
        opts.fawkes = self.fawkes;
        opts.fetch_on_start = self.fetch;
        let loaded = Loaded { state: self.state, problems: Vec::new(), writable: true };
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
