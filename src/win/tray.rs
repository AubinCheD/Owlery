//! Notifications Windows : une icône dans la zone de notification, le temps qu'une notification attend, et sa
//! bulle, que Windows affiche comme une notification. Un clic ramène Owlery au premier plan.
//!
//! L'icône appartient à une fenêtre « message » propre, dans son fil : rien n'est greffé sur la procédure de
//! la fenêtre d'eframe.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_LARGE_ICON, NIIF_RESPECT_QUIET_TIME, NIIF_USER,
    NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONUSERCLICK, NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIcon, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HICON, HWND_MESSAGE, MSG, PostMessageW,
    RegisterClassW, RegisterWindowMessageW, TranslateMessage, WM_APP, WNDCLASSW,
};

/// Message de l'icône vers la fenêtre.
const CALLBACK: u32 = WM_APP + 0x51;
/// Message « commandes en attente ».
const WAKE: u32 = WM_APP + 0x52;
const ICON_ID: u32 = 1;

enum Cmd {
    Show { title: String, body: String },
    Hide,
}

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static QUEUE: Mutex<Vec<Cmd>> = Mutex::new(Vec::new());
/// Message que l'Explorateur diffuse quand il redémarre : les icônes de la zone de notification sont perdues.
static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Démarre le fil de la zone de notification (une fois) ; faux si la fenêtre n'a pas pu être créée.
pub fn start() -> bool {
    if WINDOW.load(Ordering::Relaxed) != 0 {
        return true;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("owlery-notifications".into()).spawn(move || unsafe {
        let class = wide("OwleryNotifications");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: GetModuleHandleW(std::ptr::null()),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let taskbar = wide("TaskbarCreated");
        TASKBAR_CREATED.store(RegisterWindowMessageW(taskbar.as_ptr()), Ordering::Relaxed);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            wc.hInstance,
            std::ptr::null(),
        );
        WINDOW.store(hwnd as isize, Ordering::Relaxed);
        let _ = tx.send(!hwnd.is_null());
        if hwnd.is_null() {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
    spawned.is_ok() && rx.recv().unwrap_or(false)
}

fn post(cmd: Cmd) {
    QUEUE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(cmd);
    let hwnd = WINDOW.load(Ordering::Relaxed) as HWND;
    if !hwnd.is_null() {
        unsafe { PostMessageW(hwnd, WAKE, 0, 0) };
    }
}

/// Affiche une notification (remplace la précédente).
pub fn show(title: &str, body: &str) {
    post(Cmd::Show { title: title.into(), body: body.into() });
}

/// Retire l'icône, et avec elle sa notification si elle est encore là.
pub fn hide() {
    post(Cmd::Hide);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WAKE => {
            let cmds = std::mem::take(&mut *QUEUE.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
            for cmd in cmds {
                unsafe { apply(hwnd, cmd) };
            }
            0
        }
        CALLBACK => {
            // Version 4 : l'événement est dans le mot bas de `lparam`.
            let event = (lparam as u32) & 0xFFFF;
            if event == NIN_BALLOONUSERCLICK || event == NIN_SELECT || event == NIN_SELECT | NINF_KEY {
                // Windows autorise le premier plan juste après le clic de l'utilisateur.
                super::bring_to_front();
                unsafe { apply(hwnd, Cmd::Hide) };
            }
            0
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            // L'Explorateur a redémarré : l'icône n'existe plus, la prochaine notification la recréera.
            SHOWN.store(false, Ordering::Relaxed);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

static SHOWN: AtomicBool = AtomicBool::new(false);

unsafe fn apply(hwnd: HWND, cmd: Cmd) {
    match cmd {
        Cmd::Show { title, body } => unsafe {
            let (small, large) = icons();
            if !SHOWN.load(Ordering::Relaxed) {
                let mut d = data(hwnd);
                d.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
                d.uCallbackMessage = CALLBACK;
                d.hIcon = small;
                copy_wide(&mut d.szTip, "Owlery : de nouvelles entrées vous attendent");
                if Shell_NotifyIconW(NIM_ADD, &d) == 0 {
                    return;
                }
                d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                Shell_NotifyIconW(NIM_SETVERSION, &d);
                SHOWN.store(true, Ordering::Relaxed);
            }
            let mut d = data(hwnd);
            d.uFlags = NIF_INFO;
            copy_wide(&mut d.szInfoTitle, &title);
            copy_wide(&mut d.szInfo, &body);
            d.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON | NIIF_RESPECT_QUIET_TIME;
            d.hBalloonIcon = large;
            Shell_NotifyIconW(NIM_MODIFY, &d);
        },
        Cmd::Hide => {
            if SHOWN.swap(false, Ordering::Relaxed) {
                unsafe { Shell_NotifyIconW(NIM_DELETE, &data(hwnd)) };
            }
        }
    }
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: ICON_ID, ..Default::default() }
}

/// Icônes (petite pour la zone, grande pour la bulle), dessinées une fois.
fn icons() -> (HICON, HICON) {
    static ICONS: std::sync::OnceLock<(isize, isize)> = std::sync::OnceLock::new();
    let (s, l) = *ICONS.get_or_init(|| (make_icon(16) as isize, make_icon(64) as isize));
    (s as HICON, l as HICON)
}

/// HICON 32 bits avec transparence, à partir de la chouette d'`icon`.
fn make_icon(size: u32) -> HICON {
    let rgba = crate::icon::owl(size);
    let mut bgra = rgba.clone();
    for px in bgra.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    // Masque ET à zéro : la transparence vient du canal alpha.
    let mask = vec![0u8; (size as usize).div_ceil(8) * size as usize];
    unsafe {
        CreateIcon(GetModuleHandleW(std::ptr::null()), size as i32, size as i32, 1, 32, mask.as_ptr(), bgra.as_ptr())
    }
}

/// Copie dans un champ de taille fixe de Windows : tronqué au besoin, toujours terminé par un zéro.
fn copy_wide(buf: &mut [u16], s: &str) {
    let mut n = 0;
    for u in s.encode_utf16().take(buf.len() - 1) {
        buf[n] = u;
        n += 1;
    }
    buf[n] = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_is_cut_to_fit() {
        let mut buf = [1u16; 8];
        copy_wide(&mut buf, "abcdefghijkl");
        assert_eq!(&buf, &[97, 98, 99, 100, 101, 102, 103, 0]);
        copy_wide(&mut buf, "ab");
        assert_eq!(&buf[..3], &[97, 98, 0]);
    }

    #[test]
    fn icons_can_be_made() {
        assert!(!make_icon(16).is_null());
        assert!(!make_icon(64).is_null());
    }
}
