# stockage par etat: backlog todo review test done canceled + dossier par tache (task.tod, spec.md, plan.md)

- **UID**: 36900a151d602ec5a608a3da981c590b8057e288
- **State**: todo
- **Branch**: t/36900a1-stockage-par-etat-backlog-todo
- **Created**: 2026-10-22 12:45:46 +0000
- **Progress**: 4/5

## Plan

- [x] dot: layout par etat, task.tod avec champ Branch:, dossier t/ nomme par hash, find/load/move par etat
- [x] dot: plan.md et spec.md dans le dossier de la tache (fin du plan.dot central et du dossier md)
- [x] cli: tod state <id> <etat> + mapping start->todo, rfr->review, x->done, rm->canceled ; status/ls/ctx par etat et branche
- [x] server+web: colonnes kanban par etat, champ state/branch, endpoint state, suppression AddForm context
- [ ] migration de notre branche tod + tests + README + cycle complet
