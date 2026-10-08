# leo-services

### Streaming goes through the provider trait

`ChatProvider::complete_streaming` has a default that calls `complete` and sinks
the whole answer once, so a provider that cannot stream still works everywhere
streaming is used. `run_chat_chain_with` holds the fallback policy for both paths,
so they cannot drift on which errors are fatal. The streaming chain also takes an
`on_restart` callback: text from a provider that then failed is not part of the
answer, and without discarding it a fallback would glue half of one reply to all
of another.

`:ask` runs on a worker and writes back through `PreExpanded`, an `Ai` impl that
returns the answer already in hand. Note the distinction from `ReadyNote`, whose
`expand_prompts` deliberately echoes its input for the listen path — using that
one here streamed the answer to the screen and then discarded it on save.

### AI pipeline

`record` → `session::mic` (cpal, built in) captures 16 kHz mono →
`ai::transcribe()` runs the `[transcribe]` chain (default Parakeet on this
computer), chunking only for providers that declare a `max_bytes()` limit the
file exceeds → `ai::structure_notes()` runs the `[chat]` chain (whatever
/settings chose) → note saved.

Each chain skips unavailable providers silently, advances past retryable
errors (402/408/429/5xx, refused connections), and aborts on fatal ones
(400/401/403/404) so a bad request is not retried against every provider.
Every degradation it did take is printed once, so a silent downgrade is never
invisible.

The four functions in `ai/mod.rs` (`transcribe`, `structure_notes`,
`structure_notes_append`, `expand_prompt`) are the only entry points callers
use; nothing above `ai/` knows about providers. `transcribe_outcome` and
`chat_outcome` are the same calls without the printing, for the TUI — an
`eprintln!` would smear ink across the alternate screen, so it turns
degradations into status-line events instead.

### The note prompts

`ai/chat.rs` builds every chat prompt as `Prompt { system, user }`: rules in
the system message, material in the user message inside `<transcript>`,
`<existing_notes>`, `<typed_points>` or `<note>` tags, with the essentials
restated after the transcript. The note prompts ask the model to fix clearly
misheard terms, and to fill gaps from its own knowledge where the recording or
speaker was unclear (the user wants this kept). Typed points (`POINTS_RULE`)
are woven into the sections they belong to: no section of their own, no times
(the user asked for this). Long recordings send each part the points typed
during it (`long::points_for`: every point to exactly one part); only a
fallback with no AI answer lists them, as a plain `## Key points`. Replies go through `clean_reply` (outer code fence, "Here are your
notes:", "Let me know…") and `split_title_body` (strips "Title:", `#`, bold,
quotes). An answered `@leo` line becomes `**Q:** question` with the answer
under it (`ai::answer_each`). A reply that hits `max_tokens` is kept and warned
about; a provider's configured `max_tokens` overrides what leo requests.

### Recording sessions (`session/`)

A recording is a `Session`: `<data>/leo/recordings/<stamp>/` with
`session.json` (`Manifest`: title, append_to, dir, typed points, stopped,
saved), a `lock` (pid + 5 s heartbeat; stale when old or the pid is dead), and
segments. `capture::Capture` reads s16 PCM from `mic::Mic` (`Source::Microphone`,
or `Source::Screen` for `record screen`: default output via the macOS 14.6+
process tap or Windows loopback, else BlackHole or `LEO_SCREEN_DEVICE`; see
`mic::find_device`) (or `Source::Replay` from `LEO_FAKE_AUDIO` at `LEO_FAKE_SPEED`, or
`Source::Synthetic` in tests) and `wav::Writer` writes `seg-NNNNN.part.wav`,
header refreshed each second, renamed to `.wav` every `SEGMENT_SECS` (300)
with `OVERLAP_SECS` of the previous segment prefilled. Paused audio is never
written. `transcriber::Transcriber` transcribes each `.wav` into `.txt`
(atomic), deletes the audio, backs off per segment, never gives up while
recording, gives up after `attempts_after_stop` (writes `.err`, keeps audio)
unless the error is a rate limit. `Session::assemble` stitches segment texts in
order. `recover_parts` seals a crash's `.part.wav` and clears `.err` for retry.
`ai::long::structure_recording` groups parts into ~`WORDS_PER_PART` words, one
request per group plus a title/summary request; every failure falls back to
the raw transcript, so it cannot fail. `Session::finish` deletes the folder
unless a segment failed (then `saved: true` keeps the audio). Unfinished
sessions are resumed by the TUI at start (`App::resume_interrupted`,
`App.recordings` is None in tests) and by `leo record` before recording.
`session/stress.rs` simulates 3 h and (ignored) 24 h with a flaky provider and
a crash copy.

The TUI worker (`leo-tui/src/task.rs` `start_listen`) shows finished segment
texts plus a live tail: every `ROLL_INTERVAL` (3 s local, `CLOUD_ROLL_INTERVAL`
8 s cloud) it re-transcribes from `settled_at`, at most `LIVE_MOST_SECS` (20 s),
and once that is `SETTLE_AFTER_SECS` (12) long it settles at `live::quietest`
between 6 s and end − 2 s, so the unsettled end is rewritten rather than glued.
Only while the transcription backlog is at most one segment. Typed points go to
the worker via `Job::add_point` and are saved in the manifest. Recording needs
only speech AI and a microphone; the writing AI is optional. `LEO_NO_MICROPHONE`
turns the device off (tests, e2e, CI).

### Model configuration

Built-in providers only (`built_in_toml`: six API writers, Claude Code, Codex,
Parakeet and three cloud speech); `config/tidy.rs` rewrites old files on
load (`RETIRED` names, fields equal to built-ins or old defaults, empty chain →
default; a `whisper_cpp` block without its own `bin` → `parakeet`). Speech on
this computer is NVIDIA Parakeet TDT 0.6B v3 int8 via the `sherpa-onnx` crate
(static prebuilt libs, CPU; CoreML measured 10× slower) in
`ai/provider/parakeet.rs`: one `leo-speech` engine thread owns the recognizer,
decodes one job at a time with `threads()` = half the cores, at most 4,
process nice 10 + utility QoS, unloads after `IDLE` (60 s), and cuts audio at
the quietest 0.1 s between 20 and 25 s (`cuts`) to keep memory ~1.5 GB. Files
are verified against `MODEL_MANIFEST` before loading (`verify`, cached by size
+ mtime): sherpa calls exit() on a bad model. Model dir
`models_dir()/parakeet-tdt-0.6b-v3-int8` (`audio::models_dir`, LEO_HOME
aware), URL pinned to a Hugging Face commit. `whisper_cpp` now only runs an
external `bin` (e2e fakes). `Transcriptions` kind (`openai_transcribe`, alias `groq`)
takes `path` (xAI: `stt`).

`config/choice.rs` is the Settings model: `WRITING` (ollama, openai, codex,
anthropic, claude_code, gemini, xai, openrouter) and `SPEECH` (parakeet,
openai_whisper, gemini_speech, xai_speech),
each with a curated model list (empty = local; `Local` holds installed Ollama
models from `/api/tags` and `speech_ready`).
`selection` = first usable chain entry, else the first. `write_choice` makes
the chain that one provider; `write_model` writes only `model` (or
`model_path`) — built-ins fill the rest field by field (`fill_from`, skipped if
the user's `kind` differs). `key_from` shares a stored key (openai_whisper →
openai, gemini_speech → gemini); always look keys up by `pc.account(name)`.
`reasoning = true` sends `max_completion_tokens` and no temperature (OpenAI,
Anthropic's OpenAI-compatible endpoint). `ChatAudio` kind transcribes via chat
completions with base64 `input_audio` (Gemini; 14 MB cap).

`<config_dir>/leo/config.toml` (`~/Library/Application Support/leo/config.toml`
on macOS, `~/.config/leo/config.toml` on Linux) defines named providers and
per-task fallback chains. `/settings` then `e` creates and opens it. Providers are
tried in order; unavailable ones (no key, no binary, closed port) are skipped
silently, so a chain may list more providers than are installed.

API keys live in the OS keychain, not in files, and all of them share a single
item holding a JSON object keyed by provider. That is deliberate: macOS asks for
permission per *item* whenever the requesting binary's signature does not match
the item's ACL, which it does not after a reinstall — so an item per provider
turned opening the provider screen into a dialog per provider. `SecretStore::has`
exists for the same reason: status display needs to know whether a key exists,
and only a read costs a prompt. Credentials written by an older version are
folded into the single item once per installation.

`leo doctor` (or the key row in `/settings`) stores one; the provider screen shows which
providers have credentials without revealing them. Keys come only from that
store (`secret::resolve(account, store)`): an env var named by `key_env` is
never used (the user asked; a friend's stale `OPENROUTER_API_KEY` silently beat
his stored key). `key_env` now only means "needs a key"; settings shows
`Credential::Ignored` ("leo does not read $VAR") when one is set but nothing is
stored, and `leo doctor`'s key prompt still offers to import it, with consent. A
refused key (401; 403; 400 whose body mentions an API key, which is how Gemini
and xAI answer) goes through `error::classify_status_with_key` when a stored
key was sent: it leads with "<Name> rejected the key stored in leo (401). To
replace it: /settings, then Enter on the <Name> key row." and quotes the body
after, so a cut-off status line still shows the fix.

`claude_code` and `codex` (kinds `ClaudeCode`/`Codex`, `ai/provider/agent_cli.rs`)
write through the user's own signed-in CLI and plan: `claude -p --safe-mode
--tools "" --no-session-persistence --system-prompt …` and `codex exec
--ephemeral --ignore-user-config --sandbox read-only -`, material on stdin, run
in a fresh private `leo-writing-*` temp dir (0700, deleted after; a shared fixed folder let other users plant files or links). Claude Code streams (`--output-format stream-json
--include-partial-messages --verbose`; `read_stream` sinks `text_delta`s and
treats a `result` with `is_error` as a failure); Codex has no partial text, so
its stdout is the answer in one piece.

Plan limits (`usage.rs`): `<config>/usage.json` keeps the latest `Usage`
(5h/7d `Window { used 0..1, resets_at }`, `seen_at`) per provider name. Claude
Code's come free with every answer (`rate_limit_event` → `from_claude`, saved
in `AgentCli::run` even when the request failed); never send a request just to
check. Codex's come from `codex app-server` → `account/rateLimits/read`
(`ask_codex`, ~1 s, no model call; experimental upstream, so a failure just
shows nothing). Settings' writing row appends `label` (`5h: 9%, 7d: 93%`, plus
age past `STALE_AFTER`, a window past its reset reads 0%); the TUI re-asks
Codex every `task::USAGE_EVERY` (60 s) while Settings is open
(`App.check_usage`, a no-op fn in tests); doctor adds `<name> limits` with
reset times. Never `--bare` (it skips the
OAuth login). Usable = program on PATH (`locate`, `.cmd` on Windows); failures
are retryable and name the sign-in step; settings shows a sign-in fact row,
no key row; doctor's "answers" probe is the sign-in check.

An override that names a provider with no `[providers]` block is ignored with a
warning rather than fabricating a half-configured provider.

`.env` is loaded only from leo's data directory at startup, never the current
directory (a project's `.env` could otherwise redirect `LEO_HOME`).

`web_settings.rs` is the website's settings model: `describe` (per task:
choices, model list with prices, key account and whether it is stored, sign-in
or plan usage for agents, notes such as Ollama not running) never includes a
key; `apply` validates every change against `choice`, writes through
`toml_edit` (creating the commented default file when missing), and refuses to
store a key unless the request was secure.

`import.rs` turns uploads into a note: text from PDF (`lopdf::extract_text`;
fewer than 40 letters a page means scanned, so its DCTDecode page images are
used), .docx (`word/document.xml`), .pptx (slides in number order plus speaker
notes), text and Markdown; images go to `ai::see`. One request for short
material, otherwise ~24k-char text parts and 6-page image batches, then
`build_summary_prompt` for title and summary. Vision: `ChatProvider::
complete_with_images` (default: a clear "cannot read images" error);
`OpenAiChat` sends `image_url` data URLs, Claude Code gets the image inside a
stream-json user message with `--tools ""` (its Read tool is not confined, so
it is never enabled), Codex gets `-i` files written into that private dir and
removed after. Codex always runs with `--disable` for shell, browser, computer
and app tools that `codex features list` reports (unknown names make Codex
fail, so only listed ones are passed; cached per program).
