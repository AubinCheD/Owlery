# Journal des échanges et des décisions

Les décisions marquées *provisoire* ont été prises en l'absence de l'utilisateur, avec la session
pilote (fawkes-ed) qui le représente jusqu'au premier jet ; elles sont à confirmer.

## 21/10/2026 — démarrage

- **Pilotage** : l'utilisateur a confié la session à une session pilote jusqu'au premier jet. Les
  questions passent par elle.
- **Nom** : Owlery pour l'instant (*provisoire*).
- **Source GitHub** : recherche (PR à relire, mes PR, mentions, tickets assignés) plus l'état de la
  CI de mes PR, plutôt que l'API de notifications. L'état lu / non lu reste local (*provisoire*).
- **Comptes** : Owlery ne lit pas le coffre de DragonTree (il peut contenir des jetons d'autres
  comptes). Le partage des comptes est décrit comme contrat de suite, pour plus tard, dans
  `docs/contrat-local.md` (*provisoire*).
- **Accès GitHub** : la CLI `gh` est absente du poste et ne sera pas installée par nous. Ordre des
  sources : jeton GitHub saisi dans Owlery et rangé dans le Gestionnaire d'identifiants de Windows
  (service « Owlery ») ; sinon `gh api` si gh est présent ; sinon un état vide qui propose les deux.
  Jamais de jeton lu dans l'environnement. L'utilisateur saisira son jeton en reprenant la main.
- **GitLab et Azure DevOps** : jeton saisi, coffre de l'OS, testés seulement sur des réponses
  fictives (après le premier jet).
- **Structure** : une seule crate, bibliothèque + exécutable (comme Fawkes) ; un workspace viendra
  si une crate partagée apparaît.
- **État** : `%APPDATA%\Owlery\state.json` (ou `OWLERY_DATA_DIR` pour les essais), versionné comme
  l'état de Fawkes. Une entrée lue ou terminée redevient non lue si la forge montre une activité plus
  récente ; le choix sur une entrée disparue est gardé 30 jours.
- **Report** : plus tard (3 h), demain 9 h, lundi 9 h.

## 21/10/2026 — premier jet

- **Captures** : l'écran du poste est en veille, PrintWindow ne rend que du blanc. Capture interne
  reprise de Fawkes : `OWLERY_CAPTURE=<image.png>` et un fichier `<image.png>.demande`.
- **Démonstration** : `OWLERY_DEMO=1`, entrées fictives (contoso, fabrikam, northwind), état dans un
  sous-dossier `demo` pour ne jamais se mêler à l'état réel.
- **CI** : une de mes PR dont la CI échoue devient une entrée « CI en échec » (au lieu de « Ma PR ») ;
  les CI des branches suivies hors PR viendront plus tard.
- **Ouvrir dans DragonTree** : `dragontree.exe <dossier>` ; le dossier local de chaque dépôt est
  choisi une fois dans le détail et gardé dans les réglages.
- **Corriger avec Claude** : grisé tant que Fawkes ne reçoit pas de demandes ; à la place, *Copier
  le contexte pour Claude* (presse-papiers, rien n'est envoyé). Contrat proposé :
  `docs/contrat-local.md` (ajout d'une option `--paste-file` côté Fawkes).
- **Jeton GitHub** : l'utilisateur le saisira en reprenant la main ; non vérifié ici contre GitHub
  réel (aucun accès sur ce poste), seulement sur des réponses fictives.

### Reste à faire

- GitLab et Azure DevOps (jeton saisi, coffre « Owlery », réponses fictives seulement).
- Notifications Windows par type d'événement.
- Contrat local avec Fawkes et DragonTree, puis *Corriger avec Claude*.
- Questions ouvertes de la spec (§6) : vue équipe, doublons avec les mails des forges.

## Pistes après le premier jet

Premier jet accepté par la session pilote (dépôt vérifié, 32 tests au vert, capture de la démo).

1. **Premier essai contre GitHub réel**, avec le jeton de l'utilisateur : vérifier les quatre
   recherches, la CI des PR, les quotas et les messages d'erreur sur de vraies réponses.
2. **GitLab, Azure DevOps** (jeton saisi, coffre « Owlery », tests sur réponses fictives) et
   **notifications Windows** par type d'événement.
3. **Corriger avec Claude** quand Fawkes aura son API locale (canal nommé) ; la proposition
   `--paste-file` de `docs/contrat-local.md` sera étudiée à ce moment-là.
4. **Toolchain autonome** : le `.cargo/config.toml` local dépend aujourd'hui du dlltool de
   DragonTree. Un script de préparation comme celui de Menagerie (`scripts/setup.ps1`, qui copie
   `llvm-ar` depuis rustup) rendrait le dépôt autonome.

## 21/10/2026 — reprise par l'utilisateur, version 0.2

- **Demande** : « Refais une passe complète + les 4 points que tu as remontés. Ensuite rajoute des
  fonctionnalités que tu jugerais utiles. Ajoute superpowers et une doc/un plan de route si ce n'est
  pas déjà fait. On verra les tests réels plus tard (point 1). »
- **Superpowers** : greffon activé pour le dépôt (`.claude/settings.json`, comme Fawkes), design et
  plan dans `docs/superpowers/`, espace de travail `.superpowers/` ignoré (comme DragonTree).
- **Feuille de route** : `docs/feuille-de-route.md`.
- **Fonctionnalités** : choisies par Claude (l'utilisateur les a laissées à son jugement), listées
  dans le design §2.8 ; à relire.
- **Exécution** : Claude exécute le plan lui-même dans la session, puis un relecteur neuf relit
  toute la branche (choix de Claude : les tâches partagent `app.rs` et le modèle, et se suivent).

### Revue du premier jet

Un relecteur neuf (sous-agent, lecture seule) a relu tout le premier jet : 24 points. Décisions :

| # | Point | Suite |
|---|---|---|
| 1 | État illisible (verrouillé, droits) écrasé à la première lecture | corrigé : rien n'est écrit tant que l'état n'a été ni lu ni mis de côté |
| 2 | Deux instances écrivent le même `state.json` | corrigé : verrou `state.lock`, instance unique, second lancement → `afficher` |
| 3 | Clic sur le texte d'une ligne sans effet (libellés sélectionnables) | corrigé, et test d'interface |
| 4 | Identifiant d'une de mes PR qui change avec sa CI | corrigé : identifiants sans le type (état version 2, migration de la 0.1.0) |
| 5 | Même PR en double (revue et mention) | corrigé : une entrée par identifiant, le type le plus utile |
| 6 | Une recherche en échec fait tout échouer ; `mentions:@me` sans `is:` | mentions scindées en `is:pr` et `is:issue` ; un échec garde le cache du compte (décision : pas de fusion partielle) |
| 7 | CI de mes PR non lue au-delà de 30 relectures | corrigé : budget séparé, mes PR d'abord |
| 8 | Spinner infini (fil qui panique), `gh` sans délai | corrigé : `catch_unwind`, `gh` arrêté après 60 s |
| 9 | Mes propres actions remettent en non lu | corrigé : ouverture = vu jusqu'à maintenant ; mon dernier commentaire ne compte pas ; mes PR suivent un compteur d'activité (commentaires, revues des autres), pas mes poussées |
| 10 | « Rien ne vous attend » après une erreur | corrigé : écran d'erreur avec *Réessayer* et *Comptes…* |
| 11 | Sélection au clavier sans défilement | corrigé : défilement et ouverture du groupe |
| 12 | Fond peint par-dessus le texte | corrigé : fond réservé avant le contenu |
| 13 | Raccourcis à une touche morts après un Tab | corrigé : seuls les champs de texte gardent les lettres, un bouton garde Entrée et Espace |
| 14 | Répétition de touche qui termine plusieurs entrées | corrigé : seuls les déplacements se répètent |
| 15 | Toasts d'échec d'enregistrement empilés | corrigé : un bandeau, nouvel essai toutes les 30 s |
| 16 | Champs inconnus perdus au-delà du premier niveau | corrigé : champs inconnus gardés dans les réglages, comptes, entrées et choix ; entrées illisibles gardées telles quelles |
| 17 | Sélection restée sur une entrée masquée par le filtre | corrigé |
| 18 | Raccourci en double qui ne marche que pour une commande | corrigé : la combinaison passe à la nouvelle commande, l'ancienne la perd (dit) ; saisie annulée par un clic ailleurs |
| 19 | Actualiser pendant une lecture ignoré | corrigé : la demande est gardée |
| 20 | Message d'état sous les boutons, titres qui débordent | corrigé |
| 21 | Listes tronquées en silence | signalé (« 50 affichées sur 132 ») ; pas de pagination (choix de Claude) |
| 22 | Démo : choix perdus à chaque relance | corrigé : dates fixes dans la journée |
| 23 | Noms de touches en anglais | corrigé ; les flèches deviennent « Flèche bas »… (glyphes absents des polices, vu par un test) |
| 24 | Capture 0×0 (fenêtre réduite) qui ferait planter | corrigé : image vide ignorée |

### Ce qui a été fait (0.2.0)

- Les tâches 1 et 3 à 11 du plan sont arrivées dans un même commit : le modèle (identifiants sans
  type) et l'interface ont été refaits d'un bloc, et des étapes intermédiaires n'auraient pas compilé.
- Accessibilité : la liste déroulante des forges n'avait pas de nom (vu par egui_kittest), corrigé.
- Essais : 117 tests (logique, forges sur réponses fictives, vrai canal nommé de test, interface
  egui_kittest) ; démonstration capturée ; second lancement vérifié (il passe la main en 0,1 s).
- Non vérifié : GitHub, GitLab et Azure DevOps réels (point 1, plus tard) ; la bulle de notification
  et le clic dessus (l'écran du poste est en veille).
