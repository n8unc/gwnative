# Phase 2 parallel build plan

Source of scope: [Phase 2 brief](launcher-phase-2.md). Preserve Account/Profile identity, launcher-independent sessions, Keychain ownership, existing staggered queue and deferred MVP gates. Original brief remains unchanged.

## Work contracts

Three implementation workers use GPT 5.6 Terra, medium reasoning. Coordinator owns integration, shared-file changes and acceptance record. Workers share checkout; no resets, unrelated rewrites, commits or source-pack modifications. Each worker reads domain instructions, uses CCE retrieval and reports exact changes, checks, remaining gaps and required integration snippets. Never report synthetic checks as live proof.

| Worker | Owned files | Deliverable |
| --- | --- | --- |
| A: launch settings and groups | `src/launcher_accounts.rs`, `src/launcher_sessions.rs`, new launch-preference modules, group/settings tests | Versioned groups; atomic membership cleanup; nonsecret per-account preferences; immutable per-child launch settings; shared queue admission for all entry points |
| B: character capability | New character runtime/bridge modules and tests; `docs/launcher-phase-2-character.md` | Review exact-build integration; implement bounded, session-bound preferred-character progression only behind verified capability; readiness, roster, exact selection confirmation, one enter action, independent entered observation; explicit JSPI/Asyncify evidence |
| C: texture capability/library | New texture library/importer/runtime modules and tests; `docs/launcher-phase-2-textures.md` | Check reference reuse terms; parse supported TPF safely; validate supplied sample; stable source/revision identities, discovery, immutable preparation, ordered selection resolution, upload replacement seam, compatibility matrix |
| Coordinator | `src/main.rs`, `src/paths.rs`, `src/window/state.rs`, launcher UI/bridge files, module registration/manifests and shared harness integration | Window state migration/merge lock, per-entry-point integration, controls, runtime wiring, review and acceptance |

If workers need shared files, send coordinator exact proposed changes and interface contract first. New modules may be added without waiting. Coordinator can reassign ownership explicitly after first wave.

## Sequence and interfaces

1. A first defines persisted launch preferences, group representation, resolver inputs/output and queue API. Snapshot at request time, including queued requests. Explicit invocation overrides win over Account preferences, then defaults; unset differs from false. No credential strings in snapshots/arguments. Group launch uses ordered stable Account IDs and returns started/queued, skipped and failed member results. Account removal updates groups in same atomic catalog write.
2. B and C start feasibility while A implements. They inspect real client/reference seams and produce evidence before advertising support. Lack of live proof must remain visible; do not replace proof with guessed offsets, coordinates, timed keys or mock success.
3. Coordinator integrates A contracts into Play, selected, groups and Auto-launch; builds settings and group controls. Window store schema separates `lastObservedFrame` from optional `fixedLaunchFrame`; one lock protects read/merge/write. Game observations preserve launcher preferences. Queue captures chosen layout. Fixed/restore choice, current-frame capture and fullscreen semantics use logical points and visible-display fitting.
4. After character proof, B implements fail-closed startup lifecycle and target/list transport. Manual authentication works independently of Auto-login. Missing/ambiguous target, stale state, manual interruption, cancellation, unsupported build and bounded timeout stop automation. Sign-out never replays startup. Coordinator adds Account controls and host integration.
5. After texture proof, C implements library and runtime contract. Only top-level regular `.tpf` files, case insensitive; stable-file debounce and recovery rescans off UI thread. Same source retains identity; new content creates immutable revision; invalid replacement keeps last good revision; missing source bypasses next launch. First enabled pack wins. Runtime pins exact revisions through session lifetime. Coordinator adds path resolver, background polling/watch integration, library and per-account controls, and per-child manifest transport.
6. Review each worker diff against brief; run meaningful unit/integration checks, then assign concrete fixes to workers. Re-review fixes and repeat until no actionable implementation defects remain. New code must not weaken existing safety contracts. Full phase completion additionally requires live evidence below.

## Acceptance matrix

- Groups: create/rename/edit/reorder/delete and restart; empty/missing members; overlapping/repeated launch; queued/starting/running exclusion; later members continue after failure; queued values unchanged by later edits; Account removal atomic, rename preserves references.
- Preferences/window: two Accounts with different mute/FPS/mode/layout; all launch entry points resolve identically; explicit false/default overrides; malformed/old stores usable; running edits survive shutdown; capture current layout; disappearing display and fullscreen transitions.
- Character: exact supported build and runtime recorded; auto and manual login; reordered roster, renamed/missing/ambiguous target; slow readiness, cancellation/interruption, stale session; correct independent entered observation; two Accounts isolated. Unsupported runtimes stay manual.
- Textures: synthetic malformed/truncated/oversized/path-traversal fixtures; discovery before/after start, case/rename/removal/partial copy/replacement/recovery; restart, duplicate content and conflicts; supplied Minimalus parse AND live visible replacement separately; alpha/orientation/mips; unmatched baseline; two Account selections and removal during play; memory/frame-time comparison. uMod containers stay explicitly unverified until representative sample establishes support.
- Combined: saved group starts distinct Account settings, preferred characters and texture revisions through real launcher; restart preserves configuration. Existing targeted checks and compile/format checks pass.

## Evidence and limits

Record commands and outcomes in `docs/launcher-phase-2-implementation.md`; character/texture matrices hold detailed capability evidence. Missing Accounts, representative packs or live game observations are named gaps, never passed checks. Continue independent features when capability proof is blocked. Final report distinguishes implemented/tested components from unproven live phase gates.
