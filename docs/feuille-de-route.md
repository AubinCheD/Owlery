# Feuille de route

État au 21/10/2026. Version 0.2.0 : comptes multiples (GitHub, GitLab, Azure DevOps), notifications,
contrat local, revue du premier jet corrigée. Elle suit le plan `docs/superpowers/plans/2026-10-21-owlery-0.2.md` (design :
`docs/superpowers/specs/2026-10-21-owlery-0.2-design.md`). Les décisions et les échanges sont dans
`docs/journal.md`.

Une priorité suivie de « (estimée) » ne ressort pas des échanges : c'est une proposition.

## Sommaire

| # | Point | Qui | Priorité |
|---|---|---|---|
| V1 | Premier essai contre GitHub réel, avec le jeton de l'utilisateur | utilisateur | haute |
| V2 | Essai réel de GitLab et d'Azure DevOps | utilisateur | moyenne (estimée) |
| 0.2 | Version 0.2 (tâches 0 à 12 du plan) | Claude | faite (revue de branche corrigée) |
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

Voir le plan. Les tâches 0 et 2 ont chacune leur commit ; les tâches 1 et 3 à 11 sont arrivées dans un
même commit (le modèle et l'interface ont été refaits d'un bloc, voir le journal).

| Tâche | Contenu | État |
|---|---|---|
| 0 | superpowers, design, plan, feuille de route | fait |
| 1 | corrections de la revue du premier jet | fait |
| 2 | toolchain autonome (`scripts/setup.ps1`) | fait |
| 3 | comptes multiples, lecture par compte | fait |
| 4 | GitLab | fait |
| 5 | Azure DevOps | fait |
| 6 | détail : CI en échec, derniers commentaires | fait |
| 7 | notifications Windows, icône | fait |
| 8 | instance unique, canal d'Owlery | fait |
| 9 | dossiers locaux des dépôts | fait |
| 10 | Corriger avec Claude (Fawkes) | fait |
| 11 | annuler, tout marquer lu, dépôts ignorés, vue gardée | fait |
| 12 | version 0.2.0, revue de branche | fait |

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
