# interface agentique: drapeau --json sur toutes les commandes, erreurs structurees sur stderr et codes de sortie classifies (succes / general / auth / validation)

## Analyse de l'existant

La CLI est aujourd'hui une interface humaine uniquement :

- chaque commande affiche avec `println!` du texte libre, parfois
  melange (ex. `tod status` : en-tetes d'etats + lignes formatees,
  `tod brief` : le document composite). Rien n'est parsable sans
  regex fragiles.
- les erreurs remontent en `anyhow::Error` jusqu'a `main`, qui
  imprime `Error: <chaine implicite>` sur stderr et sort avec le
  code 1, quelle que soit la nature de l'erreur. Anecdotement,
  clap sort deja 2 sur un usage invalide.
- les warnings (sync offline, push non pousse) sont sur stderr —
  c'est deja le bon cote de la frontiere.
- `Task` derive `Serialize` (uti par `tod export`), mais c'est la
  serialization brute (y compris `content`, les dates ISO...), pas
  une surface concue pour un consommateur.
- `tod hooks context` et `tod hooks reminder` sont deja des
  protocoles machine (injection de contexte / reponse de hook),
  de meme que `tod completions` (script brut sur stdout).

L'agent le plus moteur ici (moi) parse aujourd'hui `tod status` et
`tod ls` avec les yeux ; un `--json` rend chaque verrouillage plus
fiable, et preparerait `tod next` (#741a9), les hooks d'injection
et le serveur.

## Recherches

Recherche du 2026-11-08 (scratchpad, section 1) — les convergences :

- **Machine-readable** : `--json` partout ; stdout = donnees,
  stderr = logs/warnings/erreurs ; erreurs **structurees** ;
  schema stable documente (`schemaVersion`).
- **Exit codes classes** : un agent regarde le code de sortie avant
  de parser. (Arcjet : 0 succes / 1 general / 2 auth / 3 validation
  / 4 confirmation requise ; clap sort deja 2 sur l'usage de la
  ligne elle-meme — vrai accident heureux.)
- **Surface curee** : Backlog.md expose une surface JSON
  « versionnee et curee » — pas les objets internautes.
- **Context-efficient** : la sortie d'un agent paye en tokens :
  JSON compact, champs utiles seulement.

## Réflexions

**Activation : drapeau `--json` global, explicite.** Pas de
detection TTY (pattern Arcjet) : un `tod ls | grep` humain casserait
et un pipe n'est pas fiable comme signal d'intention. L'agent
passe le drapeau, le contrat est visible dans le transcript.
V2 pourra ajouter `TOD_OUTPUT` ou la detection si le besoin monte.

**Enveloppe stable, versionee :**

```json
{ "schemaVersion": 1, "ok": true, "data": <specifique a la commande> }
{ "schemaVersion": 1, "ok": false,
  "error": { "code": "invalid_input", "message": "no task found with identifier 'xyz'" } }
```

Les erreurs JSON vont sur **stderr**, stdout reste vierge (un
consommateur peut rediriger stdout vers `jq` sans filtre).
2 codes dans l'enveloppe : `invalid_input` et `error` (v1 reste
honteusement sobre ; la famille grandira avec les besoins).

**Codes de sortie : 0 / 1 / 2.**

- 0 : succes (en JSON comme en texte).
- 1 : erreur generale (git, io, reseau).
- 2 : validation — identifiant inconnu ou ambigu, numero d'etape
  inexistant, etat inconnu, entree `tod log` vide, `.tod.toml`
  imparsable etc. Mime le code 2 de clap sur l'usage invalide.

L'« auth » de la description ne s'applique pas a la CLI (aucune
authentification locale) : reserve mais non attribue. La
classification se branche sans refonte des erreurs : la crate dot
gagne un marqueur `Invalid` (type implementant `std::error::Error`,
enveloppe par `anyhow::Error::new`) pose par les sites de
validation (match_task, check_step, set_state, config, log) ;
`main` inspecte la chaine (`Error::chain()`) et choisit le code.

**Surface curee pour les taches** (pas la serialization brute) :

```json
{ "uid": "919fea9...", "id": "919fe", "description": "...",
  "state": "todo", "branch": "t/919fea9-...", "tags": ["rust"],
  "progress": { "done": 2, "total": 5 },
  "next_step": { "number": 3, "text": "..." } }
```

**Perimetre par commande (v1) :**

- Lectures structurees : `ls`, `status`, `ctx` (taches curees),
  `brief` ({description, journal[], spec}), `log` (entrees),
  `scenario` ({feature}), `verify` ({command, exitCode}).
- Ecritures (toutes) : objet de confirmation minimal
  ({uid, state, description[, step]}).
- Admin (`init`, `skill`, `hooks`) : objets minimaux ({paths}).
- Exempions explicites, documentees : `completions` (script brut
  de shell sur stdout), `hooks context` / `hooks reminder`
  (protocoles machine deja definis, pas du JSON-enveloppe).
- En mode texte : comportement actuel strictement inchange.

**Ruptures assumees :** aucune (le drapeau est opt-in, le texte
reste le defaut). Le schema `data` etant specifique par commande,
sa stabilite n'engage que la version majeure ; `schemaVersion: 1`
assure la detectabilite d'une rupture future.

## Travail à réaliser

1. Socle : drapeau global `--json`, helpers d'ecriture de
   l'enveloppe, `dot::Invalid` pose sur les sites de validation,
   main: codes 0/1/2 + erreurs JSON sur stderr en mode `--json` +
   tests de classification.
2. Lectures : `ls`, `status`, `ctx` — surface curee des taches
   (uid, id, description, state, branch, tags, progress,
   next_step) + tests.
3. Lectures documentaires : `brief`, `log`, `scenario` — objets
   structures (journal et spec separes dans brief) + tests.
4. Ecritures : add, plan, sub, check, log, tag, state, x, rfr,
   test, rm, mv, start, finish, resolve — confirmations minimales
   en --json + tests.
5. Admin + verify : init, skill, hooks, verify — objets minimaux ;
   exemption documentee pour completions et les deux commandes
   hooks machine + tests.
6. Docs : READMEs (sections agent interface), SKILL.md (le contrat
   --json pour l'agent), dogfood (ls --json et brief --json
   parses par jq depuis un vrai shell, codes de sortie verifies) +
   committer.
