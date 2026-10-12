# Felix interview practice

## Experience

Interview is Felix's third chat style, with a sidebar Interview practice page. The page holds preparation plans, a persistent workspace, progress and a schedule. Buttons use the existing hidden `say` context for tool instructions and attempt IDs, keeping user messages natural. Chat stays natural: discuss an approach, ask a follow-up, request a hint or try a mock interview. Switching styles keeps the conversation.

Start with Coding, System design, Product/design, Marketing, Business/economics, Quant or Custom. A framework has a target company/role, language, coach/mock style, interview date, weekly time budget and editable skill criteria. Frameworks are independent of notes. Users can keep several and return to their workspace and chat.

A challenge includes a question, constraints/examples, time allowance, criteria and provenance. Generated questions say so. Researched questions retain source URLs and distinguish official preparation from unverified candidate reports. No claim that a question was asked without a source. Research uses the existing provider's own search or Leo's vetted web tools, never a new classifier/provider.

Coding uses a problem/editor split with Code, Approach and Whiteboard tabs; other tracks have Write and Whiteboard. Code and reasoning are stored separately. The native textarea has line numbers, Tab/Shift-Tab indentation and matching indentation on Enter. A bounded vector board supports pen strokes, boxes, arrows and text; keyboard-accessible controls offer undo and clear. Typed labels and geometry reach Felix as structured data; a drawing can also accompany a review as a raster image through the existing file-reading pipeline. Code is reviewed, never executed. Autosave is revision checked, serialized and bounded; failed saves retain the browser draft and expose retry, with no silent conflict overwrite. Async updates never replace a focused field. Only the latest generated board snapshot remains attached to the chat; frozen vector attempts and chat thumbnails retain the history, so repeated practice does not fill the file limit.

Submitting freezes the answer/board as an attempt before asking Felix to assess it. Assessment uses framework criteria, a 0–4 rubric, concrete evidence, strengths, gaps and a next exercise. Progress counts only actual submitted, assessed attempts, shows sample counts and treats early scores as provisional. Scores are AI feedback, not employer predictions or executable test results. Previous attempts and their challenge are retained when the next challenge starts.

Practice schedules are editable, dated checkboxes with durations and skill focus. They appear alongside connected calendar events in Leo; no Google Calendar writes. The model respects the stated weekly budget and interview date. Completion belongs to the user and never follows automatically from a model claim.

## Architecture and invariants

- Keep leo-web dependent on leo-core only. Reuse the existing chat loop, native/text tool executor, provider web/vision capabilities, sheets, Settings cards, segmented controls, buttons, Markdown, diagram rendering, sidebar and theme tokens.
- Store interview plans and attempts in dedicated rows in leo.db, independent of chat deletion. Row mutations occur in transactions with revisions. Enforce limits on plans, attempts, challenges, rubric criteria, schedules, text and board geometry. No AI/network operation while a store or database lock is held.
- Interview tools are offered only in Interview mode. Six shared tools configure a framework, set a challenge, read progress, read a complete frozen attempt, assess an existing attempt and save a schedule. A shared specification supplies native schemas and text manuals. Model mistakes yield corrective tool results, never raw parsing errors.
- Tool side effects are idempotent on stable challenge/attempt IDs. The model cannot submit an attempt, alter the user's draft, invent a completed session or mark a schedule complete. A new challenge cannot discard an unfinished draft.
- System rules are separate from tagged interview material. Frameworks, drafts, research and model-produced questions are data. Existing session gate, strict CSP, escaping and network/DNS limits apply.
- AI work runs in the existing blocking chat worker. The workspace remains usable and navigation does not stop a request; updates are tied to the interview ID. No AI request on page redraw or idle polling. No generated-code execution.

## Research informing the rubrics

- [Amazon: software development topics](https://amazon.jobs/content/en/how-we-hire/interview-prep/software-development-topics): coding and system design whiteboards, applied fundamentals, practice outside an IDE.
- [Microsoft: technical interviews](https://careers.microsoft.com/v2/global/en/hiring-tips/technical-interviewing): clarifying assumptions, approach, complexity, testing boundaries and design resilience.
- [Jane Street: interviewing](https://www.janestreet.com/join-jane-street/interviewing/): role-specific preparation; avoid treating all quantitative roles as one question bank.
- [McKinsey: interviewing](https://www.mckinsey.com/careers/interviewing): business cases and experience-based interview practice.

## Validation

Use scripted streamers/native conversations and temporary SQLite databases for persistence, malformed model calls, duplicate calls, missing submissions, revisions, size limits and both provider paths. Pure Node tests cover presets, progress, schedule grouping, board bounds and draft isolation. Mocked browser checks cover desktop/phone and light/dark, focus/caret, navigation while saving, chat activation, submissions and errors. Real model prompt checks use only the user's installed plan-backed agent and a throwaway home. Per user instruction, do not run a local Leo build, Cargo check/test/clippy or the full browser gate; push compiled/browser checks to CI and report the limits honestly.


Implementation checks: 85 Node tests (35 focused on chat/interview helpers) and four mocked browser combinations (desktop/phone, light/dark) pass, with Leo’s actual strict CSP. Twelve board submissions retained one generated attachment; an unreadable image still allowed written/label-based review with an explicit limit. Navigation during a save no longer activates chat on the next page. A deliberate bounds regression made its test fail before restoring the fix. Sixteen live Codex prompt checks passed across coach/mock tracks, hints, assessment and scheduling, using fake tool results in a temporary home; these are prompt checks rather than compiled server integration. Claude's weekly quota and Codex capacity stopped further repetitions. Rust, full browser integration and platform checks are delegated to CI.


## Workspace redesign (2026-10-12)

One Start practice action saves the optional company/role, language and coaching
style, then requests research and challenge creation together. Company research
uses the provider's existing search and public sources, distinguishing official
preparation, dated candidate reports and original exercises. With no company,
start general practice immediately; unavailable research must not block practice.
A blank page offers one Start practice action; the editor appears only with an
actual problem. Opening an existing plan never generates another request.

On desktop, show the problem and editor side by side; on phones, stack them.
Keep the existing Settings cards, sheets, chips, segmented controls and themes.
Coding problems include a starter function in the requested language, difficulty,
examples with expected outputs, constraints and research links. Keep Approach
separate from code. Reset code requires confirmation and keeps reasoning and the
board. Assessment is a Felix review, with no execution or invented test passes.
Progress/attempts and scheduling are folded below the main task; setup's advanced
schedule/assessment fields are folded too.

Persist the starter during the same database mutation that installs its challenge.
An untouched starter cannot be submitted and must not block a different challenge.
A retry with the same challenge never overwrites a user's implementation.
Reasoning, code and drawings all count as work and belong to the frozen attempt.
Old plans and browser drafts must remain readable without these new fields.
Pending starts wait for the existing answer, cancel on navigation, and recover to
an actionable start button if no challenge is created. No background AI polling.

Reference: [LeetCode's problem workspace](https://leetcode.com/problems/two-sum/)
separates Description, Code and examples/constraints;
[NeetCode practice](https://neetcode.io/practice) informs the topic-based practice
entry point. Leo retains its own layout and original generated exercises.

Validation for this redesign: 42 focused Node tests pass; manual mocked browser
checks pass in desktop/phone × light/dark with the actual CSP. Six live Codex
plan-backed prompt/tool checks pass across company/general practice, Python/
JavaScript and system design, Coach/Mock in both orders. Research results were
fixtures; these checks validate model decisions, not real company search results.
Compiled persistence/protocol tests and browser integration run in CI only.
