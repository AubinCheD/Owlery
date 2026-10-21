//! Comptes : liste, état de la dernière lecture, ajout (jeton vérifié avant d'être rangé dans le coffre),
//! retrait (jeton supprimé du coffre, entrées du compte retirées).

use std::sync::mpsc::channel;

use eframe::egui::{self, RichText};

use crate::app::{AccountDraft, App};
use crate::forges::{Account, Auth, GH_ACCOUNT_ID, normalize_host};
use crate::model::Forge;

/// Droits minimaux et page de création du jeton, par forge.
pub fn token_help(forge: Forge, host: &str, orgs: &str) -> (&'static str, String) {
    match forge {
        Forge::GitHub => (
            "Jeton à grain fin, en lecture seule : Metadata, Pull requests, Issues, Commit statuses et Checks, sur les \
             dépôts concernés. Un jeton classique marche aussi, avec la portée « repo » (ou « public_repo »).",
            format!("https://{host}/settings/personal-access-tokens/new"),
        ),
        Forge::GitLab => (
            "Jeton d'accès personnel avec la seule portée « read_api ».",
            format!("https://{host}/-/user_settings/personal_access_tokens?name=Owlery&scopes=read_api"),
        ),
        Forge::AzureDevOps => {
            let org = orgs.split(',').map(str::trim).find(|o| !o.is_empty()).unwrap_or("<organisation>");
            (
                "Jeton d'accès personnel, en lecture seule : Code, Work Items et Build. Indiquez la ou les \
                 organisations (séparées par des virgules).",
                format!("https://{host}/{org}/_usersSettings/tokens"),
            )
        }
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    poll_check(app, ctx);
    if !app.accounts_open {
        return;
    }
    let mut open = true;
    let mut remove = None;
    egui::Window::new("Comptes").open(&mut open).default_width(560.0).show(ctx, |ui| {
        let accounts = app.accounts();
        if accounts.is_empty() {
            ui.label(RichText::new("Aucun compte. Ajoutez-en un ci-dessous.").weak());
        }
        egui::Grid::new("accounts").num_columns(3).striped(true).spacing([12.0, 6.0]).show(ui, |ui| {
            for a in &accounts {
                ui.label(RichText::new(a.title()).strong());
                let s = app.status.get(&a.id);
                let state = match s {
                    _ if app.running.contains(&a.id) => RichText::new("lecture…").weak(),
                    Some(s) if s.error.is_some() => {
                        RichText::new(s.error.as_ref().map(|e| e.to_string()).unwrap_or_default())
                            .color(ui.visuals().warn_fg_color)
                    }
                    Some(s) if s.last_ok.is_some() => {
                        RichText::new(format!("lu à {}", super::date(s.last_ok.unwrap_or(0)))).weak()
                    }
                    _ => RichText::new("pas encore lu").weak(),
                };
                let resp = ui.add(egui::Label::new(state).truncate());
                if let Some(notes) = s.map(|s| s.notes.join("\n")).filter(|n| !n.is_empty()) {
                    resp.on_hover_text(notes);
                }
                if a.id == GH_ACCOUNT_ID {
                    ui.label(RichText::new("automatique").weak())
                        .on_hover_text("Lu par la CLI gh tant qu'aucun compte GitHub n'est ajouté");
                } else if ui
                    .button("Retirer")
                    .on_hover_text("Supprime le jeton du coffre et les entrées de ce compte")
                    .clicked()
                {
                    remove = Some(a.id.clone());
                }
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        ui.separator();
        add_form(app, ui, ctx);
    });
    if let Some(id) = remove {
        remove_account(app, &id);
    }
    if !open {
        app.accounts_open = false;
        app.draft = None;
    }
}

fn add_form(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    let Some(d) = &mut app.draft else {
        if ui.button("➕ Ajouter un compte").clicked() {
            app.draft = Some(AccountDraft {
                forge: Forge::GitHub,
                host: String::new(),
                orgs: String::new(),
                token: String::new(),
                use_gh: false,
                checking: None,
                error: None,
            });
        }
        return;
    };
    ui.label(RichText::new("Nouveau compte").strong());
    let checking = d.checking.is_some();
    ui.add_enabled_ui(!checking, |ui| {
        egui::Grid::new("draft").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Forge");
            ui.horizontal(|ui| {
                for f in Forge::ALL {
                    ui.selectable_value(&mut d.forge, f, f.label());
                }
            });
            ui.end_row();
            ui.label("Hôte");
            ui.add(egui::TextEdit::singleline(&mut d.host).hint_text(d.forge.default_host()).desired_width(260.0));
            ui.end_row();
            if d.forge == Forge::AzureDevOps {
                ui.label("Organisations");
                ui.add(egui::TextEdit::singleline(&mut d.orgs).hint_text("fabrikam, northwind").desired_width(260.0));
                ui.end_row();
            }
            if d.forge == Forge::GitHub {
                ui.label("Accès");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.use_gh, false, "Jeton");
                    ui.radio_value(&mut d.use_gh, true, "CLI gh (déjà connectée)");
                });
                ui.end_row();
            }
            if !(d.forge == Forge::GitHub && d.use_gh) {
                ui.label("Jeton");
                ui.add(
                    egui::TextEdit::singleline(&mut d.token)
                        .password(true)
                        .hint_text("Coller le jeton")
                        .desired_width(260.0),
                );
                ui.end_row();
            }
        });
    });
    if !(d.forge == Forge::GitHub && d.use_gh) {
        let host = normalize_host(&d.host, d.forge);
        let (help, link) = token_help(d.forge, &host, &d.orgs);
        ui.label(RichText::new(help).weak());
        if ui.link("Créer le jeton sur la forge").on_hover_text(&link).clicked() {
            let _ = crate::actions::open_url(&link);
        }
        ui.label(
            RichText::new("Le jeton est vérifié, puis rangé dans le Gestionnaire d'identifiants de Windows (service « Owlery ») ; il n'est plus jamais affiché.")
                .weak(),
        );
    }
    if let Some(e) = &d.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    let mut cancel = false;
    let mut verify = false;
    ui.horizontal(|ui| {
        if checking {
            ui.spinner();
            ui.label("Vérification…");
        } else {
            if ui.button("Vérifier et ajouter").clicked() {
                verify = true;
            }
            if ui.button("Annuler").clicked() {
                cancel = true;
            }
        }
    });
    if cancel {
        app.draft = None;
    } else if verify {
        start_check(app, ctx);
    }
}

fn draft_account(d: &AccountDraft) -> Account {
    let orgs: Vec<String> =
        d.orgs.split(',').map(|o| o.trim().trim_matches('/').to_string()).filter(|o| !o.is_empty()).collect();
    let auth = if d.forge == Forge::GitHub && d.use_gh { Auth::Gh } else { Auth::Token };
    Account::new(d.forge, &d.host, orgs, auth)
}

fn start_check(app: &mut App, ctx: &egui::Context) {
    let Some(d) = &mut app.draft else { return };
    let account = draft_account(d);
    if account.auth == Auth::Token && d.token.trim().is_empty() {
        d.error = Some("Collez le jeton.".into());
        return;
    }
    if account.forge == Forge::AzureDevOps && account.orgs.is_empty() {
        d.error = Some("Indiquez au moins une organisation.".into());
        return;
    }
    d.error = None;
    let (tx, rx) = channel();
    let (fetcher, token, ctx) = (app.fetcher.clone(), d.token.clone(), ctx.clone());
    std::thread::spawn(move || {
        let _ = tx.send(fetcher.verify(&account, &token));
        ctx.request_repaint();
    });
    d.checking = Some(rx);
}

/// Fin de la vérification : le jeton est rangé dans le coffre, puis le compte ajouté et lu.
fn poll_check(app: &mut App, ctx: &egui::Context) {
    let Some(d) = &mut app.draft else { return };
    let Some(rx) = &d.checking else { return };
    let Ok(result) = rx.try_recv() else { return };
    d.checking = None;
    match result {
        Err(e) => d.error = Some(format!("Vérification refusée : {e}")),
        Ok(identity) => {
            let mut account = draft_account(d);
            account.username = identity.username;
            account.display_name = identity.display_name;
            if account.auth == Auth::Token
                && let Err(e) = crate::secrets::store_token(app.store.as_ref(), &account.token_key(), &d.token)
            {
                d.error = Some(format!("Enregistrement du jeton impossible : {e}"));
                return;
            }
            let title = account.title();
            app.st.settings.accounts.push(account);
            app.draft = None;
            app.changed();
            app.toast(format!("Compte ajouté : {title}"));
            app.refresh(ctx);
        }
    }
}

fn remove_account(app: &mut App, id: &str) {
    let Some(pos) = app.st.settings.accounts.iter().position(|a| a.id == id) else { return };
    let account = app.st.settings.accounts[pos].clone();
    if account.auth == Auth::Token
        && let Err(e) = app.store.delete(&account.token_key())
    {
        app.toast(format!("Suppression du jeton impossible : {e}. Le compte est gardé."));
        return;
    }
    app.st.settings.accounts.remove(pos);
    app.st.inbox.remove_account(id);
    app.status.remove(id);
    if app.selected.as_ref().is_some_and(|s| !app.st.inbox.items.contains_key(s)) {
        app.selected = None;
    }
    app.changed();
    app.toast(format!("Compte retiré : {}", account.title()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_help_names_the_minimal_rights_and_the_page() {
        let (help, link) = token_help(Forge::GitHub, "github.com", "");
        assert!(help.contains("lecture seule") && help.contains("Checks"));
        assert_eq!(link, "https://github.com/settings/personal-access-tokens/new");
        let (help, link) = token_help(Forge::GitLab, "gitlab.contoso.test", "");
        assert!(help.contains("read_api"));
        assert!(link.starts_with("https://gitlab.contoso.test/-/user_settings/personal_access_tokens"));
        let (_, link) = token_help(Forge::AzureDevOps, "dev.azure.com", " fabrikam , northwind");
        assert_eq!(link, "https://dev.azure.com/fabrikam/_usersSettings/tokens");
    }
}
