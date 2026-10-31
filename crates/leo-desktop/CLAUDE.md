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
