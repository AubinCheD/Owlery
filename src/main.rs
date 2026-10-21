// Pas de console sous Windows en version publiée.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex, mpsc};

use owlery::app::{self, App, StartOptions};
use owlery::contract::{self, Snapshot};
use owlery::forges::{Fetcher, ForgeFetcher, http::UreqHttp};
use owlery::secrets::{MemoryStore, OsStore, SecretStore};
use owlery::state;

fn main() -> eframe::Result {
    if std::env::args().skip(1).any(|a| a == "--contract-version") {
        #[cfg(windows)]
        owlery::win::attach_parent_console();
        println!("{}", contract::VERSION);
        return Ok(());
    }
    let demo = std::env::var_os("OWLERY_DEMO").is_some_and(|v| v == "1");
    // La démonstration n'écrit jamais dans l'état réel.
    let dir = if demo { state::data_dir().join("demo") } else { state::data_dir() };
    let scope = (dir != state::default_data_dir()).then_some(dir.as_path());

    #[cfg(windows)]
    let sid = owlery::win::pipe::current_user_sid().unwrap_or_default();
    #[cfg(windows)]
    let pipe = contract::pipe_name("owlery", &sid, scope);
    #[cfg(windows)]
    if !owlery::win::single_instance(&contract::instance_name(scope)) {
        // Owlery tourne déjà sur ce dossier d'état : on lui demande de se montrer, et on s'arrête.
        let shown =
            owlery::win::pipe::request(&pipe, r#"{"v":1,"demande":"afficher"}"#, std::time::Duration::from_secs(3));
        if shown.is_err() {
            owlery::win::message_box("Owlery est déjà ouvert.");
        }
        return Ok(());
    }

    let lock = state::lock(&dir);
    let mut loaded = state::load(&dir);
    let writable = match &lock {
        Ok(_) => loaded.writable,
        Err(e) => {
            loaded
                .problems
                .push(format!("Dossier d'état verrouillé ({e}) : rien ne sera enregistré pendant cette session."));
            false
        }
    };

    let snapshot = Arc::new(Mutex::new(Snapshot::default()));
    let (incoming_tx, incoming_rx) = mpsc::channel();
    #[cfg(windows)]
    {
        let snap = snapshot.clone();
        let served = owlery::win::pipe::serve(&pipe, move |line| {
            let current = snap.lock().map(|s| s.clone()).unwrap_or_default();
            let (reply, forward) = contract::handle(line, &current);
            if let Some(req) = forward {
                let _ = incoming_tx.send(req);
            }
            reply
        });
        if let Err(e) = served {
            loaded.problems.push(format!(
                "Canal local indisponible ({e}) : les autres applications ne pourront pas joindre Owlery."
            ));
        }
    }
    #[cfg(not(windows))]
    drop(incoming_tx);

    let store: Arc<dyn SecretStore> = if demo { Arc::new(MemoryStore::default()) } else { Arc::new(OsStore) };
    let fetcher: Arc<dyn Fetcher> = if demo {
        Arc::new(owlery::demo::DemoFetcher)
    } else {
        Arc::new(ForgeFetcher::new(store.clone(), Arc::new(UreqHttp::default())))
    };
    #[cfg(windows)]
    let (fawkes, notifier): (Arc<dyn app::FawkesLink>, Arc<dyn app::Notifier>) = (
        Arc::new(owlery::win::PipeFawkes { name: contract::pipe_name("fawkes", &sid, None) }),
        if owlery::win::tray::start() { Arc::new(owlery::win::TrayNotifier) } else { Arc::new(app::NoNotifier) },
    );
    #[cfg(not(windows))]
    let (fawkes, notifier): (Arc<dyn app::FawkesLink>, Arc<dyn app::Notifier>) =
        (Arc::new(app::NoFawkes), Arc::new(app::NoNotifier));

    let mut window = loaded.state.window.clone();
    #[cfg(windows)]
    if window.pos.is_some_and(|p| !owlery::win::on_some_monitor(p)) {
        window.pos = None;
    }
    let options = eframe::NativeOptions {
        viewport: app::viewport(&window),
        centered: window.pos.is_none(),
        ..Default::default()
    };
    let opts = StartOptions {
        dir,
        store,
        fetcher,
        demo,
        capture: owlery::capture::target(),
        incoming: Some(incoming_rx),
        snapshot,
        fawkes,
        notifier,
        writable,
        fetch_on_start: true,
    };
    eframe::run_native(
        "Owlery",
        options,
        Box::new(move |cc| {
            // Le verrou de l'état vit autant que l'application.
            let _keep = &lock;
            Ok(Box::new(App::new(cc, opts, loaded)))
        }),
    )
}
