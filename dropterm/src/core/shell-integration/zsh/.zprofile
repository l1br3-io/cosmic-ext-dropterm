# DropTerm shell integration: the zsh chain -- the original
# .zprofile from the preserved ZDOTDIR (login shells read this).
[ -r "${DROPTERM_ZDOTDIR:-$HOME}/.zprofile" ] && . "${DROPTERM_ZDOTDIR:-$HOME}/.zprofile"
