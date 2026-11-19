# Research: should the images panel's Open verb close the panel after all?

Date: 2026-11-20
Supersedes: [images-panel-open-behavior](archive/images-panel-open-behavior.md)
(2026-11-19), whose evidence stands and whose verdict the owner reversed.

Question: *The pasted-images drawer keeps standing while its verbs run — the
superseded record ruled Open and Open folder stay quiet like the transfer
verbs, on the toolkit genus (menus close on activation, content panels do
not), the compositor mechanics (the drawer cannot influence where the
launched window lands), and the armed-delete cost (every close disarms). The
owner heard the convention and ruled the close anyway. What closes, when, and
what does the supersession change?*

## TL;DR

**The owner ruled the close: Open and Open folder hand the path to the
system and the drawer closes behind a landed launch; a refused launch (the
verb's own warn) leaves the panel standing — nothing opened to close for.**
The superseded record's evidence is unrefuted: the toolkit genus keeps
content panels open on button activation, and the drawer's open state was
never a factor in the viewer's visibility. What changes is the product call:
the owner prefers the close as the launch's feedback shape — the launched
window is the confirmation, and the drawer gets out of its way rather than
lingering behind it. That deliberately adopts Firefox's first and third
close reasons (eager feedback, no accidental second press) for a standing
surface, and accepts the superseded record's stated cost without flinching:
every Open close disarms an armed delete pair, exactly as every close always
has, and a follow-up verb means re-opening through the chip.

**The same ruling's second leg (the owner's follow-up after the live walk
confirmed the first): the stay-open surface follows the launch out.** With
`hide_on_focus_loss` on, the viewer's grab runs the natural focus-loss hide
and the window walks itself out; with the flag off the surface shares focus
and the viewer would sit behind the still-mapped drop-down — so the landed
launch hands its own Hide through the pane-signal funnel, and the whole
surface follows the drawer.

## Findings

1. **The superseded record's evidence stands; the verdict was a preference,
   not a fact fight.** Nothing in a day's use refuted the genus citations or
   the compositor mechanics — the viewer lands on top on COSMIC with or
   without the drawer. The owner heard that complete case and still ruled
   the close: on this surface, an action that launches a window outside the
   app should dismiss the surface that launched it, the way the paste rule
   already dismisses it for a paste. The call is the owner's to make; the
   record keeps the superseded one so the reasoning survives.
2. **The close joins a coherent family.** The panel already yields to every
   content event: a fresh paste (text or image) and the panel's own
   Re-paste close it first (the covering report's rule — nothing lands
   under the veil), the backdrop and the header's Close close it on demand.
   A launch is the one remaining event the panel shrugged at; the ruling
   closes the family — *the panel moves aside for whatever takes the
   foreground next, whether that content is a paste or a viewer window.*
   The quiet transfer verbs (Copy path, Copy image) launch nothing and stay
   open; the no-XDG refusal and a failed launch open nothing and keep the
   panel standing.
3. **The armed-pair cost is real and accepted.** `close_images_panel`
   clears `images_armed`, so an Open pressed while a delete pair stands
   disarms it — the same trade the paste rule already made for pastes, and
   the same rule every close has always carried. The superseded record
   called this the one destructive thing Open could do; the owner accepted
   it as the price of the feedback shape.
4. **Refusals keep the panel.** `open::that_detached` is fire-and-forget:
   its `Ok` means the launch request landed, its `Err` means nothing was
   handed off. Only the `Ok` arm closes — a refusal warns and leaves the
   panel exactly where it stood, so the close never fires for an action
   that did not happen.

## The second leg: the stay-open surface follows the launch out (2026-11-20)

The live walk confirmed the drawer close and surfaced the regime the first
verdict's compositor argument had papered over: the drop-down is a layer
surface, and a stay-open one (`hide_on_focus_loss` off — the Ghostty
`quick-terminal-autohide=false` model the flag's own doc cites) shares
keyboard focus instead of holding the exclusive grab, so the launched
viewer never triggers the focus-loss hide and, with the drawer closed, it
still sits behind the mapped drop-down. The owner ruled the same outcome as
the grab-holding regime: pressing Open walks the whole surface out.

The mechanics follow the surface's own grammar. The funnel gate stands as
documented (`focus_loss_hides` — only the automatic focus-loss path
consults it; explicit hides call the decision directly), so the launch acts
like the other explicit hides: `system_open_landed` hands `PaneSignal::Hide`
into the pane-signal channel when the flag is off, and the pump's drain runs
the machine's hide funnel with it — decided at the tick's window point,
never inside the verb's event handler (sr-1's lesson). A grab-holding
surface (`hide_on_focus_loss` on) sends nothing: its focus-loss hide already
owns the exit, and the launch must not race it with a second funnel run.

## What DropTerm has today

- `PanelVerb::Open` (`dropterm/src/app_shell.rs:3834`) matches the launch
  result: `Ok` → `system_open_landed` (`:3776`), `Err` → the warn. The
  header's `OpenCache` (`:3928`) rides the same shape, with the no-XDG
  refusal keeping the panel.
- `system_open_landed` (`:3776`) is the ruling's named seam: a landed
  launch closes the drawer through `close_images_panel` — armed pair dies
  with the close like every close — and hands the stay-open surface's own
  `PaneSignal::Hide` when `hide_on_focus_loss` is off (the second leg's
  funnel hand-off; a grab-holding surface sends nothing). The verb docs in
  `dropterm/src/images_panel.rs` (`:62-64`, `:77-79`) carry the rule.
- Pinned hermetically by `a_landed_system_open_closes_the_standing_panel`
  (`:6110`) and `a_landed_open_hides_a_stay_open_surface_itself` (`:6307`,
  the two regimes' hand-off through the funnel's own receiver) — the launch
  call itself stays outside the tests (a real viewer spawn), so the seam
  carries the pins and the refusal arms are audited at the match.
- The user shape is documented at `docs/images.md:19` (the launch closes the
  panel, refusals leave it standing, every close disarms, the stay-open
  surface follows the launch out) and the walk row owns the manual check
  (`docs/testing.md:465` — the drawer closing behind the launch, the
  flag-off surface sliding out with it).

## Recommendation

**Shipped as ruled — both Open-shaped verbs close on a landed launch.**
Rejected: keeping Open open (the superseded stance — retained for its
evidence, overridden by the owner's call), and the half-split of closing on
Open only while Open folder stays open (the superseded record's own
unlearnable-split rejection stands — the two verbs are one act aimed at a
file or its directory). Revisit trigger: if the disarm-on-Open cost bites in
real use — an armed pair repeatedly dying under launches — a future record
can rule the armed-pair carve-out (Open refuses to close while a pair
stands); no such ruling exists today.

## Sources

- DropTerm local (branch `ced2`, 2026-11-20):
  `dropterm/src/app_shell.rs:3744-3772` (close + the landed seam),
  `:3818-3826` (Open arm), `:3912-3924` (OpenCache arm);
  `dropterm/src/images_panel.rs:62-64`, `:77-79` (verb docs);
  `dropterm/src/app_shell.rs:6034` (the pin test);
  `docs/images.md:19`, `docs/testing.md:465`
- Superseded record: [images-panel-open-behavior](archive/images-panel-open-behavior.md)
  (2026-11-19) — the toolkit genus (GTK4, WinUI, Apple HIG), Firefox's
  downloads-popup close reasons, Chromium's no-action-dismiss bubble, the
  Wayland mechanics (cosmic-comp's missing focus-stealing prevention, GNOME's
  tokenless-launch path); all retained unrefuted
- Owner instruction: 2026-11-20, "proceed" on the close-on-Open offer after
  the superseded record's convention case was presented and understood
