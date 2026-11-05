# leo-host

The service wiring between leo-web and leo-services. `powers()` supplies AI,
speech, settings, imports and storage; web keeps its core-only dependency.
`leo-test-server` runs the same engine for targeted browser checks. Every worker
entry point dispatches `--leo-speech-worker` before normal startup.

`health::scan` converts doctor checks with external probes disabled to the native
Health view. Writing CLI sign-in status is bounded and parsed to a boolean; its
raw output never reaches the page. The host exposes no backup setup or push
capability; existing user repositories are never changed by app lifecycle events.
The browser-test engine has a real startup/session-gate integration test, so
Cargo tests also produce the executable used by browser checks.

The Usage sheet receives both coding accounts and their saved usage windows;
detecting a signed-in CLI never changes the user's selected writer. Codex limit
refresh does not depend on which writing provider is selected. General Settings
loads only saved limits; the explicit Usage action performs the bounded CLI
refresh and preserves saved readings when that CLI cannot return limits.

Lecture listeners pass the engine's shared preview-visibility flag into recorder
controls, including both call tracks. This flag never pauses capture or final
transcription, and recovery continues while the main window is hidden.
