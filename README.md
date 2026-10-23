# undo

**Ctrl-Z for your terminal and your AI agent.** `undo` saves a project's working tree after shell commands and before Claude Code tools. If `rm -rf src`, `git reset --hard`, or an agent command destroys uncommitted work, run `undo` to restore the last different snapshot.

![A terminal recovering src after deletion and git reset](demo.gif)

## Install

Requires Linux or macOS and Git 2.25 or newer. WSL uses the Linux build. After the first release, install with:

```sh
brew install dpologdvinity/tap/undo
# or
cargo install undo-cli
```

Prebuilt binaries for x86_64 and aarch64 Linux and macOS will be on [GitHub Releases](https://github.com/dpologdvinity/undo/releases). From source: `cargo install --path .`.

## Set up

Git repositories are protected automatically. Add one of these to your shell startup file, then start a new shell:

```sh
# ~/.zshrc
eval "$(undo init zsh)"

# ~/.bashrc, if using Bash
eval "$(undo init bash)"

# ~/.config/fish/config.fish, if using fish
undo init fish | source
```

Each shell hook starts a background snapshot when the prompt returns. For Claude Code, run `undo init claude`. It merges a `PreToolUse` hook into `~/.claude/settings.json`, backs up the original, and does nothing on repeat. The hook snapshots synchronously before Bash, Write, Edit, and NotebookEdit tools.

For a directory outside Git, run `undo protect /path/to/project`; this creates its first snapshot. `undo unprotect /path/to/project` stops protection and prints the retained shadow repository's path.

## Use

```sh
undo                         # restore the newest different working tree; repeat to toggle back
undo list                    # snapshots, newest first
undo diff @1                 # preview restore direction
undo diff @1 --patch         # full patch
undo restore @3              # restore a chosen snapshot
undo restore @3 -- src/lib.rs # restore one path
undo snap                    # capture manually
undo status                  # protection, hooks, storage, limits, errors
undo prune                   # apply retention now
```

Snapshot IDs are `@N` (`@0` is newest) or unique commit prefixes of at least four hex characters. `undo restore` saves the current tree first, so it is reversible. It restores file contents, executable bits, and symlinks. It never restores Git refs, HEAD, commits, stashes, or the project's staging area.

## How it works

`undo` records trees and commits in a private bare Git repository under `~/.local/share/undo/repos/`, separate from the project. It reads the project's Git metadata but does not write its `.git`. Its own index tracks the working tree. Unchanged trees create no commit. Snapshots within the last seven days are retained; days 7–30 are thinned to one per hour. Older snapshots are removed, subject to a 2 GiB cap per root. `undo prune` applies these rules, and snapshots start a background prune at most daily.

| Recovery case | trash-cli | Claude Code `/rewind` | Git | undo |
|---|---|---|---|---|
| `rm -rf` | Only through `trash-put` | No for Bash | Only committed files | Yes, after a snapshot |
| `sed -i` | No | No for Bash | Only committed files | Yes, after a snapshot |
| `git reset --hard` | No | No for Bash | Commits via reflog; uncommitted edits lost | Working-tree edits, after a snapshot |
| `git clean -fdx` | No | No for Bash | Ignored/untracked files lost | Included files, after a snapshot |
| Agent Bash commands | No | No | Only committed files | Yes, with the Claude hook |

## Limits and privacy

Snapshots stay local by default. They contain ignored files up to 1 MiB, including secrets such as `.env`; secure access to `~/.local/share/undo` accordingly. `undo` cannot recover:

- Commits, branches, or HEAD movement (use `git reflog`); the staging area; or stashes.
- Files outside protected roots, or work made and destroyed within a single long-running command.
- Heavy ignored directories (`node_modules`, `target`, `.venv`, `venv`, `dist`, `build`, `.next`, `.cache`, `__pycache__`), ignored files larger than 1 MiB, and nested Git repositories. Tracked files in a Git root remain protected even under those directory names.
- Work from before the first snapshot, or changes after the last snapshot when no hook ran.

A background shell snapshot can race with the next command if you start it immediately. The Claude hook completes before its tool call.

## Uninstall

Remove the shell setup line and the `undo snap --trigger claude` entry from `~/.claude/settings.json` (the installer prints the backup path). Uninstall the binary with your package manager. Once you no longer need recovery, delete `~/.local/share/undo` yourself; deleting it discards every snapshot.
