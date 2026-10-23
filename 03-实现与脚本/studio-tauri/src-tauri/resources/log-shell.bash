# Loaded only by the Studio logging shell, never written to ~/.bashrc.
if [[ -r ~/.bashrc ]]; then source ~/.bashrc; fi
HISTFILE=/dev/null
HISTCONTROL=
HISTIGNORE=
HISTSIZE=500
set -o history
shopt -s cmdhist lithist
__wcs_shell="${BASHPID}-${RANDOM}${RANDOM}"
__wcs_tmux() { tmux -L wcs-terminal "$@"; }
__wcs_b64() { printf '%s' "$1" | base64 -w 0; }
__wcs_publish() {
    __wcs_tmux set-option -w -t "$TMUX_PANE" @wcs_run "$(__wcs_b64 "$1")" 2>/dev/null
    printf '\033]777;WCSLOG;%s\007' "$1"
}
__wcs_begin() {
    local entry command number seq root
    entry=$(HISTTIMEFORMAT= builtin history 1)
    command='[Shell history disabled: command unavailable]'
    if [[ -o history && "$entry" =~ ^[[:space:]]*([0-9]+)[[:space:]]+(.*)$ ]]; then command=${BASH_REMATCH[2]}; fi
    seq=$(__wcs_tmux show-option -wqv -t "$TMUX_PANE" @wcs_seq 2>/dev/null)
    [[ "$seq" =~ ^[0-9]+$ ]] || seq=0
    seq=$((seq+1))
    __wcs_tmux set-option -w -t "$TMUX_PANE" @wcs_seq "$seq" 2>/dev/null
    root=$(git -C "$PWD" rev-parse --show-toplevel 2>/dev/null) || root=
    __wcs_publish "begin;$__wcs_shell;$seq;;$(__wcs_b64 "$command");$(__wcs_b64 "$PWD");$(__wcs_b64 "$root");;"
}
__wcs_end() {
    local status=$1 packet kind shell seq code command cwd root endcwd jobs
    packet=$(__wcs_tmux show-option -wqv -t "$TMUX_PANE" @wcs_run 2>/dev/null | base64 -d)
    IFS=';' read -r kind shell seq code command cwd root endcwd jobs <<< "$packet"
    if [[ "$kind" == begin && "$shell" == "$__wcs_shell" ]]; then
        jobs=$(jobs -pr); jobs=${jobs//$'\n'/,}
        __wcs_publish "end;$shell;$seq;$status;$command;$cwd;$root;$(__wcs_b64 "$PWD");$jobs"
    fi
    return "$status"
}
__wcs_prompt() { __wcs_end "$?"; }
if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == 'declare -a '* ]]; then
    PROMPT_COMMAND=(__wcs_prompt "${PROMPT_COMMAND[@]}")
else
    PROMPT_COMMAND="__wcs_prompt${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
fi
PS0='$(__wcs_begin)'"${PS0-}"
trap '__wcs_end "$?"' EXIT
__wcs_publish "ready;$__wcs_shell;0;;;$(__wcs_b64 "$PWD");$(__wcs_b64 "$HOME");;"
