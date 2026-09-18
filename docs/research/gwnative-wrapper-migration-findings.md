# GWToolbox++ to GWNative wrapper migration findings

Date: 2026-09-18  
Repository HEAD: `199861eb030c3703be65d1eee2ef0b1572b4008d`  
Worktree: dirty before this document; launcher, character, runtime, web, and documentation changes are uncommitted. Findings below describe current source, and label provisional work separately from committed capability policy. CCE `context_search`/recall was attempted first but did not return within a useful bounded window; targeted source inspection was used instead.

## Executive finding

GWNative already has two useful halves of a migration seam:

* generated-client file/template bridges are installed before WebAssembly instantiation (`web/harness.js:557-721`, `web/template-save.js:162-253`), and
* exact-artifact transforms, signed build certificates, first-frame proof, and passive read-only snapshots are separated in Rust (`src/wasm/mod.rs`, `src/wasm/certificate.rs`, `src/game_api.rs`).

It does not currently have a certified in-game write path. `POST /__game/v1/actions` is an explicit 409 refusal (`src/server/api.rs:208-217`), and current policy says product API is read-only (`docs/game-api-capabilities.md:151-160`). Existing character selection is a separate, session-private JSPI startup bridge and does not establish a general gameplay action ABI (`docs/game-api-capabilities.md:86-95`).

The first migration slice should therefore be a new, session-private command mailbox and certified game-thread drain, modelled on GWoNmac's current command transform. `/chest`, `/xunlai`, and `/storage` are the smallest useful proof: no user payload beyond a command opcode. `/tp la ae1` and `/travel kamadan ee1` should follow only after a separate travel payload and context proof. The page parses and validates chat text; Rust/WebKit transports a bounded request; transformed Wasm queues it; the game's own frame callback drains it. `/__game` remains read-only and companion observation remains passive.

## Reference projects

GWoNmac's implementation is a capability transform, rather than a generic plugin loader. Its command builders allocate private globals for pending command, payload, enabled state, and travel toggles, then patch a certified command parser and game-thread drain (`.references/gwonmac/src/main/certification/enhancement-command-transform.ts:1-16`, `:97-159`, `:187-238`). Storage aliases are consumed only when the mailbox is enabled and empty; busy commands are consumed without falling through to the game's “Unknown command” (`:140-156`). Travel uses a distinct mailbox and validates reviewed map IDs before queueing (`.references/gwonmac/src/main/certification/enhancement-travel-command-transform.ts:8-31`, `:113-137`).

GWoNmac's outer transform checks exact input hash, function body hash, semantic fingerprints, command types, hook table slot, and command-drain boundary before emitting an enhancement manifest (`.references/gwonmac/src/main/certification/enhancement-transform.ts:146-181`, `:295-368`, `:467-520`, `:566-624`). That is the model to port conceptually; do not copy its native C++/GWCA assumptions into the browser runtime.

GWToolbox++ supplies the semantic inventory and the harder downstream contracts. Its team build code loads player templates through `GW::SkillbarMgr::LoadSkillTemplate`, adds heroes asynchronously, waits for hero presence, loads hero templates, sets disabled skills and behavior, and uses UI messages for hero panels (`.references/GWToolboxpp/GWToolboxdll/Utils/TeamBuild.cpp:148-178`). `TeamBuildEncoder.cpp` encodes/decodes player, hero, and team templates (`.references/GWToolboxpp/GWToolboxdll/Utils/TeamBuildEncoder.cpp:419-484`, `:548-708`). These are game-thread actions with stateful preconditions, not extensions of template file saving.

GWoNmac's trade feature is an external bounded service owned by main process, with renderer subscriptions, feature gates, cleanup on renderer destruction, reconnect/backoff, bounded parsing, and search/price APIs (`.references/gwonmac/src/main/trade-ipc.ts:24-138`, `.references/gwonmac/src/main/core/trade-chat-service.ts:78-150`, `:210-280`). It should be treated as a later host integration; it is not equivalent to injecting chat commands into the client.

## Current GWNative seams

### Input and command delivery

`src/commands.rs:29-60` attaches the live WKWebView and evaluates a named `gw:command` event. Current vocabulary is host-originated focus/audio/settings/guide/diagnostic control (`web/commands.js:24-78`); there is no chat text interception or game action dispatch. `web/input.js` owns browser event normalization and reset (`web/input.js:84-181`, `:253-350`), but it does not expose a chat edit/control boundary. A `/tp` implementation cannot safely synthesize key events as its primary mechanism: it needs a certified client-side/game-thread seam, with native input only as a fallback for manual testing.

Recommended first seam:

1. Add a page-side chat command parser at the client chat input boundary, preserving ordinary chat for unknown or invalid commands.
2. Gate recognized commands on an injected per-launch action capability and exact runtime/build manifest.
3. Call a small WASM export that writes one bounded mailbox (opcode plus four scalar arguments), or post to a page-owned adapter which calls that export.
4. Add a certified transform at the reviewed game-thread callback/drain boundary. Drain at most one command per callback and clear mailbox before dispatch.
5. Keep native `/__game/v1/actions` absent/refused until the action ABI is certified. If a host route is added later, bind it to launch nonce, active session, capability ID, bounded schema, and one-shot sequence; never use browser token or publisher token as action authority.

### Existing host routes and authority

`src/server/api.rs:66-79` separates browser, game reader, and game publisher tokens. `src/server/api.rs:145-175` rejects unauthorized routes and admits untrusted bodies through a guarded sink. `src/server/api.rs:208-217` deliberately reports no certified write operation. `src/game_api.rs:1-6`, `:203-212` describe and enforce read-only state/action-unavailable semantics. These are good authority boundaries to preserve while adding a distinct session-private action channel.

### WebAssembly transform and client export

`src/wasm/certificate.rs:65-147` currently certifies exact JSPI/Asyncify artifact hashes and fixed template bridges (`ensureDirectory`, `findFiles`, `fileBaseName`, `deleteFile`, `fileExists`). `src/wasm/mod.rs:1-18` explicitly describes optional tools as passive observers and separate runtime certificates. `src/wasm/rewrite.rs:1-12` is the existing generated-module rewrite path for template file operations, with marker-carrier imports and appended forwarders; it has no action mailbox or game-thread mutation bridge.

`web/harness.js:557-621` patches imports synchronously before instantiation, installs graphics/memory/template/file bridges, and `web/harness.js:649-721` obtains the instance exports and resumes generated glue. This is the concrete place to install an action adapter after instantiation, but the actual action must execute from transformed game code, not from an arbitrary page call. `web/client-runtime.js:319-361` records launch identity, falls back to the official module when a transformed module fails, and `:364-380` records first-frame proof. The fallback machinery is valuable: action capability must clear on transform failure and original-client fallback.

### Snapshot, capability, and lifecycle certification

`web/companion-snapshot.js:1-16` and its tests decode a fixed seqlock snapshot written by a passive companion. `web/enhancement-capabilities.js:1-25` validates manifest capability masks before allocation. `docs/game-api-capabilities.md:98-120` requires exact artifact identity, layout proof, ABI/private allocation, stable validated read, and runtime invariants before publication. `docs/game-api-capabilities.md:166-185` requires separate JSPI/Asyncify certification and fail-closed optional capability selection. `docs/game-api-capabilities.md:232-239` explicitly requires Asyncify unwind/rewind to produce zero companion calls and live first-frame smoke tests.

For actions, extend this model with a separate `action` capability descriptor. It needs exact parser/hook/drain function identities, body hashes and call occurrence, mailbox globals/private allocation, opcode and argument ranges, reentrancy/one-shot rules, and runtime-state constraints. Do not infer action support from `passiveEnhancements`, map/player/target snapshot support, template-save readiness, or character capability.

### Asyncify status

Current static investigation says Asyncify has bounded roster/readiness candidates but no certified character action bridge; action adapter work requires an independent frame resolver/dispatcher and closed transaction proof (`docs/launcher-phase-2-asyncify-investigation.md:17-40`, `:63-80`, `:123-125`). Current implementation records Asyncify automatic character entry as unsupported/manual (`docs/launcher-phase-2-implementation.md:16-18`, `:36-43`). Therefore first command migration can target JSPI only, publish `supported: false` for Asyncify, and add Asyncify only after independent transform, fixture, and live evidence.

### Storage and templates

Persistent IDBFS is mounted and synchronized before client main (`web/filesystem.js:1-18`, `:95-144`). Template file bridge implements bounded path normalization, directory creation, listing, basename, delete, and existence through the derived client (`web/template-save.js:1-12`, `:81-105`, `:162-253`). This supports existing build-template file save/load. It does not load a build into the player, hero, or party, and it does not open storage UI. The storage command can reuse the existing template transform's marker/certificate machinery only for transport patterns; storage action requires its own game semantic proof and action capability.

## Proposed migration slices

### Slice 0: action contract and parser-only groundwork

Define versioned action manifest and page parser. Recognize aliases case-insensitively, trim bounded whitespace, reject control characters/oversized input, and preserve invalid commands as normal chat. Unit-test `/chest`, `/xunlai`, `/storage`, `/tp`, `/travel`, invalid district, missing args, unknown map, busy mailbox, duplicate submission, and fallback. No game mutation yet.

### Slice 1: storage aliases

Certify one argument-free `OPEN_STORAGE` opcode and a JSPI command parser/drain transform. Use GWoNmac's mailbox pattern: enabled bit, pending opcode, clear-before-dispatch, busy consumes command. Add host-visible capability status only; do not expose a general `/__game/v1/actions` route. Acceptance requires transformed fixture proof, original fallback proof, first-frame proof, and live JSPI use of all three aliases opening the same storage chest. Verify repeated/busy commands and ordinary chat.

### Slice 2: travel

Add canonical map catalogue and district parser in host/page code, with aliases `/tp` and `/travel`. Resolve `la` and `kamadan` to reviewed map IDs; resolve `ae1`/`ee1` to region/language/district fields. Queue bounded payload; derive current travel context on game thread. Refuse locked/unreviewed maps, malformed region/language/district, non-outpost contexts, and stale capability. Certify JSPI first. Live acceptance must prove travel destination and district, failure behavior, relog/zone transitions, and no accidental chat send. Asyncify remains manual until its independent drain proof exists.

### Slice 3: player build templates

Separate pure template decode/encode from in-game load. Existing file bridge and GWToolbox++ encoder provide useful formats, but loading requires certified player skillbar/profession/attribute operations and a closed transaction with acknowledgement. Start with inspect/decode and player-only load; prove current profession, skill availability, eight skills, attributes, and UI state. Add live persistence/reload checks.

### Slice 4: hero templates

Reuse decoded template model, then add party/hero observation and a game-thread state machine: resolve hero, add if absent, await party presence, load skill template, apply disabled skills/behavior, and confirm. GWToolbox++'s `TeamBuild.cpp:155-178` is the semantic reference. Certification must cover every operation and cancellation; live tests must cover locked/unavailable hero, full party, hero order, disabled skills, behavior, hard mode, and persistence.

### Slice 5: party/team templates

Decode team format using `TeamBuildEncoder.cpp`, then execute bounded ordered operations with rollback/cancel and per-hero acknowledgement. Treat party templates as orchestration over player and hero action capabilities, not one opcode. Test partial failure and stale hero identity before any live claim.

### Slice 6: Kamadan trade chat

Implement as a host-owned, opt-in external feed patterned after GWoNmac's `TradeChatService` and IPC channels. Keep raw feed/network details outside the game action ABI; publish bounded parsed messages and connection state to UI. If an in-game chat overlay or filtering is later desired, certify it separately from external trade feed. Validate source allowlist, limits, reconnect, unsubscribe cleanup, and disabled feature behavior.

## Certification and validation gates

Every action slice needs static transform tests for exact hashes, semantic anchors, table slot/call occurrence, private allocation bounds, opcode/payload limits, and malformed certificates. Web tests should cover parser, adapter, capability gating, busy/duplicate behavior, fallback, and runtime lifecycle. Rust tests should cover certificate schema and action manifest invariants. Unknown or deliberately invalid certificates must launch official client with action capability disabled.

Automated fixtures prove byte-level and state-machine logic only. They do not prove injected gameplay. Live JSPI acceptance must reach first frame, invoke each command in a real session, confirm visible result, and exercise ordinary chat/failure paths. Asyncify requires its own transform and live matrix; JSPI evidence cannot be reused. Keep `/__game` read-only, companion observation passive, and benchmark-only mutation routes separate from product mode unless a reviewed architecture decision expands that boundary.

## Current gaps

* No chat input interception seam or parser in current `web/`.
* No action capability descriptor or signed action certificate fields.
* No private action mailbox allocation/export in `src/wasm` or `web/harness.js`.
* No certified game-thread command drain in current GWNative transform.
* `/__game/v1/actions` intentionally refuses all writes.
* Template file persistence exists; player/hero/party load does not.
* Character startup action bridge is private JSPI pre-game behavior and cannot be generalized without a new proof.
* Asyncify action support is unsupported/manual.
* Trade chat service/UI is absent from current GWNative; GWoNmac's external feed is a separate host feature.
* No current live gameplay evidence for storage, travel, or template loading in this checkout.

No product code was changed by this inspection; only this findings document was added.
