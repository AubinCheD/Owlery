# Contrat local : ce dont Owlery a besoin (proposition)

Proposition à discuter avec Fawkes et DragonTree ; rien n'est encore implémenté chez eux. Elle
reprend la réflexion de Fawkes (`docs/reflexions.md`, §3.1 et §3.3) et n'en change pas le principe :
une demande passe par un canal nommé réservé au compte (tube nommé Windows), ou par la ligne de
commande quand l'application n'est pas ouverte.

## 1. Corriger avec Claude (Owlery → Fawkes)

**Besoin** : ouvrir une session Claude dans un worktree de la branche de la PR, dans le groupe du
dépôt, avec le contexte collé dans l'invite **sans être envoyé**.

**Ligne de commande** (forme prévue par Fawkes, plus deux options) :

```
fawkes.exe open --dir <dépôt> --claude --worktree <branche> --title "<titre>" --paste-file <fichier>
```

- `--worktree <branche>` : worktree de cette branche (créé s'il n'existe pas, réutilisé sinon).
- `--paste-file <fichier>` (**nouveau**) : texte collé dans l'invite de Claude une fois prête, sans
  Entrée. Un fichier plutôt qu'un argument : pas de limite de longueur ni de souci de guillemets.
  Owlery l'écrit dans son dossier temporaire et Fawkes le supprime après lecture.

**Message sur le canal** (le même contenu, en JSON, une ligne) :

```json
{"contrat": 1, "de": "owlery", "demande": "open",
 "dir": "D:\\depots\\contoso-api", "claude": true, "worktree": "export-csv",
 "titre": "contoso/api#138 — CI en échec", "coller": "CI en échec — Corrige l'export CSV…"}
```

Réponse : `{"contrat": 1, "ok": true}` ou `{"contrat": 1, "ok": false, "erreur": "…"}`.

**Fawkes fermé** : Owlery demande à l'utilisateur s'il veut démarrer Fawkes (spec §5) ; sans
réponse positive, rien n'est lancé.

**Garde-fous** : le texte collé n'est jamais envoyé à Claude par Fawkes ; le dossier doit exister et
être un dépôt git ; une demande venue d'ailleurs que du compte de l'utilisateur est refusée (le
tube est créé avec une liste de contrôle d'accès réduite au compte).

**En attendant** : Owlery propose *Copier le contexte pour Claude* (presse-papiers) et grise
*Corriger avec Claude*.

## 2. Ouvrir dans DragonTree (Owlery → DragonTree)

Aujourd'hui : `dragontree.exe <dossier>` (existe déjà). Souhaité ensuite :

```json
{"contrat": 1, "de": "owlery", "demande": "open", "dir": "D:\\depots\\contoso-api", "branche": "export-csv"}
```

`branche` : la sélectionner dans l'historique (sans la récupérer ni changer de branche active).

## 3. Comptes partagés avec DragonTree (contrat de suite, plus tard)

Owlery ne lit **pas** le coffre de DragonTree : il peut contenir des jetons d'autres comptes, et
Owlery n'a besoin que de lecture. Proposition :

- un service commun dans le Gestionnaire d'identifiants (« Suite », utilisateur `account:<id>`) pour
  les comptes que l'utilisateur choisit de **partager**, compte par compte, dans l'une ou l'autre
  application ;
- la liste des comptes partagés (sans jeton) dans un fichier commun de la suite :
  `%APPDATA%\Suite\accounts.json` (`id`, forge, hôte, nom, droits annoncés) ;
- chaque application garde ses comptes privés dans son propre service (« Owlery », « DragonTree ») ;
- dans la suite, l'hôte fournit la liste et le dialogue de compte ; les modules ne font que lire.

## 4. Badge (Owlery → suite)

Quand Owlery est un module : il expose `waiting()` (nombre de non lus) pour le badge de la barre
d'activité. Hors suite : rien.
