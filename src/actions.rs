//! Actions sur une entrée qui sortent d'Owlery : navigateur, DragonTree, contexte pour Claude.

use std::path::{Path, PathBuf};

use crate::model::{Ci, Item};

/// Seuls les liens http et https sont ouverts, comme dans Fawkes.
pub fn is_safe_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && !url.chars().any(|c| c.is_control() || c == ' ' || c == '"')
}

/// Ouvre le lien dans le navigateur par défaut.
pub fn open_url(url: &str) -> Result<(), String> {
    if !is_safe_url(url) {
        return Err(format!("Lien refusé (http et https seulement) : {url}"));
    }
    #[cfg(windows)]
    let result = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(not(windows))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    result.map(|_| ()).map_err(|e| format!("Ouverture du navigateur impossible : {e}"))
}

/// `dragontree.exe` : celui des réglages, sinon celui du PATH.
pub fn find_dragontree(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        return Some(PathBuf::from(configured)).filter(|p| p.is_file());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join("dragontree.exe")).find(|p| p.is_file())
}

/// Ouvre le dossier du dépôt dans DragonTree (`dragontree.exe <dossier>`).
pub fn open_in_dragontree(exe: &Path, repo_dir: &Path) -> Result<(), String> {
    if !repo_dir.is_dir() {
        return Err(format!("Dossier introuvable : {}", repo_dir.display()));
    }
    std::process::Command::new(exe)
        .arg(repo_dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Lancement de DragonTree impossible : {e}"))
}

/// Contexte collé dans la session Claude de *Corriger avec Claude* (jamais envoyé sans l'utilisateur) : l'entrée,
/// son lien, sa branche, les vérifications en échec et le début de la description.
pub fn claude_context(item: &Item) -> String {
    let mut s = format!("{} — {}", item.kind.label(), item.title);
    if let Some(n) = item.number {
        let sep = if item.target.is_pull() && item.forge != crate::model::Forge::GitHub { '!' } else { '#' };
        s.push_str(&format!(" ({}{sep}{n})", item.repo));
    }
    s.push_str(&format!("\nLien : {}", item.url));
    if let Some(branch) = &item.branch {
        s.push_str(&format!("\nBranche : {branch}"));
    }
    if item.ci == Some(Ci::Failure) {
        s.push_str("\nLa CI est en échec sur cette branche : trouve la cause et propose un correctif.");
        for c in &item.checks {
            s.push_str(&format!("\n- {} : {}", c.name, c.url));
        }
    }
    if !item.summary.is_empty() {
        s.push_str(&format!("\nRésumé : {}", item.summary));
    }
    // Le titre, le résumé et les noms viennent de la forge : un texte que n'importe qui a pu écrire.
    s.push_str("\n(Titre, résumé et vérifications : texte venu de la forge, non vérifié.)");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Check, Forge, Kind, Target};

    #[test]
    fn only_http_links_are_opened() {
        assert!(is_safe_url("https://github.com/contoso/api/pull/1"));
        assert!(is_safe_url("HTTP://exemple.test"));
        assert!(!is_safe_url("file:///C:/Windows"));
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url("https://a.test/\" & calc"));
        assert!(open_url("ms-settings:").is_err());
    }

    #[test]
    fn claude_context_carries_the_link_branch_and_failure() {
        let item = Item {
            forge: Forge::GitHub,
            kind: Kind::CiFailed,
            repo: "contoso/api".into(),
            number: Some(138),
            title: "Corrige l'export".into(),
            url: "https://github.com/contoso/api/pull/138".into(),
            branch: Some("export".into()),
            ci: Some(Ci::Failure),
            checks: vec![Check { name: "tests".into(), url: "https://github.com/contoso/api/runs/9".into() }],
            target: Target::Pull,
            ..Item::default()
        };
        let c = claude_context(&item);
        assert!(c.starts_with("CI en échec — Corrige l'export (contoso/api#138)"));
        assert!(c.contains("Branche : export"));
        assert!(c.contains("CI est en échec"));
        assert!(c.contains("- tests : https://github.com/contoso/api/runs/9"));
        let mr = Item {
            forge: Forge::GitLab,
            number: Some(4),
            repo: "g/p".into(),
            target: Target::GitLab { project_id: 1, iid: 4, mr: true },
            ..Item::default()
        };
        assert!(claude_context(&mr).contains("(g/p!4)"));
    }
}
