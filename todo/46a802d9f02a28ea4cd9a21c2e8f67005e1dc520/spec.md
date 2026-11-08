# monorepo: plusieurs outils de verification selon le contexte — [verify] par chemin dans .tod.toml, selection par nom, chemin ou selon la tache

## Analyse de l'existant

`.tod.toml` (crates/dot/src/config.rs) connait une seule commande de
verification : `[verify] command`, une ligne shell executee par
`tod verify` a la racine du depot (la spec est dans le README racine,
section "Project configuration"). Le schema est `Config { sync, verify }`
avec `deny_unknown_fields` : toute extension doit rester stricte, une
cle inconnue doit toujours echouer.

Ce depot est lui-meme un monorepo, et ses crates n'ont pas le meme
role ni la meme facon de se verifier :

- `crates/dot` et `crates/cli` : la base cargo, tests rapides
  (`cargo test -p dot -p cli` en quelques secondes).
- `crates/server` (+ `migration`) : sa suite propre, plus lente
  (`cargo test -p server`).
- `crates/web` : la console Yew — **une interface pour l'humain**
  (voir l'avancement des taches, les modifier et les detailer,
  visualiser les diffs des branches). Ses evolutions n'ont pas
  besoin d'un outil de verification dedie, et les informations de
  configuration ne l'interessent pas : elle ne fait pas partie des
  contextes de verification.

Aujourd'hui le `.tod.toml` du depot dogfoode une unique
`command = "cargo test"` : la notion de contexte est plate, un depot
= une commande. Dans un monorepo, plusieurs outils de test sont
utiles selon le contexte de la tache (verifier la console ne veut
pas dire relancer toute la suite du serveur, et inversement).

## Recherches

- Les bonnes pratiques "CLI agentique" (scratchpad de session,
  recherche 2026-11-08) : un agent doit pouvoir verifier son propre
  travail de facon deterministe ; la verification utile est celle qui
  correspond a ce qui a change, et l'exit code reste le contrat avec
  l'agent, le hook ou la CI (inchange).
- Cargo resout le meme probleme par le chemin : `cargo test -p dot`.
- La tache #86a29 (tod verify par tache, liee aux scenarios) etend
  la verification cote definition of done : le present travail est la
  brique "quel outil executer selon le contexte" dont elle depend.

## Réflexions

Deux familles de design :

1. **Par tache** : un champ `Verify:` dans task.tod, chaque tache
   nomme sa commande. Rejete : duplique la connaissance du depot dans
   chaque tache, devient faux quand le depot evolue (une commande par
   chemin vit a cote du chemin).
2. **Par chemin (retenu)** : la configuration declare des outils,
   chacun scope a un chemin du depot. La branche de la tache dit deja
   ce qui a change (`git diff --name-only <base>...<branch>`) : la
   verification est la reunion des outils dont le chemin couvre les
   fichiers modifies. Zero metadonnee par tache, la config reste la
   seule source de verite du depot.

Format propose (le prefixe le plus long gagne quand deux chemins
couvrent le meme fichier ; ordre de declaration pour l'egalite) :

```toml
[verify]
# defaut du depot, quand aucune tache ne cible un outil precis
command = "cargo test"

[verify.tool.cli]
path = "crates/dot/"
command = "cargo test -p dot -p cli"

[verify.tool.server]
path = "crates/server/"
command = "cargo test -p server"
```

`crates/web` est volontairement absent : interface humaine, pas un
contexte de verification.

Semantique de `tod verify` :

- `tod verify` — la commande par defaut (comportement actuel, inchange).
- `tod verify <nom>` — l'outil nomme de la config (ex. `server`).
- `tod verify <id de tache>` — la reunion des outils couvrant les
  fichiers modifies par la branche de la tache ; sans outil couvrant,
  la commande par defaut. Sequentiels, premier echec = code de sortie
  de tod (le rapport de l'outil reste affiche sur stdout/stderr, le
  contrat de #bdff4 reste).

`Verify { command, tools: BTreeMap<String, VerifyTool> }` dans
config.rs ; `deny_unknown_fields` garde la stricteur. Le champ `path`
est un prefixe de chemin relatif a la racine, normalise (barre
oblique finale acceptee). La console web n'expose rien de la
configuration : hors du sujet.

## Travail à réaliser

1. Etendre `config.rs` : `VerifyTool { path, command }`, table
   `tools`, tests du schema (chemin, cle inconnue, prefixe le plus
   long).
2. `tod verify` : accepter un argument optionnel ; `<nom>` de l'outil,
   default sinon. Execution a la racine, exit code propage (deja le
   cas).
3. `tod verify <id>` : fichiers modifies de la branche de la tache
   (`git::` helpers : base = main, `--name-only`) -> reunion des
   outils couvrant (afficher la selection : "verified: cargo test -p
   server"), sinon defaut.
4. Dogfooder le `.tod.toml` du depot : outils `cli` (cargo test -p
   dot -p cli, la suite rapide) et `server` (cargo test -p server),
   defaut `cargo test` inchange. Rien pour crates/web : interface
   humaine, pas un contexte de verification.
5. Docs : README racine (section config), README dot et cli.
