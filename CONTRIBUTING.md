# Contributing to undo

Thank you for contributing to `undo`! This document outlines local development workflows, testing requirements, safety rules, and code contribution standards.

---

## 1. Prerequisites & Environment

Ensure you have the following installed on your development machine:
- **Rust Toolchain:** Version `1.89` or newer (Edition 2024). Install via [rustup](https://rustup.rs/):
  ```sh
  rustup update stable
  rustup component add rustfmt clippy
  ```
- **Git:** Version `2.25.0` or higher (`git --version`).

---

## 2. Local Development Workflow

### Build
Compile the project in debug mode:
```sh
cargo build
```

Run the built binary directly:
```sh
cargo run -- --help
```

### Formatting & Linting
All contributions must pass format checks and clippy with all warnings denied:
```sh
# Verify code formatting
cargo fmt --all -- --check

# Run static analysis and linter
cargo clippy --all-targets --locked -- -D warnings
```

---

## 3. Testing & Sandbox Safety Rules

### Crucial Safety Invariant
> **CRITICAL:** Tests must **NEVER** run against your actual `$HOME` directory or personal git repositories.

All integration tests must instantiate an isolated `Sandbox` (from `tests/common/mod.rs`), which sets up:
- A dedicated temporary directory.
- An isolated `$HOME`, `$XDG_DATA_HOME`, and `$UNDO_HOME`.
- A hermetic Git configuration (`GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=<tmp>/.gitconfig`).
- Unset `UNDO_SHELL_HOOK` environment variable.

### Running Tests
Execute the full test suite locally:
```sh
cargo test --all-targets --locked
```

Run a specific test target:
```sh
cargo test --test restore
cargo test --test snapshot
```

---

## 4. Git Commit & Pull Request Guidelines

### Conventional Commits
All commit messages must follow the [Conventional Commits](https://www.conventionalcommits.org/) specification:

| Prefix | Usage | Example |
|---|---|---|
| `feat:` | New feature or capability | `feat: add fish shell hook support` |
| `fix:` | Bug fix or security hardening | `fix: preserve directory permissions on restore` |
| `perf:` | Performance optimization | `perf: skip force-add for ordinary tracked files` |
| `test:` | New tests or testing improvements | `test: cover claude code hook trigger` |
| `docs:` | Documentation changes only | `docs: update architecture diagram and guide` |
| `chore:` | Tooling, dependencies, or scaffolding | `chore: update dependencies` |
| `ci:` | CI/CD workflow configuration | `ci: add test matrix and coverage reporting` |

### Hygiene & Excluded Files
Never commit AI agent scratchpads, prompt files, or harness configs (`CLAUDE.md`, `AGENTS.md`, `.claude/`, `.superpowers/`, `.codex/`, `docs/agents/`). Add them to `.git/info/exclude`.

### Submitting Pull Requests
1. Create a descriptive feature branch (e.g., `feat/my-new-feature` or `fix/issue-description`).
2. Verify all local checks pass:
   ```sh
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```
3. Open a pull request against `master`. Ensure the GitHub Actions CI pipeline passes all checks:
   - Environment and dependency validation
   - Formatting and clippy linting
   - Cross-platform test suite (Ubuntu and macOS)
   - Code coverage reporting
   - Production artifact build
