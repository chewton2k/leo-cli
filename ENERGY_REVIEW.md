# Battery and energy investigation

Status: battery optimizations remain under discussion. The PR also fixes
settings and explorer refresh interruptions requested during the review.

## Goal

Reduce Leo's battery use on macOS while idle or hidden, during ordinary note use,
and while recording, without delaying shortcuts or risking recorded audio.

## Review plan

Trace the desktop window lifecycle, native background threads, page timers,
request-driven filesystem work, and recording/transcription pipelines. Record
concrete code paths separately from hypotheses about their energy cost.

Discuss the findings and trade-offs before implementing any optimization.
Prioritize work using profiles of the released app rather than debug builds.

## Measurement plan

Compare visible idle, hidden idle, note editing, microphone recording, and call
recording with microphone plus system audio. Include recording stopped and
speech-model worker released as a control. Use a disposable notes fixture and
avoid paid AI requests or real note contents in captured diagnostics.

Record the commit, app build, Mac model, macOS version, power mode, note count,
model, capture source, and background load. Repeat equal-duration runs and
report CPU time, wakeups, disk activity, child-process activity, and available
energy measurements for the app and its webview/speech processes. Measure
shortcut latency, audio continuity, and time to final transcript as guardrails.

## Next step

Review findings together and agree on the first measured, reversible change.

## Settings and explorer refresh fix

Settings redraws and asynchronous explorer completions replaced entire DOM
subtrees, detaching controls and file rows during interaction. Regression checks
reproduced both an active input being replaced and a pressed file row disappearing.

The page now reconciles these panels by stable element identity, changing text,
attributes and controls in place. Settings navigation survives section changes;
drafts, selections, open disclosures, file inputs and explorer scroll stay intact.
Background reloads keep existing note rows without flashing a loading label.
Existing request generations still reject stale page and explorer responses.

This approach retains the existing templates and delegated event handlers. It
avoids adding a UI framework or suppressing legitimate status updates while a
control is focused. Battery savings from fewer DOM replacements are not measured.

Validation: the two initial browser regressions failed before the fix and passed
after it. Final checks passed: 1,298 Rust tests (14 ignored), 180 Node tests,
22 affected browser checks across Chromium and WebKit, formatting and Clippy.
The browser checks use disposable HTTP fixtures; CI runs the compiled-app suite.
Branch coverage percentage was not measured. Independent review identified and
resolved history-row focus loss and Storage identity/event-binding edge cases.

One WebKit cleanup timeout passed in isolation and in the final affected run;
machine load was high (11.03 one-minute load average). An unrelated speech
migration test collided with another test's temporary LEO_CHAT_PROVIDER override;
it passed alone and in the final full Rust run. That existing test-isolation
problem remains outside this change.
