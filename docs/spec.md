# Owlery — spécification

Reprise et approfondissement de la réflexion du 21/10/2026 (Fawkes, `docs/reflexions.md` §3.4,
« Boîte de réception du développeur »).

## 1. Ce qui arrive dans la boîte

| Type | Source | Exemple |
|---|---|---|
| Revue demandée | PR où l'on est relecteur | « contoso/api #142 : Ajoute la pagination » |
| Ma PR a bougé | commentaires, revue, conflits, CI | « Ta PR #138 : 2 commentaires, CI rouge » |
| CI en échec | pipeline d'une branche suivie | « main : build en échec (tests) » |
| Mention | commentaire qui me cite | « @moi dans fabrikam/web #57 » |
| Ticket assigné | issue ou work item | « northwind/app #12 : Erreur à l'export » |

Chaque entrée garde son lien, son dépôt, sa branche, son auteur, sa date et son état (non lu, lu,
reporté, terminé).

## 2. Interface

- **Liste** groupée par dépôt (ou par type, au choix), non lus en tête, compteur par groupe.
- **Filtres** : type, forge, dépôt, « seulement ce qui m'attend ».
- **Détail** : résumé (titre, description courte, état de la CI, derniers commentaires), sans
  chercher à remplacer la page web.
- **Actions** sur une entrée :
  - *Ouvrir* (navigateur) ;
  - *Ouvrir dans DragonTree* (dépôt et branche) ;
  - *Corriger avec Claude* : demande à Fawkes une session Claude dans un worktree de la branche, avec
    le contexte collé (lien, échec de CI ou commentaire) sans être envoyé ;
  - *Me le rappeler* (plus tard, demain, lundi) ;
  - *Marquer terminé*.
- **Palette** : *Aller à la boîte de réception*, *PR à relire*, *CI en échec*.
- **Badge** dans la barre d'activité de la suite (nombre de choses qui m'attendent).

## 3. Données

- **Comptes** : forges et jetons partagés avec DragonTree (coffre de Windows, Gestionnaire
  d'identifiants) ; jamais dans un fichier de l'application.
- **Cache** des entrées et de leur état dans le dossier de l'application, versionné comme l'état de
  Fawkes (version qui l'a écrit, copie avant mise à jour, champs inconnus gardés).
- **Fréquence** : lecture périodique (quelques minutes, réglable), plus un bouton *Actualiser* ;
  respect des limites d'appels des API (en-têtes de quota, attente).

## 4. Sécurité et garde-fous

- Lecture seule par défaut : approuver, fusionner ou commenter passe toujours par un clic
  explicite, avec confirmation pour fusionner.
- Jetons avec les droits minimaux nécessaires ; le dialogue de compte dit lesquels.
- Aucun contenu de PR ou de ticket n'est envoyé à Claude sans action de l'utilisateur
  (*Corriger avec Claude*).
- Les liens ouverts passent par les mêmes règles que Fawkes (http et https seulement).

## 5. Intégrations

- **DragonTree** : comptes communs ; *Ouvrir dans DragonTree* ; DragonTree peut afficher un badge
  « PR à relire » sur ses dépôts.
- **Fawkes** : *Corriger avec Claude* par le contrat local (ouvrir un shell ou une session dans le
  groupe du dépôt, sans démarrer Fawkes s'il n'est pas ouvert : l'utilisateur choisit).
- **Patronus** : les mêmes événements (PR, CI) servent de déclencheurs de tâches.
- **Contrat local** : demandes reçues et émises par le canal nommé commun.

## 6. Questions ouvertes

- Utiliser l'API de notifications des forges (GitHub en a une) ou interroger chaque liste ?
- Comment éviter les doublons avec les notifications par mail des forges ?
- Faut-il une vue « équipe » (PR des collègues sans relecteur) ?
- Synchroniser l'état lu / non lu avec la forge, ou le garder local ?
