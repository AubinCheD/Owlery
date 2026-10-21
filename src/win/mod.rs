//! Ce qui est propre à Windows : canal nommé du contrat local, instance unique, zone de notification,
//! premier plan.

pub mod pipe;
pub mod tray;

use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND};
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsIconic, MB_ICONINFORMATION, MB_OK, MessageBoxW, SW_RESTORE, SetForegroundWindow, ShowWindow,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Verrou d'instance unique : faux si une autre instance le tient déjà. Le verrou dure jusqu'à la fin du
/// processus.
pub fn single_instance(name: &str) -> bool {
    unsafe {
        let w = wide(name);
        let handle = CreateMutexW(std::ptr::null(), 0, w.as_ptr());
        !(handle.is_null() || GetLastError() == ERROR_ALREADY_EXISTS)
    }
}

/// Fenêtre principale d'Owlery, connue dès la première image.
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);

pub fn set_main_window(hwnd: isize) {
    MAIN_WINDOW.store(hwnd, Ordering::Relaxed);
}

/// Ramène la fenêtre principale au premier plan (restaurée si elle est réduite).
pub fn bring_to_front() {
    let hwnd = MAIN_WINDOW.load(Ordering::Relaxed) as HWND;
    if hwnd.is_null() {
        return;
    }
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
}

/// Message d'information (avant que la fenêtre n'existe, par exemple une seconde instance).
pub fn message_box(text: &str) {
    let (t, c) = (wide(text), wide("Owlery"));
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), MB_OK | MB_ICONINFORMATION) };
}

/// Écrire dans la console qui a lancé Owlery (version publiée, sans console à elle).
pub fn attach_parent_console() {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// La position enregistrée tombe-t-elle sur un écran encore branché ? (sinon la fenêtre est centrée) La position
/// est en points logiques : on essaie les facteurs d'échelle courants, comme Fawkes.
pub fn on_some_monitor(pos: [f32; 2]) -> bool {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint};
    [1.0f32, 1.25, 1.5, 1.75, 2.0].iter().any(|s| {
        let pt = POINT { x: ((pos[0] + 60.0) * s) as i32, y: ((pos[1] + 12.0) * s) as i32 };
        unsafe { !MonitorFromPoint(pt, MONITOR_DEFAULTTONULL).is_null() }
    })
}

/// *Corriger avec Claude* : Fawkes par son canal (`\.\pipe\fawkes-<SID>`).
pub struct PipeFawkes {
    pub name: String,
}

impl crate::app::FawkesLink for PipeFawkes {
    fn available(&self) -> bool {
        pipe::exists(&self.name)
    }

    fn send(&self, line: &str) -> Result<String, String> {
        pipe::request(&self.name, line, std::time::Duration::from_secs(10)).map_err(|e| e.to_string())
    }

    fn find_exe(&self, configured: &str) -> Option<std::path::PathBuf> {
        let configured = configured.trim();
        if !configured.is_empty() {
            return Some(std::path::PathBuf::from(configured)).filter(|p| p.is_file());
        }
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path).map(|d| d.join("fawkes.exe")).find(|p| p.is_file())
    }

    fn start(&self, exe: &std::path::Path) -> Result<(), String> {
        std::process::Command::new(exe).spawn().map(|_| ()).map_err(|e| format!("lancement de Fawkes impossible : {e}"))
    }
}

/// Notifications par la zone de notification.
pub struct TrayNotifier;

impl crate::app::Notifier for TrayNotifier {
    fn show(&self, title: &str, body: &str) {
        tray::show(title, body);
    }

    fn hide(&self) {
        tray::hide();
    }
}
