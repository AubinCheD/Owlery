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
