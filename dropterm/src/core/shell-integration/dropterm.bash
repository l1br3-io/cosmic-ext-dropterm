# DropTerm shell integration, bash half: the OSC 133 prompt/command
# marks and the OSC 7 cwd report, emitted from bash's own prompt
# hooks. DropTerm spawns bash with this file as the --rcfile, so the
# user's ~/.bashrc -- the file bash itself would have read -- runs
# first, and the hooks chain around whatever it set.

[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"

__dropterm_prompt_command() {
    # The prompt cycle's head: the finished command's exit code (D),
    # the prompt mark (A), and the cwd as a file:// URL (percent
    # escapes for the two bytes a path can carry that a URL must
    # not, so the consumer's decode round-trips).
    printf '\033]133;D;%d\007' "$?"
    printf '\033]133;A\007'
    local __dropterm_path=${PWD//[%]/%25}
    __dropterm_path=${__dropterm_path//[ ]/%20}
    printf '\033]7;file://%s%s\007' "$HOSTNAME" "$__dropterm_path"
}

# The head of the user's own PROMPT_COMMAND, whatever it was --
# the report must see the command's exit code before any other
# prompt hook can spend it.
PROMPT_COMMAND="__dropterm_prompt_command${PROMPT_COMMAND:+;$PROMPT_COMMAND}"

# The command mark (C): PS0 expands right after the line is read
# and before it runs, so its substitution prints before execution.
PS0='$(printf "\033]133;C\007")'"${PS0}"
