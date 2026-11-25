# DropTerm shell integration: the zsh chain -- the original .zshrc
# runs whole first, then the hooks append behind it.
[ -r "${DROPTERM_ZDOTDIR:-$HOME}/.zshrc" ] && . "${DROPTERM_ZDOTDIR:-$HOME}/.zshrc"

__dropterm_precmd() {
    # The prompt cycle's head: the finished command's exit code (D),
    # the prompt mark (A), and the cwd as a file:// URL (percent
    # escapes for the two bytes a path can carry that a URL must
    # not, so the consumer's decode round-trips).
    printf '\033]133;D;%d\007' $?
    printf '\033]133;A\007'
    local __dropterm_path=${PWD//[%]/%25}
    __dropterm_path=${__dropterm_path//[ ]/%20}
    printf '\033]7;file://%s%s\007' "$HOST" "$__dropterm_path"
}

__dropterm_preexec() {
    # The command mark (C): preexec fires right before the line
    # runs.
    printf '\033]133;C\007'
}

# The head of the user's own precmd/preexec hooks -- the report must
# see the command's exit code before any other precmd can spend it.
precmd_functions=(__dropterm_precmd $precmd_functions)
preexec_functions=(__dropterm_preexec $preexec_functions)
