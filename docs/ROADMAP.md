# undo — Roadmap

Each release gets its own spec and plan update before code: extend `docs/SPEC.md`
and add tasks to `docs/PLAN.md`. Scope stays fixed per release.

## v0.1 — Core
Spec: `docs/SPEC.md`. Plan: `docs/PLAN.md`.
- Shadow-git snapshots of git repos and protected dirs.
- zsh/bash/fish prompt hooks, and the Claude Code `PreToolUse` hook.
- `undo`, `list`, `diff`, `restore`, `status`, `protect`, `prune`.
- Retention of 7 d full, 30 d hourly, and a 2 GiB cap.

Exit: all SPEC §13 tests pass on Linux and macOS, and the manual checks are done.

## v0.2 — More agents, less friction
- `undo init codex|cursor|gemini`: hook installers for other agents that support pre-tool hooks (check each agent's current hook docs first), with the same merge, backup and idempotency rules as §10.
- `undo init claude --project`: write to `<repo>/.claude/settings.json` instead of the user settings.
- `undo restore --staged`: also restore the staging area. Store the project index tree as a second tree in each snapshot (via a `undo-index-tree:` trailer, from the project's `git write-tree`) and restore it with `git read-tree`. This needs care around merge conflicts, so refuse if the index has unmerged entries.
- `undo list --grep <text>`, and `undo list --trigger claude`.

## v0.3 — Performance and edge cases
- Benchmark no-change snapshots on 10k, 50k, and 200k-file repos, then publish the numbers in the README.
- Respect `core.untrackedCache` and `core.fsmonitor` from the project repo when safe, so huge repos stay fast.
- WSL `/mnt/c`: detect slow drvfs and warn once in `status`.
- Nested git repos: snapshot each nested repo as its own root instead of skipping it.
- Configurable heavy dirs and size limit through `$D/config.toml` (only if users ask, since v0.1 has no config on purpose).

## v0.4 — Restore UX
- `undo pick`: an interactive TUI chooser over `list`, with a diff preview.
- `undo restore <id> --to <dir>`: extract a snapshot elsewhere without touching the working tree.
- `undo show <id> <path>`: print a file's content from a snapshot.

## v1.0 — Stability
- Freeze the on-disk layout (§3) and the commit message trailers. Add a `$D/version` file with a migration path.
- Shell completions (`undo completion zsh|bash|fish`).
- Packaging: Homebrew core submission, AUR, and Nix.

## Launch checklist
- VHS GIF: an agent deletes `src/` and resets git, then `undo` brings everything back.
- Comparison table (trash-cli, `/rewind`, git, undo).
- Posts: Show HN, r/ClaudeAI, and r/commandline.
- Issue templates: bug report (with `undo status` output) and new agent hook request.

## Parked (not scheduled)
- An optional filesystem watcher daemon for editors and GUIs that run no shell commands (the v0.1 non-goal stands until users ask).
- Windows-native support (a PowerShell prompt hook).
- Encrypting snapshots at rest.
