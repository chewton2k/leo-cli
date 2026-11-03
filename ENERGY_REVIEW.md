# Battery and energy investigation

Status: investigation and discussion; no runtime changes proposed for merge yet.

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
