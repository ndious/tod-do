# monorepo: plusieurs outils de verification selon le contexte — [verify] par chemin dans .tod.toml, selection par nom, chemin ou selon la tache

## Analyse de l'existant

`.tod.toml` (crates/dot/src/config.rs) connait une seule commande de
verification : `[verify] command`, une ligne shell executee par
`tod verify` a la racine du depot (la spec est dans le README racine,
section "Project configuration"). Le schema est `Config { sync, verify }`
avec `deny_unknown_fields` : toute extension doit rester stricte, une
cle inconnue doit toujours echouer.

Ce depot est lui-meme un monorepo : `crates/dot`, `crates/cli`,
`crates/server` (cargo, default members) et `crates/web` (Yew/wasm,
construit par `trunk`, **hors** des default members). Aujourd'hui le
`.tod.toml` du depot dogfoode `command = "cargo test"` : `crates/web`
n'est verifie par personne, et verifier tout le workspace a chaque
verification est lent quand la tache travaille un seul crate.

La notion de "projet" dans la config est donc aujourd'hui plate : un
depot = une commande. Dans un monorepo, plusieurs outils de test sont
utiles selon le contexte de la tache.

## Recherches

- Les bonnes pratiques "CLI agentique" (voir le scratchpad de session,
  recherche 2026-11-08) : un agent doit pouvoir verifier son propre
  travail de facon deterministe ; la verification utile est celle qui
  correspond a ce qui a change, et l'exit code reste le contrat avec
  l'agent, le hook ou la CI (inchange).
- Cargo resout le meme probleme par le chemin : `cargo test -p dot`
- La tache #86a29 (tod verify par tache, liee aux scenarios) etend
  la verification cote definition of done : le present travail est la
  brique "quel outil executer selon le contexte" dont elle depend.

## Réflexions

Deux familles de design :

1. **Par tache** : un champ `Verify:` dans task.tod, chaque tache
   nomme sa commande. Rejete : duplique la connaissance du depot dans
   chaque tache, devient faux quand le depot mueve (une commande par
   chemin vit a cote du chemin).
2. **Par chemin (retenu)** : la configuration declare des outils,
   chacun scope a un chemin du depot. La branche de la tache dit deja
   ce qui a change (`git diff --name-only <base>...<branch>`) : la
   verification est la reunion des outils dont le chemin couvre les
   fichiers modifies. Zero metadonnee par tache, la config reste la
   seule source de verite du depot.

Format propose (le prefixe le plus long gagne quand deux chemins
couverte le meme fichier ; ordre de declaration pour l'egalite) :

```toml
[verify]
# defaut du depot, quand aucune tache ne cible un outil precis
command = "cargo test"

[verify.tool.web]
path = "crates/web/"
command = "trunk build"

[verify.tool.server]
path = "crates/server/"
command = "cargo test -p server"
```

Semantique de `tod verify` :

- `tod verify` — la commande par defaut (comportement actuel, inchange).
- `tod verify <nom>` — l'outil nomme de la config (ex. `web`).
- `tod verify <id de tache>` — la reunion des outils couvrant les
  fichiers modifies par la branche de la tache ; sans outil couvrant,
  la commande par defaut. Sequentiels, premier echec = code de sortie
  de tod (rapport de l'outil deja affiche sur stdout/stderr, ce qui
  reste le contrat de #bdff4).

`Verify { command, tools: BTreeMap<String, VerifyTool> }` dans
config.rs ; `deny_unknown_fields` garde la stricteur. Le champ `path`
c'est un prefixe de chemin relatif a la racine, normalise
(barre oblique finale acceptee).

## Travail à réaliser

1. Etendre `config.rs` : `VerifyTool { path, command }`, table
   `tools`, tests du schema (chemin, cle inconnue, prefixe le plus
   long).
2. `tod verify` : accepter un argument optionnel ; `<nom>` de l'outil,
   default sinon. Resolution des outils + execution a la racine, exit
   code propagate (deja le cas).
3. `tod verify <id>` : fichiers modifies de la branche de la tache
   (`git::` helpers : base = main, `--name-only`) -> reunion des
   outils couvrant (afficher la selection : "verified: cargo test,
   trunk build").
4. Dogfooder le `.tod.toml` du depot : outil `web` = `trunk build`
   (a la condition que le build de la console soit verifiable sans
   navigateur), et garder `cargo test` comme defaut.
5. Docs : README racine (section config), README dot et cli.
