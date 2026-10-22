# spec.md rédigé : le prompt détaillé d'une tâche

## Analyse de l'existant

Le spec.md était généré par `tod brief` : il répétait les
métadonnées de `task.tod` (uid, état, branche, dates) et le plan —
aucune valeur ajoutée, et il écrasait toute contribution à chaque
`sub`/`check` via `refresh_brief`. Le CLI (`brief`, `spec`), le
serveur (`add_sub`, `check_step`) et le scaffold du skill agent
reposaient tous sur cette génération.

## Recherches

Les commandes qui touchent au spec : `plan` (création), `brief`
(lecture), `spec` (lecture/édition). Côté serveur, seul
`refresh_brief` écrivait. Le module `dot::brief` expose
`markdown()`, `write_brief()`, `read()`, `SPEC_FILE`. Le skill
agent (SKILL_MD) documentait encore l'ancien stockage
(contextes, plan.dot, md/) et appelait le brief « generated ».

## Réflexions

Le spec doit être le **prompt détaillé** : analyse de l'existant,
recherches, réflexions, travail à réaliser. Rédigé par l'humain et
l'agent, jamais régénéré — sinon toute écriture est perdue au
`sub`/`check` suivant. Les métadonnées restent dans `task.tod`,
les étapes dans `plan.md` : le spec ne fait que décrire le travail.
Le module devient `dot::spec` : scaffold + lecture, plus aucune
génération. `tod brief` reste l'alias de lecture (le workflow
agent du skill l'utilise). Les 23 spec.md générés du repo sont du
boilerplate sans contenu rédigé : purge, les prochains seront
rédigés.

## Travail à réaliser

- Renommer `dot::brief` en `dot::spec` : `scaffold()` (analyse,
  recherches, réflexions, travail), `read()`, `write_scaffold()`.
- CLI : `plan` scaffolde le spec ; `brief`/`spec` lisent (scaffold
  si absent), `-e` édite ; plus de `refresh_brief` sur `sub`/`check`.
- Serveur : plus de régénération ; le TaskView sert le spec lu.
- Skill agent et README : documenter le spec rédigé.
- Purger les spec.md générés de la branche tod.
