# DropTerm shell integration, zsh half: this bootstrap directory
# rides as ZDOTDIR, and every startup file chains its original from
# the ZDOTDIR DropTerm preserved at spawn (DROPTERM_ZDOTDIR,
# defaulting $HOME for a hand-sourced bootstrap).
[ -r "${DROPTERM_ZDOTDIR:-$HOME}/.zshenv" ] && . "${DROPTERM_ZDOTDIR:-$HOME}/.zshenv"
