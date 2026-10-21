# Owlery

La boîte de réception du développeur : PR à relire, CI en échec, mentions et tickets de toutes les
forges, au même endroit, avec de quoi agir tout de suite.

## Nom

Nom de travail : **Owlery**. Propositions, à choisir plus tard :

| Nom | Origine |
|---|---|
| **Owlery** | La volière des hiboux postaux de Poudlard (Harry Potter) : l'endroit où arrive le courrier. |
| Hedwig | La chouette de Harry Potter, qui apporte les messages. |
| Corvus | Le genre des corbeaux, messagers dans de nombreuses mythologies (Huginn et Muninn d'Odin). |

Les noms inspirés de Harry Potter (Owlery, Hedwig) conviennent à un projet personnel ; pour une
diffusion commerciale, ils poseraient une question de marque, et Corvus serait plus sûr.

## Mission

Rassembler ce qui attend une réponse du développeur sur GitHub, GitLab et Azure DevOps : revues
demandées, CI en échec, mentions, tickets assignés. Permettre d'y répondre ou de s'y mettre en un
clic : ouvrir la PR, le dépôt dans DragonTree, ou une session Claude dans Fawkes pour corriger.

## Périmètre

**Fait** :
- lire les notifications et listes des forges (PR, revues, CI, mentions, tickets) ;
- trier, regrouper par dépôt, marquer lu, reporter (« me le rappeler demain ») ;
- actions : ouvrir dans le navigateur, dans DragonTree, *Corriger avec Claude* (session dans un
  worktree de la branche, dans Fawkes) ;
- notifications Windows choisies (par type d'événement).

**Ne fait pas** :
- remplacer l'interface web des forges (revue ligne à ligne, administration) ;
- fusionner, approuver ou commenter sans action explicite de l'utilisateur ;
- stocker les jetons ailleurs que dans le coffre de Windows.

## Place dans l'écosystème

- Application autonome, et futur module de la suite avec Fawkes (terminaux) et DragonTree (git).
- Même pile : Rust, eframe et egui 0.37, rendu glow.
- État dans son propre dossier : passer de l'application autonome à la suite ne demande aucune
  migration.
- Reçoit ses demandes par le contrat local commun aux applications (canal nommé réservé au compte).
- Comptes des forges partagés avec DragonTree (mêmes identifiants dans le coffre de Windows).

## Lancer

```
cargo run --release
```

Toolchain MSVC : rien à préparer. Toolchain GNU (`stable-x86_64-pc-windows-gnu`) : il faut un
`dlltool` complet ; le plus simple est de réutiliser celui préparé par DragonTree
(`scripts\setup-toolchain.ps1` du dépôt DragonTree) et d'écrire un `.cargo/config.toml` local qui y
pointe (fichier ignoré par git, car il contient des chemins de la machine) :

```toml
[target.x86_64-pc-windows-gnu]
rustflags = ["-C", "dlltool=<DragonTree>/.tools/mingw64/bin/dlltool.exe",
             "-L", "native=<DragonTree>/.tools/mingw64/lib"]
```

Tests : `cargo test`.

### Accès à GitHub

Au premier lancement, Owlery cherche, dans cet ordre :

1. un **jeton GitHub** saisi dans Owlery (bouton *Jeton GitHub…*), rangé dans le Gestionnaire
   d'identifiants de Windows (service « Owlery ») et nulle part ailleurs ; *Retirer le jeton* le
   supprime. Jeton à grain fin conseillé, en lecture seule : Metadata, Pull requests, Issues, Commit
   statuses, Checks ;
2. sinon la **CLI `gh`**, si elle est installée et connectée (`gh auth login`) : Owlery l'appelle à
   chaque lecture et ne garde rien ;
3. sinon un écran propose les deux.

Aucun jeton n'est jamais lu dans les variables d'environnement.

### Utilisation

- Liste groupée par dépôt ou par type (Ctrl+G), non lus en tête ; filtres par type, forge, texte
  (Ctrl+F), « seulement ce qui m'attend » (Ctrl+W).
- ↑ ↓ (ou J K) pour se déplacer, Entrée pour ouvrir dans le navigateur, R lu, U non lu, E terminé,
  H « me le rappeler demain » ; clic droit sur une entrée pour les autres reports.
- F5 ou Ctrl+R : actualiser (lecture automatique toutes les 5 min, réglable) ; Ctrl+P : palette ;
  Ctrl+, : réglages (thème, intervalle, chemin de DragonTree, raccourcis).
- Une entrée lue ou terminée revient en non lu si la forge montre une activité plus récente.
- État : `%APPDATA%\Owlery\state.json`, versionné ; un fichier illisible est mis de côté, jamais
  écrasé.

### Essais

- `OWLERY_DEMO=1` : entrées fictives, aucune lecture réelle ; état dans un sous-dossier `demo`.
- `OWLERY_DATA_DIR=<dossier>` : dossier d'état isolé.
- `OWLERY_CAPTURE=<image.png>` : à chaque apparition de `<image.png>.demande`, Owlery enregistre
  l'image de sa fenêtre (fonctionne écran en veille).

## Premières étapes

État au 21/10/2026 : 1 et 2 faits, 4 en attente du contrat local de Fawkes
([`docs/contrat-local.md`](docs/contrat-local.md)). Décisions : [`docs/journal.md`](docs/journal.md).


1. Lecture des PR à relire et des CI en échec sur GitHub (un compte), liste simple.
2. Regroupement par dépôt, état lu / non lu, report.
3. GitLab et Azure DevOps.
4. *Corriger avec Claude* : demande à Fawkes par le contrat local.
5. Notifications Windows.

Détail : [`docs/spec.md`](docs/spec.md).
