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
   and `.side-head` are `data-tauri-drag-region="deep"`.

## Behaviour

- Close hides the window and queues a backup; Dock click and "Open leo" show
  it; "Quit leo" (menu bar icon) or ⌘Q exits, and `RunEvent::Exit` backs up and
  stops the engine (6 s cap). Tauri's default menu supplies Edit (⌘C/⌘V/⌘Z/⌘A)
  and binds none of the page's shortcuts.
- `links::stays_inside`: only `http://127.0.0.1|localhost:<port>` and the
  bundled page stay in the window; navigation and new windows elsewhere open in
  the default browser.
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
generation. It opens the selected microphone only for capture, choosing the
lid-closed alternative through a bounded IORegistry query. Capture releases
its input before local VAD/transcription, word rules, local history and paste.
Start/stop WAVs are optional and excluded from lecture recordings by AudioGate.
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
copy wins, Enter requires a confirmed receipt and an uncanceled generation.
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
Shutdown cancels dictation/shortcuts and stops the speech worker before the
loopback engine finishes.

## Settings and updates

`app_settings.rs` serializes atomic private app preferences through a bounded
actor; native login/tray failures restore the previous file and cached state.
First-launch microphone, Accessibility and model steps persist independently.
Permission status checks never open the microphone; requests are explicit.
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
No Linux release/test rows remain. Chromium and targeted WebKit browser jobs run
on macOS. Main installer cutover happens only after published assets validate.
