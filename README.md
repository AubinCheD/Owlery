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

## Premières étapes

1. Lecture des PR à relire et des CI en échec sur GitHub (un compte), liste simple.
2. Regroupement par dépôt, état lu / non lu, report.
3. GitLab et Azure DevOps.
4. *Corriger avec Claude* : demande à Fawkes par le contrat local.
5. Notifications Windows.

Détail : [`docs/spec.md`](docs/spec.md).
