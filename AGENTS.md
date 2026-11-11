# AGENTS.md

Canonical instructions for AI coding agents (Codex, Claude Code and others) working in this repo. `CLAUDE.md` only imports this file.

## Project

`undo` (crate `undo-cli`) is Ctrl-Z for the terminal. It snapshots project directories into a private shadow git repository after each shell prompt and before each Claude Code tool call, and restores them on request. It is a single Rust binary (edition 2024, MSRV 1.89) that shells out to Git >= 2.25. See `ARCHITECTURE.md` for the module map.

## Commands

```sh
cargo build
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
python3 tests/distribution.py   # apt packaging tests; needs gpg and apt-ftparchive (apt-utils)
```

CI (`.github/workflows/ci.yml`) runs all of these, with tests on Ubuntu and macOS. `lefthook install` enables local hooks: gitleaks, rustfmt and typos on commit, clippy on push.

## Rules

- Tests must never touch the real `$HOME` or a real repo. Use `Sandbox` from `tests/common/mod.rs`, which isolates `HOME`, `XDG_DATA_HOME`, `UNDO_HOME` and git config in a temp dir.
- Bug fixes start with a failing test.
- Conventional Commits (`feat`, `fix`, `perf`, `test`, `docs`, `chore`, `ci`, `build`), small and atomic.
- Releases use cargo-dist (`dist-workspace.toml`). Pushing a version tag runs `release.yml`, which publishes the GitHub Release, the Homebrew tap and the signed apt repo (`publish-apt.yml`). Don't create or move tags unless asked.

## Environment

Development needs no env vars or secrets. `UNDO_HOME` overrides the data dir (default `$XDG_DATA_HOME/undo`). CI uses the repository secrets `APT_SIGNING_KEY`, `CODECOV_TOKEN` and `HOMEBREW_TAP_GITHUB_TOKEN` (or `HOMEBREW_TAP_TOKEN`). Never commit `.env*`, keys or `*.local.*` files.

## Specs and plans

Features follow a design → spec → plan → code workflow:

1. `docs/DESIGN.md`: the approved design from brainstorming.
2. `docs/SPEC.md`: exact behavior. It wins when it disagrees with the design.
3. `docs/PLAN.md`: ordered tasks, one commit each, with tests first.
4. `docs/ROADMAP.md`: later releases. Each one extends SPEC and PLAN before any code is written.

## Agent config

- `.agents/skills/`: shared skills. Codex reads them directly, and `.claude/skills/` symlinks to them. `verify-and-stop` proves acceptance criteria without expanding scope.
- `.claude/settings.json`: project plugins (`ponytail`, `commit-commands`, `rust-analyzer-lsp`).
- `.codex/config.toml`: enables `ponytail` for Codex.
- Personal overrides go in `.claude/settings.local.json` or `CLAUDE.local.md`, both gitignored.
