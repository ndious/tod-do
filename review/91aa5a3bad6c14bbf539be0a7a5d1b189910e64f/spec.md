# tod hooks: installer les hooks des agents (SessionStart injecte l'etat, PostCompact le reinjecte, Stop rappelle le check de l'etape)

## Analyse de l'existant

- `crates/cli/src/skill.rs` connait deja la selection d'agents
  (`Agent`: Claude Code, Cursor, Mistral Vibe, generique) et
  l'installation ecrite par `tod skill` / proposee par `tod init` :
  un dossier de skill par agent, commite dans le depot. Le skill est
  **non deterministe** — l'agent decide de le suivre.
- Le tod skill installe dit a l'agent de lancer `tod status` /
  `tod brief` au debut de session : rien ne garantit qu'il le fait,
  rien ne lui rappelle apres une compaction, rien ne bloque un
  arret a la moitie d'un plan.
- La recherche du 2026-11-08 (scratchpad) : la doc Claude Code
  oppose explictement instructions non deterministes (CLAUDE.md,
  skill) et controls deterministes (hooks) : « si on reecrit la
  meme ligne avec insistance sans effet, c'est un hook qui la
  remplace ». C'est la couche qui manque a tod.

## Recherches

**Claude Code** (docs hooks, code.claude.com/docs/en/hooks ; confirme
2026-11) :

- Hooks dans `.claude/settings.json` (portee projet, commite), par
  event ; format : `hooks: { "<Event>": [ { "matcher": ..., "hooks":
  [ { "type": "command", "command": "..." } ] } ] }`.
- `SessionStart` matche `startup | resume | clear | compact` : un
  hook sans matcher se declenche donc aussi **apres une compaction** —
  c'est le PostCompact de fait, un seul event a installer.
  stdout texte (exit 0) est ajoute au contexte de Claude.
- `Stop` : exit 2 + stderr **bloque l'arret** et donne le message a
  Claude ; l'entree JSON contient `stop_hook_active` (true quand
  Claude continue deja suite a un hook Stop) — le lire evite la
  boucle infinie : armer le blocage seulement quand il est false.

**Mistral Vibe** (skill vibe, confirme dans la doc embarquee) :

- Hooks dans `.vibe/hooks.toml` (portee projet, dossier trusted) :
  types `pre_tool` / `post_tool` / `post_agent`. Pas d'equivalent
  SessionStart : l'injection a l'ouverture reste au skill (chargee
  seule).
- `post_agent` tourne a chaque fin de tour ; reponse structuree
  stdout (exit 0) avec `{"decision":"deny","reason":...}` :
  `reason` injecte comme message utilisateur de relance, **cappe a
  3 relances par tour** — la protection anti-boucle est native.
- L'entree vient sur stdin : JSON avec `hook_event_name`.

## Réflexions

**Perimetre v1** : Claude Code et Mistral Vibe — les deux agents dont
le systeme de hooks est connu et documente. Cursor et l'agent
generique n'ont pas de format d'hooks equivalent : `tod hooks
--agent cursor|generic` refuse proprement et rappelle que le skill
reste le vehicule.

**Trois comportements, deux commandes machine :**

1. `tod hooks context` — l'etat compact a injecter (branche
   courante, ses taches actives avec [done total], la prochaine
   etape non cochchee de chaque plan, le rappel `tod brief <id>`).
   Quelques centaines de tokens max, calcule a la volee.
2. `tod hooks reminder` — lit stdin (JSON du protocole de l'agent) et
   s'adapte a `hook_event_name` :

   - `Stop` (Claude Code) : si la branche courante a une tache active
     dont le plan a des etapes non coches, ET `stop_hook_active`
     absent/false : exit 2 + stderr listant les etapes restantes
     (blocage, Claude continue, coche, puis s'arrete proprement) ;
     sinon exit 0 muet.
   - `post_agent` (Vibe) : meme condition ; reponse
     `{"decision":"deny","reason":...}` sur stdout ; le cap de 3
     relances/tour de Vibe fait le garde-body anti-boucle.

**Installateur** `tod hooks` (miroir de `tod skill`) : `--agent
<claude|vibe|cursor|generic>`, `--remove`, question interactive
sinon.

- Claude Code : fusion **idempotente** dans `.claude/settings.json`
  (lire, merger sous `hooks`, re-ecrire ; conserver ce qui existe
  deja). `SessionStart` (sans matcher : startup+resume+clear+compact,
  un seul event couvre l'injection et la re-injection post-compact :
  rien a faire du PostCompact disjoint) -> `tod hooks context` ;
  `Stop` -> `tod hooks reminder`.
- Vibe : append dans `.vibe/hooks.toml` d'un `[[hooks]]`
  `name = "tod-check"`, `type = "post_agent"`, `command = "tod hooks
  reminder"`. Idempotent : remplacer le bloc tod existant.
- Identification a la suppression : les entrees dont la commande
  contient `tod hooks` — les autres hooks du fichier restent
  intouches.

**Choix assumees a valider** : (a) le Stop de Claude Code bloque
l'arret tant qu'il reste des etapes non cochees — determination
forte, c'est le but recherche, mais c'est bloquant ; (b) le rappel
Vibe peut se declencher a chaque fin de tour tant qu'un plan est
en cours (cap 3/tour) — a ajuster a l'usage si trop bavard.

## Travail à réaliser

1. `tod hooks context` : l'etat compact (branche, taches actives,
   prochaine etape de chaque plan, rappel brief) + tests.
2. `tod hooks reminder` : lecture de stdin, `hook_event_name` ->
   protocole Stop (exit 2/stderr, garde `stop_hook_active`) ou
   post_agent (JSON deny/reason) + tests.
3. Installateur Claude Code : fusion idempotente dans
   `.claude/settings.json` (SessionStart + Stop), `--remove` ne
   touche que les entrees tod + tests.
4. Installateur Vibe : `.vibe/hooks.toml` post_agent idempotent,
   `--remove` + tests ; cursor/generic refusent proprement.
5. Le SKILL.md installer mentionne les hooks installes (l'agent sait
   que l'etat arrive tout seul en debut de session) ; docs READMEs.
6. Dogfood : installer les hooks dans ce depot (claude + vibe),
   verifier un arret bloque puis debloque par le check, commiter les
   fichiers d'hooks.
