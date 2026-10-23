# undo: design

Date: 2026-10-23
Status: approved design, pending spec review

## Summary

`undo` is Ctrl-Z for the terminal. It continuously snapshots project directories into a private shadow git repository, so you can recover from `rm -rf`, `mv` overwrites, `sed -i`, `>` redirection, `git reset --hard`, `git clean -fdx`, and anything an AI coding agent did through its shell tool. It never parses commands. It snapshots directory state at safe moments and restores a chosen snapshot on request.

## Problem

- trash-cli and similar tools only cover `rm`, and only when you remember to call them.
- Git cannot recover uncommitted work discarded by `git reset --hard`, `git checkout .`, `git restore`, or `git clean`.
- Claude Code's `/rewind` only tracks the Write, Edit, and NotebookEdit tools. Its docs state that changes made through Bash commands are not captured, and that creating, moving, or deleting directories is not undone. An agent running `rm -rf src` or `git reset --hard` cannot be rewound.
- Intercepting individual commands (shell preexec hooks, PATH shims) is fragile. Shell hooks see unexpanded command text (verified for the bash `DEBUG` trap; zsh and fish hooks are documented the same way), they do not fire in non-interactive shells such as the ones agents use, and they miss scripts, `xargs`, `find -delete`, and redirection.

## Goals

- Recover the common destructive operations with one command: `undo`.
- Zero configuration inside git repositories.
- Never block, slow, or break the user's shell or agent.
- Be explicit about what cannot be recovered.

## Non-goals (v1)

- Windows (WSL counts as Linux and is supported).
- Protecting arbitrary paths outside protected directories (for example `~/Downloads` or `/etc`).
- Restoring git refs, HEAD, or commits. `git reflog` already covers those.
- Capturing intermediate states inside a single long-running command.
- A GUI, a daemon, or a filesystem watcher.

## Naming

- Command: `undo`.
- Package name: `undo-cli`. The `undo` crate on crates.io is an existing undo-redo library. The Homebrew formula name `undo` is free, so the formula is named `undo`.
- Repository: `dpologdvinity/undo`.

## Protected directories

- Inside a git repository, the snapshot root is the repository top level (`git rev-parse --show-toplevel`). No setup is needed.
- Outside git, a directory is protected only after `undo protect <dir>`. Protected roots are listed in `~/.local/share/undo/roots`.
- Nested roots: the innermost root wins.

## Snapshot contents

A snapshot includes:

1. Every file git would see under the root: tracked and untracked files, honoring the project's ignore rules.
2. Ignored files of 1 MB or less, because `git clean -fdx` destroys files like `.env` that are usually ignored.

A snapshot always excludes:

- The project's own `.git` directory (git skips it natively).
- Heavy directories, by name at any depth: `node_modules`, `target`, `.venv`, `venv`, `dist`, `build`, `.next`, `.cache`, `__pycache__`. In git roots this applies only to ignored files (step 2 below), so a tracked or unignored `build/` or `dist/` stays protected. In non-git roots it applies to everything.
- Nested git repositories (they would be recorded as gitlinks with no content). They are skipped and listed by `undo status`.

## Snapshot triggers

- **Shell `precmd`** (zsh `precmd`, bash `PROMPT_COMMAND`, fish `fish_prompt` event): after every command, run `undo snap --trigger shell --cmd "<last command>"` in the background, detached, so the prompt never waits. The state right before the next command is therefore already saved. Known gap: if the next command starts before the background snapshot finishes, that snapshot may be partial. Typing speed makes this rare, and it is documented.
- **Claude Code `PreToolUse` hook** with matcher `Bash|Write|Edit|NotebookEdit`: run `undo snap --trigger claude` synchronously, reading the tool input JSON from stdin to record the command or path. This captures exact pre-state. The hook always exits 0.
- **Manual `undo snap`**: also the integration point for other agents and editors.

`undo init zsh|bash|fish` prints the shell hook for `eval`. `undo init claude` adds the hook entry to `~/.claude/settings.json`, keeping existing hooks and writing a backup of the original file first.

## Storage

- One shadow repository per root at `~/.local/share/undo/repos/<sha256-of-root-path>.git` (bare). It is never placed inside the project, so the user's repository is never touched.
- Each shadow repository has its own index file, and git runs with `GIT_DIR`, `GIT_INDEX_FILE`, and `GIT_WORK_TREE` set explicitly. Every invocation also passes `-c core.hooksPath=/dev/null` and `-c core.fsmonitor=false` and ignores user git config that could change behavior.
- Snapshot algorithm:
  1. `git add -A`, honoring the project's `.gitignore` files. For non-git roots, the heavy-directory names are also written to the shadow repository's `info/exclude`.
  2. `git ls-files -o -i --exclude-standard -z` (git roots only), filtered to files of 1 MB or less outside heavy directories, then `git add -f` on that list.
  3. `git write-tree`. If the tree equals the previous snapshot's tree, stop: no new snapshot.
  4. `git commit-tree` with the previous snapshot as parent and a message holding the metadata (trigger, command, cwd, timestamp), then update `refs/heads/snapshots`.
- No-change snapshots stay cheap because of git's stat cache.
- Concurrency: an `flock` on `<repo>/undo.lock` serializes snapshots and restores per root. A snapshot that cannot get the lock within 2 seconds gives up and logs it.

## Retention

- Keep every snapshot for 7 days.
- From 7 to 30 days, keep one snapshot per hour.
- Delete snapshots older than 30 days.
- Each root has a 2 GB size cap by default. When the cap is exceeded, the oldest snapshots are dropped first.
- Pruning rewrites the snapshot chain, then runs `git gc --prune=now`. At most once per day per root, a snapshot starts a background prune.

## Commands

- `undo`: take a snapshot of the current state, then restore the most recent snapshot whose tree differs from the current state. Because the current state is saved first, running `undo` again returns to it.
- `undo list [-n N]`: ID, age, trigger, recorded command, and number of files changed compared with the previous snapshot.
- `undo diff <id>`: a file-level summary of what restoring `<id>` would change (added, modified, deleted). `--patch` shows the full diff.
- `undo restore <id> [-- <paths>...]`: restore a whole snapshot or just some paths.
- `undo protect <dir>` / `undo unprotect <dir>`.
- `undo status`: protected roots, snapshot count, disk usage, skipped nested repositories, recent errors, and the list of things undo cannot recover.
- `undo snap [--trigger T] [--cmd C]`: take a snapshot now.
- `undo init <zsh|bash|fish|claude>`.

## Restore semantics

- Restore runs inside the root of the current directory.
- Before restoring, undo always takes a snapshot, so every restore can be reversed.
- Scope: only paths a snapshot can contain. The project's `.git`, nested repositories, excluded heavy directories, and ignored files larger than 1 MB are never written or deleted.
- Within that scope, the working tree is made to match the target snapshot: deleted files are recreated, modified files are reverted, and files created since are removed. File mode (executable bit) and symlinks are restored as git stores them.
- Implementation: read the target tree into a temporary index, write files with `git checkout-index`, and delete in-scope files that are missing from the target tree.
- Path-limited restore only touches the given paths.
- Restore prints a summary of the changes (counts plus the first files) and does not ask for confirmation. It is reversible, and asking would slow down the panic moment.

## Failure policy

- Snapshot failures never surface at the prompt and never change the exit status the user or agent sees. Errors are appended to `~/.local/share/undo/log`, and `undo status` reports recent ones.
- If `git` is missing, or its version is older than 2.25, `undo snap` exits silently and `undo status` explains why.
- Restore failures are loud. Restore stops at the first write error and prints what was and was not restored. The pre-restore snapshot is still available.

## What undo cannot recover

Stated in `undo status`, `undo --help`, and the README:

- Commits, branches, and HEAD moved by git commands (use `git reflog`).
- Anything outside protected roots.
- Changes made and destroyed within one long-running command.
- Heavy directories, nested repositories, and ignored files larger than 1 MB.
- Work done before undo was installed, or since the last snapshot in a shell without the hook.

## Tech

- Rust, single binary, shelling out to the system `git` (2.25 or newer).
- Releases are built with `cargo-dist` for Linux (x86_64, aarch64) and macOS (x86_64, aarch64), published to GitHub Releases, a Homebrew tap, and crates.io as `undo-cli`.

## Testing

Integration tests, each in a fresh temporary directory with an isolated `XDG_DATA_HOME`. Each test creates files, runs `undo snap`, runs the disaster, runs `undo`, and asserts the tree matches the original:

- `rm -rf src`
- `mv a b` over an existing `b`
- `sed -i` on a tracked file
- `> file` truncation
- `git reset --hard` with staged and unstaged changes
- `git clean -fdx` including an ignored `.env`
- a Claude `PreToolUse` hook invocation with sample JSON on stdin
- `undo` twice returns to the post-disaster state
- path-limited restore leaves other paths alone
- ignored heavy directories and the project's `.git` are untouched by restore
- a tracked `build/` directory is snapshotted and restored
- a snapshot with no changes creates no new commit
- retention thinning keeps the expected snapshots

Unit tests cover root detection, the ignored-file size filter, and settings.json merging.

## Launch

- A README with a VHS-recorded GIF at the top: an agent deletes `src/` and resets git, then `undo` brings everything back.
- A comparison table of trash-cli, Claude Code `/rewind`, git, and undo, covering `rm`, `sed -i`, `git reset --hard`, `git clean`, and agent Bash commands.
- Posts: Show HN, r/ClaudeAI, r/commandline.
