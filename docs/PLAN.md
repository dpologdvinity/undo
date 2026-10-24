# undo — Implementation Plan (v0.1)

Read `docs/SPEC.md` first. Section references (§) point there. Do the tasks in order,
one commit each, using Conventional Commits. Don't commit `CLAUDE.md`, `AGENTS.md`,
`.claude/`, `.superpowers/`, `.codex/`, or `.mcp.json`.

Before each commit:
```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

**Safety rule for every task:** tests must never run against the real `$HOME` or a
real repo. Always use the sandbox from Task 2. If a test needs a disaster command,
run it inside the sandbox root only.

---

## Task 1: Scaffold

Files: `Cargo.toml`, `src/main.rs`, `LICENSE`, `.gitignore`

1. `cargo init --name undo-cli`, then set `[[bin]] name = "undo"`, `path = "src/main.rs"`, `edition = "2024"`, and `rust-version = "1.89"`.
2. Dependencies:
   ```toml
   [dependencies]
   clap = { version = "4.6", features = ["derive"] }
   serde = { version = "1", features = ["derive"] }
   serde_json = { version = "1", features = ["preserve_order"] }
   anyhow = "1"
   sha2 = "0.11"

   [dev-dependencies]
   tempfile = "3"
   ```
3. In `main.rs`, define the clap derive CLI with the subcommands `snap`, `restore`, `list`, `diff`, `status`, `protect`, `unprotect`, `init`, and `prune` (§6.5, §7–§10), and make running with no subcommand mean `undo` (§7.2). Each handler is `todo!()` except `--version`. `main` returns `ExitCode`. Errors print `undo: {e:#}` and return 1.
4. `.gitignore`: `/target`, `.env`. `LICENSE`: MIT, `Copyright (c) 2026 Kaitlyn Bassford`.

Check: `cargo run -- --version` and `cargo run -- --help` list every subcommand.

Commit: `chore: scaffold rust crate and cli`

## Task 2: Test sandbox

Files: `tests/common/mod.rs`, `tests/sandbox.rs`

```rust
pub struct Sandbox { pub dir: TempDir, pub home: PathBuf, pub root: PathBuf }
impl Sandbox {
    pub fn new() -> Self;                       // creates home/, home/.local/share/undo, root/
    pub fn git_root() -> Self;                  // new() + git init root + initial commit of a few files
    pub fn undo(&self, args: &[&str]) -> Output;          // runs CARGO_BIN_EXE_undo in root with env below
    pub fn undo_stdin(&self, args: &[&str], stdin: &str) -> Output;
    pub fn sh(&self, script: &str) -> Output;            // sh -c in root with same env
    pub fn tree_hash(&self) -> String;                    // §13 helper, excludes .git/
    pub fn write(&self, rel: &str, contents: &str);
}
```
Env for every child: `HOME=<home>`, `UNDO_HOME=<home>/.local/share/undo`, `XDG_DATA_HOME=<home>/.local/share`, `GIT_CONFIG_GLOBAL=<home>/.gitconfig` (written with `user.name=t`, `user.email=t@t`, `init.defaultBranch=main`), `GIT_CONFIG_NOSYSTEM=1`, and `UNDO_SHELL_HOOK` removed.

Check: a test that creates a sandbox, writes files, and checks that `tree_hash` changes when content changes and stays the same when only mtime changes.

Commit: `test: add integration sandbox`

## Task 3: Paths, log, lock

Files: `src/paths.rs`, `src/log.rs`, `src/lock.rs`

- `paths::data_home() -> PathBuf` (§3), `paths::repo_for(root: &Path) -> PathBuf` (sha256 hex of `root.as_os_str().as_encoded_bytes()`).
- `log::error(root: Option<&Path>, msg: &str)`, which never panics or returns an error (it swallows I/O errors), and does the 1 MiB truncation (§3).
- `lock::acquire(repo: &Path, deadline: Duration) -> Result<File>` (§6.4).

Unit tests: `repo_for` is stable and differs per path; log truncation keeps the tail; and a second lock attempt times out while the first is held.

Commit: `feat: add data paths, error log, and repo lock`

## Task 4: Root detection and protect

Files: `src/root.rs`, `tests/root.rs`

- `pub struct Root { pub path: PathBuf, pub is_git: bool, pub git_common_dir: Option<PathBuf> }`
- `pub fn find_root(cwd: &Path) -> Option<Root>` (§4)
- `fn pick_innermost(git: Option<PathBuf>, protected: &[PathBuf], cwd: &Path) -> Option<(PathBuf, bool)>`. Make it pure and unit-test it, including the `/a/bc` vs `/a/b` case.
- `protect(dir, force)`, `unprotect(dir)`, `read_roots()`, `write_roots()` (sorted, deduped, atomic via tmp + rename)

Integration tests: a git root is detected from a subdirectory, a protected non-git dir is detected, a protected dir inside a git repo wins, `protect $HOME` is refused without `--force`, and outside everything gives None.

Commit: `feat: detect protected roots and add protect/unprotect`

## Task 5: Git runner

Files: `src/git.rs`

```rust
pub struct Git { cmd_base: ... }
impl Git {
    pub fn plain() -> Git;                         // user env minus GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE (for rev-parse in §4)
    pub fn shadow(root: &Root, repo: &Path) -> Git; // §5 env + -c args
    pub fn index(self, path: &Path) -> Git;         // override GIT_INDEX_FILE
    pub fn run(&self, args: &[&str]) -> Result<String>;              // stdout as String, error includes stderr
    pub fn run_stdin(&self, args: &[&str], input: &[u8]) -> Result<String>;
    pub fn run_status(&self, args: &[&str]) -> Result<bool>;         // exit status only (for rev-parse -q --verify)
}
pub fn check_version() -> Result<()>;   // >= 2.25 (§5)
pub fn set_deadline(d: Instant);        // global watchdog: children past deadline are killed (§6.5)
```
Watchdog: after spawn, poll `child.try_wait()` every 10 ms. If the deadline passes, `child.kill()` and return the error `timed out`. Write stdin from a separate thread to avoid pipe deadlock with large path lists.

Unit test: parse versions `git version 2.55.0`, `git version 2.39.3 (Apple Git-146)`, and `2.24.1` (rejected).

Commit: `feat: add git runner for shadow repos`

## Task 6: Snapshot

Files: `src/snapshot.rs`, `tests/snapshot.rs`

- `pub fn capture_tree(root: &Root, repo: &Path) -> Result<String>` (§6.1)
- `pub struct Meta { pub trigger: String, pub cmd: Option<String>, pub cwd: PathBuf }`
- `pub fn format_message(m: &Meta) -> String` (§6.2 step 5). Make it pure and unit-test it.
- `pub fn snap(root: &Root, m: &Meta) -> Result<Option<String>>` (§6.2, takes the lock itself)
- `pub fn snap_locked(root, repo, m) -> Result<(String /*tree*/, Option<String>)>` (no lock, used by restore)
- `pub fn parse_claude_input(json: &str) -> (Option<PathBuf>, Option<String>)` (§6.5)
- `cli_snap(args)`: wires §6.5, including `UNDO_DISABLE`, silent output, always `Ok`, and the 10 s deadline.

Integration tests (SPEC §13): no-change snap makes no commit (12), an ignored 2 MiB file is excluded while an ignored 10-byte `.env` is included (14 partial), a nested repo is removed from the index and listed in `undo-nested` (15 partial), a non-git root excludes `node_modules/`, a git root keeps a tracked `build/` (11 partial), and outside a root gives exit 0 with empty stdout and stderr (16).

Commit: `feat: capture snapshots into shadow repo`

## Task 7: Snapshot IDs

Files: `src/snapshot.rs` (or `src/ids.rs`)

`pub fn resolve(root, repo, id: &str) -> Result<String /*commit*/>` (§6.3). Errors: `unknown snapshot <id>` and `ambiguous snapshot prefix <id>`.

Unit test for `@N` parsing. Integration test for prefix resolution.

Commit: `feat: resolve snapshot ids`

## Task 8: Restore and undo

Files: `src/restore.rs`, `tests/restore.rs`

- `pub fn restore(cwd: &Path, id: &str, paths: &[PathBuf]) -> Result<()>` (§7.1)
- `pub fn undo(cwd: &Path) -> Result<()>` (§7.2)
- `fn summarize(changes: &[(char, String)], …) -> String` (§7.1 step 11). Make it pure and unit-test the 10-line cap and the counts.

Integration tests: SPEC §13 scenarios 1–6, 8, 9, and 10, each as its own `#[test]`. For scenario 5, stage a change (`git add`) plus an unstaged change, run `git reset --hard`, then `undo`, and assert both edits are back in the working tree.

Commit: `feat: restore snapshots and add undo`

## Task 9: list, diff, status

Files: `src/view.rs`, `tests/view.rs`

- `fn age(secs: u64) -> String`, pure and unit-tested at the boundaries 59, 60, 3599, 3600, and 86400.
- `list(cwd, n)`, `diff(cwd, id, patch)`, `status(cwd)` (§8)

Tests: list shows `@0` first with the trigger and cmd, diff after a delete shows `+ path` for the restore direction, and status contains `cannot recover:`.

Commit: `feat: add list, diff, and status`

## Task 10: Prune

Files: `src/prune.rs`, `tests/prune.rs`

- `pub fn keep(commits: &[(String, u64 /*ct*/)], now: u64) -> Vec<bool>`, pure (§9 step 2) and unit-tested.
- `pub fn prune(root: &Root) -> Result<()>` (§9 steps 1–6)
- Hook into `snap` (§6.2 step 7) by spawning `std::env::current_exe()` with `prune --root <root>`.

Integration test: SPEC §13 scenario 13. Build the commits directly with `git commit-tree` in the shadow repo using `GIT_COMMITTER_DATE`, rather than waiting.

Commit: `feat: prune snapshots by age and size`

## Task 11: init (shell + Claude)

Files: `src/init.rs`, `src/hooks/undo.zsh`, `src/hooks/undo.bash`, `src/hooks/undo.fish`, `tests/init.rs`

- Shell scripts exactly as in §10, embedded with `include_str!`.
- `fn merge_claude_settings(existing: Option<&str>) -> Result<Option<String>>`, pure. It returns `None` when already installed. Unit-test it against: no file, `{}`, existing unrelated `PreToolUse` hooks, an existing `hooks` object with other events, already installed, and invalid JSON (error).
- `init claude` does the backup and atomic write (§10 steps 3–6) against `$HOME/.claude/settings.json`.

Integration tests: SPEC §13 scenario 17. For the shell scripts: `bash -n` and `zsh -n` (skip if zsh is missing) syntax-check the output. A bash interactive test runs `bash --norc -i -c 'eval "$(undo init bash)"; __undo_precmd; sleep 1'` in the sandbox root and asserts a snapshot appeared.

Commit: `feat: add shell and claude code hook installers`

## Task 12: Claude trigger end-to-end

Files: `tests/claude.rs`

SPEC §13 scenario 7. Also: invalid JSON on stdin still exits 0 and logs to `$UNDO_HOME/log`.

Commit: `test: cover claude code hook trigger`

## Task 13: README, CI, release

Files: `README.md`, `.github/workflows/ci.yml`, `dist-workspace.toml` (generated), `demo.tape`

- README sections: the pitch ("Ctrl-Z for your terminal and your AI agent"), the GIF, install (`brew install dpologdvinity/tap/undo`, `cargo install undo-cli`, release binaries), setup (`eval "$(undo init zsh)"` in `~/.zshrc`, and `undo init claude`), usage, how it works (shadow git repo, nothing touches your `.git`), the "cannot recover" list (§8.3), a comparison table (trash-cli, Claude Code `/rewind`, git, and undo against `rm`, `sed -i`, `git reset --hard`, `git clean`, and agent Bash commands), privacy (snapshots stay local under `~/.local/share/undo`, and contain ignored files ≤ 1 MiB such as `.env`), and uninstall.
- `ci.yml`: matrix `ubuntu-latest` and `macos-latest`, `dtolnay/rust-toolchain@stable`, then fmt, clippy, and test.
- Release: `cargo install cargo-dist --locked`, then `dist init` (choose GitHub CI, the Homebrew installer with tap `dpologdvinity/homebrew-tap`, and targets `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`). Commit the generated files.
- `demo.tape` (VHS): set up a repo, `rm -rf src`, `git reset --hard`, `ls`, `undo`, `ls`.

Commits: `docs: add readme`, `ci: add test workflow`, `build: configure dist releases`

## Task 14: Manual verification

1. In a real zsh or bash with the hook: `rm -rf src` then `undo`. Confirm no prompt lag on a repo with more than 10k files (time `undo snap` directly. If it takes over 300 ms on a no-change run, investigate before release).
2. In a real Claude Code session after `undo init claude`: ask the agent to run `git reset --hard` with uncommitted edits, then run `undo`.
3. WSL: repeat step 1 in a repo under `/mnt/c/...` and record the timing in the README if it is slow.
4. Tag `v0.1.0` **only when the maintainer asks**. Never push or tag without explicit approval.

Verification record (2026-10-24):
- Interactive bash and zsh in disposable repositories: prompt hook, `rm -rf src`, then `undo` restored the original files.
- Linux filesystem, 10,001 tracked files: warm no-change snapshots measured 144–270 ms after the force-add optimization.
- Real Claude Code CLI in headless mode, with temporary HOME and UNDO_HOME: the installed PreToolUse hook captured uncommitted contents before `git reset --hard`; `undo` recovered those contents.
- WSL2 `/mnt/c` with 10,001 tracked files: the initial snapshot exceeded the 10 s deadline. The slow filesystem measurement is recorded in README. This remains a documented performance limit, so the WSL check is not a performance pass.
