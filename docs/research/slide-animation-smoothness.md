# Why does the drop-down slide judder, and what drives a smooth one?

Date: 2026-11-19 · Module: slide-animation (the reported slide
judder) · Record: immutable — the verdict is a recommendation pending
owner ruling, not a shipped decision.

## TL;DR

The judder's most likely root cause is **not the animation math and
not a timer**: GPUI at the pin throttles *inactive* windows to
~30 fps (`WindowOptions::inactive_frame_interval`, default ≈ 30 fps),
and DropTerm's slide paths put the animating surface in exactly the
throttled class — the hide recreates the window keyboard-less at
slide-out start (inactive for the whole slide), and every show's head
frames run before the compositor grants the keyboard. A second,
latent defect: the slide's `AnimationElement` (constant id
`"slide"`) flips `done` at mount+130 ms and stops scheduling frames —
masked today only because every slide re-target incidentally
recreates the window; the de-recreate below would expose it as ~10 fps
pump-tick stepping. The minimal fixes are app-level: set
`inactive_frame_interval` on the layer windows' `WindowOptions`, key
the animation element per slide epoch (the enabler for the third
fix), and then drop the hide's `Hold{Released}` recreate — itself a
138–146 ms surface gap at slide start and a decision-layer change
with a real trade. The premise of a "1 ms slide tick" in the design
docs is stale: no code spawns `SLIDE_TICK`; frames come from GPUI's
`with_animation` → `request_animation_frame` → `wl_surface.frame`
callbacks, which is the correct driver.

## Findings

1. **The frame-callback contract (why timers can't pace Wayland
   frames).** `wl_surface.frame` is the protocol's animation
   throttle: "Request a notification when it is a good time to start
   drawing a new frame… This is useful for throttling redrawing
   operations, and driving animations… If the client commits an
   update earlier than that, it is likely that some updates will not
   make it to the display, and the client is wasting resources by
   drawing too often." The request takes effect on the next commit,
   fires once, and must be re-requested each frame
   (wayland.xml `wl_surface.frame`). `wl_output.mode` adds: "Clients
   should not use the refresh rate to schedule frames. Instead, they
   should use the wl_surface.frame event or the presentation-time
   protocol." The callback carries "the current time, in
   milliseconds, with an undefined base" — the frame timestamp — and
   compositors may withhold callbacks entirely for invisible surfaces
   (KWin explicitly stopped sending them for minimized/off-desktop
   windows, Zahorodnii 2022). A timer-driven redraw loop therefore
   commits buffers at arbitrary phase against the compositor's
   vblank queue: on a FIFO swapchain the early commits stack up and
   every second pose is never scanned out — held frames, then jumps.
   A 1 ms tick would also serialize ~130 wakeups per slide through
   the event loop with no alignment guarantee at all.

2. **GPUI at the pin already drives frames off `wl_surface.frame`
   callbacks — after the first present.** The wayland backend
   (`gpui_linux/src/linux/wayland/window.rs` at zed `d2a4fb6`, the
   rev gpui-pre 0.3.12 is a verified snapshot of) requests the frame
   callback inside `draw()` right before presenting, parks in
   `FrameLoop::AwaitingCallback`, and restarts the frame on the
   callback with `FrameRequestSource::NativeCallback`; local
   scheduling (a calloop ping, immediate, not vblank-aligned) only
   feeds the *first* frame after idle (`FrameLoop::Parked →
   Scheduled`) and degenerate retry paths. Resizes are throttled to
   once per vblank client-side; present mode is Mailbox with FIFO
   fallback; there is no presentation-feedback use and no
   ms-subtraction from the refresh interval. So continuous
   animations are compositor-paced by construction — the backend is
   not the problem.

3. **GPUI core throttles inactive windows to ~30 fps, and the pin's
   defaults are live in DropTerm.** GPUI's frame handler computes
   `should_throttle = !request_frame_options.require_presentation`
   (false on the normal callback path, confirmed in the wayland
   backend) and, through `minimum_frame_interval`, applies the
   maximum of three `WindowOptions` intervals: `inactive_frame_interval`
   (only when `!active` and no sustained high-rate input), the
   low-power interval, and the thermal interval. `FramePacer::should_defer`
   then defers frame ticks arriving before `last_frame + interval`
   (1 ms tolerance; catch-up correction bounded to a quarter
   interval). The field docs: inactive and low-power "default to
   approximately 30 fps", thermal to ≈ 60 fps, and "the limits can
   be disabled via None" (gpui-pre 0.3.12 `WindowOptions`, docs.rs).
   DropTerm builds its layer windows' `WindowOptions` in
   `executor.rs` (`measuring_options`/`shaped_options`) and takes
   `..Default::default()` — so the ≈ 30 fps inactive throttle is
   armed on every DropTerm surface.

4. **DropTerm's slide paths put the animating window in the
   throttled class.**
   - *Hide*: `decide_hide` emits `Hold{Released}` at slide-out start
     ("Keyboard interactivity releases at slide-out START, not at
     destroy"), and keyboard mode is create-time at this pin, so the
     executor recreates the window (`Action::SwapWindow`,
     `SwapReason::KeyboardHold`) with `KeyboardInteractivity::None`
     (executor.rs module doc; `execute_hold`; the tests' "~139 ms
     swap cost" note). A keyboard-less layer surface is never
     focused — GPUI's `active` stays false for the entire slide — so
     the whole slide-out animates at the ≈ 30 fps inactive cadence
     with the pacer's defer/restart jitter on top.
   - *Show*: the fresh surface's early frames run before the
     compositor grants the keyboard — DropTerm's own logs classify
     them ("every surface's first frame reads LOST before the
     compositor grants the exclusive keyboard",
     `reset_show_watch`) — so the slide-in's fastest frames (the
     ease-out head) run throttled, then the cadence snaps to full
     rate on focus grant. The mid-slide reversal's re-hold recreate
     adds a +141 ms focus re-grab window (gpui-spike numbers) spent
     inactive.
   - The 138–146 ms recreate itself is a second-order judder source:
     the old surface dies, nothing presents during the gap, and the
     successor maps and replays from the re-base (`render`'s
     OPEN->FIRST_FRAME `playing.started = Instant::now()`). The
     re-base makes the pose continuous; the gap is not.

5. **A latent frame-supply defect sits under the constant animation
   id.** `AnimationElement` keeps per-element state keyed by the
   element id (`window.with_element_state`): its `start` is mount
   time and a oneshot animation flips `done` when elapsed exceeds
   the duration, at which point it stops calling
   `window.request_animation_frame()` (animation.rs at the pin).
   DropTerm's slide wrapper uses the constant id `"slide"`
   (app_shell.rs `sliding_band`). Today this is *masked*: every
   slide re-target — the hide's `Hold{Released}` and the reversal's
   re-hold, which always differ from the surface's recorded
   `None`/held mode — rides a window recreate (`execute_hold`
   no-ops only on an unchanged mode), and the fresh window remounts
   a fresh element. The hazard goes live the moment a slide
   re-targets on a still-mounted finished element, i.e. exactly
   what recommendation 3's de-recreate introduces: toggle to hide
   in the 130–230 ms window after a show's first frame (the In
   slide has not parked yet — the machine keeps
   `slide = Some(...)`, so the element stays mounted and `done`),
   and the Out slide would advance only when something else
   notifies — the 100 ms pump tick notifies unconditionally ("Keep
   frames flowing (the clock band)"), giving ~10 fps irregular
   stepping. The pose closure itself is correct (draw-time
   evaluation against `Playing.started`, clamped at the endpoint);
   only the frame supply dies.

6. **The "1 ms slide tick" premise is stale.** `slide::SLIDE_TICK`
   (1 ms) has no runtime caller — its only reference besides the
   definition is a lifecycle test. Frames are driven by
   `with_animation` (the chunk ruling recorded in the
   `sliding_band` doc comment), and the app's only loop is the
   100 ms pump tick whose `decide_slide_tick` merely *parks*
   finished slides (an `Out` slide's completion destroy runs up to
   100 ms late — an end-of-hide linger, not motion judder). The
   `SLIDE_TICK` doc comment (redraw-provoking ticks, held-frame
   jumps, AboutToWait batching) describes the superseded libcosmic
   design and should not be read as today's behavior.

7. **The modern protocol answer exists but is not actionable here.**
   `wp_commit_timing_v1` lets a client attach a target presentation
   time to a commit ("presented as closely as possible to, but not
   before, a specified time", in the presentation-clock domain) —
   the frame-perfect version of callback pacing. Its published
   compositor support at research time: Jay, Pila, Weston — not
   COSMIC, not sway, not KWin/Mutter/Hyprland. Not a path for
   DropTerm's primary platforms today; a watch item only.

8. **Known pitfalls in this exact stack, checked:**
   - *Resize/reconfigure during slide*: already defended —
     `decide_reshape` defers geometry changes "while a slide is
     animating" (surface_lifecycle.rs), and GPUI's backend throttles
     interactive resizes to once per vblank. The slide translates
     content *inside* a fixed-size surface, so no layer reconfigure
     rides the animation.
   - *Keyboard-hold swaps mid-slide*: the recreate gap and the
     inactive re-grab window (finding 4) — the dominant envelope
     distortion.
   - *Held frames / damage*: a Wayland surface keeps presenting its
     last committed buffer when the client stops committing — the
     held-frame-then-jump the stale SLIDE_TICK comment describes;
     with callback-paced frames supplied every refresh this does not
     occur.
   - *Fractional scaling*: the carried ledger already holds "fractional
     visible output on COSMIC plus integer-rounded display metadata"
     (gpui-spike); the slide is a logical-coordinate translation at
     draw time, so fractional scale cannot quantize the animation
     itself — it distorts geometry, not the slide.

## What DropTerm has today

The slide model is pure (`core/slide.rs`: quadratic ease-out/ease-in
over `SLIDE_DURATION_MS = 130`, exact endpoints, teleport-free
reverse), carried through view rebuilds as `Playing { started,
active }`. The draw site wraps the content column in
`with_animation("slide", Animation::new(130 ms), …)` and evaluates
the offset at draw time from `started.elapsed()` (app_shell.rs
`sliding_band`), re-basing the clock at the show window's first
frame. The executor recreates windows for every keyboard-hold change;
the pump ticks at 100 ms. GPUI enters as the ruled gpui-pre 0.3.12
pin — the crates.io snapshot of zed `d2a4fb6`, content-verified
byte-identical in the kit-pass (gpui-spike) — *not* an in-tree
carried copy; `carried/` holds only `alacritty-terminal` and
`gpui-component`. Measured envelope: release show 34 ms against the
130 ms slide (sway debug 121 ms), recreate re-grab +141 ms
(gpui-spike, portability-plan Phase 2 results).

## Recommendation

Ranked; 1 and 2 are one-site app-level changes and target the most
likely root causes; do them first and re-walk before reaching further.

1. **Unarm the inactive-window throttle on DropTerm's layer windows
   (root-cause candidate for the hide-slide judder and the show
   head).** Set `inactive_frame_interval` explicitly in
   `measuring_options`/`shaped_options` (executor.rs) — `None`
   disables the arm entirely (the docs' own off switch), or a small
   value such as 1 ms keeps a bound while restoring full-rate
   animation. The slide surface is either focused (show) or
   deliberately keyboard-less mid-slide (hide); a 30 fps
   background-window saver serves neither. Cost: idle DropTerm
   windows may redraw unfocused updates at up to full rate — the
   surface only animates or shows terminal output, and the 100 ms
   pump cadence caps ambient churn anyway.
2. **Key the animation element per slide epoch (cheap now, load
   bearing later).** Make the `with_animation` id unique per
   `Playing` (e.g. a monotonic epoch the machine bumps on every
   start/reverse, folded into the id), so a re-targeted slide
   remounts `AnimationElement` state and `request_animation_frame`
   scheduling restarts. Today the recreates mask the stale-element
   hazard (finding 5); recommendation 3 removes that mask, so this
   must land with it — and the epoch id is worth taking now as the
   one-line form of the same guarantee.
3. **If a walk still shows a freeze-then-slide envelope on hide:
   drop the hide's `Hold{Released}` recreate.** Defer the keyboard
   release to the destroy that follows slide completion (the grab
   dies with the surface; the compositor refocuses the previously
   focused window at destroy). This removes the 138–146 ms surface
   gap *and* keeps the animating window focused (which findings
   3–4 already cover, so this is then about the gap alone). Land
   recommendation 2 in the same change: without the recreate, the
   element-staleness window of finding 5 goes live. Trade to rule:
   focus returns ~130 ms later than today's slide-out start;
   `hide_on_focus_loss` hides are unaffected (focus is already
   lost). Rejected shape: keeping the hold through the slide
   defeats the intent the current comment states (the previous
   window regains focus while the slide plays).
4. **Clean the stale design record** — `SLIDE_TICK`'s doc comment
   describes the retired tick design; with no runtime caller it is
   dead weight plus a trap for the next reader (this research
   opened on it). Remove the const or re-point the comment at the
   `with_animation` ruling. Also: the parked-slide linger (an
   `Out` completion destroy waiting on the 100 ms tick) can tighten
   by parking on the animation's own final frame, but that is
   polish, not judder.
5. **Not recommended now:** evaluating the pose from the frame
   callback's timestamp (GPUI hands `signal_at` to the frame handler
   but exposes it to no view API at this pin — a carried one-line
   patch in the kit-root-border shape could) and adopting
   `wp_commit_timing_v1` (no COSMIC/sway support). Both are second
   decimal places behind findings 3–4; revisit only if a walk with
   1–3 landed still measures per-frame phase jitter.

Verification shape (owner-run per the runtime ruling): toggle walks
on COSMIC and sway with a frame-interval log at the slide wrapper
(elapsed between animator invocations), comparing before/after on
the hide path, the show head, and the mid-slide reversal — the
reversal walks a toggle 101 ms into the slide (the gs3 row), and
with fixes 2–3 landed the 130–230 ms same-window toggle is the
deterministic probe for finding 5's element staleness.

## Sources

- wayland.app core protocol, `wl_surface.frame` / `wl_surface.commit`
  / `wl_output.mode` (fetched 2026-11-19) — frame-callback contract,
  frame timestamps, the no-refresh-rate-scheduling rule, held-buffer
  semantics.
- wayland.app staging `commit-timing-v1` (fetched 2026-11-19) —
  commit timestamps and its compositor support table.
- zed @ `d2a4fb6ded7c470be25d1541281da3882495b65b` (fetched
  2026-11-19; the rev gpui-pre 0.3.12 snapshots, verified in
  docs/research/gpui-spike.md):
  - `crates/gpui/src/elements/animation.rs` — `AnimationElement`
    state (`start` at mount), `done` → stops
    `request_animation_frame`, max-fps timer path.
  - `crates/gpui/src/window.rs` — frame handler
    (`should_throttle = !require_presentation`),
    `minimum_frame_interval` + `FramePacer::should_defer` (verbatim
    quoted), `inactive_without_high_rate_input`, intervals from
    `WindowOptions`, `next_frame_callbacks` drained before
    draw/present, `schedule_frame` arming.
  - `crates/gpui/src/platform.rs` + docs.rs gpui-pre 0.3.12
    `WindowOptions` (fetched 2026-11-19) —
    `inactive_frame_interval: Option<Duration>`, default ≈ 30 fps,
    "can be disabled via None".
  - `crates/gpui_linux/src/linux/wayland/window.rs` — frame
    callback requested in `draw()`, `FrameLoop` state machine,
    `require_presentation` false on the normal path, resize
    throttle, Mailbox/FIFO, no presentation feedback.
  - docs.rs gpui `Window::request_animation_frame`/"Schedule a frame
    to be drawn on the next animation frame" (family API doc,
    fetched 2026-11-19).
- Vlad Zahorodnii, "What's cooking in KWin? Plasma 5.25" (2022-05-09)
  — KWin withholds frame callbacks for invisible windows.
- This repo, read 2026-11-19: `dropterm/src/core/slide.rs` (17, 19–31,
  142–148), `dropterm/src/app_shell.rs` (4231–4237, 4391–4476,
  4495–4511, 1574–1589, 5431–5497), `dropterm/src/core/surface_lifecycle.rs`
  (216–270, 284–333, 381–410, 412–418, 633–651), `dropterm/src/executor.rs`
  (5–17, 52–57, 302–308, 566–589, 1185–1211), `dropterm/Cargo.toml`
  (39–47), `docs/research/gpui-spike.md` + `docs/research/portability-plan.md`
  (Phase 2 numbers: show 34 ms / 121 ms, recreate +141 ms, fractional
  ledger), `tasks/gpui-surface.md` (161, 210 — mid-slide reversal walks).

## Unverified

- The exact `Default` duration of `inactive_frame_interval` (docs say
  "approximately 30 fps"; the literal was in the truncated tail of
  `platform.rs`'s Default impl, not quoted).
- That GPUI's `active` flag stays false for a `KeyboardInteractivity::None`
  layer surface: inferred from the seat-focus mapping and DropTerm's
  own activation logs (first frame LOST pre-grant), not traced into
  the wayland backend's activation event handling.
- Finding 5's stepping magnitude (~10 fps) is a code-read conclusion
  — the reachability analysis (every current re-target swaps the
  window; the hazard goes live under fix 3) is derived from
  `execute_hold`'s no-op rule and the machine's hold bookkeeping,
  not from a live walk.
- cosmic-comp's own frame-callback pacing internals were not
  investigated; the record relies on the protocol-level semantics.
