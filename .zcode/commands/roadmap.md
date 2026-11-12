---
description: Rebuild the portability status board fresh from the repo's own task state
---

Read `tasks/todo.md` and `SPEC.md` in full, and `tasks/plan.md`'s "Open questions" for outstanding rulings. These are the only sources — classify fresh every run and never serve a cached or session-stale board.

Classify each task into exactly one status:

- **done** — its box is checked in `todo.md`; carry the merge sha from its note
- **ready now** — unchecked, and nothing gates starting it (no hardware, no ruling)
- **hardware** — gated on an owner-hardware session or COSMIC-on-metal verification
- **owner** — waiting on an owner ruling or decision (release cut, floor, channels, Kit, …)
- **gated** — waiting on another task finishing first

Render one board, phase by phase (0; 1 flag day and lanes A–C; 2; future 3–5), checkpoints styled as milestone rows:

- In ZCode Desktop (visualize skill and a session visualization directory available): load the visualize skill, build the interactive fragment — four stat cards (modules merged, ready at the desk, hardware sessions open, owner rulings open), filter pills per status with live counts, one section per phase — and emit its content reference.
- In any other host: the same content as one markdown table per phase, with a Status column.

Close with one line: what is actionable now, and what the rest wait on.
