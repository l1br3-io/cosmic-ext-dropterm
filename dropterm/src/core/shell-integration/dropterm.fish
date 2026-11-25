# DropTerm shell integration, fish half: the OSC 133 prompt/command
# marks and the OSC 7 cwd report, on fish's own prompt/command
# events. DropTerm spawns fish with an init command that sources
# this file after fish's own config has run.

function __dropterm_postexec --on-event fish_postexec
    # The finished command's exit code (D): postexec fires after the
    # command lands, before the next prompt.
    printf '\033]133;D;%d\007' $status
end

function __dropterm_preexec --on-event fish_preexec
    # The command mark (C): preexec fires right before the line
    # runs.
    printf '\033]133;C\007'
end

function __dropterm_prompt --on-event fish_prompt
    # The prompt mark (A) and the cwd as a file:// URL (percent
    # escapes for the two bytes a path can carry that a URL must
    # not, so the consumer's decode round-trips).
    printf '\033]133;A\007'
    set --local __dropterm_path (string replace --all -- '%' '%25' $PWD)
    set __dropterm_path (string replace --all -- ' ' '%20' $__dropterm_path)
    printf '\033]7;file://%s%s\007' $hostname $__dropterm_path
end
