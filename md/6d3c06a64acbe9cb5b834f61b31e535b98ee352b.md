# Creer un repository depuis le serveur (comme sur GitHub)

- **UID**: 6d3c06a64acbe9cb5b834f61b31e535b98ee352b
- **Context**: creer-un-repository-depuis-le-serveur-comme-sur-github
- **Status**: in progress
- **Created**: 2026-10-21 20:11:04 +0000
- **Progress**: 4/4

## Plan

- [x] server: POST /api/repos url vide -> creation bare + branches main/tod (commit orphelin) + worktree tod
- [x] repos.rs: prepare/relay tolerent un repository sans origin
- [x] console: URL vide = creation d'un nouveau repository + selection auto
- [x] smoke: creation via API + clone SSH + push + todos visibles + suite verte + README
