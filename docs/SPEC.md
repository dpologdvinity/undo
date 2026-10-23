# undo — Specification (v0.1)

`undo` is Ctrl-Z for the terminal. It snapshots project directories into a
private shadow git repository at safe moments (after each shell command, and
before each AI-agent tool call). One command, `undo`, restores the last
good state. It never parses or intercepts commands.

This spec turns the approved design
(`docs/superpowers/specs/2026-10-23-undo-design.md`) into exact,
implementable behavior. If they disagree, this spec wins for v0.1. Build order
is in `docs/PLAN.md`, and later work is in `docs/ROADMAP.md`.

## 1. Scope

### In (v0.1)
- Protected roots: git repositories automatically, plus other directories via `undo protect`.
- Snapshot triggers: zsh/bash/fish prompt hook (background), Claude Code `PreToolUse` hook (synchronous), and manual `undo snap`.
- Commands: `undo`, `list`, `diff`, `restore`, `protect`, `unprotect`, `status`, `snap`, `init`, `prune`.
- Retention: all snapshots for 7 days, hourly from 7 to 30 days, nothing older, and a 2 GiB cap per root.
- Linux and macOS (WSL counts as Linux).

### Out (v0.1)
- Windows-native, a daemon or watcher, or a GUI.
- Restoring git refs, HEAD, commits, stashes, or the staging area (the index). Only working-tree file contents, modes and symlinks are restored.
- Paths outside protected roots.
- Intermediate states inside one long-running command.

## 2. Tech stack (fixed)

| Item | Choice |
|---|---|
| Language | Rust, edition 2024, MSRV 1.89 (needed for `std::fs::File::lock`/`try_lock`) |
| Crate / package name | `undo-cli` (crates.io `undo` is taken). The binary name is `undo`. |
| Repo | `github.com/dpologdvinity/undo` |
| Dependencies | `clap` 4.6 (`derive`), `serde` 1 (`derive`), `serde_json` 1 (`preserve_order`), `anyhow` 1, `sha2` 0.11 |
| Dev dependencies | `tempfile` 3 |
| External runtime | system `git` ≥ 2.25 (for `--pathspec-from-file`) |
| Release | `dist` (cargo-dist): GitHub Releases + Homebrew tap |

No other dependencies. No chrono: times are Unix epoch seconds from git (`%ct`),
and ages are computed with `std::time::SystemTime`.

## 3. Files and directories

Data home `$D`: `$UNDO_HOME` if set, else `$XDG_DATA_HOME/undo`, else
`$HOME/.local/share/undo` (macOS uses the same path).

```
$D/
├── roots                  # protected non-git roots, one absolute canonical path per line, sorted, no dups
├── log                    # error log, "<epoch> <root|-> <message>\n"; when >1 MiB keep the last 512 KiB
└── repos/
    └── <hex sha256 of canonical root path>.git/   # bare shadow repo, one per root
        ├── undo-root      # the root path (plain text), for status/debugging
        ├── undo-index     # shadow index (GIT_INDEX_FILE)
        ├── undo.lock      # lock file
        ├── last-prune     # epoch seconds of last prune
        ├── info/exclude   # heavy dir names (non-git roots only)
        └── refs/heads/snapshots
```

Canonical path means `std::fs::canonicalize` (which resolves symlinks).

## 4. Root detection

`find_root(cwd) -> Option<Root>` where `Root { path, is_git }`:
1. Git candidate: run `git -C <cwd> rev-parse --show-toplevel` with `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE` removed from the environment. On success, canonicalize the output.
2. Protected candidates: every line of `$D/roots` that equals `cwd` or is an ancestor of it (compare canonical paths component-wise, not as string prefixes).
3. Pick the candidate with the most path components (innermost wins). `is_git` is true if the chosen path came from step 1.
4. None means not protected.

`undo protect <dir>`: canonicalize, then refuse `/` and `$HOME` unless `--force` (error: `refusing to protect <dir>: too broad (use --force)`). Add the path to `roots`, then take a first snapshot. `unprotect` removes the line, leaves the shadow repo alone, and prints its path plus the hint `remove with: rm -rf <path>`.

## 5. Running git against the shadow repo

All shadow operations go through one helper, `Git::shadow(root)`, which builds a
`std::process::Command` for `git` with:

- Working dir: the root.
- Environment cleared of every var starting with `GIT_`, then set:
  - `GIT_DIR=<repo>`, `GIT_WORK_TREE=<root>`, `GIT_INDEX_FILE=<repo>/undo-index`
  - `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1` (user config can't change behavior)
  - `GIT_AUTHOR_NAME=undo`, `GIT_AUTHOR_EMAIL=undo@localhost`, `GIT_COMMITTER_NAME=undo`, `GIT_COMMITTER_EMAIL=undo@localhost`
  - `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`
- Leading args: `-c core.hooksPath=/dev/null -c core.fsmonitor=false -c core.autocrlf=false -c core.safecrlf=false -c gc.auto=0 -c core.quotePath=false -c advice.addEmbeddedRepo=false`.
- For git roots, also `-c core.excludesFile=<root>/.git/info/exclude` if that file exists. In linked worktrees, use `git rev-parse --git-common-dir` from step 4.1 to locate `info/exclude`.
- All path lists use `-z` / NUL separators.

Create the repo lazily on first snapshot with `git init --bare -q <repo>`, then write `undo-root`.

Version check: `git --version` is parsed once per process. If it is below 2.25, `snap` logs `git >= 2.25 required` and exits 0. All other commands print that error and exit 1.

## 6. Snapshot

### 6.1 Capture (`capture_tree(root) -> TreeId`)

Heavy dir names (const): `node_modules target .venv venv dist build .next .cache __pycache__`.

1. Non-git root: make sure `<repo>/info/exclude` contains one line `<name>/` per heavy name.
2. `git add -A` (stage all tracked, modified and untracked files, honoring `.gitignore`).
3. Remove nested repos: `git ls-files -s -z`. For every entry with mode `160000`, run `git rm --cached -q -z --pathspec-from-file=- --pathspec-file-nul` with those paths, and record them in `<repo>/undo-nested` (one path per line, overwritten each time) for `status`.
4. Git roots only, to add small ignored files: run `git ls-files -o -i --exclude-standard -z -- . ':(exclude,glob)**/<heavy>/**' …` (one exclude pathspec per heavy name). Keep paths whose `symlink_metadata` is a regular file of at most 1,048,576 bytes, or a symlink. Pass them to `git add -f --pathspec-from-file=- --pathspec-file-nul`. Skip the call if the list is empty.
5. `git write-tree` gives the TreeId.

### 6.2 Commit (`snap(root, meta) -> Option<CommitId>`)

1. Take the lock (§6.4). If it isn't acquired within 2 s, log `lock timeout` and return None.
2. `tree = capture_tree(root)`.
3. `parent = git rev-parse -q --verify refs/heads/snapshots` (absent on the first snapshot).
4. If a parent exists and `git rev-parse <parent>^{tree}` equals `tree`, return None (no change).
5. Commit message:
   ```
   <trigger>: <cmd, first line, truncated to 200 chars, or "(none)">

   undo-trigger: <shell|claude|manual|pre-restore|protect>
   undo-cmd: <cmd, newlines replaced by "\n" literal, max 2000 chars>
   undo-cwd: <cwd>
   ```
   `git commit-tree <tree> [-p <parent>] -F -` with the message on stdin.
6. `git update-ref refs/heads/snapshots <new> [<parent>]`.
7. If `now − last-prune ≥ 86400`, spawn `undo prune --root <root>` detached, with all stdio set to null and not waited on.

### 6.3 Snapshot IDs

- `undo` displays IDs as `@N` (N = 0 is the newest, counting along the first-parent chain) plus an 8-hex short SHA.
- Every command that takes `<id>` accepts `@N`, or a hex prefix of at least 4 characters that resolves uniquely among snapshot commits (`git rev-parse --verify -q <prefix>^{commit}`, then confirm it is in `git rev-list refs/heads/snapshots`).

### 6.4 Locking

Open (create) `<repo>/undo.lock`, then poll `File::try_lock()` every 50 ms up to the deadline (2 s for snap and 10 s for restore and prune). The OS releases the lock when the file handle closes.

### 6.5 `undo snap` (CLI)

`undo snap [--trigger <t>] [--cmd <c>] [--cwd <dir>]`. Default trigger is `manual`, and cwd is the process cwd.

- `--trigger claude`: read stdin (max 1 MiB) as JSON. `cwd` comes from `.cwd`. `cmd` comes from `.tool_input.command` if `.tool_name == "Bash"`, else `"<tool_name> <tool_input.file_path or notebook_path>"`. Invalid JSON is logged, and the process cwd is used instead.
- Not in a root: exit 0 silently.
- Must print nothing to stdout or stderr, and always exit 0. Errors go to `$D/log`.
- `UNDO_DISABLE=1` makes it exit 0 immediately.
- Overall deadline is 10 s (if exceeded, log `snapshot timed out` and exit 0). Enforce it by running git children with a watchdog thread that kills the child.

## 7. Restore

### 7.1 `undo restore <id> [-- <paths>…]`

1. `find_root(cwd)`. If there is none, error `not inside a protected directory (use a git repo or 'undo protect <dir>')` and exit 1.
2. Lock with a 10 s deadline.
3. Pre-restore snapshot: run §6.2 steps 2–6 with trigger `pre-restore` and cmd `undo restore <id>`. Let `cur` be the tree just captured, whether or not a new commit was made.
4. `target` = the tree of `<id>`.
5. Paths, if given, are made relative to the root (they are relative to the cwd as given, and must stay inside the root or the command errors). They become pathspecs.
6. `git diff-tree -r -z --no-renames --name-status <cur> <target> [-- <pathspecs>]`. Status `D` means delete from the working tree. `A`, `M` and `T` mean write.
7. If nothing differs, print `already matches <id>` and exit 0.
8. Deletes first: remove each path (`remove_file`, which also works for symlinks), then remove now-empty parent directories bottom-up, stopping at the root.
9. Writes: create a temporary index `<repo>/restore-index-<pid>`, run `GIT_INDEX_FILE=<tmp> git read-tree <target>`, then `GIT_INDEX_FILE=<tmp> git checkout-index -f -z --stdin` with the write paths on stdin. Delete the temp index afterwards, including on error.
10. On the first error, stop and print `restore stopped: <error>` along with what was done. Mention that `undo restore @0` returns to the pre-restore state. Exit 1.
11. Success output:
    ```
    restored @3 3f2a91c4 (12m ago, before: rm -rf src)
      recreated 14, reverted 2, removed 1
      + src/main.rs
      + src/lib.rs
      ~ README.md
      - scratch.txt
      … 13 more
    run `undo` again to go back
    ```
    Show at most 10 path lines. Diff status `A` (in the target but not now) prints `+` and counts as recreated. `M` or `T` prints `~` and counts as reverted. `D` prints `-` and counts as removed. The "before" text is the `undo-cmd` of the next newer snapshot (the command that ran after this state), or it is omitted if there is none. No confirmation prompt, because the action is reversible.

### 7.2 `undo` (no args)

1. Lock, then pre-restore snapshot as in §7.1 step 3, giving tree `cur`.
2. Walk `git rev-list --first-parent refs/heads/snapshots` from newest to oldest and pick the first commit whose tree is not `cur`.
3. If there is none, print `nothing to undo` and exit 0.
4. Otherwise run §7.1 steps 4–11 for that commit (the lock is already held).

Calling `undo` twice toggles. The second call's pre-restore snapshot holds the
restored state, so the newest differing snapshot is the post-disaster state.

## 8. Read-only commands

### 8.1 `undo list [-n N]` (default 20)

`git log --first-parent -n N --format=%H%x00%ct%x00%B%x1e refs/heads/snapshots`.
"Changed" = number of lines from `git diff-tree --no-commit-id -r --name-only -z <c>` (for the first snapshot, use `git ls-tree -r --name-only -z <c>`).

```
@0  3f2a91c4  just now  pre-restore   4 files  undo restore @2
@1  a81b0c55  2m ago    shell         1 file   rm -rf src
@2  0be7d913  9m ago    claude       12 files  Edit src/lib.rs
```
Ages: `just now` (under 60 s), `Nm ago`, `Nh ago`, `Nd ago`. The cmd is truncated to the terminal width minus the other columns (use `COLUMNS`, default 100).

### 8.2 `undo diff <id> [--patch]`

`cur = capture_tree(root)` with no commit. Then print `git diff-tree -r --name-status <cur> <id-tree>` using the same `+ ~ -` symbols as §7.1. With `--patch`, run `git diff <cur> <id-tree>` with `--color=auto`, inheriting stdout so git's pager logic applies, and passing `-c core.pager=less -FRX`.

### 8.3 `undo status`

```
root:       /home/me/proj (git)
snapshots:  42 (newest 2m ago, oldest 6d ago)
disk:       38.2 MiB (cap 2.0 GiB)
hooks:      shell yes · claude yes
skipped:    vendor/lib (nested git repo)
errors:     none in last 24h
cannot recover: commits/branches/HEAD (use git reflog), the staging area,
  anything outside protected roots, changes made and destroyed inside one
  command, heavy dirs (node_modules target .venv venv dist build .next .cache
  __pycache__ when ignored), ignored files > 1 MiB, nested git repos
```
- Disk usage comes from `git count-objects -v` (`size` + `size-pack`, in KiB).
- Shell hook detection: the env var `UNDO_SHELL_HOOK=1` is exported by the init script. Claude hook detection: `~/.claude/settings.json` contains the command string `undo snap --trigger claude`.
- Errors: lines in `$D/log` from the last 24 h for this root, showing the last 5.
- Outside a root, show only the hooks, errors, and the "cannot recover" list.

## 9. Retention (`undo prune [--root <dir>]`)

Default root: from the cwd. Lock with a 10 s deadline, and write `last-prune` = now at the start.

1. List the first-parent chain, oldest first, as `(commit, tree, ct, message)`.
2. Keep rule: age < 7 d keeps all. 7 d ≤ age < 30 d keeps only the newest commit per `ct / 3600` bucket. Age ≥ 30 d drops everything. Always keep the newest commit, even if it's old.
3. If the kept set equals the full chain, skip to step 5.
4. Rewrite: for each kept commit, oldest first, run `git commit-tree <tree> [-p <newparent>] -F -` with the original message and `GIT_AUTHOR_DATE`/`GIT_COMMITTER_DATE` = `@<ct> +0000`. Then `update-ref refs/heads/snapshots <newest-new>`.
5. `git reflog expire --expire=now --all` then `git gc --prune=now --quiet`.
6. Cap: if disk (§8.3) is over `UNDO_MAX_BYTES` (default 2 GiB), drop the oldest ⌈10%⌉ of kept commits (minimum 1, never the newest), then repeat steps 4–6. Stop when under the cap or only one commit remains.

## 10. `undo init <zsh|bash|fish|claude>`

Shell targets print a script to stdout for `eval "$(undo init zsh)"` (fish: `undo init fish | source`). The scripts live in `src/hooks/undo.{zsh,bash,fish}` and are embedded with `include_str!`.

zsh:
```zsh
export UNDO_SHELL_HOOK=1
__undo_last_cmd=""
__undo_preexec() { __undo_last_cmd="$1" }
__undo_precmd() {
  [[ -n "$UNDO_DISABLE" ]] && return
  ( command undo snap --trigger shell --cmd "$__undo_last_cmd" >/dev/null 2>&1 & )
}
autoload -Uz add-zsh-hook
add-zsh-hook preexec __undo_preexec
add-zsh-hook precmd __undo_precmd
```

bash:
```bash
export UNDO_SHELL_HOOK=1
__undo_precmd() {
  local s=$?
  if [[ -z "$UNDO_DISABLE" ]]; then
    local c; c=$(HISTTIMEFORMAT= history 1 | sed 's/^ *[0-9]* *//')
    ( command undo snap --trigger shell --cmd "$c" >/dev/null 2>&1 & )
  fi
  return $s
}
case ";${PROMPT_COMMAND};" in *";__undo_precmd;"*) ;; *) PROMPT_COMMAND="__undo_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}";; esac
```
(`PROMPT_COMMAND` may be an array in bash ≥ 5.1. If `declare -p PROMPT_COMMAND` shows `-a`, prepend an element with `PROMPT_COMMAND=(__undo_precmd "${PROMPT_COMMAND[@]}")` instead. Implement both branches.)

fish:
```fish
set -gx UNDO_SHELL_HOOK 1
function __undo_postexec --on-event fish_postexec
    set -q UNDO_DISABLE; and return
    command undo snap --trigger shell --cmd "$argv[1]" >/dev/null 2>&1 &
    disown 2>/dev/null
end
```

`claude` target: edit `~/.claude/settings.json`:
1. Read the file. If it is missing, start from `{}`. If the JSON is invalid, error and change nothing.
2. If any `hooks.PreToolUse[*].hooks[*].command` equals `undo snap --trigger claude`, print `already installed` and exit 0.
3. Back up the original to `settings.json.undo-backup-<epoch>`, if it existed.
4. Append `{"matcher":"Bash|Write|Edit|NotebookEdit","hooks":[{"type":"command","command":"undo snap --trigger claude"}]}` to `hooks.PreToolUse`, creating any missing objects or arrays.
5. Write to `settings.json.tmp` with 2-space pretty printing and key order preserved, then rename it over the original.
6. Print `installed Claude Code hook in <path> (backup: <path>)`.

The Claude Code hook input is JSON on stdin with `cwd`, `hook_event_name`, `tool_name`, and `tool_input` (`.command` for Bash, `.file_path` for Write/Edit). The hook must exit 0 with empty stdout, which means "no decision". This was confirmed against the Claude Code hooks docs on 2026-10-23.

## 11. Exit codes and errors

| Code | Meaning |
|---|---|
| 0 | success, `nothing to undo`, `already matches`, or any `snap` |
| 1 | runtime error (not in root, unknown id, restore failure, git missing) |
| 2 | usage error (clap's default) |

Errors are printed as `undo: <message>` on stderr. `snap` and the background `prune` never print anything.

## 12. Crate layout

```
undo/
├── Cargo.toml
├── src/
│   ├── main.rs        # clap CLI, dispatch, exit codes
│   ├── paths.rs       # data home, repo path (sha256), canonicalization helpers
│   ├── root.rs        # find_root, roots file, protect/unprotect
│   ├── git.rs         # Git::shadow builder, run/output helpers, watchdog, version check
│   ├── lock.rs        # lock with deadline
│   ├── snapshot.rs    # capture_tree, snap, message format, claude stdin parsing
│   ├── restore.rs     # restore, undo, summary output
│   ├── view.rs        # list, diff, status, age formatting
│   ├── prune.rs       # retention
│   ├── init.rs        # shell scripts + claude settings merge
│   ├── log.rs         # error log append + truncate
│   └── hooks/{undo.zsh,undo.bash,undo.fish}
├── tests/
│   ├── common/mod.rs  # sandbox: tempdir, UNDO_HOME, HOME, git config, tree hashing
│   └── *.rs           # one file per area
├── README.md
├── LICENSE            # MIT
└── dist-workspace.toml  # generated by `dist init`
```

## 13. Testing requirements

Every integration test runs in a sandbox: a fresh `tempfile::TempDir` with `HOME`,
`UNDO_HOME`, and `XDG_DATA_HOME` pointed inside it. Fixture repos are created with
`git -c user.name=t -c user.email=t@t init`. The binary is invoked through
`env!("CARGO_BIN_EXE_undo")`. Tree equality uses a helper that hashes
`(relative path, file type, mode & 0o111, contents or link target)` for every
entry under the root, excluding `.git/`.

Each of these scenarios is a separate test: snap, disaster, `undo`, then assert the tree equals the pre-disaster tree.
1. `rm -rf src`
2. `mv a b` over an existing `b`
3. `sed -i.bak 's/x/y/' f && rm f.bak` (portable across GNU and BSD)
4. `: > file` truncation
5. `git reset --hard` with staged and unstaged changes (assert working tree only)
6. `git clean -fdx` deleting an ignored `.env`
7. Claude trigger: pipe `{"cwd":"<root>","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"rm -rf src"}}` into `undo snap --trigger claude`, run the disaster, then `undo`
8. `undo` twice returns to the post-disaster tree
9. Path-limited restore changes only the given path
10. Ignored `node_modules/` and the project `.git/` are byte-identical before and after restore
11. Tracked `build/` is restored
12. No-change snap creates no commit (`list` count unchanged)
13. Prune: let `h = ((now − 8 d) / 3600) * 3600` (an hour boundary). Create commits with forced dates (`GIT_COMMITTER_DATE`) at `now − 31 d`, `h − 7200`, `h + 600`, `h + 1200`, and `now − 1 h`, then prune, and assert that exactly `h − 7200`, `h + 1200`, and `now − 1 h` remain
14. Ignored file > 1 MiB is not snapshotted and not deleted by restore
15. Nested git repo is skipped and listed in `status`
16. `snap` outside any root exits 0 with no output
17. `init claude` merge: existing settings with other hooks keep them, a second run is idempotent, and a backup is created

Unit tests: root selection (innermost wins, and component-wise prefix so `/a/bc` is not under `/a/b`), age formatting, ID parsing (`@N`, prefix), commit message formatting and truncation, and the keep rule in §9 step 2 as a pure function.

CI runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` on `ubuntu-latest` and `macos-latest`.

## 14. Definition of done (v0.1)

- Everything in §13 passes in CI on both OSes.
- Manual check, in a real zsh or bash session with the hook: `rm -rf src` followed by `undo` restores, and the prompt shows no visible lag. In a real Claude Code session with the hook: have the agent run `git reset --hard` on uncommitted work, then `undo` restores it.
- README with a GIF (VHS), install instructions, a hook setup section, the comparison table, and the "cannot recover" list.
- `dist` builds for x86_64/aarch64 Linux and macOS.
