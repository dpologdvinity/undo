Output demo.gif
Require undo
Require git
Require mktemp
Set Shell "bash"
Set FontSize 20
Set Width 920
Set Height 430
Set TypingSpeed 18ms
Hide
Type `set -eu; for name in "${!GIT_@}"; do unset "$name"; done; UNDO_DEMO_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/undo-demo.XXXXXX"); trap 'rm -rf -- "$UNDO_DEMO_ROOT"' EXIT; export HOME="$UNDO_DEMO_ROOT/home" UNDO_HOME="$UNDO_DEMO_ROOT/data" GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1; mkdir -p "$HOME" "$UNDO_DEMO_ROOT/project/src"; cd "$UNDO_DEMO_ROOT/project"; git init -q; printf 'project\n' > README.md; git add README.md; git -c user.name=demo -c user.email=demo@localhost commit -qm initial`
Enter
Type "printf 'uncommitted work\\n' > src/main.rs; undo snap"
Enter
Show
Type "ls src"
Enter
Sleep 500ms
Type "rm -rf src"
Enter
Type "git reset --hard"
Enter
Type "ls"
Enter
Sleep 500ms
Type "undo"
Enter
Sleep 500ms
Type "ls src"
Enter
Sleep 2s
