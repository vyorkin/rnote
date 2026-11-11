## Agent skills

### Issue tracker

GitHub Issues on `origin` (`vyorkin/rnote`), via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default canonical labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: root `GLOSSARY.md` plus `docs/adr/`. See `docs/agents/domain.md`.

## Repo gates

Run these before opening a PR, from the meson build folder (`just setup-dev` once, if `_mesonbuild` is missing):

- `just fmt-check`
- `just check`
- `just lint`
- `just test`

`CONTRIBUTING.md` documents the same commands; `just test` expects `cargo-nextest`.
