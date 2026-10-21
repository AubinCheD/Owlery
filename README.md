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
- Reçoit ses demandes par le contrat local commun aux applications (canal nommé réservé au compte) :
  [`docs/contrat-local.md`](docs/contrat-local.md).
- Comptes des forges : propres à Owlery pour l'instant ; leur partage avec DragonTree est proposé dans le
  contrat local (§5).

## Compilation et lancement

Windows, Rust stable (édition 2024).

Avec la toolchain `x86_64-pc-windows-gnu`, préparer une fois le clone :

```powershell
powershell -ExecutionPolicy Bypass -File scripts\setup.ps1
```

Ce script copie `llvm-ar` de rustup (composant `llvm-tools`, ajouté s'il manque) en
`tools\dlltool.exe` et écrit `.cargo\config.toml` (éditeur de liens `rust-lld`). Ces deux fichiers
sont **propres au poste** (chemins absolus) et **ignorés par git** : les recréer avec le script sur
chaque poste, `-Force` pour réécrire la configuration. Avec la toolchain MSVC (Build Tools C++
installés), il n'y a rien à préparer.

```
cargo run --release
```

Tests : `cargo test` (logique, réponses fictives des forges, canal nommé, et interface avec egui_kittest).

## Comptes

*Comptes…* (en haut à droite) : ajouter, voir l'état de la dernière lecture, retirer.

| Forge | Hôte | Jeton (lecture seule) |
|---|---|---|
| GitHub | `github.com` ou GitHub Enterprise | à grain fin : Metadata, Pull requests, Issues, Commit statuses, Checks ; ou classique « repo » |
| GitLab | `gitlab.com` ou une instance | portée « read_api » |
| Azure DevOps | `dev.azure.com` et une ou plusieurs organisations ; ou un serveur, avec la collection en organisation (`tfs/DefaultCollection`) | Code, Work Items, Build |

- Le jeton est **vérifié** auprès de la forge, puis rangé dans le Gestionnaire d'identifiants de Windows
  (service « Owlery ») et nulle part ailleurs ; il n'est plus jamais affiché. *Retirer* le supprime du
  coffre, avec les entrées du compte.
- GitHub peut aussi passer par la **CLI `gh`** déjà connectée (`gh auth login`) : Owlery l'appelle à
  chaque lecture et ne garde rien. Sans compte GitHub, `gh` est utilisé tout seul s'il est installé.
- Aucun jeton n'est jamais lu dans les variables d'environnement.
- Le jeton GitHub saisi dans la 0.1 est repris dans un compte au premier lancement.

## Ce qui arrive dans la boîte

| Type | GitHub | GitLab | Azure DevOps |
|---|---|---|---|
| Revue demandée | `review-requested:@me` | MR où je suis relecteur | PR où je suis relecteur, sans avoir voté |
| CI en échec | mes PR dont la CI échoue | mes MR dont le pipeline échoue | mes PR dont la build échoue |
| Ma PR | mes PR ouvertes | mes MR ouvertes | mes PR actives |
| Mention | `mentions:@me` (PR et tickets) | à-faire « mentionné » | mentions récentes (`@RecentMentions`) |
| Ticket assigné | tickets assignés | tickets assignés | éléments de travail assignés, non fermés |

- Une PR ou un ticket n'apparaît qu'une fois, avec le type le plus utile.
- Une entrée lue ou terminée **revient en non lu** quand elle bouge : nouvelle activité, CI devenue
  rouge, revue demandée. Pas quand l'activité est la mienne (mon dernier commentaire), ni, pour mes
  PR, quand je pousse : elles reviennent quand quelqu'un d'autre commente ou relit (GitHub : commentaires,
  commentaires de revue et revues ; Azure DevOps : commentaires et votes ; GitLab : notes, pas les
  approbations). Une de mes PR vue pour la première fois arrive lue.
- Une liste tronquée (plus de 50 résultats), un détail illisible ou une organisation en échec sont
  signalés dans la ligne d'état (infobulle), jamais passés sous silence ; le cache du compte est gardé.

## Utilisation

- Liste groupée par dépôt ou par type (Ctrl+G), non lus en tête ; filtres par type, forge, texte
  (Ctrl+F), « seulement ce qui m'attend » (Ctrl+W). Filtres, regroupement, taille et position de la
  fenêtre sont gardés. Le titre de la fenêtre dit combien d'entrées attendent.
- Flèches haut et bas (ou J K) pour se déplacer, Entrée pour ouvrir dans le navigateur, R lu, U non lu,
  E terminé, H « me le rappeler demain », Maj+R tout marquer lu, Ctrl+Z annuler (50 changements).
- Clic droit sur une entrée : reports (3 h, demain, lundi), *Ignorer ce dépôt* ; sur un groupe :
  *Tout marquer lu*.
- Détail : branche, CI et vérifications en échec (avec leur lien), trois derniers commentaires.
- *Ouvrir dans DragonTree* et *Corriger avec Claude* demandent le dossier local du dépôt : choisi dans
  le détail, ou trouvé tout seul dans les **dossiers des dépôts** des réglages.
- *Corriger avec Claude* : demande à Fawkes une session Claude dans un worktree de la branche, le
  contexte (lien, branche, CI en échec) collé sans être envoyé. Tant que Fawkes ne reçoit pas de
  demandes, Owlery propose de le démarrer ou de copier le contexte.
- F5 ou Ctrl+R : actualiser (lecture automatique toutes les 5 min, réglable) ; Ctrl+P : palette
  (vues, commandes, *Aller au dépôt…*) ; Ctrl+, : réglages (thème, notifications, robots masqués,
  dépôts ignorés, dossiers des dépôts, programmes, raccourcis).
- **Notifications Windows** quand Owlery n'est pas au premier plan : nouvelles entrées des types
  choisis (rien au premier chargement d'un compte), et rappel quand un report arrive à échéance.
- État : `%APPDATA%\Owlery\state.json`, versionné ; un fichier ou une entrée illisible est mis de
  côté ou gardé tel quel, jamais écrasé ; deux instances ne peuvent pas écrire le même état (un
  second lancement ramène la fenêtre ouverte).

## Essais

- `OWLERY_DEMO=1` : comptes et entrées fictifs (GitHub, GitLab, Azure DevOps), aucune lecture réelle,
  aucun accès au coffre ; état dans un sous-dossier `demo`.
- `OWLERY_DATA_DIR=<dossier>` : dossier d'état isolé (et canal, verrou propres à ce dossier).
- `OWLERY_CAPTURE=<image.png>` : à chaque apparition de `<image.png>.demande`, Owlery enregistre
  l'image de sa fenêtre (fonctionne écran en veille).
- `owlery.exe --contract-version` : version du contrat local prise en charge.

## Documentation

- [`docs/spec.md`](docs/spec.md) : spécification de départ.
- [`docs/feuille-de-route.md`](docs/feuille-de-route.md) : ce qui est fait, ce qui reste.
- [`docs/journal.md`](docs/journal.md) : échanges et décisions.
- [`docs/contrat-local.md`](docs/contrat-local.md) : contrat avec Fawkes, DragonTree, Patronus.
- [`docs/superpowers/`](docs/superpowers/) : design et plans d'implémentation.
