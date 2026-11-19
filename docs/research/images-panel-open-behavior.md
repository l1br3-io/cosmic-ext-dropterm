# Research: should the images panel's Open verb close the panel, or stay open like the quiet verbs?

Date: 2026-11-19
Question: *The pasted-images drawer carries per-row verbs — Open (the system
viewer via `open::that_detached`), Copy path, Copy image, Re-paste, the
delete cluster — and a standing ruling keeps any press inside the panel open,
with only the backdrop (or Close) shutting it. The transfer verbs are
deliberately quiet. When the user presses Open — and the header's Open folder
(finder/cache) verb — should the panel close so the just-launched viewer is
actually visible (on Wayland the viewer may appear beside/behind or be
deferred by focus-stealing prevention), or does the convention keep the panel
open like every other verb?*

## TL;DR

**Keep the panel open: Open and Open folder stay quiet like the transfer
verbs.** Every surveyed toolkit puts its close-on-activation rule on the
*menu* genus, and puts a panel holding buttons in the stay-open genus — GTK4
popovers dismiss on Esc/outside-click/`popdown()` and their menu-item popdown
is an override a button can decline (`keep-open`), WinUI's content Flyout is
light-dismiss only while MenuFlyout is the auto-closing one, and Apple's HIG
itself carves out the exact shape DropTerm ships: "If multiple selections are
possible, make sure the popover remains open" — a drawer of N rows × five
verbs is nothing but multiple selections. The one verified close-on-open peer
is Firefox's downloads popup, and its own code comment gives three reasons
that belong to a *transient* popup (eager feedback for a click that would
otherwise show nothing, not wanting to wait on the external handler before
closing, and preventing accidental double-opens); Chrome's download bubble —
the same genus, the same two verbs — closes only on deactivation, and every
standing panel surveyed (iTerm2's Toolbelt lists, Steam's screenshot manager,
the file managers' persistent views) keeps its surface while the external app
launches. The owner's visibility worry dissolves mechanically: on COSMIC
today there is no focus-stealing prevention to defer the viewer
(cosmic-comp's request for one is still an open issue), and on GNOME a
tokenless terminal launch lands in the "<App> is ready" path regardless of
what DropTerm does with its own drawer — the panel's open state cannot make
the viewer more or less visible in either regime, while closing *would* be
destructive to the panel's own state (every close disarms an armed delete
pair) and would break the multi-image, multi-verb workflow the panel exists
for. Open folder gets the same answer: it is Open aimed at a directory.

## What DropTerm has today

The panel is the `▦ N images` band chip's door (gpui-parity py-6, the
libcosmic paste-panel spec's port): a right-anchored drawer over the live
terminal — the settings drawer's composition, the backdrop a sibling beneath
an occluding panel (`dropterm/src/images_panel.rs:244-322`), the backdrop's
press and the header's Close the two close doors (`:306-319`). The press
rule is pinned by test: a press over the panel's bare interior stays open, a
press over the backdrop closes (`a_panel_press_stays_open_and_a_backdrop_press_closes`,
`images_panel.rs:456-510`). Each row's verbs are Kit buttons — the four
recoverable verbs in the quiet secondary style, the destructive cluster in
danger style (`quiet_verb`, `:112-131`) — and the row's whole identity line
(thumbnail plus stamp) is itself an Open target on `on_mouse_down`
(`:180-189`): the image is its own largest Open button.

The verb handlers sit in the shell (`dropterm/src/app_shell.rs:3724-3833`):

- **Open** (`:3732-3738`) — `open::that_detached(&path)`, one warn on
  refusal. No close, no announcement. This is the verb in question.
- **Copy path / Copy image / Re-paste** (`:3739-3779`) — the quiet transfer
  verbs; no close, no announcement.
- **Delete cluster** (`:3780-3815`) — the arm/confirm/keep pair, held set
  apart at the row's far edge.
- **Open folder** (`:3816-3830`) — `paste_cache_dir()` into
  `open::that_detached`, the no-XDG case warned; the file manager.
- **Close** (`:3831` → `close_images_panel` `:3679-3687`) — the only verb
  that closes, and closing is destructive to panel state: it sets
  `images_open = false` *and* `images_armed = None`, so an armed delete pair
  never survives a close (`PanelVerb::Close`'s own doc, `images_panel.rs:79-81`).

The one existing "the panel moves on" moment is a *content* change, not a
verb: a new paste into the tab closes the standing panel
(`note_pasted_image`, `app_shell.rs:3712-3715`). The shipped user shape is
documented at docs/images.md:19 (five verbs, the subject-is-its-own-Open-target
rule, the header's **Open folder**), docs/status-bar.md:10/137/159 (the chip,
its tip, and the band menu's always-live **Pasted Images…** row — the last
not yet wired on the GPUI build) and docs/keybindings.md:32 (the
`Ctrl+Shift+P` reference door).

## Findings

### 1. Every surveyed toolkit splits by genus: menus close on activation, panels holding content do not

- **GTK4** — `GtkPopover`'s dismissal grammar never mentions button
  activation: "the popover is dismissed in the expected situations (clicks
  outside the popover, or the Escape key being pressed)", Escape closes it,
  and `gtk_popover_popdown()` is the explicit close call
  (docs.gtk.org/gtk4/class.Popover.html, fetched 2026-11-19). The
  close-on-activation rule lives one layer up, on *menu items*: a
  `GtkModelButton`'s clicked handler pops down its ancestor popover only
  `else if (!self->keep_open)` (GNOME/gtk `gtk/gtkmodelbutton.c:1123`,
  popdown at `:1128-1129`), and the role default makes checkbox/radio items
  keep the menu open while normal command items close it
  (`self->keep_open = self->role != GTK_BUTTON_ROLE_NORMAL`, `:1147`) —
  staying open is a named, supported configuration of GTK's own popover
  vocabulary. The GNOME HIG's popover guidance works in the same genus: the
  close grammar it names is Esc ("Ensure that the Esc key closes the popover
  when it is open") and it warns "Close or Done buttons are not usually
  required in a popover" (developer.gnome.org/hig/patterns/containers/popovers.html,
  fetched 2026-11-19) — popover content is meant to be worked in, not
  consumed by one selection.
- **WinUI/Fluent** — the genus split is the class hierarchy: "Use a [Flyout]
  control to display single items and a MenuFlyout control to show a menu of
  items" (MenuFlyout class remarks), and the content Flyout's own definition
  is light-dismiss only: "Unlike a dialog, a Flyout can be light dismissed by
  clicking or tapping outside of it, pressing the device's back button, or
  pressing the 'Esc' key" (Flyout class description). The flyouts design page
  enumerates exactly those four dismissal actions — tap outside, Esc, Back,
  gamepad B — and nothing about activating content; a button inside the
  content closes it only by calling `Hide()` (learn.microsoft.com flyouts +
  the Microsoft Q&A thread, fetched 2026-11-19). The page even gives
  Microsoft's own reason for keeping such surfaces open across rapid
  interaction: with pass-through enabled "it's reasonable to expect that
  users may want to interact with multiple flyouts in rapid succession."
- **Apple** — the HIG states the general rule and then the exact carve-out
  DropTerm's shape falls into: "a popover generally closes when people click
  or tap outside its bounds or select an item in the popover. If multiple
  selections are possible, make sure the popover remains open until people
  explicitly dismiss it or they click or tap outside its bounds"
  (HIG popovers page data, fetched 2026-11-19). The menu-close convention is
  stated for the single-choice control: "After people choose an item from a
  pop-up button's menu, the menu closes" (HIG pop-up buttons page data).

Consequence: "does activating a button inside a popup close it" is answered
"Yes" only for menu items in a single-choice surface. DropTerm's drawer is a
content panel — N rows, five verbs each, no selection state — and in all
three ecosystems that genus's close grammar is outside-click, Esc, and an
explicit Close, which is precisely the doors DropTerm built.

### 2. The one verified close-on-open peer is Firefox's downloads popup — and its comment gives three reasons that belong to a transient popup

Firefox's Downloads panel closes itself on both of DropTerm's Open-shaped
verbs, deliberately and with a written rationale (mozilla/gecko-dev `main`,
`browser/components/downloads/content/downloads.js`, fetched 2026-11-19):

- `downloadsCmd_open` (the row's open-with-default-handler command) runs the
  open then calls `DownloadsPanel.hidePanel()` with the comment: "We
  explicitly close the panel here to give the user the feedback that their
  click has been received, and we're handling the action. Otherwise, we'd
  have to wait for the file-type handler to execute before the panel would
  close. This also helps to prevent the user from accidentally opening a
  file several times." (~line 1240-1246)
- `downloadsCmd_show` (open the containing folder in the file manager — the
  Open-folder twin) does the same: "Otherwise, we'd have to wait for the
  operating system file manager window to open before the panel closed. This
  also helps to prevent the user from opening the containing folder several
  times." (~line 1293-1303)
- The system-viewer variants (`downloadsCmd_openInSystemViewer`,
  `downloadsCmd_alwaysOpenInSystemViewer`, `downloadsCmd_alwaysOpenSimilarFiles`)
  carry the same "feedback that their click has been received" comment and
  hide the panel (~lines 1268-1284).

Read closely, all three reasons are properties of Firefox's *surface*, not of
the verb: (1) eager feedback matters because the panel gives none otherwise
and would otherwise linger looking inert while the handler cold-starts; (2)
"wait for the handler" only bites when the default is to linger until
something happens; (3) the double-open worry is about a one-item transient
list where the panel is the only record. DropTerm's drawer is a standing
per-tab ledger whose buttons carry Kit pressed feedback and whose ruling
already keeps every press open — adopting Firefox's close would import the
reasoning for a surface DropTerm did not build.

### 3. The same genus with the same verbs in Chrome stays open, closing only on deactivation

Chromium's download bubble (the 2023+ replacement of the download shelf) does
not close on either verb. Its command executor's open path is
`case DownloadCommands::PLATFORM_OPEN: OpenUsingPlatformHandler(); break;`
with no bubble-close call anywhere in the switch
(chromium/chromium `main`, `chrome/browser/download/download_ui_model.cc`,
`ExecuteCommand`, fetched 2026-11-19), and the bubble's lifecycle is
light-dismiss by design — the navigation handler exposes a
`CloseOnDeactivatePin` whose doc says "For the lifetime of the returned pin
(if non-null), the download bubble will not close on deactivate"
(`chrome/browser/ui/views/download/bubble/download_bubble_navigation_handler.h`),
i.e. the bubble stays while the user interacts with it and closes when the
bubble loses activation, not because a verb fired. (Caveat carried honestly:
on Windows the launched viewer usually takes activation itself, so the
bubble's visible lifetime often ends *via the launch* — through
deactivation — without the verb ever closing it; the mechanism is still
light-dismiss, not action-dismiss.) The old download shelf, the bubble's
ancestor, held the same model: a persistent strip that stayed while files
opened out of it.

### 4. Standing panels with per-item external-launch verbs keep their surface

- **iTerm2's Toolbelt** — the paste-image-ux record's list-panel precedent:
  Paste History's items paste on click and the Toolbelt pane persists; the
  Toolbelt is a dock of standing per-session tools, and none of its
  click-to-act lists dismisses their pane (iterm2.com 3.0 menu-items docs;
  the sibling record's Finding 4).
- **Steam's screenshot manager** — "Show on Disk" opens the file manager at
  the screenshot's location; the manager window stays open (search-level:
  Steam community/guide descriptions, fetched 2026-11-19).
- **File managers' persistent views** — GNOME Files' own help defines the
  row-open contract as "When you double-click (or middle-click) a file in the
  file manager, it will be opened with the default application"
  (help.gnome.org gnome-help, files-open, fetched 2026-11-19) — the Files
  window showing the list is standing UI and stays while the viewer
  launches. Starred/Recent work the same way. (The window-vs-drawer
  distinction is real, but the contract — a list surface survives its rows
  being opened externally — is the shared shape; the drawer occludes only
  DropTerm's own window, exactly as a file-manager window occupies only its
  own.)
- **KDE Dolphin's information panel / Telegram's media viewer / Slack's
  image viewer** — same family at search level only (their docs do not state
  dismissal behavior); see Unverified.

### 5. The Wayland mechanics: the drawer's open state is orthogonal to the viewer's visibility

- **The launch is fire-and-forget by construction.** `open::that_detached`
  is "Open path with the default application using a detached process" — it
  does not wait, so DropTerm receives no "viewer mapped" signal and no
  completion event it could couple a close (or a toast) to
  (docs.rs/open, fetched 2026-11-19). Any close-on-Open policy is therefore
  an *immediate* close on press, not a close-on-viewer-visible.
- **COSMIC today defers nothing.** cosmic-comp has no focus-stealing
  prevention yet — the request for a KWin-style setting is open
  (pop-os/cosmic-comp issue #2363, created 2026-05-11, fetched 2026-11-19,
  whose premise is "Every time a new window is created, it steals the
  focus"). On COSMIC the just-launched viewer takes focus and lands on top
  whether the drawer closed or not; the drawer occludes only DropTerm's own
  window, never the viewer's. When the user returns to DropTerm, the drawer
  is there as they left it — including an armed delete pair, which a
  close-on-Open policy would have destroyed.
- **GNOME defers tokenless launches regardless of the drawer.** GNOME
  Shell's focus-stealing-prevention explainer: an activation token is
  "linked to the focused window (Wayland surface) and the most recent user
  interaction", and without one a new window is denied activation in favor
  of the "<App> is ready" notification (blogs.gnome.org/shell-dev,
  2024-09-20, fetched 2026-11-19). A terminal launching `xdg-open` is the
  named tokenless case — the blog's smart-mode rationale explicitly cites
  terminal launches as the case prevention cannot decide. So on GNOME the
  viewer opens behind/deferred *with or without* the drawer closing; closing
  the drawer would leave the user staring at the same unchanged terminal,
  only with their panel state gone. The xdg-activation spec states the
  contract underneath both regimes: a token "may be ineffective ... for
  example if it was not focused, for focus stealing prevention", and the
  compositor "might decide not to follow through with the activation if it's
  considered unwanted" (wayland.app/protocols/xdg-activation-v1, fetched
  2026-11-19).

In both regimes the owner's sub-question has the same answer: the common
convention is not "close the popup so the viewer is visible," because the
popup lives inside the caller's window and cannot influence where the
compositor puts the viewer. The surface genus decides the close question
(Findings 1-4); the compositor decides the visibility question.

### 6. DropTerm-internally, closing is the one destructive thing Open could do

The panel's only close is already known to destroy state:
`close_images_panel` clears `images_armed` (`app_shell.rs:3679-3687`), and
`PanelVerb::Close`'s doc records the rule — "an armed pair never survives
the close" (`images_panel.rs:79-81`). A user's natural flow runs through
several verbs in sequence — open the image in the viewer, then copy its path
for the AI CLI, then re-paste, then delete an older row — and across
multiple rows; the panel is the ledger those verbs work on. Making Open (or
Open folder) a close would force a re-open through the chip for every
follow-up verb and silently disarm any pending delete. The one existing
close-on-content-change (a new paste closes the standing panel,
`app_shell.rs:3712-3715`) is the pattern a verb-close would muddy: closes
happen when the panel's *contents* move under it, never when the user
launches something *from* it.

## Design candidates, evaluated

**A. Stay open, quiet — status quo.** The toolkit-graded default for the
content-panel genus (Finding 1); the multi-selection carve-out Apple names
verbatim (Finding 1); the Chrome/Toolbelt/file-manager field behavior
(Findings 3-4); zero interaction with the compositor's placement of the
viewer (Finding 5); no state destruction (Finding 6). Carries one honest
cost: Firefox's reason (3) — an impatient second press can launch a second
viewer instance while the first cold-starts, since the identity line and the
Open button are adjacent large targets (`images_panel.rs:180-200`). Accepted
as the standing trade (see Open questions).

**B. Close on Open (and Open folder) — the Firefox model.** Rejected: it
buys nothing on either compositor regime (Finding 5) — the viewer's
visibility is not a function of the drawer — while paying Firefox's reason
(1) in reverse: the eager feedback would be the *removal* of the surface the
user is working in. It destroys armed delete state (Finding 6), breaks the
multi-verb flow, and contradicts the panel's own press ruling that only the
backdrop and Close close. Firefox earns its close from a transient
one-record popup; this drawer is a standing per-tab ledger by design.

**C. Close on Open only, keep Open folder quiet.** Rejected as the worst
split: the two verbs are the same act (hand a path to `open::that_detached`,
`app_shell.rs:3735` and `:3822`) aimed at a file or its directory; a rule
that closes on one and not the other is unlearnable, and it couples the
close to the *least* destructive verb while the destructive one (delete)
stays put.

**D. Close on any verb.** Rejected: reduces the panel to a launcher menu,
which is the menu genus DropTerm did not build (Finding 1), destroys armed
state on a quiet transfer verb (Finding 6), and has no field support — no
surveyed standing panel dismisses itself on transfer verbs, and the quiet
verbs' whole point (per the standing ruling) is to let work continue.

**E. Stay open, but animate the panel aside (shrink/slide) when Open fires.**
Rejected: it cannot promise the viewer visibility any more than closing can
(Finding 5), it spends the slide machinery (the surface lifecycle and
animation path already carry the slide-animation-smoothness record's
carried debts) on a cosmetic guess, and it punishes every launch with motion
the user did not ask for.

**F. Stay open plus a toast/announcement ("launched viewer") on Open.**
Rejected for now: the launch is detached and unobservable
(`open::that_detached`, docs.rs/open — Finding 5), so the announcement would
report a *request*, not a result, arriving before anything appears; it breaks
the quiet-verbs ruling for a signal whose useful form (the viewer actually
mapped) is unavailable; and the band's `Copied`-report pattern is reserved
for actions with verifiable outcomes. Revisit only if field reports show
users pressing Open repeatedly because nothing seems to happen.

## Recommendation

**Keep both verbs exactly as shipped — quiet, panel open.** No code change;
this record rules the standing question. The panel's close grammar stays
"backdrop press or Close"; the paste-closes-panel content rule stays the
one automatic close; and Open folder rides the same verdict as Open because
it is the same verb with a directory.

**Rejected alternatives:** B (close on Open — Firefox's transient-popup
reasoning, no compositor-regime benefit, state destruction), C (close on
Open only — an unlearnable split of one act), D (close on any verb — turns
the ledger into a launcher menu), E (animate aside — cosmetic churn, same
zero visibility gain), F (launch toast — announces a request it cannot
verify, breaks quiet verbs).

**Relations to existing records:** applies the paste-image-ux verdict's
panel (2026-11-08) and its quiet-verb list to the one verb whose close
behavior was left unruled; inherits the press rule the GPUI port pinned
(`images_panel.rs:456-510`); touches nothing in terminal-images, the status
bar records, or the archived clipboard-ownership. If the owner instead rules B or C, the
supersession lands in this file's replacement, not an edit here (records are
immutable).

## Unverified / open questions

- **Firefox's double-open concern at DropTerm's scale** (the cost verdict A
  accepts): how often a cold-starting viewer tempts a second press. The
  identity line is an `on_mouse_down` Open target (`images_panel.rs:182-189`),
  so a fast double-press fires twice before any close could intervene — the
  mitigation, if it ever bites, is a per-row press lock, not a panel close.
  Not ruled here; flagged for the owner's manual walks.
- **Telegram Desktop, Slack, Discord, Thunderbird attachment pane**: their
  open-externally verbs behave like the standing-surface family in practice
  (the media viewer / message window stays; their context menus close), but
  none of their docs states dismissal behavior, so they rest on
  search-level description and were not verified on the fetch date — not
  load-bearing for the verdict.
- **Steam's "Show on Disk"** is likewise search-level (community
  descriptions; no Steam documentation states the manager's dismissal
  behavior).
- **NSSPopover's `behavior` enum** (transient/semitransient/
  applicationDefined) would have been the AppKit-level citation for macOS
  popover persistence; the documentation page did not resolve on the fetch
  date. The HIG-level quotes above carry the Apple side.
- **GNOME HIG's menu pages** do not state "menus close on activation" in
  prose; the GTK source (Finding 1) carries that fact instead.
- **Chrome bubble's visible lifetime on Windows** often ends via the launch
  itself (the viewer takes activation and the bubble light-dismisses on
  deactivate) — Finding 3 words the mechanism, not the typical Windows
  observation; the source-level fact (no action-dismiss) is what the
  verdict uses.

## Sources

- DropTerm local (branch `ced2`, HEAD `cd0ef70`, 2026-11-19):
  `dropterm/src/app_shell.rs:3679-3687` (close disarms),
  `:3712-3715` (paste closes), `:3724-3833` (verb handlers — Open `:3732-3738`,
  Open folder `:3816-3830`, Close `:3831`);
  `dropterm/src/images_panel.rs:60-82` (PanelVerb, Close's doc `:79-81`),
  `:112-131` (quiet_verb), `:180-200` (identity-line Open + verb row),
  `:244-322` (overlay, backdrop doors), `:456-510` (stay-open press pin
  test); `docs/images.md:19`; `docs/status-bar.md:10`, `:137`, `:159`;
  `docs/keybindings.md:32`; `docs/testing.md:463`; sibling records
  `paste-image-ux.md` (§4 list-panel precedents), `slide-animation-smoothness.md`
  (slide machinery cost), `archive/clipboard-ownership.md` (the quiet-verb
  write paths)
- GTK4: GtkPopover reference —
  <https://docs.gtk.org/gtk4/class.Popover.html> ("dismissed in the expected
  situations (clicks outside the popover, or the Escape key being pressed)";
  `gtk_popover_set_autohide`; `gtk_popover_popdown`) (fetched 2026-11-19)
- GTK source (GNOME/gtk `main`, fetched 2026-11-19):
  `gtk/gtkmodelbutton.c:1123` (`else if (!self->keep_open)` → popdown
  `:1128-1129`), `:1147` (role default keeps checkbox/radio open);
  `gtk/gtkpopovermenu.c:304` (focus-loss popdown)
- GNOME HIG: Popovers —
  <https://developer.gnome.org/hig/patterns/containers/popovers.html>
  ("Ensure that the Esc key closes the popover when it is open."; "Close or
  Done buttons are not usually required in a popover.") (fetched 2026-11-19)
- Apple HIG: Popovers —
  <https://developer.apple.com/design/human-interface-guidelines/popovers>
  (page data JSON: "a popover generally closes when people click or tap
  outside its bounds or select an item in the popover. If multiple
  selections are possible, make sure the popover remains open until people
  explicitly dismiss it or they click or tap outside its bounds.";
  "Always save work when automatically closing a nonmodal popover.") and
  Pop-up buttons —
  <https://developer.apple.com/design/human-interface-guidelines/pop-up-buttons>
  (page data JSON: "After people choose an item from a pop-up button's menu,
  the menu closes, and the button can update its content to indicate the
  current selection.") (both fetched 2026-11-19)
- WinUI: MenuFlyout class —
  <https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.xaml.controls.menuflyout>
  ("Use a Flyout control to display single items and a MenuFlyout control to
  show a menu of items."); Flyout class —
  <https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.xaml.controls.flyout>
  ("Unlike a dialog, a Flyout can be light dismissed by clicking or tapping
  outside of it, pressing the device's back button, or pressing the 'Esc'
  key."); Flyout controls design —
  <https://learn.microsoft.com/en-us/windows/apps/design/controls/dialogs-and-flyouts/flyouts>
  (light-dismiss action list; the "multiple flyouts in rapid succession"
  rationale) (all fetched 2026-11-19)
- Firefox: mozilla/gecko-dev `main`,
  `browser/components/downloads/content/downloads.js` —
  `downloadsCmd_open` (~line 1240, the three-reason hidePanel comment),
  `downloadsCmd_openInSystemViewer`/`alwaysOpen*` (~lines 1268-1284),
  `downloadsCmd_show` (~line 1293, the file-manager twin) (fetched
  2026-11-19)
- Chromium: chromium/chromium `main` —
  `chrome/browser/download/download_ui_model.cc` (`ExecuteCommand`,
  `PLATFORM_OPEN` → `OpenUsingPlatformHandler()`, no bubble close),
  `chrome/browser/ui/views/download/bubble/download_bubble_navigation_handler.h`
  (`CloseOnDeactivatePin` doc), `chrome/browser/ui/views/download/bubble/download_dialog_view.cc`
  (`CloseBubble` = the explicit close button's path) (fetched 2026-11-19)
- Wayland: xdg-activation-v1 —
  <https://wayland.app/protocols/xdg-activation-v1> (token "may be
  ineffective ... for example if it was not focused, for focus stealing
  prevention"; compositor "might decide not to follow through with the
  activation if it's considered unwanted"; the `XDG_ACTIVATION_TOKEN`
  child-process transfer path) (fetched 2026-11-19)
- GNOME Shell: "Understanding GNOME Shell's focus stealing prevention" —
  <https://blogs.gnome.org/shell-dev/2024/09/20/understanding-gnome-shells-focus-stealing-prevention>
  (token "linked to the focused window... and the most recent user
  interaction"; smart default "in most cases new windows get focus";
  terminal launches as the tokenless case; the "<App> is ready"
  notification) (fetched 2026-11-19)
- COSMIC: pop-os/cosmic-comp issue #2363 "Focus stealing prevention"
  (open, created 2026-05-11; "Every time a new window is created, it steals
  the focus") — <https://github.com/pop-os/cosmic-comp/issues/2363>
  (fetched 2026-11-19)
- `open` crate: <https://docs.rs/open/latest/open/> (`that_detached`: "Open
  path with the default application using a detached process") (fetched
  2026-11-19)
- GNOME help: files-open —
  <https://help.gnome.org/users/gnome-help/stable/files-open.html.en>
  ("When you double-click (or middle-click) a file in the file manager, it
  will be opened with the default application.") (fetched 2026-11-19)
- iTerm2: 3.0 menu-items documentation (Paste History/Recent Directories
  click verbs in the standing Toolbelt) — carried from paste-image-ux §4's
  citations (fetched 2026-11-08 there)
- Steam screenshot manager "Show on Disk", Telegram Desktop, Slack, Discord,
  Thunderbird: search-level only (fetched 2026-11-19; see Unverified)
