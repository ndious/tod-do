# tod log <id>: journal de handoff par tache — entrees datees affichees en tete du brief, pour survivre a la compaction et changer de session

## Analyse de l'existant

Le dossier d'une tache porte aujourd'hui trois contenus : `task.tod`
(l'identite structuree), `plan.md` (la checklist en cours) et
`spec.md` (le prompt detaille, contenu redige — on l'etend, on ne le
regenere jamais). Rien ne capte **l'etat de travers** : ce que
l'agent vient d'apprendre, ce qui bloque, pourquoi on a change
d'approche a mi-chemin.

- le plan ne dit pas *pourquoi* une etape rebondit trois fois ;
- le spec est un contenu redige : y empiler les avalees de la
  session le corrompt ;
- les notes d'agent (scratchpad, transcripts) vivent et meurent
  avec la session : apres une compaction ou un `--resume`, ou en
  changeant d'agent, tout est perdu ;
- `tod brief <id>` est regenere a chaque appel — c'est le point
  d'entree naturel de la reprise, mais il ne montre que le passe
  redige, pas le present en cours.

La maquette du stockage (branche `tod`, un dossier par tache) est le
seul espace partage par l'humain, l'agent et la console, deja
synchronise (pull/push automatiques) et versionne : le journal doit
y vivre.

## Recherches

- Backlog.md traite le dev journal et le decision log comme des
  artefacts de premier plan du projet — pas comme des para-feuilles
  de session (recherche du 2026-11-08, scratchpad, section 6).
- Claude Code / Vibe : la reprise apres compaction est un pattern
  documente (« context recovery ») — reinjecter un fichier d'etat
  ecrit par le precedent tour. Le journal est ce fichier d'etat,
  transporte par le depot lui-meme.
- Le spec d'une tache precedente (#91aa5) a installe
  `tod hooks context` : l'injection de contexte au demarrage lit le
  store — le journal est donc une extension naturelle de cette
  brique (cite en extension, pas dans le perimetre v1).

## Réflexions

**Options de stockage rejetees :**

1. Une entree dans `plan.md` — le plan est une checklist, pas un
   journal ; melanger les deux rend le check bruyant.
2. Un champ multi-ligne `Log:` dans `task.tod` — format dot
   structure : la re-ecriture du fichier a chaque append est
   fragile et le parsing devient un melange de donnees et de
   narration.
3. Les notes de session du framework (scratchpad, transcripts) —
   non transportees, non partagees entre agents : exactement le
   probleme a resoudre.

**Retenu : un fichier dedie `log.md` dans le dossier de la tache**,
append-only, une entree par bloc markdown date :

```markdown
## 2026-11-08T19:12:44Z - ndious
l'entree : une a quelques lignes, l'etat de travers de la session.
```

**Semantique de la commande `tod log`** (symetrie avec
`spec` / `scenario` : sans argument on lit, avec on ecrit) :

- `tod log <id> "note"` — append d'une entree datee (auteur =
  `git user_name`, comme les taches), commit sur la branche tod
  (`tod: log <uid7>`). Commande **agent et humain** : c'est
  le protocole de handoff, pas une mutation de gestion.
- `tod log <id>` — le journal complet, du plus recent au plus
  ancien.
- `tod brief <id>` — les dernieres entrees en tete (les plus
  recentes d'abord, 10 par defaut) : la reprise de session voit
  d'abord ou en est le travail, avant le spec redige.

**Qui doit ecrire, quand** (a documenter dans le skill) : a la fin
d'une unite de travail, l'agent note ce qui est fait, ce qui
bloque et la prochaine intention ; l'humain y consigne ses
decisions mid-task. Le journal est append-only : jamais edite,
jamais regenere.

**Perimetre v1** : crate dot (module log + integration), CLI,
brief, skill, READMEs. La console web (affichage du journal dans
le volet de tache) et l'injection du journal par
`tod hooks context` sont des extensions ulterieures — citees ici
pour interdit de hors-sujet : rien a faire dans cette tache.

## Travail à réaliser

1. Crate dot : module `log` — blocs `log.md` (append date +
   auteur + texte, lecture, ordre recent d'abord) + tests du
   module.
2. CLI : `tod log <id>` (affiche le journal) et
   `tod log <id> "note"` (append + commit `tod: log <uid7>`) +
   tests (sans/index invalide = erreur claire).
3. `tod brief <id>` : les dernieres entrees du journal en tete
   (10 par defaut, mention du total), sous le titre de la tache et
   avant le spec + tests.
4. SKILL.md : quand ecrire une entree (fin d'unite de travail,
   blocage, decision), et que le brief la montre ; READMEs
   racine, dot et cli.
5. Dogfood : consigner de vraies entrees au fil de l'execution
   de cette tache (celle-ci incluse), verifier la reprise par le
   brief, committer.
