# Security and cleanup review — 2026-10-16

Three parallel reviewers examined server/authentication/files, AI subprocesses and
network access, and browser execution/recording. The coordinating agent reviewed
and combined findings, added task/network/import protections and regression tests.
This is a source review with isolated reproductions, not a penetration-test or a
guarantee that every vulnerability or leak has been eliminated.

## Confirmed findings and changes

| Priority | Finding and prerequisite | Change |
| --- | --- | --- |
| High | Native Codex app-server could start user-configured MCP servers, despite Leo's intended tool restrictions. Requires configured local MCP; this was reproduced with a harmless fake server. Usage polling had the same startup path. | Require explicit support for ignoring user configuration and rules. Otherwise use isolated text tools and skip live usage polling. Bounded feature probes fail closed. |
| Medium | Subprocess deadlines could be bypassed by steady output, early stdout EOF followed by a hanging child/pipe, blocked stdin, or a hanging feature probe. | Shared absolute/quiet watchdogs cover the complete process lifetime; kill process groups, cap output/line sizes and native queues. |
| Medium | Existing sign-in files could remain readable to other OS accounts; predictable credential temporary paths followed planted symlinks. Concurrent credential updates could overwrite one another. Exploitation requires filesystem access/misconfiguration. | Atomic private temporary files, permission repair, nofollow bounded regular-file reads, hardlink rejection and a bounded credential writer. Session write failures refuse authentication. |
| Medium | Attachment and task paths could follow local symlinks outside their intended roots. Existing unreadable task files could cause migration to delete legacy task rows. | Component containment, safe leaf opens, unique atomic writes, and exact-row migration only after verifying the saved task. |
| Medium | Public fetches inherited proxies that bypassed destination DNS checks; HTTPS fetches could downgrade during redirects; IPv6 site-local addresses were accepted. Slow OS DNS and search responses lacked effective resource bounds. Requires a fetch path or configured proxy. | Disable proxies for public fetches, enforce HTTPS across secure redirects, reject private/site-local addresses, cap bodies/addresses and isolate DNS behind a deadline/four-slot bound. |
| Medium | Failed runner startup retained listeners; overlapping runs could mix results and timers. Python could forge completion and keep consuming CPU in a reused worker. Malformed worker messages could throw during rendering. | Cleanup on all startup outcomes, four-job serialized queue, iframe active guard, fresh terminated Python workers, bounded result validation. |
| Low | Computer recording levels accumulated when no waveform was mounted. | Bound the background waveform queue independently of rendering. |
| Medium | Claude's loopback MCP bridge admitted unlimited connections and left idle sockets/tool waits alive after disposal. Requires local access. | Cap connections at eight, close sockets on drop, cancel pending tool waits, and bound writes, batches and per-connection requests. |
| Medium | Repeated imports accumulated workers/job summaries and duplicated decoded originals in RAM; ZIP traversal and concurrent builds had no common resource budget. Requires authenticated requests. | Bound request readers, detached checks, jobs, file counts/bytes, retained summaries, ZIP traversal/input and concurrent builds/streams; stage originals privately without cloning their payload. |

## Security boundaries reviewed

- Application/API routes remain session-gated; public runner/runtime assets are
  deliberate exceptions and contain no notes or credentials.
- Loopback trust comes from the TCP peer. Forwarded headers cannot turn a remote
  peer into a trusted local connection.
- Session cookies are HttpOnly/SameSite=Lax. The runner has an opaque sandbox
  origin and constrained CSP. Fixture requests carried no session cookies;
  cross-port requests were blocked. No browser sandbox escape or remote
  authentication bypass was found in this review.
- Agent commands are confined to the existing isolated writing paths/tool lists.
  No real microphone, keychain, DNS resolution or paid AI API was used in tests.

## Evidence and validation

- 56 targeted Node tests passed: 37 interview/editor/runner and 19 recording tests.
- One isolated installed-Chrome lifecycle regression passed. It covers forged
  Python completion, correct queued-run tickets, malformed output recovery and
  repeated startup cleanup. It uses local fake runtime assets, not real Pyodide.
- Five browser lifecycle regressions failed against the old source. Removing the
  schema validation also made the new Node and Chrome checks fail.
- Four std-only process-helper regressions and ten exact credential-module checks
  passed in isolated rustc harnesses. Extending the watchdog deadline made the
  early-EOF test fail; the credential regression failed against the old source.
- The exact import admission helper passed its isolated rustc regression; removing
  the active-job cap made it fail.
- Installed-CLI capability checks used a throwaway configuration, fake MCP server
  and loopback-only failing provider. Unsupported native startup was refused.
- Five exact MCP-module fixture tests passed, including shutdown of eight idle
  sockets, refusal of excess sockets and pending tool cancellation. The idle-drop
  regression failed against the old source.
- Eight disposable filesystem policy checks passed for atomic replacement,
  permissions, nofollow/hardlink handling and attachment path containment. Export
  policy fixtures reproduced four old cases, refused four guarded cases and passed
  a valid control. These policy fixtures do not execute the Rust handlers.
- Rust regression tests were added for authentication/files, provider isolation,
  subprocess bounds, network boundaries, task containment/migration and import/
  export admission. Their compiled results are pending CI.
- Targeted rustfmt, JavaScript checks and whitespace checks run locally. No local
  Cargo build/check/test or full browser suite: the repository's website policy
  delegates those checks to CI.

## Remaining limits and follow-up

1. Browser workers can be terminated, but browsers provide no per-worker hard RAM
   quota. A user program can allocate aggressively before a timeout. Returning
   enormous structured-clone data can also allocate before parent validation.
2. OS DNS calls cannot be forcibly cancelled. At most four may remain outstanding;
   further requests time out waiting for a slot instead of spawning more threads.
3. Sessions still perform small synchronous atomic saves under their session mutex.
   A bounded single-writer actor with async acknowledgments is the appropriate
   follow-up; a rushed persistence rewrite would risk revocation/write ordering.
4. Credential serialization prevents in-process lost updates. Separate Leo
   processes editing credentials simultaneously still need cross-process locking.
5. Filesystem component checks plus nofollow leaf opens do not constitute a full
   handle-relative defense against an attacker concurrently replacing parent
   directories. Leo does not isolate itself from malicious software running as
   the same OS user. Unix owner-only modes also are not a Windows ACL guarantee.
6. Python now initializes a fresh worker per run; downloaded runtime files remain
   cached. Real Pyodide startup latency was not measured in this review. Codex text
   fallback may increase multi-tool latency and reduces native cache reuse.
7. This pass did not run a dependency-advisory scanner or sustained whole-app heap
   profiling. Cache/model memory is not automatically a leak. A machine snapshot
   showed Spotlight (`corespotlightd`) as the dominant CPU user and no active Leo
   process; that does not explain every earlier slowdown.

CI status: pending the security patch push. Do not treat the uncompiled Rust
changes as a validated release until that run is green.
