# Architecture & Technical Design

`undo` is a low-latency, zero-configuration local state recovery system for developers and autonomous AI coding agents. It provides a local "Ctrl-Z" safety net for accidental deletions, destructive terminal commands (`rm -rf`, `sed -i`), and unintended git operations (`git reset --hard`, `git clean -fd`).

---

## 1. Core Principles & Mental Model

1. **Isolation from Project Git State:**
   `undo` maintains an independent shadow Git repository under `~/.local/share/undo/repos/<hash>.git/`. It **never** writes to or alters the target repository's `.git/` directory, branch refs, index, or reflog.
2. **Deterministic Root Detection:**
   A project root is either the innermost enclosing Git repository or an explicitly protected directory configured via `undo protect`.
3. **Double-Safety on Restore:**
   Before any snapshot is restored into the working tree, `undo` captures an immediate pre-restore snapshot (`@0`). Any restore operation is itself reversible with another `undo`.
4. **Agent & Shell Native:**
   Automatic hooks integrate with interactive shells (`bash`, `zsh`, `fish`) via precmd/prompt hooks and with AI agent harnesses (Claude Code `PreToolUse` hook).

---

## 2. System Architecture & Components

```
                                +-------------------+
                                | CLI Entry Point   |
                                |   (main.rs)       |
                                +---------+---------+
                                          |
          +-------------------------------+-------------------------------+
          |                               |                               |
+---------v---------+           +---------v---------+           +---------v---------+
|  Root Detection   |           |    Paths & Lock   |           |     Git Runner    |
|   (root.rs)       |           | (paths.rs/lock.rs)|           |     (git.rs)      |
+---------+---------+           +---------+---------+           +---------+---------+
          |                               |                               |
          +-------------------------------+-------------------------------+
                                          |
    +-----------------+-------------------+-----------------+-----------------+
    |                 |                   |                 |                 |
+---v-----------+ +---v-----------+ +-----v---------+ +-----v---------+ +-----v---------+
| Snapshot      | | Restore Engine| | View Engine   | | Pruning Engine| | Hook Installer|
| (snapshot.rs) | | (restore.rs)  | | (view.rs)     | | (prune.rs)    | | (init.rs)     |
+---------------+ +---------------+ +---------------+ +---------------+ +---------------+
```

### Component Breakdown

| Module | File | Role & Invariants |
|---|---|---|
| **CLI Dispatch** | `src/main.rs` | Parses command-line arguments using `clap` derive. With no subcommand, restores the newest snapshot whose tree differs from the current working tree. Formats errors as `undo: <msg>` and exits 1. |
| **Root Detection** | `src/root.rs` | Evaluates innermost enclosing root. Resolves Git roots using `git rev-parse --show-toplevel` and `--git-common-dir` to support worktrees. Reads protected roots from `~/.local/share/undo/roots`. `undo protect` refuses `/` and `$HOME` without `--force`. |
| **Paths & Hashing** | `src/paths.rs` | Computes shadow repository directory as `~/.local/share/undo/repos/<sha256(canonical_root_path)>.git`. The data directory honors `UNDO_HOME`, then `$XDG_DATA_HOME/undo`. Sets the data, `repos`, and shadow repository directories to `0700`. |
| **Concurrency Lock** | `src/lock.rs` | Advisory file lock on `<repo>/undo.lock`, polled every 50 ms until a deadline (2 seconds for snapshots, 10 seconds for restore, diff, and pruning) to prevent concurrent shadow corruption. |
| **Git Runner** | `src/git.rs` | Invokes `git` subprocesses under strict isolation: overrides `GIT_DIR`, `GIT_WORK_TREE`, and `GIT_INDEX_FILE`. Disables system and global configs (`GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`). Disables hooks and fsmonitor (`-c core.hooksPath=/dev/null`, `-c core.fsmonitor=false`). Suppresses attribute-driven conversion by writing `* -text -filter -ident -working-tree-encoding` to the shadow repository's `info/attributes`, so snapshots store bytes unchanged. Enforces minimum Git version >= 2.25.0. |
| **Snapshot Engine** | `src/snapshot.rs` | Creates atomic commits in shadow repo. Stages tracked and untracked files. Preserves ignored files `<= 1 MiB` (e.g., `.env`) while excluding heavy dependencies (`node_modules`, `target`, build artifacts). Prunes nested git repository boundaries from the index to avoid corrupting submodules or nested clones. |
| **ID Resolution** | `src/ids.rs` | Maps `@N` indexes (`@0` is newest) and unique commit prefixes of at least four hex characters (full SHAs included) to shadow commits. |
| **Restore Engine** | `src/restore.rs` | Restores working tree state to match snapshot. Captures pre-restore snapshot `@0`. Calculates additions, modifications, and deletions. Writes files through `git checkout-index` into a `0700` staging directory, sets regular files to owner read/write plus the executable bits Git wrote (`0600`, or `0711` for executables under a `022` umask), and renames them into place. Leaves untouched files and nested repos unmodified. |
| **View Engine** | `src/view.rs` | Implements `undo list`, `undo diff`, and `undo status`. Formats relative timestamps (e.g., `42s ago`, `3m ago`) and parses snapshot commit headers. |
| **Pruning Engine** | `src/prune.rs` | Background lifecycle manager. Retains snapshots according to decay policy: keep all snapshots for 7 days, 1/hour for days 7–30, drop older ones, and drop the oldest while a root exceeds its 2 GiB cap. |
| **Hook Manager** | `src/init.rs` | Prints the embedded shell hooks (`undo.bash`, `undo.zsh`, `undo.fish`) for `eval` or `source`, and merges the `PreToolUse` hook into `~/.claude/settings.json` with backup preservation. |
| **Logging** | `src/log.rs` | Non-panicking append logger for errors and warnings. When the log exceeds 1 MiB, keeps only its newest 512 KiB. |

---

## 3. Data Flow & Snapshot Lifecycle

### Capture Workflow (`undo snap`)

```mermaid
sequenceDiagram
    participant User/Hook as Shell/Agent Hook
    participant Snap as undo snap
    participant Root as Root Detector
    participant Lock as Repo Lock
    participant Git as Shadow Git Repo

    User/Hook->>Snap: Trigger (precmd / PreToolUse)
    Snap->>Root: find_root(cwd)
    alt Outside any protected/git root
        Snap-->>User/Hook: Silent Exit 0
    else Root resolved
        Snap->>Lock: acquire(repo, deadline: 2s)
        Snap->>Git: add -A, then drop nested repos from the index
        Snap->>Git: ls-files -o -i, add -f ignored files <= 1 MiB outside heavy dirs
        Snap->>Git: write-tree
        alt Tree matches the latest snapshot's tree
            Snap-->>User/Hook: No-op (Exit 0)
        else State changed
            Snap->>Git: commit-tree with metadata
            Snap->>Git: update-ref refs/heads/snapshots
            Snap-->>User/Hook: Snapshot committed (Exit 0)
        end
    end
```

### Restore Workflow (`undo` / `undo restore <id>`)

```mermaid
sequenceDiagram
    participant User as Terminal User
    participant Restore as undo restore [id]
    participant Lock as Repo Lock
    participant Snap as Snapshot Engine
    participant Target as Working Tree

    User->>Restore: run "undo" (newest snapshot that differs) or "undo restore <id>"
    Restore->>Lock: acquire(repo, deadline: 10s)
    Restore->>Snap: Resolve explicit ID to target commit SHA
    Restore->>Snap: Snapshot current tree (pre-restore snapshot @0)
    Restore->>Target: Compute diff between current tree and target tree
    Restore->>Target: Delete removed files, then write staged files and rename into place
    Restore-->>User: Print restored file summary
```

---

## 4. Security & Safety Invariants

1. **Strict File Permissions:**
   The data directory, `repos/`, and each shadow repository directory are mode `0700` (`rwx------`), and the error log is `0600`. Restored regular files are `0600` (`rw-------`) plus the executable bits Git wrote for executable entries, which is `0711` under a `022` umask. Git stores only the executable bit, so original sharing permissions are not restored.
2. **Nested Repository Protection:**
   Nested git repositories within the project root are discovered and excluded from shadow gitlinks (`git rm --cached`). The restore process strictly avoids traversing or modifying files inside nested git repositories.
3. **Resource Bounding:**
   - Command timeouts: Watchdog monitors Git child processes and kills long-running calls exceeding deadlines.
   - Ignored file threshold: Ignored files larger than 1 MiB are excluded from snapshots.
   - Error log capping: Once the log exceeds 1 MiB, only its newest 512 KiB is kept.
   - Storage pruning: Background pruning cleans obsolete shadow commits automatically.
