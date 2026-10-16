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

Writing choices are Ollama, Codex, Claude Code, Google Antigravity (`agy`) and
GitHub Copilot (`copilot`). Speech remains Parakeet and the existing OpenAI,
Gemini and xAI cloud options. Older API protocol implementations and account names
remain for compatibility and speech credentials; do not expose them as writing
choices. `config/tidy.rs` removes retired named cloud writers from saved chat
chains and rewrites old files on load (`RETIRED` names, fields equal to built-ins or old defaults, empty chain →
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

`config/choice.rs` is the Settings model: `WRITING` (ollama, codex,
claude_code, agy, copilot) and `SPEECH` (parakeet, openai_whisper,
gemini_speech, xai_speech),
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

Speech keys and calendar secrets go through `secret::default_store()`: a private
`credentials.json` beside config by default, or the OS keychain with
`LEO_USE_KEYCHAIN=1`. The file is unencrypted; owner-only file/directory permissions
and atomic replacement protect it from other OS accounts, not programs running as
the same user. The optional keychain holds one JSON item to avoid a permission
prompt per provider after upgrades. Status checks use `SecretStore::has`; tests use
fake stores and never access the real keychain.

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
--tools WebSearch --allowedTools WebSearch --no-session-persistence --system-prompt …` and `codex exec
--ephemeral --ignore-user-config --ignore-rules --sandbox read-only -`, material on stdin, run
in a fresh private `leo-writing-*` temp dir (0700, deleted after; a shared fixed folder let other users plant files or links). Claude Code streams (`--output-format stream-json
--include-partial-messages --verbose`; `read_stream` sinks `text_delta`s and
treats a `result` with `is_error` as a failure); Codex has no partial text, so
its stdout is the answer in one piece.

AGY and Copilot use the same bounded text tool protocol, not native sessions, and
accept text only; scans/photos need a compatible vision writer. `scoped_cli.rs`
creates private `agent-profiles/agy` and `agent-profiles/copilot` beside config,
sets provider-specific home directories, removes inherited token/customization
variables during help probes and real commands, rejects custom MCP/hooks/plugins
and requires advertised isolation flags before launch. AGY gets a private tool-free `leo` agent with command execution
off; Copilot disables built-in MCP, custom instructions, remote export and updates,
with filesystem/shell/memory tools denied. Only the explicitly configured tool
allowlist is available; these controls are not an OS sandbox for arbitrary CLI
binaries. Calls use fresh temporary working folders, while private sign-in/cache
state persists in the profile. `leo doctor --sign-in agy|copilot` signs in there;
an ordinary global CLI login is separate. A cross-process `.leo-profile.lock`
protects calls/sign-in/cleanup. Storage exposes sizes and confirmed clearing only,
refuses an active profile, and never exports authentication contents.

Plan limits (`usage.rs`): `<config>/usage.json` keeps the latest `Usage`
(5h/7d `Window { used 0..1, resets_at }`, `seen_at`) per provider name. Claude
Code's come free with every answer (`rate_limit_event` → `from_claude`, saved
in `AgentCli::run` even when the request failed); never send a request just to
check. Codex's come from an isolation-capable `codex app-server` →
`account/rateLimits/read` (`ask_codex`, no model call); unsupported versions
retain previously saved usage without starting the app-server. Settings' writing row appends `label` (`5h: 9%, 7d: 93%`, plus
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
another page must not suppress it. Photos and scans still require `ai::see`;
local OCR is not included. Text and Markdown remain direct reads. One request for short
material, otherwise ~24k-char text parts and 6-page image batches, then
`build_summary_prompt` for title and summary. Vision: `ChatProvider::
complete_with_images` (default: a clear "cannot read images" error);
`OpenAiChat` sends `image_url` data URLs, Claude Code gets the image inside a
stream-json user message with only `WebSearch` allowed (its Read tool is not confined, so
it is never enabled, and WebFetch is left off so a document cannot make it send notes to a URL), Codex gets `-i` files written into that private dir and
removed after. Codex always runs with `--disable` for shell, browser, computer
and app tools that `codex features list` reports (unknown names make Codex
fail, so only listed ones are passed; cached per program).

### Part sizes follow the writing provider (`ai/budget.rs`)

`writing_budget(cfg)` takes the tightest budget over the chat chain. Codex,
Claude Code, AGY and Copilot use the agent budget (three parts at once); Ollama
uses the smaller local budget (one part). Context/output limits bound actual part
sizes. Long recordings and uploads share this calculation and report progress from
the calling thread. Speech chunking and note synthesis/gap-filling stay unchanged.

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

### The quick model and Python (`ai::quick_config`, `pyodide.rs`)

`quick_outcome` runs the `[chat]` chain with each provider set to the first
(cheapest) model in its `choice::WRITING` list and its lowest effort (Claude
Code: Haiku, low), for interview grading and coverage. `pyodide` pins Pyodide
0.28.3's five files by SHA-256 in `models_dir()/pyodide-0.28.3`, fetched only
when the user first runs Python (`LEO_PYODIDE_URL` / `LEO_PYODIDE_MANIFEST`
for tests). `clang` does the same for C++ (`@yowasp/clang` 22 from the npm
registry, tarball and unpacked files SHA-256 pinned in `models_dir()/clang-…`,
unpacked with `tar`; `fetch_into` is tested with a local tarball). `runtimes`
fetches the Go and Java toolchains leo's own release carries (`GO`/`JAVA`
packs: `leo-go.tar.gz`, `leo-java.tar.gz`, built by CI from
`scripts/go-runner` and `scripts/java-runner`) and trusts one only when every file matches its sum in the pack's
`files.sha256`, which for Go and Rust must also match the pinned sum (Java
builds differ between machines, so leo takes its own release's).

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
prints model, reasoning effort and "tokens used" on stderr (`codex_banner`).
An optional provider `effort` (letters and digits only) is passed as
`--effort` to Claude Code and `-c model_reasoning_effort="…"` to Codex.
`spend::answered` names the provider and prices tokens from `choice.rs`'s
price strings (`rates`); agents are "on your plan" and local models free.
`ai::chat_streaming` returns the text and that `Answered`.

### Context windows (`ai/budget.rs`)

`window_of` supplies Claude Code/AGY 200k, Codex 256k, Copilot 128k and
Ollama/local 8k tokens; a valid `context` setting overrides it. `note_tokens`
reserves 45%, capped at 120k for plan providers; legacy API accounting keeps its
50k cost cap. `felix_room` supplies Felix's note budget, while recording/upload
parts also respect output limits. Effort choices come from `choice::efforts` and
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
isolation capability checks below. AGY/Copilot always use text tools. Every call
goes through Leo's validated `Turn.call`; malformed arguments receive a repair
message, never execution. A provider that cannot open a native session falls back
to the shared text protocol. Legacy API parsers/cache accounting remain internal
compatibility code and are not additional Writing choices. Tests use local fake
servers, never paid APIs.

### Finding notes by meaning (`meaning.rs`)

BAAI bge-small-en-v1.5 (384 numbers per text, 134 MB) runs on the CPU through
candle (`candle-transformers` BERT, `tokenizers` without onig): CLS pooling,
unit length, queries prefixed with `QUERY`. Files pinned to a Hugging Face
commit with SHA-256s (`MODEL_MANIFEST`), kept in
`models_dir()/bge-small-en-v1.5`, loaded on demand (`model()`) and unloaded
after 60 seconds idle once callers release their model handles. `leo update`
fetches it (`ensure_meaning_model`), and `leo serve` fetches it quietly in
the background when missing (`web_felix::meaning`). `LEO_INSTALL_NO_MODEL`
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

Browser-fed manifests are recovered through their browser journal, never by the
terminal startup scan. Simultaneous tracks allocate session folders with exclusive
`create_dir`; metadata uses unique synced temporary files so track startup cannot
share a directory or race on a fixed temporary name.


### Agent, credential and public-fetch boundaries (2026-10-16)

Native Codex sessions and live usage polling require app-server help to advertise
both `--ignore-user-config` and `--ignore-rules`, and pass both flags. Unsupported
versions use the existing isolated text tools and previously saved usage; never
start an app-server that could load the user's MCP servers, hooks or rules.
Feature discovery is bounded and fails closed, including the shell control and
hooks. CLI probes, native turns and text runs share a process-group watchdog that
covers stdin, stdout EOF, exit and pipe joins; native queues and output are bounded.
Isolated text fallback sacrifices native conversation caching and may add latency.
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

Public fetch clients disable proxies, check actual resolved addresses, reject
private IPv4/IPv6 including deprecated site-local addresses, and bound response
bytes and redirects. HTTPS-only fetches cannot downgrade on redirects. OS DNS
lookups run outside the HTTP runtime with a deadline and at most four concurrent
lookup threads; a timed-out OS lookup retains its slot until it actually returns.
The BGE meaning model unloads after 60s idle once no caller retains it, matching
Parakeet's idle policy; loading uses a reservation/condition variable and performs
file/model work outside the cache mutex. `Process::stop` waits/reaps child processes
after killing their process group to prevent defunct zombies. `CODEX_OFF` disables `daemon_auto_start` to
prevent orphan background daemon leaks.

Recording capture uses bounded queues; shutdown signals cancellation without
waiting indefinitely for an active transcription call. Session lock heartbeats
stop/join on drop, and temporary write-up WAVs are guarded. Verified downloads use
owned temporary paths, connection/total deadlines and checksum checks; extraction
has a process deadline. Downloaded models/toolchains remain disk caches until the
user clears them in Storage; there is no automatic LRU quota.
