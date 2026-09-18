# Memory cleanup fixes — 18 September 2026

Implemented against `82f7a7e`. Source changes only; no deployment or live gameplay soak performed.

## Changes

- **Audio resume listeners:** `web/audio.js` owns gesture listeners for the exact captured AudioContext. It removes them after successful resume, on closed state, and before stale-context closure. Rejected resume remains retryable. `web/harness.js` installs the replacement synchronously in `Module.instantiateWasm`, using the current classic glues' global `autoResumeAudioContext` binding. No EventTarget methods or official generated client files are patched.
- **Rejected image reads:** `web/image-read-tracking.js` supplies the host-owned registry before glue loads, with or without detailed frame auditing. It preserves original Promise identity and leaves successful deletion to the game glue. Rejected entries are removed on the next task, after the original fatal callback's microtask chain. Pending and late waits during that chain still reject. Cleanup checks Promise identity before deleting reused IDs and contains audit cleanup errors.
- **Template enumeration:** `web/template-save.js` requires matching allocation/free functions. It validates destination and record spans using refreshed heap views, hands successful allocations to the game, and frees valid allocations when enumeration fails before handoff. Malformed allocator results are rejected without being passed to `free`.

## Review loop

Implementation workers used **GPT-5.6 Luna, low reasoning**. Review workers used **GPT-5.6 Terra, high reasoning**, as requested.

| Area | Issues returned and fixed | Final Terra result |
| --- | --- | --- |
| Audio | Removed broad listener interception; added exact-context ownership, closed-state cleanup, actual generated-source VM tests, retry coverage and startup ordering check | No issues, third review |
| Image reads | Deferred rejection cleanup past fatal notification; tested actual pending/late ImageWait behavior in both glues; checked thrown fatal callback and audit errors | No issues, second review |
| Template listing | Corrected invalid-pointer cleanup ownership; documented free contract; added malformed-pointer tests | No issues, second review |

Parent review also caught a new test fixture's live diagnostics interval, which Luna corrected so the suite terminates normally. Final parent review checked combined startup wiring, resource ownership, unchanged client artifacts, and complete browser-module test results. No outstanding findings remained in this reviewed scope.

## Validation

```sh
node --test web/*.test.js
git diff --check
git diff --exit-code -- web/Gw.js web/Gw.jspi.js web/Gw.wasm web/Gw.jspi.wasm
```

Results: **263 tests passed, zero failures, zero skipped**; whitespace check passed; official generated client diff empty.

Focused coverage includes:

- Both actual generated OpenAL create functions and auto-resume global bindings in VM fixtures: successful resume, rejected resume followed by same-event retry, and external context closure. Existing host tests cover stale-context replacement.
- Both actual generated image-read and ImageWait functions: Promise identity, successful completion, rejected early/late waits, fatal-before-cleanup ordering, reused IDs and audit-error handling. Terra additionally compared a throwing fatal callback with the original Map behavior: exactly one fatal rejection in both cases.
- Template bridge success ownership, absent free function, invalid destination/record spans, missing/refreshed heaps, thrown writes and malformed allocator results.

## Limits

These changes address the concrete cleanup candidates, not a proved cause of all wrapper memory growth. Node mocks do not prove WKWebView gesture-policy behavior, native audio resource reclamation, or a stable long-session memory baseline. A controlled gameplay soak remains needed to measure impact.

The original probes in `docs/investigations/probes/` deliberately execute unadapted official glue. They still reproduce the original retention paths; the new host-integrated tests validate the fixes. Native whole-response buffering and allocator-capacity telemetry remain separate investigation topics, unchanged by this patch.
