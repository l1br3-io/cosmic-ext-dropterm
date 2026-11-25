# DropTerm shell integration: the zsh chain -- the original
# .zlogin from the preserved ZDOTDIR (login shells read this).
[ -r "${DROPTERM_ZDOTDIR:-$HOME}/.zlogin" ] && . "${DROPTERM_ZDOTDIR:-$HOME}/.zlogin"
