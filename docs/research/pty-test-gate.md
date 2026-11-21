# Why do pane-booting gpui tests trip the test scheduler's thread-affinity gate?

Date: 2026-11-21 · Opened by key-delivery Task 11's landing attempt ·
Status: **needs a spec** — the class is diagnosed, the fix is an
initiative, not a patch.

## TL;DR

**Every gpui test that boots a real pane rides a tokio channel whose
senders live on foreign threads, and any send or drop that lands inside
an executor poll window schedules the pump task from that thread —
which gpui's test scheduler reports as nondeterminism.** Two tests have
already been `#[ignore]`d over it; key-delivery's pin test (which needs
a real PTY as its witness) widened the windows enough to flake the
suite ~50% under load. Per-test defusal demonstrably moves the flake
rather than closing it. The verdict: one initiative fixes the class at
the source — a test-only pane harness, or a pump whose waker never
lives on the foreground test executor — with un-ignoring the two lost
tests as its acceptance proof.

## The anatomy

1. **The pump is a foreground task over a tokio channel.**
   `terminal/view.rs`'s `spawn_pump` runs
   `cx.spawn(async |this, cx| while let Some((_, event)) =
   events.recv().await …)` — the receiver's first poll registers a
   tokio `AtomicWaker` with the gpui foreground executor's schedule
   closure (`gpui-pre-0.3.12/src/executor.rs:357`). From then on,
   whoever touches the channel's senders can schedule that task.

2. **The senders live on foreign threads.** The engine's event proxy
   is cloned into alacritty's PTY reader/event-loop thread
   ("PTY reader", `event_loop::spawn`); the suggest generators' waiter
   threads are unnamed (`core/terminal/engine.rs`,
   `run_under_deadline`'s `std::thread::spawn`), and their
   landed-fetch nudge sends a `Wakeup` on the requester's channel
   (`engine.rs`'s `if let Some((tab_id, tx)) = wake` arm) — with the
   generator cache globally shared, a nudge can even route across
   panes.

3. **Three wake shapes, all backtrace-proven (2026-11-21):**
   - *Teardown drop*: test body ends → engine drops → PTY closes →
     child dies → reader thread exits → its `EventLoop` drop drops the
     `UnboundedSender` → tokio's disconnect wake
     (`tokio-1.53.1/src/sync/task/atomic_waker.rs:307`, reached via
     `Tx::drop` → `wake_rx`).
   - *Live send*: PTY output (a paste's echo) → `pty_read` →
     `TaggedEventProxy::send_event` → `UnboundedSender::send` →
     `Chan::send` → wake.
   - *Unnamed send*: a generator waiter's `Wakeup` nudge landing on
     whichever pane requested the key.

4. **The gate only fires inside poll windows.**
   `gpui-pre-scheduler-0.3.12/src/test_scheduler.rs:581`'s
   `assert_correct_thread` flags a foreign-thread `schedule_local`
   only while the scheduler is actively running on the test's thread;
   wakes between windows queue harmlessly. Hence pure timing flake,
   load-sensitive: a busy machine deschedules the test thread
   mid-poll, the foreign wake lands inside the window, the test
   aborts at `end_test` (`test_scheduler.rs:194`) with
   "Your test is not deterministic."

5. **The house has paid this class twice already.**
   `terminal/view.rs`'s `pasted_paths_read_on_the_painted_prompt_line`
   and `app_shell.rs`'s `a_profile_respawn_seeds_the_departed_pane_
   repair` are both `#[ignore]`d with notes naming surviving reader
   threads — two tests lost to the gate, their coverage gone from
   `just check` entirely.

6. **The key-delivery pin is the third encounter, with measurements**
   (2026-11-21): its own teardown race was defused by leaking the
   engine Arc and booting echo-silent (0/20 standalone failures); its
   sibling-aggravation is real and pre-existing — the repaste pair
   went 0/10 alone, 3/10 paired with the pin, and **5/8 on the tree
   without key-delivery's fix at all**. The flake is not the fix's;
   the fix's test merely loads the machine.

7. **Per-test defusal is whack-a-mole, measured.** Leaking the
   sibling pane tests' engines cut the repaste pair 3/10 → 1/10;
   silencing repaste's echo cut it to 0/10 — and the next full-suite
   batch flaked 4/6 with the font test tripping on an unnamed
   generator thread. Every patch moves the flake to the next
   foreign-wake shape; only an architectural close ends it.

## What DropTerm has today

Two `#[ignore]`d tests (`pasted_paths`, `a_profile_respawn`), a suite
that flakes under load whenever pane tests run concurrently, and
key-delivery Task 11 parked mid-landing: its fix (the Terminal-context
Tab claim) is green standalone 20/20 but its gate run collides with
this class. The parked diff sits uncommitted in the checkout, per the
owner's sequencing call (2026-11-21): root-fix first, land Task 11 on
a truly green gate after.

## Reproducing

```
# the pair (repaste alone: 0/10; paired: ~3/10; pre-fix tree: 5/8)
just test a_repaste_verb -- a_dispatched_tab_reaches

# the suite, until it bites (~50% under load with the pin present)
for i in $(seq 1 6); do just test || echo "run $i flaked"; done
```

The abort names the woken test and the foreign thread; with
`RUST_BACKTRACE=1` the captured backtrace carries the sender's frames
(`Tx::drop` for teardown, `Chan::send` for live output, the generator
waiter for the unnamed shape).

## Recommendation

One initiative, spec'd on its own (`/spec`), closing the class at the
source. Candidate shapes for the spec to weigh:

- **A test-only pane harness** — a `boot_pane` variant (or pty seam)
  whose tests never ride a real reader thread: an in-memory pty, or a
  pump-less boot with direct event injection, so gpui tests own every
  wake. Acceptance: the two `#[ignore]`d tests un-ignored and green in
  the gate, the pin's pair and full-suite loops at 0 flakes.
- **A pump restructure** — `spawn_pump`'s receive loop moved where its
  waker cannot sit on the foreground test executor (a std-thread pump
  marshaling into gpui through a thread-safe notify), production and
  test alike. Same acceptance; riskier, it touches live behavior.

Rejected alternatives, on today's evidence:

- **Per-test defusal** (engine leaks, echo-silencing): measured to
  move the flake, not close it (finding 7); also spreads test-only
  workarounds across modules.
- **More `#[ignore]`s**: the existing debt, doubled — coverage lost
  to a scheduler gate the tests cannot help tripping.

## Sources

- `gpui-pre-scheduler-0.3.12/src/test_scheduler.rs:581`, `:194`;
  `gpui-pre-0.3.12/src/executor.rs:357` (the gate and the schedule
  closure), read at `~/.cargo/registry/src/index.crates.io-…/`, fetch
  date 2026-11-21.
- `tokio-1.53.1/src/sync/task/atomic_waker.rs:307` and
  `sync/mpsc/chan.rs` (`Tx::drop`/`Chan::send` wake paths), same
  registry tree, 2026-11-21.
- `dropterm/src/terminal/view.rs` (`spawn_pump`,
  `pasted_paths…`'s ignore note),
  `dropterm/src/app_shell.rs` (`a_profile_respawn…`'s ignore note),
  `dropterm/src/core/terminal/engine.rs` (`run_under_deadline`'s
  unnamed waiter, the generator wake arm), repo as of 2026-11-21
  (key-delivery Task 10 at commit `4e92058`, Task 11 parked in the
  working tree).
- The 2026-11-21 measurement runs: pin standalone 0/20; repaste pair
  0/10 alone, 3/10 paired, 5/8 pre-fix; defusal batches 1/10 → 0/10
  pair with a 4/6 full-suite flake moving to the font test.
