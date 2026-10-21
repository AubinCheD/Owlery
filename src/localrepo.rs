//! Dossiers locaux des dépôts : retrouvés par leurs `remote` dans des dossiers racines choisis dans les
//! réglages, pour *Ouvrir dans DragonTree* et *Corriger avec Claude*.
//!
//! Une clé identifie un dépôt distant quelle que soit la forme de son adresse : `hôte/chemin`, en
//! minuscules, sans port, sans `.git` ; les adresses Azure DevOps (`_git`, ssh `v3`,
//! `*.visualstudio.com`) sont ramenées à `dev.azure.com/<org>/<projet>/<dépôt>`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::model::Item;

/// Dossiers jamais parcourus (builds, dépendances).
const SKIPPED: [&str; 6] = ["target", "node_modules", "bin", "obj", "dist", "build"];

/// Clé d'une adresse de dépôt distant, `None` pour un chemin local ou une adresse illisible.
pub fn parse_remote(url: &str) -> Option<String> {
    let url = url.trim();
    let (host, path) = if let Some(i) = url.find("://") {
        let scheme = &url[..i];
        if !matches!(scheme, "https" | "http" | "ssh" | "git") {
            return None;
        }
        let rest = &url[i + 3..];
        let (authority, path) = rest.split_once('/')?;
        (authority.rsplit('@').next()?.to_string(), path.to_string())
    } else {
        // Forme scp : `git@github.com:contoso/api.git`.
        let (authority, path) = url.split_once(':')?;
        if authority.len() == 1 || authority.contains(['\\', '/']) {
            return None; // `C:\…`, chemin local
        }
        (authority.rsplit('@').next()?.to_string(), path.to_string())
    };
    let host = host.split(':').next()?.to_ascii_lowercase();
    let mut segments: Vec<String> =
        path.split('/').filter(|s| !s.is_empty()).map(|s| percent_decode(s).to_lowercase()).collect();
    if let Some(last) = segments.last_mut()
        && let Some(stripped) = last.strip_suffix(".git")
    {
        *last = stripped.to_string();
    }
    if host.is_empty() || segments.is_empty() {
        return None;
    }
    let azure_ssh = host == "ssh.dev.azure.com" || host == "vs-ssh.visualstudio.com";
    if azure_ssh && segments.first().map(String::as_str) == Some("v3") {
        segments.remove(0);
        return Some(format!("dev.azure.com/{}", segments.join("/")));
    }
    segments.retain(|s| s != "_git");
    if let Some(org) = host.strip_suffix(".visualstudio.com") {
        // `https://fabrikam.visualstudio.com/[DefaultCollection/]Projet/_git/dépôt`.
        if segments.first().map(String::as_str) == Some("defaultcollection") {
            segments.remove(0);
        }
        return Some(format!("dev.azure.com/{org}/{}", segments.join("/")));
    }
    Some(format!("{host}/{}", segments.join("/")))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%' && i + 3 <= bytes.len())
            .then(|| std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()))
            .flatten();
        match hex {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Clé du dépôt d'une entrée lue sur `host`.
pub fn item_key(item: &Item, host: &str) -> String {
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    format!("{host}/{}", item.repo.to_lowercase())
}

/// Adresses des `remote` d'un fichier `.git/config`, `origin` en premier.
pub fn remotes_in_config(config: &str) -> Vec<String> {
    let mut found: Vec<(bool, String)> = Vec::new();
    let mut current: Option<String> = None;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            current = line.strip_prefix("[remote \"").and_then(|r| r.strip_suffix("\"]")).map(str::to_owned);
            continue;
        }
        let Some(remote) = &current else { continue };
        if let Some((k, v)) = line.split_once('=')
            && k.trim().eq_ignore_ascii_case("url")
        {
            found.push((remote == "origin", v.trim().trim_matches('"').to_string()));
        }
    }
    found.sort_by_key(|(origin, _)| !origin);
    found.into_iter().map(|(_, url)| url).collect()
}

/// Parcourt les racines (profondeur `max_depth`) : clé de chaque `remote` → dossier du dépôt. Le premier
/// dossier trouvé (ordre alphabétique, le moins profond d'abord) l'emporte.
pub fn scan(roots: &[PathBuf], max_depth: usize) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    let mut level: Vec<PathBuf> = roots.iter().filter(|r| r.is_dir()).cloned().collect();
    for _ in 0..=max_depth {
        let mut next = Vec::new();
        level.sort();
        for dir in level {
            let git = dir.join(".git");
            if git.is_dir() {
                if let Ok(config) = std::fs::read_to_string(git.join("config")) {
                    for url in remotes_in_config(&config) {
                        if let Some(key) = parse_remote(&url) {
                            out.entry(key).or_insert_with(|| dir.clone());
                        }
                    }
                }
                continue; // pas de dépôt dans un dépôt
            }
            if git.is_file() {
                continue; // worktree ou sous-module : le dépôt principal est trouvé par ailleurs
            }
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_lowercase();
                if name.starts_with('.') || SKIPPED.contains(&name.as_str()) {
                    continue;
                }
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    next.push(e.path());
                }
            }
        }
        level = next;
    }
    out
}

/// Dossier local d'une entrée : le choix manuel (par dépôt) d'abord, puis celui trouvé par le parcours.
pub fn local_dir(
    item: &Item,
    host: &str,
    manual: &BTreeMap<String, String>,
    found: &BTreeMap<String, PathBuf>,
) -> Option<PathBuf> {
    if let Some(d) = manual.get(&item.repo) {
        return Some(PathBuf::from(d));
    }
    found.get(&item_key(item, host)).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_forms_give_the_same_key() {
        for url in [
            "https://github.com/contoso/api.git",
            "https://camille@github.com/Contoso/API",
            "git@github.com:contoso/api.git",
            "ssh://git@github.com:22/contoso/api.git",
        ] {
            assert_eq!(parse_remote(url).as_deref(), Some("github.com/contoso/api"), "{url}");
        }
        assert_eq!(
            parse_remote("https://gitlab.contoso.test:8443/group/sub/proj.git").as_deref(),
            Some("gitlab.contoso.test/group/sub/proj")
        );
    }

    #[test]
    fn azure_forms_give_the_same_key() {
        for url in [
            "https://fabrikam@dev.azure.com/fabrikam/Ventes%20Web/_git/api",
            "https://dev.azure.com/fabrikam/Ventes%20Web/_git/api",
            "git@ssh.dev.azure.com:v3/fabrikam/Ventes%20Web/api",
            "fabrikam@vs-ssh.visualstudio.com:v3/fabrikam/Ventes%20Web/api",
            "https://fabrikam.visualstudio.com/Ventes%20Web/_git/api",
            "https://fabrikam.visualstudio.com/DefaultCollection/Ventes%20Web/_git/api",
        ] {
            assert_eq!(parse_remote(url).as_deref(), Some("dev.azure.com/fabrikam/ventes web/api"), "{url}");
        }
    }

    #[test]
    fn local_paths_are_not_remotes() {
        assert_eq!(parse_remote(r"C:\depots\api"), None);
        assert_eq!(parse_remote("file:///C:/depots/api"), None);
        assert_eq!(parse_remote("../api"), None);
        assert_eq!(parse_remote("https://github.com/"), None);
    }

    #[test]
    fn origin_comes_first() {
        let config = "[core]\n\tbare = false\n[remote \"amont\"]\n\turl = https://github.com/northwind/api.git\n\
                      [remote \"origin\"]\n\turl = git@github.com:contoso/api.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n";
        assert_eq!(
            remotes_in_config(config),
            ["git@github.com:contoso/api.git", "https://github.com/northwind/api.git"]
        );
    }

    #[test]
    fn scan_finds_repositories_and_skips_builds() {
        let root = std::env::temp_dir().join(format!("owlery-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let repo = |rel: &str, url: &str| {
            let git = root.join(rel).join(".git");
            std::fs::create_dir_all(&git).unwrap();
            std::fs::write(git.join("config"), format!("[remote \"origin\"]\n\turl = {url}\n")).unwrap();
        };
        repo("contoso/api", "https://github.com/contoso/api.git");
        repo("fabrikam-web", "git@gitlab.com:fabrikam/web.git");
        repo("target/copie", "https://github.com/contoso/copie.git");
        repo("a/b/c/d/trop-loin", "https://github.com/contoso/loin.git");
        std::fs::create_dir_all(root.join("contoso/api/sous/.git")).unwrap();
        let found = scan(std::slice::from_ref(&root), 3);
        assert_eq!(found.get("github.com/contoso/api"), Some(&root.join("contoso/api")));
        assert_eq!(found.get("gitlab.com/fabrikam/web"), Some(&root.join("fabrikam-web")));
        assert!(!found.contains_key("github.com/contoso/copie"));
        assert!(!found.contains_key("github.com/contoso/loin"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn manual_choice_wins() {
        let item = Item { repo: "contoso/api".into(), ..Item::default() };
        let found = BTreeMap::from([("github.com/contoso/api".to_string(), PathBuf::from("D:/trouve"))]);
        assert_eq!(local_dir(&item, "github.com", &BTreeMap::new(), &found), Some(PathBuf::from("D:/trouve")));
        let manual = BTreeMap::from([("contoso/api".to_string(), "D:/choisi".to_string())]);
        assert_eq!(local_dir(&item, "github.com", &manual, &found), Some(PathBuf::from("D:/choisi")));
        assert_eq!(local_dir(&item, "gitlab.com", &BTreeMap::new(), &found), None);
    }
}
