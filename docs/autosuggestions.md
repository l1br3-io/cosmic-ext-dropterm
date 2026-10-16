# Autosuggestions

Typing a repeated command finishes itself: as you type, the rest of the most recent matching command from this tab's own history shows as dim ghost text after the cursor — fish's interaction, computed entirely terminal-side.

The rest of the most recent matching command from *this tab's own history* renders as dim ghost text after the cursor as you type. Right accepts all of it, Alt+Right accepts one shell-word, and nothing ever executes without an explicit accept. The rules:

- Right accepts the whole suggestion; Alt+Right accepts one word (leading spaces included); any other key dismisses it. The ghost is inert pixels until an accept key sends it - ignore it and it does nothing.
- History is this tab's own, scraped from the screen as commands finish: nothing is read from disk, nothing leaves the machine, and a respawned tab starts empty.
- No shell integration, by construction: prompt detection reads the kernel's own foreground truth, so it works in any shell and transparently over ssh - no scripts shipped, no environment injected, no escape-sequence handling.
- Honest silence: no suggestion inside vim or htop (a child holds the foreground), on a `read -p` prompt or a multi-line / right-side prompt (the line no longer matches the prompt anchor), or while scrolled back. Uncertainty always degrades to no suggestion, never a wrong one.
- Toggle with `autosuggestions_enabled` (default `true`, Behavior section in the [settings drawer](settings.md)); a flip applies live - a showing suggestion disappears the moment the switch lands, and re-enabling suggests again at the next typed prefix.
