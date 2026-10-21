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
