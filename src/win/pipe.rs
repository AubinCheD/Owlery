//! Canal nommé réservé au compte (`\\.\pipe\<application>-<SID>`), pour le contrat local : serveur d'Owlery
//! et client vers les autres applications (Fawkes). Une ligne JSON par message.
//!
//! Le tube est créé avec un descripteur de sécurité qui n'autorise que l'utilisateur courant, refuse les
//! clients distants, et ne peut pas être pris par une autre application qui l'aurait créé avant (première
//! instance obligatoire).

use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::sync::mpsc;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_SEM_TIMEOUT, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

use crate::contract::MAX_LINE;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// SID de l'utilisateur courant (`S-1-5-21-…`).
pub fn current_user_sid() -> Option<String> {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        // Tampon aligné pour la structure TOKEN_USER.
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
        CloseHandle(token);
        if ok == 0 {
            return None;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut text: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 || text.is_null() {
            return None;
        }
        let mut n = 0;
        while *text.add(n) != 0 {
            n += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
        LocalFree(text.cast());
        Some(sid)
    }
}

/// Erreur d'une demande à une autre application.
#[derive(Debug, Clone, PartialEq)]
pub enum PipeError {
    /// Personne n'écoute sur ce canal (application fermée, ou trop ancienne).
    Absent,
    /// Pas de réponse dans le délai.
    Timeout,
    Io(String),
}

impl std::fmt::Display for PipeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipeError::Absent => f.write_str("l'application n'écoute pas sur son canal"),
            PipeError::Timeout => f.write_str("pas de réponse"),
            PipeError::Io(m) => f.write_str(m),
        }
    }
}

/// Quelqu'un écoute-t-il sur ce canal ?
pub fn exists(name: &str) -> bool {
    let w = wide(name);
    unsafe { WaitNamedPipeW(w.as_ptr(), 1) != 0 || GetLastError() == ERROR_SEM_TIMEOUT }
}

/// Envoie une ligne et attend la réponse (une ligne), dans le délai donné. Le serveur peut ensuite passer au
/// premier plan (la demande vient d'un geste de l'utilisateur).
pub fn request(name: &str, line: &str, timeout: Duration) -> Result<String, PipeError> {
    let (tx, rx) = mpsc::channel();
    let (name, line) = (name.to_string(), line.to_string());
    std::thread::spawn(move || {
        let _ = tx.send(request_blocking(&name, &line));
    });
    rx.recv_timeout(timeout).unwrap_or(Err(PipeError::Timeout))
}

fn request_blocking(name: &str, line: &str) -> Result<String, PipeError> {
    let mut file = None;
    for _ in 0..3 {
        match std::fs::OpenOptions::new().read(true).write(true).open(name) {
            Ok(f) => {
                file = Some(f);
                break;
            }
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) => return Err(PipeError::Absent),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                let w = wide(name);
                unsafe { WaitNamedPipeW(w.as_ptr(), 2000) };
            }
            Err(e) => return Err(PipeError::Io(e.to_string())),
        }
    }
    let mut file = file.ok_or(PipeError::Timeout)?;
    unsafe {
        let mut pid = 0u32;
        if GetNamedPipeServerProcessId(file.as_raw_handle() as HANDLE, &mut pid) != 0 {
            AllowSetForegroundWindow(pid);
        }
    }
    let mut msg = line.trim_end().to_string();
    msg.push('\n');
    file.write_all(msg.as_bytes()).map_err(|e| PipeError::Io(e.to_string()))?;
    read_line(&mut file).map_err(PipeError::Io)
}

/// Lit jusqu'au premier `\n` (non compris), `MAX_LINE` octets au plus.
fn read_line(r: &mut impl Read) -> Result<String, String> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = r.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if let Some(i) = out.iter().position(|b| *b == b'\n') {
            out.truncate(i);
            break;
        }
        if out.len() > MAX_LINE {
            return Err("message trop long".into());
        }
    }
    String::from_utf8(out).map_err(|_| "message illisible (UTF-8 attendu)".into())
}

/// Ouvre le canal et traite les demandes, une à la fois, dans un fil dédié. Erreur si le canal existe déjà (une
/// autre instance, ou une autre application qui l'aurait pris).
pub fn serve(name: &str, handler: impl Fn(&str) -> String + Send + 'static) -> std::io::Result<()> {
    let sid = current_user_sid().ok_or_else(|| std::io::Error::other("SID de l'utilisateur introuvable"))?;
    let handle = create(name, &sid)?;
    let file = unsafe { File::from_raw_handle(handle as RawHandle) };
    std::thread::Builder::new().name("owlery-canal".into()).spawn(move || {
        loop {
            let h = file.as_raw_handle() as HANDLE;
            let connected =
                unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) != 0 || GetLastError() == ERROR_PIPE_CONNECTED };
            if connected {
                let mut reader = &file;
                let reply = match read_line(&mut reader) {
                    Ok(line) => handler(&line),
                    Err(e) => crate::contract::error_reply(&e),
                };
                let mut writer = &file;
                let _ = writer.write_all(format!("{reply}\n").as_bytes());
                unsafe { FlushFileBuffers(h) };
            }
            unsafe { DisconnectNamedPipe(h) };
        }
    })?;
    Ok(())
}

fn create(name: &str, sid: &str) -> std::io::Result<HANDLE> {
    unsafe {
        // Accès complet pour l'utilisateur courant seulement ; rien pour les autres.
        let sddl = wide(&format!("D:P(A;;GA;;;{sid})"));
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        let w = wide(name);
        let h = CreateNamedPipeW(
            w.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            &sa,
        );
        let err = std::io::Error::last_os_error();
        LocalFree(sd);
        if h == INVALID_HANDLE_VALUE { Err(err) } else { Ok(h) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_name(tag: &str) -> String {
        format!(r"\\.\pipe\owlery-test-{tag}-{}", std::process::id())
    }

    #[test]
    fn the_user_sid_is_found() {
        assert!(current_user_sid().is_some_and(|s| s.starts_with("S-1-")));
    }

    #[test]
    fn pipe_round_trip() {
        let name = test_name("aller-retour");
        assert!(!exists(&name));
        assert_eq!(request(&name, "{}", Duration::from_secs(2)), Err(PipeError::Absent));
        serve(&name, |line| format!("reçu {}", line.len())).unwrap();
        assert!(exists(&name));
        assert_eq!(request(&name, "abc", Duration::from_secs(5)), Ok("reçu 3".into()));
        // Le serveur se remet en écoute après chaque demande.
        assert_eq!(request(&name, "abcdef\n", Duration::from_secs(5)), Ok("reçu 6".into()));
        // Le même canal ne peut pas être ouvert deux fois.
        assert!(serve(&name, |_| String::new()).is_err());
    }
}
