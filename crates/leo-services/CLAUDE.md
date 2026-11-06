# leo-services

### Streaming goes through the provider trait

`ChatProvider::complete_streaming` has a default that calls `complete` and sinks
the whole answer once, so a provider that cannot stream still works everywhere
streaming is used. `run_chat_chain_with` holds the fallback policy for both paths,
so they cannot drift on which errors are fatal. The streaming chain also takes an
`on_restart` callback: text from a provider that then failed is not part of the
answer, and without discarding it a fallback would glue half of one reply to all
of another.

### AI pipeline

`record` → `session::mic` (cpal, built in) captures 16 kHz mono →
`ai::transcribe_outcome()` runs the `[transcribe]` chain (the selected local model
on this computer) → `ai::long::structure_recording` runs the `[chat]` chain
(whatever /settings chose) → note saved.

Each chain skips unavailable providers silently, advances past retryable
errors (402/408/429/5xx, refused connections), and aborts on fatal ones
(400/401/403/404) so a bad request is not retried against every provider.
Every degradation it did take is reported, so a silent downgrade is never
invisible.

`ai/mod.rs` holds the entry points callers use (`transcribe_outcome`,
`chat_outcome`, `chat_streaming`, `quick_outcome`, `open_session`, ...); nothing
above `ai/` knows about providers.

### The note prompts

`ai/chat.rs` builds every chat prompt as `Prompt { system, user }`: rules in
the system message, material in the user message inside `<transcript>`,
`<existing_notes>`, `<typed_points>` or `<note>` tags, with the essentials
restated after the transcript. The note prompts ask the model to fix clearly
misheard terms, and to fill gaps from its own knowledge where the recording or
speaker was unclear (the user wants this kept). Typed points (`POINTS_RULE`)
go into the sections they belong to in the user's exact words (copied as
written, the explanation built around them; 2026-10-10, from Granola): no section
of their own, no times (the user asked for this). Measured on Codex and Claude
Code: the old "your own words or theirs" rule reworded all three test points,
the new one kept all three (`the_real_writing_ai_keeps_typed_points_word_for_word`,
ignored; run with a LEO_HOME that has a writing AI). `long::points_kept` puts back
any point the reply lost (85% of its words, or the whole point, must be there)
under `## Your points, as you wrote them`. Long recordings send each part the points typed
during it (`long::points_for`: every point to exactly one part); only a
fallback with no AI answer lists them, as a plain `## Key points`. Replies go through `clean_reply` (outer code fence, "Here are your
notes:", "Let me know…") and `split_title_body` (strips "Title:", `#`, bold,
quotes). A reply that hits `max_tokens` is kept and warned
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
sessions are resumed by the app (`recorder::resume`).
`session/stress.rs` simulates 3 h and (ignored) 24 h with a flaky provider and
a crash copy.

The live transcript, for catalog models marked fast enough, shows finished segment texts plus a live tail: every `ROLL_INTERVAL` (3 s local, `CLOUD_ROLL_INTERVAL`
8 s cloud) it re-transcribes from `settled_at`, at most `LIVE_MOST_SECS` (20 s),
and once that is `SETTLE_AFTER_SECS` (12) long it settles at `live::quietest`
between 6 s and end − 2 s, so the unsettled end is rewritten rather than glued.
Only while the transcription backlog is at most one segment. Typed points go to
the worker via `Job::add_point` and are saved in the manifest. Recording needs
only speech AI and a microphone; the writing AI is optional. `LEO_NO_MICROPHONE`
turns the device off (tests, e2e, CI).

### Model configuration

Writing choices are Ollama, Codex and Claude Code. Speech is local only. The
`parakeet` provider name calls `ai/provider/local_speech.rs`. Provider kinds are
`openai` (Ollama and other OpenAI-compatible servers), `parakeet`, `claude_code`
and `codex`. Loading a saved config removes (`config/tidy.rs`, before
deserialization) any provider block of another kind, such as the old cloud
speech kinds, `whisper_cpp`, `agy` and `copilot`, drops chain entries that named
them and falls back to the local defaults; a leftover `[theme]` table is ignored.

`speech/catalog.json` is the pinned catalog; `catalog.rs` exposes its 29 curated
ASR models, languages, checksums, benchmark speeds and accuracy. Accuracy uses
the chosen language, or the geometric mean of measured word error rates for
auto detection; missing or invalid measurements remain unknown. The selected
model and language live in `<config>/speech-model.json`; files live in
`models_dir()/speech`. `Library` verifies exact size and SHA-256, keeps verified
size/mtime stamps in `<config>/speech-verified.json` (hashing on every launch held
the first dictation back 1.6 s before its microphone opened), downloads at most two models into owned temporary files,
checks cancellation while awaiting the network, and promotes atomically.
Linked paths and hard-linked models are refused. Selection/deletion/promotion
use a private file lock. An empty selected ID means the user deleted
the current model.

`speech/engine.rs` admits eight jobs, gives dictation priority between audio
chunks, and pauses lower-priority work during a dictation reservation. One
actor owns the supervisor and model. Recording pins it; otherwise idle unload
is five minutes (Handy's default). Stop and updater unload retry a full request channel within one
five-second deadline shared by enqueue and acknowledgement; ordinary job
admission stays immediate and bounded. Saturation tests free queue capacity
explicitly rather than depending on actor-thread scheduling.
Model deletion waits behind inference, unloads the worker first,
and refuses deletion of an active recording/dictation model. Long audio uses
30-second target / 40-second maximum cuts at VAD pauses. `vad::detector` is Silero
at 0.3, Handy's default, with Earshot at 0.5 if Silero cannot load. Dictation filters
while capturing, and the engine only cuts and pads clips under a second, as Handy
does. Recordings have no Handy equivalent and are never trimmed: a chunk goes to the
model whole unless VAD, on a copy raised to speech level (median frame level of the
whole clip to 0.1, gain 1-50), hears nothing in it (2026-11-04: lecturers often have
no microphone, and the audio is deleted once transcribed). On a synthetic distant
voice at 8 dB over room noise with a loud nearby talker 15% of the time, words found
were 63% with 95th percentile raised trimming vs 90% untrimmed; plain Silero trimming
of a distant voice gave 34.7% WER vs 13.1%. Chatter 10 dB under the voice or closer
defeats the model itself (59%+ WER untrimmed). Raising means amplified noise can
reach the model; Parakeet stayed empty on it, Whisper was not tested. Slow catalog
models skip live passes.

`speech/supervisor.rs` owns a crash-isolated worker, re-executing the current
binary with `--leo-speech-worker` before normal app/CLI startup. Only the worker
initializes GPU backends, loads a speech model or runs speech inference. The one
exception is Silero VAD: a small CPU-only model that runs in the app process on a
CPU-mask backend, as in Handy (2026-11-04, the user's call), written to
`models_dir()/vad` from bytes embedded in the crate and compared before first use.
Each detector loads its own `Model`, because one model's compute lock would make
dictation wait behind chunk scoring. A native fault there takes the app down, which
the worker rule otherwise prevents. Framed JSON/PCM,
queues, stderr and deadlines are bounded; crashes/hangs are reaped and retried
once (CPU after GPU failure). Apple Silicon uses Metal; Intel macOS and Windows
use CPU. The Rust binding and native sys crate are both pinned to exactly
0.4.0: pre-1.0 native patch versions are not ABI-compatible. The outer actor
keeps filesystem/native work outside application mutexes.

`Config::load` and `load_from` never touch credentials. Settings offers no speech
key controls. Calendar secrets still use the private credentials file by default
or the optional keychain with `LEO_USE_KEYCHAIN=1`; tests use MemoryStore.

`config/choice.rs` keeps writing model/effort choices and the single local
speech entry. The full speech catalog reaches the page through `SpeechApi`,
implemented in leo-host. `web_settings.rs` still owns writing and backup
settings, validates changes against `choice`, and writes through `toml_edit`.

`claude_code` and `codex` (kinds `ClaudeCode`/`Codex`, `ai/provider/agent_cli.rs`)
write through the user's own signed-in CLI and plan: `claude -p --safe-mode
--tools WebSearch --allowedTools WebSearch --no-session-persistence --system-prompt-file <private file>` (a file, not an argument: Windows refuses line breaks in arguments to npm's `claude.cmd`, and cmd.exe caps a command line at 8,191 characters) and `codex exec
--ephemeral --ignore-user-config --ignore-rules --sandbox read-only -`, material on stdin, run
in a fresh private `leo-writing-*` temp dir (0700, deleted after; a shared fixed folder let other users plant files or links). Claude Code streams (`--output-format stream-json
--include-partial-messages --verbose`; `read_stream` sinks `text_delta`s and
treats a `result` with `is_error` as a failure); Codex has no partial text, so
its stdout is the answer in one piece.

All writing CLI launches, native sessions, capability probes and usage polling
clear inherited credentials, provider/routing overrides and runtime startup hooks.
Codex and Claude Code retain the user's signed-in HOME, XDG, CODEX_HOME and
CLAUDE_CONFIG_DIR locations plus OS/PATH, certificate settings and the Linux
D-Bus session address used by the signed-in keyring. An ambient API key cannot
silently replace the intended CLI login. A deliberately stored API-key login is
still the CLI's own authentication choice; environment cleanup cannot guarantee
subscription billing in that case.

Plan limits (`usage.rs`): `<config>/usage.json` keeps the latest `Usage`
(5h/7d `Window { used 0..1, resets_at }`, `seen_at`) per provider name. Claude
Code's come free with every answer (`rate_limit_event` → `from_claude`, saved
in `AgentCli::run` even when the request failed); never send a request just to
check. Codex's come from an isolation-capable `codex app-server` →
`account/rateLimits/read` (`ask_codex`, no model call); unsupported versions
fall back to recent local Codex `token_count` rate-limit events without starting the
app-server. This read-only fallback scans bounded recent folders and file tails,
rejects links, preserves the event timestamp and never exposes conversation text;
an older event cannot replace newer saved usage. Settings' writing row appends `label` (`5h: 9%, 7d: 93%`, plus
age past `STALE_AFTER`, a window past its reset reads 0%); the Usage action in
Settings asks Codex; doctor adds `<name> limits` with
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
`toml_edit` (creating the commented default file when missing).

`coding_plans` detects both configured coding CLIs without changing the selected
writer. It returns only availability, tri-state sign-in and saved usage windows;
auth probes use the same cleaned plan environment as writing and never expose
their raw output or make a model call. An installed CLI alone is not sign-in.
Availability and sign-in use the same normalized `AgentCli::bin`, including
when a missing, empty or whitespace-only setting falls back to the default CLI.

`import.rs` turns uploads into a note: native Markdown extraction precedes AI
study-note writing. `import/office.rs` preserves Word headings (including
inherited styles), lists, emphasis, hyperlinks, tables and common OMML equations;
PowerPoint follows presentation and notes relationships, orders shapes by
position, and preserves titles, lists, tables and cached chart values. Picture
slide numbers follow the same presentation order. This is a Rust implementation
of the relevant [MarkItDown](https://github.com/microsoft/markitdown) conversion
patterns, not a Python dependency or a complete port of its other formats.
Office parts, decompressed bytes, XML nodes/depth and output are bounded; a
streaming XML preflight rejects DTDs and excessive depth before tree parsing.
Relationships stay inside the ZIP; external links are never fetched.
PDF text uses `lopdf::extract_text_with_limit` per page: short digital text stays
local, while empty pages or sparse text over a nearly full-page image use JPEG
or RGB/gray PNG scan images. A page number must not hide a scan, and text on
another page must not suppress it. PDF image decoding lives in `figures/pdf.rs`.
JPEG and RGB/gray Flate scans above 16 million pixels get an aspect-preserving
reading copy with a 4,000 px longest edge; the original PDF is never rewritten.
Source images remain capped at 64 million pixels, 32,768 px per side and 20 MB
encoded; image decoding uses a 192 MiB allocation limit and output uses a bounded
writer. Unpredicted Flate data is processed row by row rather than copied into
multiple full decoded buffers. Prepared scan figures are reused, reading images
share a 90 MB budget, and optional figures share 64 MB. An unreadable scan fails
with its page number instead of silently leaving only that page's sparse text.
The same extractor serves new-note imports and documents attached to existing
Felix/note conversations. Photos and scans still require `ai::see`;
local OCR is not included. Text and Markdown remain direct reads. One request for short
material, otherwise ~24k-char text parts and 6-page image batches, then
`build_summary_prompt` for title and summary. A single upload whose file name is a
tracking name (`leo_core::filename::file_label`: letters and a number, like
`ma161_hw3`, never camera, scan, screenshot or date names) keeps it: the
prompt asks for it first and `filename::with_label` puts it in front of any
title without it (2026-10-20, the user's request). A recording made for a
calendar event with such a name (`tracking_label`) keeps it the same way.
Otherwise `series_label` looks for a numbered lecture, week, chapter,
homework, lab or problem set ("Lecture 1", "CS 61B Lecture 3", "hw3") in
the file name, then in the first 2,000 characters of the material (its title
slide), or in the event's name.
Action items, in recordings and uploads, are the fewest that matter: one per
deliverable or date, never sub-steps, readings or study advice. Vision: `ChatProvider::
complete_with_images` (default: a clear "cannot read images" error);
`OpenAiChat` sends `image_url` data URLs, Claude Code gets the image inside a
stream-json user message with only `WebSearch` allowed (its Read tool is not confined, so
it is never enabled, and WebFetch is left off so a document cannot make it send notes to a URL), Codex gets `-i` files written into that private dir and
removed after. Codex always runs with `--disable` for shell, browser, computer
and app tools that `codex features list` reports (unknown names make Codex
fail, so only listed ones are passed; cached per program).

### Part sizes follow the writing provider (`ai/budget.rs`)

`writing_budget(cfg)` takes the tightest budget over the chat chain. Codex and
Claude Code use the agent budget (three parts at once); Ollama uses the smaller
local budget (one part). Context/output limits bound actual part sizes. Long
recordings and uploads share this calculation and report progress from the
calling thread. Note synthesis/gap-filling stay unchanged.

### Claude Code gets leo's tools natively (`ai/provider/mcp.rs`)

When Felix offers tools, `session::open` starts Claude Code with
`AgentCli::claude_native`: a `Bridge` (an MCP server over HTTP on
127.0.0.1, one per answer, its address carrying a random 64-hex secret;
requests with an `Origin` header or another path get 404) lists the tools
with their JSON schemas and forwards each `tools/call` to the running turn
(`Incoming::Call` on the same channel as Claude's stdout lines), where
`ClaudeSession::say_with` runs it through `Turn.call`. Claude then chains
calls inside one turn like Codex instead of ending a turn per call, and its
arguments are structured, so long briefs with quotes and line breaks cannot
break a call. `--safe-mode` turns MCP off, so native sessions isolate with
`--setting-sources "" --disable-slash-commands --strict-mcp-config` (no user
settings, hooks, plugins, skills or other MCP servers; safe mode still listed
the user's plugins) and allow only `WebSearch mcp__leo`. If the bridge cannot
start, the text protocol is used. Cache reads and writes from Claude Code's
`result.usage` are recorded (`Heard.cache`). Measured with Claude Code
2.1.295 (`real_claude_calls_leos_tools_natively_inside_one_turn`, ignored):
two calls and the answer in one turn, about 5 s.

### The quick model (`ai::quick_config`)

`quick_outcome` runs the `[chat]` chain with each provider set to the cheapest
model in its `choice::WRITING` list and its lowest effort, for Study grading and
other shared quick reviews. `Powers.quick` remains independent of the retired
interview runtimes. Runtime downloads and pinning were removed on 2026-11-05;
existing downloaded caches stay on disk; the Storage page that listed them was removed on 2026-11-05.

### What the agent CLIs may use

Web search is allowed (the user wants it): Claude Code gets `--tools WebSearch
--allowedTools WebSearch` and nothing else; Codex keeps its default web search.
Codex is given `--disable` for every feature in `CODEX_OFF` that `codex
features list` reports (shell, browser and computer use, apps, plugins, skills,
goals, multi-agent, image generation, tool suggestions), so its own tools do
not pull it away from leo's text tools. Never disable `code_mode_host`,
`unified_exec` or `shell_snapshot`: Codex answers through code mode, and with
it off every reply came back empty (measured). Commands still cannot run in
the read-only sandbox.

Agent CLI runs have a watchdog: Claude Code is stopped after `QUIET_LIMIT`
(180 s) without any output, either CLI after `TOTAL_LIMIT` (15 min); the whole
process group is killed (Unix), since a child holding the pipe kept leo
waiting, and the run is tried once more. A Claude Code turn really did hang for
21 minutes before this.

### What an answer used (`ai/spend.rs`)

Every `ChatProvider` may report `spent()` for its last answer (`Spent { model,
effort, input, output, estimated }`); the chain copies it into
`ChainOutcome.spent`. OpenAI-compatible providers read `usage` (and ask for it
while streaming only from api.openai.com, openrouter.ai and api.x.ai, which are
known to accept `stream_options`), else estimate 4 characters a token. Claude
Code gives its model in the `init` event and tokens in `result.usage`; Codex
uses `exec --json` agent messages and `turn.completed.usage`, including cached
input. Reasoning tokens are already included in exec output tokens and are not
added twice. Stderr banners remain a legacy fallback.
An optional provider `effort` (letters and digits only) is passed as
`--effort` to Claude Code and `-c model_reasoning_effort="…"` to Codex.
`spend::answered` names the provider and prices tokens from `choice.rs`'s
price strings (`rates`); agents are "on your plan" and local models free.
`ai::chat_streaming` returns the text and that `Answered`.

### Context windows (`ai/budget.rs`)

`window_of` supplies Claude Code 200k, Codex 256k and
Ollama/local 8k tokens; a valid `context` setting overrides it. `note_tokens`
reserves 45%, capped at 120k for plan providers; legacy API accounting keeps its
50k cost cap. `felix_room` independently caps Felix's note budget at 32,000
characters, or the weakest provider's smaller allowance. `felix_context` separately
bounds warm-session retention by the weakest model window, capped at 480,000
characters, so smaller retrieval does not force frequent cold starts. Recording/upload
synthesis and gap-filling retain their larger model-sized budgets and output limits. Effort choices come from `choice::efforts` and
are passed only in the selected provider's supported format.

### How notes are shaped (`ai::chat::LEARNING`)

Recording notes (whole, appended and per part) and upload notes share one
guide: tell it as a story (why it matters, the core idea, build up from the
simple case), examples right where ideas appear, connections and common
mistakes, Mermaid diagrams and the material's pictures where they help; code
problems get a breakdown, then the code, then foldable `> [!example]-`
explanations; every note ends with `## Check yourself` questions whose
answers fold away (only the last part of a long recording or upload writes
it). Uploads may explain beyond the source; facts stay the source's.

### Native tools and text fallback (`ai/session.rs`)

Ollama uses OpenAI-compatible native tool calls when its model supports them;
Claude Code uses the loopback MCP bridge, and Codex native sessions require the
isolation capability checks below. Every call
goes through Leo's validated `Turn.call`; malformed arguments receive a repair
message, never execution. Claude Code that cannot open a native session falls back
to the shared text protocol. A Codex whose app-server cannot start (an old
version that cannot isolate native sessions) is skipped for native sessions, so
Felix answers through the same text protocol; the `codex exec` resume and
stateless Codex sessions are gone.
Tests use local fake servers, never paid APIs.

Session turns check cancellation before more requests or tools; writing CLI output
is polled through bounded queues and stopped children are killed and reaped.
Native tool budgets and a final no-tools request bound models that ignore limits.

### Finding notes by meaning (`meaning.rs`)

All three Candle crates pin 0.10.0: 0.11 uses ARM FP16 types unavailable on
the workspace's Rust 1.88 floor. The model, tokenizer and pooling stay unchanged.

BAAI bge-small-en-v1.5 (384 numbers per text, 134 MB) runs on the CPU through
candle (`candle-transformers` BERT, `tokenizers` without onig): CLS pooling,
unit length, queries prefixed with `QUERY`. Files pinned to a Hugging Face
commit with SHA-256s (`MODEL_MANIFEST`), kept in
`models_dir()/bge-small-en-v1.5`, loaded on demand (`model()`) and unloaded
after 60 seconds idle once callers release their model handles. `leo update`
fetches it (`ensure_meaning_model`), and the app fetches it quietly in
the background when missing (`leo_host::felix::meaning`). `LEO_INSTALL_NO_MODEL`
or `LEO_NO_MEANING_MODEL` turns that off (e2e and Playwright set them);
`LEO_MEANING_URL` / `LEO_MEANING_MANIFEST` point tests at file:// stand-ins.
Doctor's notes section says whether it is ready. The real-model test is
`#[ignore]`d.

### Durable recording workflows

`Session::archive` retains timestamped passages, original points, the event
context and what the user wants, warnings and metadata-only diagnostics. `recorder::write_up` caches its prepared
result by input hash; only a successful caller save permits `Session::commit` and
audio cleanup. Finish retries, including persistent rate limits, have a bounded
window; Finish available cancels queued work while allowing the current request to
complete. Failed audio stays recoverable. `long::with_profile` adds the calendar event's
details (`<recording_context>`) and what the user wants (`<what_the_user_wants>`,
restated last) while keeping transcript repair, knowledge gap-filling and point
merging. `web::fetch_public` is the one https fetch for addresses the user gives
(calendar links): public hosts only, capped size.

Simultaneous tracks allocate session folders with exclusive
`create_dir`; metadata uses unique synced temporary files so track startup cannot
share a directory or race on a fixed temporary name.


### Agent, credential and public-fetch boundaries (2026-10-16)

Native Codex sessions and live usage polling require app-server help to advertise
both `--ignore-user-config` and `--ignore-rules`, and pass both flags. Unsupported
versions give an update-Codex error for writing and keep previously saved usage; never
start an app-server that could load the user's MCP servers, hooks or rules.
Feature discovery is bounded and fails closed, including the shell control and
hooks. CLI probes, native turns and text runs share a process-group watchdog that
covers stdin, stdout EOF, exit and pipe joins; native queues and output are bounded.
Claude's loopback MCP bridge caps active connections at eight, closes registered
sockets on drop, checks shutdown during pending calls, and bounds writes, batches
and requests per connection. Request reads preserve partial bodies across polling
timeouts and check cancellation on Windows as well as Unix.

Credential updates use one bounded writer queue without I/O under a mutex, unique
synced 0600 temporary files, bounded nofollow regular-file reads and atomic replace.
Corrupt credential files are reported and preserved; permission repair refuses
symbolic/hard links. The writer also holds an exclusive `.credentials.lock`
through read/modify/write, with a five-second lock deadline, so independent Leo
processes cannot overwrite each other's credential changes.
Private-file guards explicitly unlock before closing, so a handle inherited by
a concurrently starting child cannot keep completed settings operations locked.

Public fetch clients disable proxies, check actual resolved addresses, reject
private IPv4/IPv6 including deprecated site-local addresses, and bound response
bytes and redirects. HTTPS-only fetches cannot downgrade on redirects. OS DNS
lookups run outside the HTTP runtime with a deadline and at most four concurrent
lookup threads; a timed-out OS lookup retains its slot until it actually returns.
The BGE meaning model unloads after 60s idle once no caller retains it, matching
the speech worker's idle policy; loading uses a reservation/condition variable and performs
file/model work outside the cache mutex. `Process::stop` waits/reaps child processes
after killing their process group to prevent defunct zombies. `CODEX_OFF` disables `daemon_auto_start` to
prevent orphan background daemon leaks.

Recording capture uses bounded queues; shutdown signals cancellation without
waiting indefinitely for an active transcription call. Session lock heartbeats
stop/join on drop, and temporary write-up WAVs are guarded. Verified downloads use
owned temporary paths, connection/total deadlines and checksum checks; extraction
has a process deadline. Downloaded models/toolchains remain disk caches that leo never
expires; there is no automatic LRU quota.

Synthetic capture regressions exercise twelve segment rolls and check every
segment's sample count/overlap using short segment intervals. They avoid processing
an hour of PCM in an unoptimized, parallel CI run merely to test the same boundaries.

Capability probes combine stdout and stderr into one bounded pipe so CLIs may
write successful help listings to either stream. The command's parent pipe writers
are dropped immediately after spawn so EOF remains meaningful; the existing
output cap and whole-process watchdog cover both streams. A present CLI that
fails this check reports unverified safety controls, never a missing installation.

Leo also uses the existing quick-model configuration for long-chat memory;
chat titles are local. The root wiring meters writing/quick/summary outcomes before
string-only writer callbacks discard token metadata. Foreground Felix turns and
hidden grading are metered in leo-web, without recording prompt or reply contents.

Stateless cancellation tests wait for the fake CLI's startup marker before
requesting cancellation, including an intentionally delayed startup. Shutdown
latency is measured from that signal, and the child PID must be gone afterward;
runner scheduling time must not be mistaken for a provider-cleanup failure.

Native app recording uses `Source::Microphone` / `Source::Screen`; calls capture
both tracks in Rust. `session::awake::Awake` holds an App Nap/process activity
exception through recording. Capture opens only when requested, never because
the page is visible. A missing/damaged model leaves stopped segments pending
without `.err` files; a model becoming ready retries them, and Finish available
still cancels waiting work. Transcription attempts and filesystem traversal are
snapshotted outside their mutexes before disk I/O.

`recorder::resume_with_controls` exposes the same finish control during native
recovery. leo-host groups retained call tracks by recording ID, forwards recovery
progress, and refuses to write a note when recovery failed. Audio is committed
only after the caller durably saves the note and archive.


## Dictation

`dictation/settings.rs` validates bounded preferences in private atomic
`<config>/dictation.json`; failed live rebinds restore the previous file.
Malformed settings fall back without overwriting the damaged file, leaving a
repair message for Settings. `activation.rs` implements hold, tap and the
300 ms hold-or-tap decision, ignoring repeats. `generation.rs` gates completion
and state updates after acquiring their state lock, so cancel wins stale work.

`dictation/text.rs` applies custom-word fuzzy/phonetic matching at 0.18 before
language-aware filler rules (on by default), with Unicode-safe punctuation and
capitalization, then writes clock times only when marked (`230 pm` → `2:30 pm`;
an AM radio dial like `1050 AM` is the accepted casualty): a bare `230` may be
a quantity, so context is left to the AI step. `polish.rs` is the optional AI writing step (`Writing`: off
by default, `quick` = `ai::quick_outcome`, the cheapest model and effort on the
user's plan, 12 s, or `selected` = the Writing AI choice, 30 s): a cleanup
prompt (2026-11-03, the user's call) that removes fillers and verbal tics,
reformats into paragraphs/lists and writes times like 2:30 from context, and
treats the dictation as text, never instructions. Replies are only
unwrapped (code fence, `<dictation>` tags), never run through `clean_reply`,
which would drop dictated "Sure…" or "Let me know…" lines. A reply must keep
70% of the dictation's words (characters for unspaced scripts) and add at most
half new ones (15% when the dictation is a question), so an answer is refused.
`FILLERS` are left out of that count and number words/digits count as one
token, so removing "like"/"you know" or writing "two thirty" as 2:30 is not a
change of meaning, but adding a number that was never said is. A reply must
also keep the same number of negations (`not`, `n't`, `dont`, `never`,
`ne`/`pas`, `nicht`…), so a meaning flip is refused. Failures are logged without the provider's error text, which can
contain the dictation. The call goes through
`ChatProvider::complete_until`, so a cancel or the deadline stops an agent CLI
instead of leaving it running. Every fallback returns a reason the popup shows. `capture.rs` bounds dictation to
300 seconds, closes the input before draining its final queued audio and before
transcription. `Mic::open_named` opens only the selected input; `microphones`
enumerates without opening. `RETIRED` keys (lid-closed microphone, trailing
space, Enter) are stripped before parsing, since the struct denies unknown fields.

`AudioGate` makes lecture capture wait for an already-playing feedback sound
and prevents new sounds until that recording ends. It does no I/O under a mutex.
The engine actor owns streaming dictation too; bounded feeds fall back to the
retained batch PCM when streaming fails. Live audio queued while the worker is
busy is coalesced into the next feed (at most 16,000 samples each) instead of
abandoning the stream, and the preview callback receives committed and
tentative text separately. `Engine::warm` loads the selected model in the background (dictation sends
it for non-streaming models at keypress, as Handy does). A reservation pauses
segment/live passes; recording's pin retains the worker through dictation.

Health checks use owned temporary files for notes writability, preserving any
existing `.leo-doctor-probe`. Agent sign-in probes accept only recognized status
output, honor exit status and share the bounded process watchdog. `Engine::unload_worker`
reaps the current worker without closing the actor, so a failed app update can
resume transcription. Native recording holds a core activity lease until finished.
The model library lists damaged catalog files and uncatalogued regular GGUF files
with disk totals; deletion stays contained and never follows links.

The terminal update command fetches the published release installer with HTTPS,
time/size limits and explicit failure handling before executing it. It never
pipes a failed curl into an empty successful shell. Model checking still goes
through the shared catalog library; legacy ONNX installer fixtures are retired.

### Recording energy

Lecture live previews pass PCM directly to the speech engine; they never write
or synchronize a temporary live.wav. `Controls.preview` optionally carries the
native window's visibility; false skips new preview passes only. Capture, durable
segments, final transcription, recovery and the recording activity guard are
unchanged. An already-running preview finishes; showing the window permits the
next pass. Dictation and callers without a preview flag retain their behavior.
