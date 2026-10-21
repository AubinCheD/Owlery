# Feuille de route

État au 21/10/2026. Version 0.1.0 (premier jet : GitHub seul). La version 0.2 est en cours, selon
le plan `docs/superpowers/plans/2026-10-21-owlery-0.2.md` (design :
`docs/superpowers/specs/2026-10-21-owlery-0.2-design.md`). Les décisions et les échanges sont dans
`docs/journal.md`.

Une priorité suivie de « (estimée) » ne ressort pas des échanges : c'est une proposition.

## Sommaire

| # | Point | Qui | Priorité |
|---|---|---|---|
| V1 | Premier essai contre GitHub réel, avec le jeton de l'utilisateur | utilisateur | haute |
| V2 | Essai réel de GitLab et d'Azure DevOps | utilisateur | moyenne (estimée) |
| 0.2 | Version 0.2 (tâches 0 à 12 du plan) | Claude | en cours |
| C1 | Contrat local reçu par Fawkes (canal nommé, demande `ouvrir`) | Fawkes | haute (estimée) |
| C2 | DragonTree : ouvrir sur une branche, badge « PR à relire » | DragonTree | basse (estimée) |
| P | Pistes | — | basse |

## Validation en conditions réelles

### V1. Premier essai contre GitHub réel

- **État** : la lecture n'a été testée que sur des réponses fictives ; aucun accès à GitHub sur le
  poste de développement.
- **À faire** : saisir un jeton à grain fin en lecture seule (*Comptes…*), actualiser, comparer avec
  les pages *Pull requests* et *Issues* de GitHub ; vérifier la CI d'une PR rouge, une mention, un
  ticket assigné ; laisser tourner une heure (quota).
- **Critère de fin** : les quatre listes correspondent à GitHub ; aucun message d'erreur inattendu.

### V2. GitLab et Azure DevOps réels

- **À faire** : un compte de chaque, mêmes vérifications que V1. Azure : un jeton limité à une
  organisation, puis un jeton global.

## Version 0.2

Voir le plan. Chaque tâche fait l'objet d'un commit ; ce tableau est mis à jour à la fin.

| Tâche | Contenu | État |
|---|---|---|
| 0 | superpowers, design, plan, feuille de route | en cours |
| 1 | corrections de la revue du premier jet | à faire |
| 2 | toolchain autonome (`scripts/setup.ps1`) | à faire |
| 3 | comptes multiples, lecture par compte | à faire |
| 4 | GitLab | à faire |
| 5 | Azure DevOps | à faire |
| 6 | détail : CI en échec, derniers commentaires | à faire |
| 7 | notifications Windows, icône | à faire |
| 8 | instance unique, canal d'Owlery | à faire |
| 9 | dossiers locaux des dépôts | à faire |
| 10 | Corriger avec Claude (Fawkes) | à faire |
| 11 | annuler, tout marquer lu, dépôts ignorés, vue gardée | à faire |
| 12 | version 0.2.0, revue de branche | à faire |

## Dépendances vers les autres applications

### C1. Fawkes reçoit des demandes

Owlery envoie `ouvrir` sur `\\.\pipe\fawkes-<SID>` (voir `docs/contrat-local.md`). Tant que Fawkes
n'a pas de canal, *Corriger avec Claude* propose de copier le contexte.

### C2. DragonTree

`dragontree.exe <dossier>` suffit aujourd'hui. Souhaité : ouvrir sur une branche ; badge « PR à
relire » en interrogeant Owlery (`etat` avec `depot_url`).

## Pistes

- Vue « équipe » : PR des collègues sans relecteur (question ouverte de la spec).
- API de notifications de GitHub en complément de la recherche, et synchronisation du lu / non lu.
- Éviter les doublons avec les mails des forges (règle de messagerie à documenter).
- Module de la suite (barre d'activité, badge, palette commune), boîte à outils commune avec Fawkes
  et DragonTree (thèmes, raccourcis, notifications).
- Patronus : envoyer les événements « PR ouverte », « PR commentée », « CI en échec » comme
  déclencheurs (`evenement`, proposé par Patronus).
