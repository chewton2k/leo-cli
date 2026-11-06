# leo-host

The service wiring between leo-web and leo-services. `powers()` supplies AI,
speech, settings, imports and the recording `Housekeeper`; web keeps its core-only dependency.
`leo-test-server` runs the same engine for targeted browser checks. Every worker
entry point dispatches `--leo-speech-worker` before normal startup.

`health::scan` converts doctor checks with external probes disabled to the native
Health view. Writing CLI sign-in status is bounded and parsed to a boolean; its
raw output never reaches the page. `WebSettings::setup_backup` validates repository
names and refuses to replace an existing remote before invoking gh. Web runs it
outside the notes lock and serializes setup requests.

The Usage sheet receives both coding accounts and their saved usage windows;
detecting a signed-in CLI never changes the user's selected writer. Codex limit
refresh does not depend on which writing provider is selected. General Settings
loads only saved limits; the explicit Usage action performs the bounded CLI
refresh and preserves saved readings when that CLI cannot return limits.

Lecture listeners pass the engine's shared preview-visibility flag into recorder
controls, including both call tracks. This flag never pauses capture or final
transcription, and recovery continues while the main window is hidden.

`housekeeping.rs` holds `Housekeeping`, leo-web's `record::Housekeeper`: `pending_recordings`
(unfinished native sessions, unlocked ones only, each with its manifest's `started`) and
`discard_recording` (validated id, contained paths, refuses a locked session). Settings →
Keeping's audio expiry calls it, so a recording that is running or being recovered (its
session is locked) is never discarded. The Storage page, its areas and actions were removed
on 2026-11-05; files they managed stay on disk.
