# Memory retention audit — 18 September 2026

Source baseline: `82f7a7e`. Investigation only; no production behavior changed.

Follow-up: [implemented cleanup fixes and review results](2026-09-18-memory-fixes.md). This document preserves the pre-fix audit findings; its direct-glue probes bypass the new host adapters.

Two retention paths reproduced using actual generated JavaScript functions in isolated Node probes. Neither establishes the cause or size of long-session gameplay memory growth. No GWNative process was running during this audit. No official mobile build or other wrapper was measured, so the reported wrapper/mobile difference remains an observation supplied by the user, not independently verified here.

## Findings

| Priority | Finding | Evidence and limits |
| --- | --- | --- |
| First fix candidate | Destroyed audio contexts can remain reachable through un-fired DOM listeners | Both generated glues add six once-only resume listeners per AudioContext. Destruction does not remove them. After 100 simulated context replacements and keyboard/mouse events, 200 touch listeners remain per glue. Actual-source probe confirms listener accumulation; retained native audio bytes are unmeasured. |
| Secondary cleanup | Failed asynchronous image reads remain in `Module.imageReads` | Both generated glues delete entries only on success. 100 successful reads retain zero entries; 100 rejected reads retain 100. Fatal-error callback is stubbed in probe, so this does not establish repeated failures during real gameplay. |
| Measurement gap | Existing heap gauge reports capacity, not live allocations | `web/harness.js:588-590` supplies `Module.HEAPU8.byteLength` to `web/memory.js`. Native host footprint excludes the game’s WebKit process. Neither proves a leak. |
| Burst pressure | Native transport buffers whole responses before caller size checks | `src/transport.rs:215-269` receives NSData then copies to a Rust vector; `src/proxy.rs:273-280` checks size afterward. An upstream body above the proxy’s 8 MiB limit can still be fully buffered before rejection. Potential transient duplication, not demonstrated permanent retention. |

### Audio context listeners

Sources: `web/Gw.js:7776-7784,7924-7938`; `web/Gw.jspi.js:7745-7753,7893-7907`.

`autoResumeAudioContext` installs keydown, mousedown and touchstart listeners on both document and canvas. Every callback captures `ctx`. `{once:true}` removes only the listener whose event fires. Ordinary desktop keyboard and mouse use leaves touch listeners installed. `_alcDestroyContext` clears the feeding interval and deletes OpenAL bookkeeping but does not unregister these callbacks.

The host already closes stale native contexts after two seconds and removes its own retained entries (`web/audio.js:111-126`). Closing a context does not remove these external DOM references. This is a JavaScript reachability defect; do not infer that a closed context keeps all its former native audio buffers or render resources alive. Growth depends on context recreation, not elapsed play time alone.

Proposed repair: lifecycle-owned resume listeners, all removed together after successful resume or context disposal. Keep a current live context resumable. Use the existing certified adaptation process if changing generated glue; do not casually edit shipped client bytes. A host-side replacement must retain the same audio semantics and avoid broad event-listener interception.

### Rejected image reads

Sources: `web/Gw.js:11786`; `web/Gw.jspi.js:11560`; host failure handling `web/harness.js:743-745`.

The rejection handler calls the fatal-read export without deleting the tracked Promise. Its rejection reason remains reachable with the Promise. Successful paths clean up normally. The probe holds the fatal callback harmless so repeated requests can run: actual fatal-path client behavior may stop further work, bounding practical exposure to one incident. No large successful response payload is retained by this finding.

Proposed repair: delete the entry on failure as well as success, preserving the original fatal callback and ImageWait semantics. Validate via the client certification/adaptation path.

## Controls and exclusions

- Snapshot warming already uses payload-free `__warm` requests (`web/image.js:65-91`), avoiding the older repeated 256 KiB JavaScript allocation path. Demand reads still temporarily allocate a response buffer before copying into WASM; that alone is not a leak.
- Chunk file handles have eviction (`src/chunks/files.rs:37-58`). Native connection/thread and socket counts have caps. Chunk bookkeeping can grow with distinct chunks but is bounded by content.
- Native application/menu/delegate owners retain the WebView for process lifetime. One-time deliberate lifetime ownership is not evidence of per-frame or per-map accumulation.
- Host presentation resources have explicit cleanup: `web/presentation-barrier.js:178-205` deletes framebuffer, texture and renderbuffer; initialization disposes the previous set first (`:311-318`). Resizing reuses the objects. No monotonic host-renderer retention was established.
- Companion allocations are caller-owned and released on teardown while the runtime is idle (`web/enhancements.js:232-251,404-410`). During Asyncify unwind/rewind, teardown deliberately avoids calling into WASM; page destruction releases the instance. This is not evidence of per-frame accumulation.
- Template enumeration has a defensive cleanup gap (`web/template-save.js:220-239`): after malloc, a missing refreshed heap or an exception before publishing the pointer has no free path. A missing heap after valid synchronous malloc was not established in the real client; treat this as a failure-path robustness issue, not a demonstrated normal-gameplay leak.
- The current generated client uses 20% heap overgrowth capped at 96 MiB extra (`web/Gw.jspi.js:9458-9513`). Equal-sized growth steps cannot establish reference retention; the contrary comment in `web/memory.js:64-66` is misleading.
- A missing JavaScript `_free` after socket delivery is not sufficient evidence of a leak: ownership crosses into the compiled game client and may be released there.

## Reproducible checks

Run from repository root:

```sh
node docs/investigations/probes/audio-listener-retention.mjs
node docs/investigations/probes/image-read-retention.cjs
node --test web/audio.test.js web/memory.test.js
```

Both probes passed their assertions of the current defective behavior. They are investigation fixtures, not green regression tests proving cleanup. Audio probe executes extracted resume and destroy functions with fake DOM/OpenAL objects; it does not instantiate a real AudioContext. Image probe executes the extracted image-read callback with controlled resolved/rejected Promises and a stub fatal callback. Existing audio/memory tests: 16 passed.

Audio checkpoints per glue: 600 listeners after 100 registrations/destructions; 400 after keydown; 200 after mousedown; zero after touchstart. Image checkpoints per glue: zero tracked entries after 100 successful reads; 100 after 100 rejected reads.

## Next live experiment

Use the same client generation, profile, map sequence and graphics settings across comparisons. Record native host, attributed WebContent, GPU and networking physical footprint separately, plus WASM capacity. The existing benchmark documentation explains process attribution; its login-screen measurements cannot substitute for a gameplay soak.

1. Warm up, repeat the same map/return-to-outpost cycle, then idle. Compare settled baselines, not only peaks. A rising settled baseline is the signal to trace further; a plateau is consistent with bounded caching or allocator reuse.
2. Repeat with tools/enhancements disabled. Growth disappearing would implicate optional modules; growth persisting would narrow scope to game client, shared glue or rendering/runtime.
3. Hold map/scene steady and repeatedly change the game audio context/output setting. Count created/closed contexts and remaining resume listeners. This directly tests the reproduced audio defect’s practical impact.
4. Repeat the controlled scene in supported JSPI and Asyncify modes. Record generation and hashes. Divergence would narrow runtime-specific suspension/stack or engine behavior.
5. If WASM capacity rises without corresponding browser/native object accumulation, obtain allocator live-byte/free-space evidence from a supported client diagnostic or appropriately instrumented build. Wrapping only exported `_malloc`/`_free` is incomplete: internal WASM calls can bypass JavaScript wrappers.

Do not patch unknown game allocator behavior or introduce periodic reloads based solely on RSS/capacity growth. The official mobile exception needs comparable engine, client build and workload evidence before assigning its cause.

## Primary references

- [WebAssembly memory-control proposal](https://github.com/WebAssembly/memory-control/blob/main/proposals/memory-control/Overview.md): current linear memory growth and lack of shrink/release API; capacity remaining high is not live-allocation evidence.
- [Emscripten debugging](https://emscripten.org/docs/porting/Debugging.html): browser memory tools generally see JavaScript allocations, with separate tooling required for C/C++ heap analysis.
- [Emscripten compiler settings](https://emscripten.org/docs/tools_reference/settings_reference.html): geometric/linear overgrowth policy.
