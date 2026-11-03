# Battery and refresh work

Status: implemented in PR #9. The user approved the battery changes after the
initial review; whole-app battery/energy measurement remains outstanding.

## Goal and design

Reduce unnecessary work while Leo is idle/hidden, during note use and while
recording, without risking audio or slowing shortcut activation.

- The shortcut coordinator receives keyboard/control events through a bounded
  queue instead of polling every 10 ms. It retains the existing chord, repeat,
  modifier-only, Fn, Secure Input and Escape behavior. One-second waits cover
  permission checks and receiver shutdown; event delivery wakes immediately.
- The native window supplies visibility to the engine and page, including after
  page-load completion. Hidden/minimized windows stop status/display polling and
  new lecture-preview passes. Capture, final transcription, recovery, dictation
  and the recording sleep-prevention guard remain active.
- Lecture previews send PCM directly to the speech engine. The unused temporary
  live.wav write, atomic rename, fsync and deletion are removed.
- Proven read-only note/task/graph paths use filesystem notifications to coalesce
  scans. Initial checks, foreground return, rescan events and a 60-second deadline
  reconcile the tree; missing/broken watchers retain per-access checks. Mutations
  always reconcile first. This conservative split prevents a save from absorbing
  an unread external edit into its fingerprint.
- Background graph/vector/caption work defers while hidden or recording/dictating.
  Automatic status GETs do not count as user interaction. Ordinary note views use
  a 30-second app-status cadence; Settings, active updates and onboarding retain
  their responsive cadence. Visibility return refreshes immediately.

Visible lecture previews retain the current cadence and final segment pass.
Incremental streaming or slower visible previews would change responsiveness or
model behavior; a rewrite is deferred until real-model profiling establishes its
benefit. New visibility suppression allows an in-flight preview to finish.

## Component measurements

These measure work removed, not watts or battery life. Runs used temporary
fixtures on macOS 27.2 (26B123), arm64, with unoptimized Rust test binaries. Model
inference, HTTP serialization and rendering are excluded from the Rust timings.

| Workload | Before | After |
| --- | --- | --- |
| App-status requests in 60 simulated hidden seconds, three trials | 25 each | 0 each |
| App-status requests in 60 simulated visible-note seconds, three trials | 25 each | 3 each |
| Visible Settings control workload, three trials | 25 each | 25 each |
| 100 unchanged scans of 1,000 notes, five trials | 100 scans; 735.341 ms median (668.087–818.582 ms) | One initial scan in 6.947 ms; subsequent trials skipped scans in 0.006 ms |
| Prepare eight 20-second preview buffers, five trials | 145.354 ms median, eight synchronized WAV writes | 12.806 ms median, zero synchronized WAV writes |

Reproduce from the repository root:

```sh
node crates/leo-web/tests/app-polling.bench.cjs b88b878
cargo test -p leo-web store_refresh::tests::benchmark_unchanged_note_scans -- --ignored --nocapture
cargo test -p leo-services session::recorder::tests::benchmark_preview_audio_preparation -- --ignored --nocapture
```

The benchmark tests are ignored in the default suite. The polling benchmark uses
an injected clock and API fake; the scan benchmark uses a real OS watcher. Real
watcher delivery and missing-root fallback have separate tests. The scan results
exclude writes, which deliberately keep their full pre-write reconciliation.

## Refresh fixes retained

Settings redraws and asynchronous explorer completions previously replaced
entire DOM subtrees. The page now reconciles panels by stable element identity,
changing text, attributes and controls in place. Settings navigation, drafts,
caret positions, disclosures, file inputs and unchanged file rows survive updates.
Background reloads retain cached rows without flashing a loading label; request
generations still reject stale responses. Regression checks failed before the
fix for both an active input replacement and a pressed file row disappearing.

Independent review caught history-row/Storage identity cases in the refresh work
and a delayed-watcher fingerprint problem in the battery work. These were fixed;
external creation/editing followed by another save is tested with notifications
held back. That regression was seen to fail with the pre-write guard removed.

## Validation limits and next measurements

The final checks passed: 1,303 Rust tests (16 ignored), 29 desktop unit tests,
185 Node tests, 22 Chromium/WebKit fixture checks, formatting and Clippy. Polling tests
use controlled time and late responses; browser fixtures cover actual DOM state
in Chromium and WebKit. The compiled-app browser suite runs in CI. Node-only
coverage of app-settings.js is 66.43% lines / 51.52% branches; its rendering and
setup paths also have browser checks whose coverage is not aggregated. Rust branch
coverage was not collected. Native global-shortcut interaction and actual-model
recording energy have not been manually measured in the rebuilt desktop app.

For whole-app validation, compare visible idle, hidden/minimized idle, editing,
microphone recording and dual-track call recording. Include recording stopped
and the speech worker released as controls. Use a disposable notes fixture and
record the commit, release build, Mac model, OS, power mode, note count, speech
model and capture source. Repeat equal-duration runs; report CPU time, wakeups,
disk activity and energy for Leo plus its WebKit/speech processes. Check shortcut
latency, audio continuity, transcript quality and finalization time as guardrails.

An existing speech-migration test can race another test's temporary
LEO_CHAT_PROVIDER override. The final Rust gate runs serially to avoid that
unrelated isolation defect. An earlier WebKit cleanup timeout passed alone and in
subsequent affected runs; machine load was high at the initial failure.
