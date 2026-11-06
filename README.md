# leo

Leo is a desktop note app for macOS and Windows, with local recording and
dictation and Felix to help you study. Your notes stay as
Markdown files on your computer. Writing AI uses your signed-in Claude Code or
Codex plan, or a local Ollama model.

## Install

Download Leo from [Releases](https://github.com/chewton2k/leo-cli/releases).
Choose a macOS disk image (`.dmg`) for Apple Silicon or Intel, or the Windows x64
installer (`-setup.exe`). Linux is not supported by the desktop app.

During the desktop transition, check that the release includes those app assets.
Older releases contain the terminal version; until the first desktop release is
published, the install commands below still provide that version.

On macOS, drag Leo into Applications. For browser downloads on macOS 15, open
System Settings → Privacy & Security and choose **Open Anyway** after the first
attempt to open Leo. The app uses a stable self-signed certificate; it is not
notarized. Windows browser downloads may show a SmartScreen prompt.

After the first desktop release, these commands install the app too:

```sh
curl -fsSL https://github.com/chewton2k/leo-cli/releases/latest/download/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://github.com/chewton2k/leo-cli/releases/latest/download/install.ps1 | iex
```

The existing main-branch installers switch over only after every desktop app
asset is published. Before that switch, they keep installing the terminal version.

The macOS installer puts Leo in `/Applications`; Windows uses
`%LOCALAPPDATA%\Leo`. The terminal command points into the app, so app updates
update it too. `LEO_INSTALL_DIR` chooses the terminal command's folder;
`LEO_INSTALL_SKIP_PATH=1` leaves your shell or user PATH unchanged.

Open Leo like any other app, or type `leo` in a terminal. First launch offers
three skippable steps: microphone, Accessibility, and a local speech model.
Dictation defaults to Option+Space on macOS and Ctrl+Space on Windows; rebind it
in Settings. Models and speech stay on this computer. Choose Writing AI when
you need it; dictation never uses writing AI.

Settings → Health has setup checks and fixes. Updates & about checks for app
updates, shows what changed, and installs when you quit by default. Updates wait
for recording and dictation to finish. `leo update` uses the same app download;
quit Leo before using the terminal installer. `leo uninstall` removes the app,
its login item and its local settings/models while keeping your notes.

## Moving from the terminal version

After the desktop release is published, macOS users with a standard installation
can quit their old Leo session, run `leo update`, then open Leo in Applications.
The installer replaces the terminal command with the app launcher, so typing
`leo` opens the app. The terminal interface and browser-server workflow are
replaced by the desktop window.

Your notes stay in the same folder. Leo reads the existing database, writing
settings and backup repository; there is no export/import step. Keep the same
operating-system account and data location when switching.

On first launch, allow microphone access for recording and Accessibility access
for pasting dictation into other apps. Download a speech model in Settings →
Speech models. The previous speech model cannot be reused; Leo removes its old
folder after a new model is verified and ready. Previous cloud speech choices
fall back to local speech. Stored keys, writing AI sign-ins and calendar
credentials stay in leo's credentials file.

Two migration cases are not automatic yet:

- **Windows:** the old `leo update` command keeps its executable running while
  the installer tries to replace it. Use the downloaded Windows installer after
  quitting old Leo, then open the app from Start. An existing terminal command
  may still point to the old version; automatic command migration is not ready.
- **Custom `LEO_HOME`:** a value set only in your shell is not remembered by an
  app opened from Finder or Start. That launch may show a different notes folder.
  Keep your existing setup until custom-location migration is supported.

Linux users can keep their existing terminal installation and notes; the desktop
app supports macOS and Windows only.

## What you can do

**Organize your notes.** Browse folders in a grid or list, search your notes, and
keep tasks close by.

![Leo's note library with folder cards, note previews, and a tasks panel](public/notes-library.png)

**Study with Felix.** Explore your notes with questions, explanations and feedback.


**Dictate into other apps.** Hold or tap your global shortcut, speak, and Leo
pastes your words back. Custom words, filler removal, the last 20 dictations and
popup appearance are in Settings. Speech stays local, with no AI cleanup.

**Choose your speech model.** Dictation and recording share one local model.
Settings → Speech models shows downloaded models, language and streaming support,
size, accuracy and speed, with download progress and cancellation.

[Contributing](CONTRIBUTING.md) · [Developer guide](DEVELOPER.md)

## License

MIT. Speech models have their own licenses, listed in the vendored
[model catalog](crates/leo-services/src/speech/catalog.json).
See [NOTICE](NOTICE) for third-party attribution.
