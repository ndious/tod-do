# web — the console of the tod server

A Yew 0.21 CSR (client-side rendered) single page app, styled with
the vendored [daisyUI](https://daisyui.com) full CSS (night theme,
no npm in the build). It talks to the [server API](../server) and
is served by the server itself as its static fallback.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
cd crates/web && trunk build   # produces crates/web/dist
```

The crate is **not** part of the workspace default members: a
plain `cargo build` at the root only builds dot, cli and server.
The server serves whatever sits in its `static_dir` (default
`../web/dist`) — a `trunk build` is enough, no copy needed.

## Pages (hash routing)

| route | page |
|---|---|
| `#/` | the repository list |
| `#/new` | create or register a repository |
| `#/repo/{id}` | the task listing: list view by default (expandable states) or kanban board |
| `#/account` | the user: password change, SSH keys |

## The task listing

One column (board view) or one expandable section (list view) per
state: backlog, todo, review, test, done, canceled. A card shows
its tags, branch and plan progress. Clicking a card opens the
**side panel**: state selector, branch badge, tags editor, the
plan with clickable steps, the spec (the authored prompt), the
Gherkin scenarios, and the diff of the branch against main.

## Implementation notes

- `main.rs` — everything: the API helpers (`ApiOutcome` carries
  the server error messages), the `Route` enum + hashchange
  listener, `TaskCard`, `TaskDetail` (the side panel), `CloneBar`,
  `AccountPage`, `AddForm`, `Login`.
- `styles/main.css` — layout only (the components come from
  daisyUI); `styles/daisyui.css` is the vendored theme.
- The bearer token survives page refreshes in localStorage.
- `dist/` is committed: the server serves it directly.
