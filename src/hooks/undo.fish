set -gx UNDO_SHELL_HOOK 1
function __undo_postexec --on-event fish_postexec
    set -q UNDO_DISABLE; and return
    command undo snap --trigger shell --cmd "$argv[1]" >/dev/null 2>&1 &
    disown 2>/dev/null
end
