# Chunk: keybind-docs — binding the summon key per desktop

Spec: [SPEC-keybind-docs.md](../SPEC-keybind-docs.md) · Module id:
`keybind-docs` · Phase 1, lane C · branch `port/keybind-docs` (main
checkout).

**Files touched:** `docs/summon.md` (new), `README.md` (Documentation
index).

## Planning rulings (made at /planning; veto at plan review)

- **Page: `docs/summon.md`**, placed after Keybindings in the index —
  the existing [keybindings](../docs/keybindings.md) page stays
  in-app-chords-focused; the summon key is the OS-level binding and
  gets its own entry point.
- Row coverage follows the desktops a Phase 0 session verified — if
  session 5 ran i3 rather than Xfce, the row is i3's binding mechanism,
  not Xfce's (a recording choice honored, not overridden).

## Task 1: The per-desktop summon page

**Description:** One page: binding `dropterm toggle` per desktop — sway
`bindsym`, Hyprland `bind`, KDE custom shortcuts, Xfce keyboard settings
(or the recorded X11 session's mechanism), beside the COSMIC shortcut
the docs already cover (link to it, don't duplicate it). Every row
cites the Phase 0 session that verified it (by session slot, from the
plan record's "Phase 0 results"); desktops no session verified are
marked untested or unsupported. Linked from README's Documentation
index.

**Acceptance criteria:**
- [ ] Each row cites its Phase 0 session
- [ ] Untested desktops are marked as such; nothing documents a binding
      no session verified
- [ ] The page is linked from README's Documentation index

**Verification:**
- [ ] Tests pass: `just docs-check`
- [ ] Manual: each row's snippet matches its cited session's record

**Dependencies:** `app-identity` merged (the page names the final
command); `reality-check`'s sessions filled (a desktop gets a row only
after a session verified it) · **Files:** `docs/summon.md`, `README.md`
· **Scope:** S
