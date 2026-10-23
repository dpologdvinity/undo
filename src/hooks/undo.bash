export UNDO_SHELL_HOOK=1
__undo_precmd() {
  local s=$?
  if [[ -z "$UNDO_DISABLE" ]]; then
    local c; c=$(HISTTIMEFORMAT= history 1 | sed 's/^ *[0-9]* *//')
    ( command undo snap --trigger shell --cmd "$c" >/dev/null 2>&1 & )
  fi
  return $s
}
if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
  __undo_present=""
  for __undo_hook in "${PROMPT_COMMAND[@]}"; do
    [[ "$__undo_hook" == __undo_precmd ]] && __undo_present=1
  done
  if [[ -z "$__undo_present" ]]; then
    PROMPT_COMMAND=(__undo_precmd "${PROMPT_COMMAND[@]}")
  fi
  unset __undo_present __undo_hook
else
  case ";${PROMPT_COMMAND};" in *";__undo_precmd;"*) ;; *) PROMPT_COMMAND="__undo_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}";; esac
fi
