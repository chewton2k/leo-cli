# leo-desktop: the leo app

The Tauri 2 app (`leo-app`, product "Leo", bundle id `io.github.chewton2k.leo`).
Rust 1.90 (Tauri's floor); not in `default-members`, so plain `cargo test` skips
it. Try it by hand with `pnpm --dir crates/leo-desktop tauri dev` (the one local
build allowed); point `LEO_HOME` at a throwaway folder to keep real notes out of
a test, or leave it unset to see the user's own data.

## Startup (`lib.rs`)

1. macOS: `PATH` = the login shell's PATH (`shell_path::login_path`, markers,
   3 s timeout, process group killed on timeout) merged with the current PATH and
   `usual_folders` (Homebrew, `/usr/local/bin`, `~/.local/bin`, `~/.cargo/bin`),
   so Codex, Claude Code, Ollama, gh and git are found when Finder starts leo.
2. `leo_web::start` on `port::remembered` (`<config>/app-port`, default 47318,
   kept stable because localStorage and IndexedDB are per origin), with
   `fresh_token()`; the port actually used is remembered. A failure opens
   `dist/index.html?problem=…` instead.
3. Plugins: single instance (a second launch shows the window), autostart
   (enabled once, marker `<config>/app-launched`; never in debug builds).
4. Window `main`: the engine's sign-in URL, 1200×800, `--hidden` when started at
   login. macOS: overlay title bar, hidden title, window buttons at (18, 31) in
   the sidebar head; `window.__LEO_APP__ = 'mac'` makes `theme.js` set
   `html[data-app="mac"]`, which hides the sidebar wordmark and makes room for
   the buttons (2026-10-31, the user found the grey title strip ugly). `#bar`
   and `.side-head` are `data-tauri-drag-region="deep"`. The icon rail stays
   visible when the explorer is hidden; the sidebar toggle sits fixed right after
   the window buttons whether the explorer is shown or hidden, and the folded
   toolbar leaves room for both.

## Behaviour

- Close hides the window and queues a backup; Dock click and "Open leo" show
  it; "Quit leo" (menu bar icon) or ⌘Q exits, and `RunEvent::Exit` backs up and
  stops the engine (6 s cap). Tauri's default menu supplies Edit (⌘C/⌘V/⌘Z/⌘A)
  and binds none of the page's shortcuts.
- `links::stays_inside`: only `http://127.0.0.1|localhost:<port>` and the
  bundled page stay in the window; navigation and new windows elsewhere open in
  the default browser.
- Menu bar icon (`tray.rs`, 2026-11-02, the user's request): Open Leo, Record a
  lecture / Record a call (shows the window and calls the page's
  `window.leoFromApp('record', kind)`, which presses Start for that kind; a
  running recording is just shown), Copy last dictation (newest row of
  `leo_web::dictation::recent`, native clipboard), status rows (speech model
  `Library::selected_name`, writing AI and model plus 5h/7d plan use from
  `web_settings::writing_status`, AI use since launch `Running::usage_line`),
  Settings…, Check for updates… (queues a check, opens Updates & about), the
  version, Quit. A `leo-tray-status` thread refreshes the rows every 20 s; Codex
  limits are asked from its app-server at most every 5 min and only when Codex is
  the chosen writing AI and its saved use is stale (`writing_status(may_ask)`).
  Page commands are offered for 5 s (the page may still be loading) and run once
  per click (`__leoAsked`); copying runs off the main thread. A speech model that
  is chosen but not on disk reads "(not downloaded)".
- The window disables Tauri's drag-drop handler (`disable_drag_drop_handler`):
  left on, it swallows the webview's HTML drag events, so moving notes and
  folders and combining notes did nothing in the app (2026-11-02). Dropped
  files reach the page's own handlers (`drag.js`).
- Downloads save to `~/Downloads` under a free name (`links::download_path`)
  and are revealed when done.
- `window.print` calls the `print_page` command (the webview's own print).
- Capability `default`: remote `http://127.0.0.1:*`/`localhost:*`, core
  defaults, window dragging and double-click maximize.

The `leo-app` entry checks `speech::is_worker_invocation` before Tauri and
single-instance startup; native speech runs only in that child process.
Info.plist declares microphone and computer-audio capture uses. Recording
capture and its process activity guard live in services and continue when
`main` is hidden; the webview only controls and displays the job.


## Dictation

`Dictation` is managed as an Arc before the loopback engine starts and supplies
`Powers.dictation`. A bounded coordinator serializes shortcut inputs, handles
hold/tap and remembered input during decoding, and rejects stale completion by
generation. It opens the selected microphone only for capture. Lid-closed
microphone, trailing space and Press Enter after pasting were removed
(2026-11-02, the user's call); `dictation.json` files that still name them load
and drop them on the next save. Capture releases
its input before local VAD/transcription, word rules, local history and paste.
Start/stop WAVs are optional and excluded from lecture recordings by AudioGate;
they play on their own thread after the microphone is open (Handy's order), so
the start chime never delays capture and the first words are kept. The app warms
CoreAudio at launch and non-streaming models load at keypress.
Debug-only replay/output controls require `LEO_DICTATION_TEST`; normal builds
never expose the test actions or write a test paste file.

`shortcut.rs` owns handy-keys on a dedicated thread, re-creating it after
Accessibility changes. Recording a binding pauses global registration, supports
modifier-only/Fn and expires after 30 s. Secure Input uses the Carbon fallback
for representable chords (side-specific modifiers widen); modifier-only events
stay on handy-keys. Escape registers only during capture, including fallback,
and is reserved from being the dictation binding. Status exposes missing
permissions, unavailable registration and uncovered Secure Input chords.

`paste_tx` publishes lazy clipboard data, waits for post-injection reads and a
200 ms quiet period, with an 8 s cap. Restoration checks ownership again; a newer
copy wins.
macOS snapshots every pasteboard item/type. Windows retains memory formats and
copies bitmap, metafile and palette handles; a format it cannot preserve causes
an error before publishing. Total snapshot bytes/items/types are bounded.
Enigo is taken out of its state slot for native I/O; layout-aware Cmd+V resolves
on the main thread. Without Accessibility or with an elevated Windows target,
text remains on the clipboard with a manual-paste instruction.

`overlay.rs` creates a bundled `dist/dictation.html` popup: a nonactivating NSPanel
on macOS, nonfocusable always-on-top window on Windows. It uses the focused
window's monitor, with cursor/primary fallbacks, and supports top/bottom and
Live/Minimal/Off. Its CSS token block comes directly from the web page's theme;
the main page sends Light/Dark/Match changes. Queued shows and updates check the
current generation. The popup's IPC is limited to its own window label.
The popup follows Handy's live preview: state changes are pushed as a
`dictation` event (revision-stamped, skipped when the popup is Off; the page
polls only if listening fails), committed and tentative words render with a
caret, the text pins to its newest line, and 16 FFT buckets
(`dictation::visualizer`) drive nine smoothed bars. Streaming models get the
1650 ms streaming VAD hangover and ~32 ms feeds, so the model hears the tail
it needs for the last words and their punctuation; text stays visible while
the final pass runs. With AI writing on, Escape stays registered through the
polishing step (its own `escape` flag, separate from `capturing`), and a
fallback shows a four-second "Pasted without AI writing" notice with the
reason. Settings warns when no writing AI is set up (`writing_ready`).
Shutdown cancels dictation/shortcuts and stops the speech worker before the
loopback engine finishes.

## Settings and updates

`app_settings.rs` serializes atomic private app preferences through a bounded
actor; native login/tray failures restore the previous file and cached state.
First-launch microphone, Accessibility and model completion flags persist
independently; the page groups both permissions before the model choice.
Permission status checks never open the microphone; requests are explicit.
A microphone request after a macOS denial opens the Microphone privacy pane,
since the system shows its dialog only once.
Health combines host doctor checks with native permission, model and shortcut
checks. macOS helper output is bounded, nonblocking and timed out by process group.

`updates.rs` owns updater checks, verified downloads and installation on one actor.
Automatic checks and install-on-quit default on; debug startup checks stay off.
Downloads use public HTTPS resolution, deadlines and a one-GiB cap. Signed bytes
stay in memory. Core activity leases cover capture through clipboard restoration;
installation takes an exclusive lease, unloads/reaps the speech worker, and keeps
the lease until exit. Failed installation leaves the engine actor usable.

Release-only `tauri.release.conf.json` includes the root CLI as `binaries/leo`,
so ordinary checks/dev need no prebuilt sidecar. CI builds that command first,
then Tauri bundles/signs it with the app and produces updater artifacts. The
committed updater key is public only. Stable private signing files live outside
this repository; release requires four repository Actions secrets. Microphone
entitlements accompany hardened runtime. Config omits version; workspace version
and scripts/set-version.sh remain authoritative.

Release assets are macOS arm64/x64 app archives and DMGs plus Windows x64 NSIS.
CI uses one stable self-signed identity and verifies both bundle and CLI signatures;
updater signatures cover the same app archive or NSIS used by terminal installation.
Windows uses the dynamic CRT to match transcribe-cpp; its release-only config
bundles the Visual C++ runtime DLLs next to the executables.
No Linux release/test rows remain. Chromium and targeted WebKit browser jobs run
on macOS. Main installer cutover happens only after published assets validate.

The bundle minimum is macOS 10.15, matching the native C++ filesystem floor.
The macOS build script links clang_rt.osx for Metal availability checks and
weak-links CoreAudio so older supported systems can load the app. Compiler
runtime discovery uses quiet_command with an 8 KiB output bound and five-second
deadlines; leo-core is a build dependency for the same child-process rules.

Release preflight signs a disposable probe with the updater key before the
version bump or native builds. Signer output is suppressed on failure so only
secret names and corrective steps appear; private values stay in the environment.

## Idle energy

`shortcut_events` owns the raw handy-keys listener and forwards relevant events
through a bounded queue. The coordinator waits on that queue; the old 10 ms poll
and handy-keys manager timer are gone. One-second waits cover permission/Secure
Input checks and receiver shutdown, while key/control messages wake immediately.
Bindings preserve key/modifier release order, repeats, Fn and side-specific keys;
Escape eligibility wakes the coordinator explicitly. Feed shutdown clears native
blockers before joining and cancels a producer waiting on a full queue.

The main window publishes visibility to the engine and page on hide/show,
minimize/restore and page-load completion. Hidden/minimized lecture previews pause;
native capture, final transcription and dictation remain active. A finished page
load resynchronizes visibility in case hide/show happened during navigation.

Foreground requests (single instance, Dock and tray page commands) are refused
while the core installation lease is held. Explicit update restarts leave a
private atomic one-use `update-foreground` marker, consumed only after the
single-instance check, so `--hidden` from login cannot hide the updated app.
Failed installation removes the marker and releases the opening restriction.

The main window uses MoveToActiveSpace, clearing CanJoinAllSpaces while retaining
other collection flags: opening from the tray, Dock or a second launch brings
Leo to the current desktop instead of switching back to its original desktop.
Automatic updater checks run at release startup and after an hour without an
updater command. Installation replaces the running bundle; the staged previous
bundle is removed, and unrelated application copies are never deleted.
