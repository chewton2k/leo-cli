# Codebase analysis: leo's friction surfaces

Phase A only: a findings report, no code changed. Scope is the code that decides
what a new user meets: onboarding, writing-AI readiness, recording to note,
dictation, and how errors reach the page. Each finding names what it blocks in
the action list ([README.md](README.md)).

## System overview

- **Onboarding**: `crates/leo-web/src/web/app/onboarding.js`. Two pages (Access,
  Speech model) over three stored flags, `Preferences.onboarding: [bool; 3]`
  (`crates/leo-desktop/src/app_settings.rs:19`).
- **Writing readiness is computed in five places**:
  1. `ai::writing_ready()` (`crates/leo-services/src/ai/mod.rs:261`), used by dictation;
  2. `health::provider_usable` / `chain_check` (`crates/leo-services/src/health.rs:208-288`), used by the doctor scan and Settings' `ready`;
  3. `web_settings::describe` notes and sign-in text (`crates/leo-services/src/web_settings.rs:104-131`);
  4. `crates/leo-host/src/health.rs:14-18`, which probes sign-in for the first provider only;
  5. `taskCard` in `crates/leo-web/src/web/app/settings.js:193-197`, which merges `ready` with `plans[].signed_in` in the browser.
- **Recording**: `routes/record.rs` start, `crates/leo-host/src/record.rs` listen, `session::recorder::write_up`, `ai/long.rs`. A writing failure saves the transcript and emits warnings.
- **Every `Powers` field is `Some` in the real app** (`crates/leo-host/src/powers.rs`); the `None` branches serve the fake-powers tests.

## Safety net

- Rust: `health.rs` tests, `leo-desktop/src/app_settings.rs` (onboarding flags), `tray.rs`, `web_settings.rs`, `config/choice.rs`, `leo-web/src/tests/{recording,dictation,felix,margin}.rs`.
- Node: `crates/leo-web/tests/{onboarding,app-settings,settings-lifecycle,settings-navigation,dictation,recording,native-recording,printing,margin-lifecycle}.test.js`.
- Browser: `tests/browser/{settings,dictation,flows}.spec.js`.
- **Gaps**: nothing checks that the five readiness answers agree, or the wording of Health's fixes. Several strings are pinned by `margin-lifecycle.test.js` and `dictation.test.js`, so message changes need test updates.

## Findings (ranked by leverage)

### 1. "Is writing ready?" gets different answers  [Structural]
- **What**: the default chain is `["ollama"]` (`config/mod.rs:25`). `writing_ready()` asks each provider's `available()`, which for a keyless provider is `!self.needs_key || self.key.is_some()` (`ai/provider/openai.rs:528`): always true, port never checked. `provider_usable` does check the port (`health.rs:284`) but not whether a model exists. So on a fresh install dictation's "No writing AI is set up yet" never fires, Settings says Not set up, and an Ollama with no model is Ready.
- **Why it matters here**: N1 (onboarding step), N3 (pre-flight), Q3 (one message) and Q4 all need one trustworthy answer.
- **Cost if unchanged**: high. Every friction fix touches 4–5 sites, which will drift.
- **Proposed fix**: `writing_ready()` delegates to `health::chain_check(.., Chain::Chat, ..)` (and uses `store.has`, never a read, per the keychain rule). For Ollama, "ready" also needs at least one model, which `describe` already knows (`local.ollama`). Expose one `writing` status object from `describe` that Settings, Health, dictation and the tray read (see system-design.md).
- **Effort**: S–M. **Risk**: low. The dictation warning will start showing for Ollama-default users without Ollama running, which is correct.

### 2. Health's fixes are stale and terminal-first  [Maintainability]
- **What**: the speech fix says "leo update (downloads the speech model) … or: Settings in leo, choose OpenAI, Gemini or xAI, add its key" (`health.rs:241-244`); installers no longer download models and cloud speech is gone. The writing fix leads with `brew install ollama && ollama pull qwen3:8b` on every platform (`:236`). Doctor checks reach the page without a `section`, so no "Open Writing AI" button is drawn (`app-settings.js:76`). Speech appears as three separate rows.
- **Why it matters here**: onboarding's skip text sends people to Health. Q1.
- **Proposed fix**: rewrite both strings for the app and per platform; add a `section` when the host maps checks; drop the duplicate speech-chain check (the speech-model check covers it).
- **Effort**: S. **Risk**: low.

### 3. The "no writing AI" error exists in about fourteen forms  [Quality]
- **What**: `polish.rs:10`, `ai/chain.rs:96`, `combine.rs:105`, `felix.rs:451`, `routes/margin.rs:450,657`, `margin.rs:1118`, `margin-notes.js:1152,1169,1190,1193`, `tray.rs:271`, `error.rs:120`, `ai/long.rs:160`. Only `dictation.js:19` links to `#/settings/writing`. Felix can pass the chain's raw "every chat provider failed" through.
- **Proposed fix**: one constant next to `NOT_SET_UP` plus a stable `why: "no-ai"` code (margin already uses it); the page maps the code to one message with a link.
- **Effort**: M. **Risk**: low (pinned strings in two node tests).

### 4. Recording has no pre-flight  [System]
- **What**: `health::recording()` (`health.rs:168`) exists but nothing calls it outside tests; the record route checks neither speech model nor writing AI. Problems surface after Stop (`recorder.rs:356`, `long.rs:160`).
- **Proposed fix**: expose the recording checks plus finding 1's writing status through the Record overview; show warnings beside Start. Never block.
- **Effort**: S–M. **Risk**: low.

### 5. Onboarding flags can't grow safely  [Structural]
- **What**: `Preferences` is `#[serde(default, deny_unknown_fields)]` with `onboarding: [bool; 3]` (`app_settings.rs:12-19`). Growing the array breaks parsing of existing files; adding a field breaks a downgrade (unknown field), and the load fallback quietly turns automatic updates off. Index 1 is never shown on its own (step 0 finishes 0 and 1), and pages are `step === 0` / `step === 2` branches.
- **Why it matters here**: blocks N1 and a dictation practice step.
- **Proposed fix**: add `#[serde(default)] writing_done: bool` instead of growing the array; drop `deny_unknown_fields` on `Preferences` (or tolerate unknown fields on load) so an older build can read a newer file; let `ONBOARDING_STEPS` drive which page draws.
- **Effort**: M. **Risk**: medium (preference migration; test both directions).

### 6. The tray matches a display string  [Quality]
- **What**: `tray.rs:270` compares against `"No writing AI chosen"`, produced by `web_settings.rs:731`.
- **Proposed fix**: pass the selection (or a bool) instead of the string. Falls out of finding 1.
- **Effort**: S. **Risk**: low.

### 7. Calendar prompts only exist inside the page  [Structural]
- **What**: the "event happening now" match (`recording.js:81`, a 15-minute window) runs only when the Record page loads. There is no notification plugin in `leo-desktop/Cargo.toml` and no engine-side event timer.
- **Why it matters here**: blocks L1.
- **Proposed fix**: move the match into Rust on the existing calendar refresh; add the notification plugin in desktop. See system-design.md.
- **Effort**: M–L. **Risk**: medium.

### 8. Share only means PDF  [Quality]
- **What**: `data-action="share"` is labelled "Export as PDF" (`core.js:788`) and calls print (`sheets.js:559`). No copy-Markdown or .md download. The desktop and page error texts for a failed print disagree (`leo-desktop/src/lib.rs:232` vs `sheets.js:575`).
- **Proposed fix**: a `popupMenu` with Export as PDF, Copy as Markdown (the clipboard plugin is already a dependency), Save .md (reuse the downloads handler). Q8.
- **Effort**: S–M. **Risk**: low.

### 9. The write-up result drops its warnings on the way to the note  [System]
- **What**: `accept()` in `recording.js:199-212` navigates to the note and toasts success on `state === 'done'` regardless of `view.warnings`. The fallback note carries no marker that it was not written up.
- **Proposed fix**: when `done` arrives with warnings, toast "Saved the transcript. The notes were not written." with an action to Writing AI; record the session as not written up in leo.db so the note can show a notice and offer "Write the notes" (which exists today behind Show transcript, `app/sources.js:59`). Q2.
- **Effort**: S–M. **Risk**: low.

## Deliberately not flagged

- The two `beforeprint` listeners (`core.js:944`, `events.js:323`): different jobs.
- The legacy built-in cloud providers in `built_in_toml` and `error.rs:116`: they keep old configs loading.
- The `None` branches for writer and toolless in routes: used by tests; only their wording needs sharing (finding 3).
- A chat-first home (N5): `#/` is a one-line empty state and `chat.askNew` and `quickLeo` already exist, so this is a product decision, not debt. The only conflict is the `leo-last-place` restore (`events.js:86`).
- A dictation practice step: feasible as is, with a text box in the sheet and the global shortcut.

## Recommended order of work

1. Finding 1, then 2 and 6. Cheap and unblock most of the action list.
2. Finding 3, then 9.
3. Finding 4.
4. Finding 5 before any new onboarding step.
5. Finding 8, then 7.
