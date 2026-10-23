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
