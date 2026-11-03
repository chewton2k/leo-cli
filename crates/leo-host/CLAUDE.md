# leo-host

The service wiring between leo-web and leo-services. `powers()` supplies AI,
speech, settings, imports and storage; web keeps its core-only dependency.
`leo-test-server` runs the same engine for targeted browser checks. Every worker
entry point dispatches `--leo-speech-worker` before normal startup.

`health::scan` converts doctor checks with external probes disabled to the native
Health view. Writing CLI sign-in status is bounded and parsed to a boolean; its
raw output never reaches the page. `WebSettings::setup_backup` validates repository
names and refuses to replace an existing remote before invoking gh. Web runs it
outside the notes lock and serializes setup requests.

Lecture listeners pass the engine's shared preview-visibility flag into recorder
controls, including both call tracks. This flag never pauses capture or final
transcription, and recovery continues while the main window is hidden.
