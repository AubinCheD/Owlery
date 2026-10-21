# Contrat local : ce qu'Owlery reçoit et ce dont il a besoin

Proposition à discuter avec Fawkes et DragonTree. Côté Owlery, tout ce qui suit est **implémenté** (0.2) ;
côté Fawkes et DragonTree, rien ne l'est encore. Elle reprend la réflexion de Fawkes (`docs/reflexions.md`,
§3.1 et §3.3) et s'aligne sur les propositions de Menagerie (`docs/contrat-fawkes.md`), Patronus et Grimoire
(`docs/contrat-local.md` de chacun).

## 1. Transport (commun)

- **Canal** : tube nommé `\\.\pipe\<application>-<SID du compte>` (`owlery-S-1-5-21-…`, `fawkes-S-1-5-21-…`).
  Une instance d'essai (dossier d'état choisi par `OWLERY_DATA_DIR`) ajoute l'empreinte de son dossier :
  `owlery-<SID>-<16 hexadécimaux>`, pour tourner à côté de celle de tous les jours.
- **Sécurité** : descripteur de sécurité `D:P(A;;GA;;;<SID>)` (l'utilisateur courant seulement),
  `PIPE_REJECT_REMOTE_CLIENTS`, et `FILE_FLAG_FIRST_PIPE_INSTANCE` (une autre application ne peut pas avoir pris
  le nom avant). Aucun schéma d'adresse (`owlery://`) : une page web ne doit pas pouvoir envoyer de demandes.
- **Message** : une ligne JSON UTF-8 terminée par `\n`, 64 Kio au plus ; réponse sur la même connexion, une
  ligne. Un champ inconnu est ignoré.
- **Version** : `"v": 1` dans chaque message. Version plus récente que celle comprise →
  `{"v": 1, "ok": false, "erreur": "version"}`. `owlery.exe --contract-version` affiche `1`.
- **Réponse** : `{"v": 1, "ok": true, …}` ou `{"v": 1, "ok": false, "erreur": "<message en français>"}` ;
  rien n'est ignoré en silence (demande inconnue → `"erreur": "demande inconnue : …"`).
- **Nom du champ** : `demande` (comme Patronus). Menagerie propose `op`, Grimoire `action` : à unifier ; Owlery
  lit `erreur` et `error` dans les réponses.

## 2. Ce qu'Owlery reçoit

| Demande | Champs | Effet | Réponse |
|---|---|---|---|
| `afficher` | `type` facultatif : `revue`, `ci`, `mention`, `ticket`, `ma_pr` | ramène la fenêtre au premier plan, filtrée sur ce type | `ok` |
| `actualiser` | — | lit les forges maintenant | `ok` |
| `etat` | `depot_url` facultatif (adresse d'un remote, toutes formes) | — | `en_attente`, `par_type`, et `depot.a_relire` si `depot_url` |

Exemple, pour le badge « PR à relire » de DragonTree sur un dépôt :

```json
{"v": 1, "demande": "etat", "depot_url": "git@github.com:contoso/api.git"}
{"v": 1, "ok": true, "en_attente": 6, "par_type": {"revue": 3, "ci": 1, "mention": 1, "ticket": 1, "ma_pr": 0},
 "depot": {"a_relire": 1}}
```

L'adresse est ramenée à une clé `hôte/chemin` (https, ssh `git@`, `ssh://`, formes Azure DevOps `_git`,
`ssh.dev.azure.com:v3`, `*.visualstudio.com`) : `github.com/contoso/api`.

**Instance unique** : un second `owlery.exe` sur le même dossier d'état envoie `afficher` et se termine
(verrou `Local\Owlery-single-instance`, comme Fawkes). Le client autorise le serveur à passer au premier plan
(`AllowSetForegroundWindow`), puisque la demande vient d'un geste de l'utilisateur.

## 3. Ce qu'Owlery demande à Fawkes : *Corriger avec Claude*

**Besoin** : ouvrir une session Claude dans un worktree de la branche de la PR, dans le groupe du dépôt, avec
le contexte collé dans l'invite **sans être envoyé**.

```json
{"v": 1, "demande": "ouvrir", "de": "owlery", "dir": "D:\\depots\\contoso-api",
 "titre": "contoso/api #138", "claude": true, "worktree": "export-csv",
 "consigne": "CI en échec — Corrige l'export CSV (contoso/api#138)\nLien : …\nBranche : export-csv\n…"}
```

- Champs repris de la demande `ouvrir` de Patronus (`dir`, `titre`, `claude`, `consigne`), plus `worktree` :
  la branche dont Fawkes crée (ou réutilise) le worktree.
- `consigne` : collée dans l'invite de Claude une fois prête, **sans Entrée**.
- Réponse attendue : `ok`, ou `erreur` (dossier introuvable, branche inconnue…), affichée telle quelle.

**Comportement d'Owlery** :
- Fawkes écoute (son canal existe) : la demande part au clic, la réponse est affichée.
- Fawkes n'écoute pas : un dialogue propose *Démarrer Fawkes et envoyer* (lancé **sans argument**, puis
  attente de son canal 20 s au plus), *Copier le contexte* ou *Annuler*. Rien n'est lancé sans ce choix.
- Fawkes refuse, ou n'ouvre pas son canal : le message est affiché et le contexte copié dans le
  presse-papiers.
- `fawkes.exe` : chemin des réglages, sinon le PATH.

**Ligne de commande** : Owlery n'en a pas besoin (il passe par le canal après avoir démarré Fawkes). Si Fawkes
préfère accepter la demande au démarrage, la forme prévue dans ses réflexions conviendrait, avec une option
pour la consigne :

```
fawkes.exe open --dir <dépôt> --claude --worktree <branche> --title "<titre>" --paste-file <fichier>
```

## 4. Ouvrir dans DragonTree

Aujourd'hui : `dragontree.exe <dossier>` (existe déjà). Souhaité ensuite, sur le canal de DragonTree :

```json
{"v": 1, "demande": "ouvrir", "dir": "D:\\depots\\contoso-api", "branche": "export-csv"}
```

`branche` : la sélectionner dans l'historique, sans la récupérer ni changer de branche active.

## 5. Comptes partagés avec DragonTree (plus tard)

Owlery ne lit **pas** le coffre de DragonTree : il peut contenir des jetons d'autres comptes, et Owlery n'a
besoin que de lecture. Proposition pour la suite :

- un service commun dans le Gestionnaire d'identifiants (« Suite », utilisateur `account:<id>`) pour les
  comptes que l'utilisateur choisit de **partager**, compte par compte ;
- la liste des comptes partagés (sans jeton) dans un fichier commun : `%APPDATA%\Suite\accounts.json` (`id`,
  forge, hôte, organisations, nom, droits annoncés) ;
- chaque application garde ses comptes privés dans son propre service (« Owlery », « DragonTree ») ;
- dans la suite, l'hôte fournit la liste et le dialogue de compte ; les modules ne font que lire.

## 6. Patronus

Les événements d'Owlery serviraient de déclencheurs (`evenement` dans la proposition de Patronus) :
`pr_ouverte`, `pr_commentee`, `ci_echec`, avec `depot` et `lien`. Owlery les connaît déjà (ce sont ses
notifications) ; il faudra décider qui écoute qui.
