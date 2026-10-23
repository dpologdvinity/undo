Output demo.gif
Require undo
Require git
Set Shell "bash"
Set FontSize 20
Set Width 920
Set Height 430
Set TypingSpeed 18ms
Hide
Type "mkdir -p $UNDO_DEMO_ROOT/src; cd $UNDO_DEMO_ROOT; git init -q; printf 'project\\n' > README.md; git add README.md; git commit -qm initial"
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
