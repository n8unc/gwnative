# Phase 2 character capability

Status: exact-build JSPI automatic character entry passed native acceptance
for Cycone after both automatic and manual sign-in, and for Spiritard through
a sparse Selector model after protected sign-in. Combined saved-group
character/world inspection passed with both accounts independently entered.
Asyncify stays manual; its protected sign-in fallback passed.

## Runtime and proof boundary

The private launcher bridge is separate from the public read-only game API.
It grants no generic memory, frame lookup or dispatch route to external callers.
`src/wasm/character.rs` validates the whole module and exact credential-prefill
input SHA-256 `e2ad01cc8f3ccb82263399b72ec596f47e9a5a7439e640f7089463cb5abe07db`.
Its raw JSPI parent is
`1eb07332632e2fca8aabf5baa14fa1a1e6a2a59ec7134dfb8f6231d924c9fd7b`.

Proofs bind current function bodies, signatures, table entries and instruction
semantics. The host enables capability only after successful transformation;
unknown or malformed input retains ordinary manual play. This is a host-owned
exact-artifact transform, not a new signed companion certificate. A JSPI proof
does not establish Asyncify support.

| Module | Responsibility |
| --- | --- |
| `character.rs` | Bounded roster/name/UUID exports, transform assembly and exact input gate |
| `character_action_proof.rs` | Current frame registry, label hashing, callback and action semantic proofs |
| `character_actions.rs`, `character_selector.rs` | Closed queue and verified callback-only Select/Play actions |
| `character_world.rs` | Independent current-character UUID, map and populated player-table observation |
| `web/character-adapter.js` | One launch/session bridge, coherent roster reads, target identity, interruption and cancellation |
| `web/character-startup.js` | Bounded six-operation lifecycle with at most one entry request |

No screen coordinates, synthetic Enter or fixed-delay key sequence is used.
The exact callback at function 6661/table slot 1721 is relocated byte-for-byte,
called once, then drains the closed queue. Mutations never run from a page getter.
Cancellation disables the queue and clears its target. Callback health counters
are diagnostics, not evidence of successful selection or entry.

## Identity and lifecycle

Account configuration stores an exact preferred name, never a saved row index.
The live roster supplies a bounded UUID; the session freezes it after one unique
exact-name match. Reordered rows are revalidated against that identity.
Names are bounded UTF-16 strings; malformed names, duplicate identities, absent
or ambiguous names and changing snapshots stop progression.

The six operations are `observeReady`, `readRoster`, `selectCharacter`,
`readSelected`, `enterCharacter`, and `observeEntered`. Every result carries the
launch session ID. Runtime/build mismatch, stale responses, manual input,
cancellation, malformed memory, failed selection and bounded timeouts stop
startup and leave the client available for manual choice. A later sign-out does
not replay startup. Auto-login and preferred-character entry remain independent.

Selection confirmation must observe the current Selector callback result.
The static buffer derived from function 10129 is a stored character name and
proved stale after a live carousel change; it must not authorize Play. The implementation
binds the Selector constructor/handler and queries live selection only
inside the verified game callback. Play must independently recheck the frozen
identity against the selected row before dispatch.

Selector model rows may contain null inactive slots. After resolving one unique
target row, selection walks monotonically from the callback-reported current
index toward it, skips only null slots, validates the first non-null row, and
submits one click. Its pending index is that actual clicked UI index; the next
callback query must confirm it before another click. It never jumps across a
non-null row or relies on roster ordering to navigate the Selector model.

Stage diagnostics are internal fixed reason codes. The missing-row case adds
only the bounded Selector model count (1–64); diagnostics contain no character
name, UUID, heap pointer or credential, and do not dispatch a client action.

World entry is observed separately from Play submission. The current client
proof binds CharContext UUID and map/player fields plus the live player table.
A nonzero map alone is insufficient. Bounds checks cover the complete table
before multiplication/indexing, and a missing player row remains waiting.

## Last-seen suggestions

The launcher stores names only as Account-scoped last-seen suggestions. The
publication endpoint requires that game's publisher capability and current
launch nonce, rejects unknown fields, and writes a bounded Profile file. UUIDs,
heap pointers and credentials are not persisted. Suggestions never replace
live revalidation.

## Evidence

| Check | Result |
| --- | --- |
| JSPI protected saved sign-in | Repeatedly reaches live character selection |
| Live roster | Eight Main Alt names observed; bounded readiness and callback ticks confirmed |
| Configured selection | Live callback query confirmed Cycone after carousel movement; native entry passed |
| Missing configured name | Native `missing-or-ambiguous-target`; remained at character selection |
| Automatic Play/world entry | Native JSPI `entered` plus visible Cycone party row in loaded game world |
| Manual sign-in followed by entry | Auto-login disabled; native Log In button then Cycone entered Ran Musu Gardens |
| Sparse Selector entry | Native Main selected Spiritard across null Selector slots, logged `entered`, and showed Spiritard in Minister Cho's Estate |
| Two concurrent preferred characters | Main entered Spiritard in Minister Cho's Estate; Main Alt entered Cycone in Ran Musu Gardens, with independent profile settings |
| Asyncify | Native `unsupported-build`; saved sign-in reaches manual character selection |
| Lifecycle and malformed-state fixtures | Covered by Rust and JavaScript suites; not a substitute for native entry |

Current acceptance outcomes and test counts live in
[implementation record](launcher-phase-2-implementation.md). Supported
character-entry scope is complete; documented unsupported-build and
format-observation limits remain explicit.

## Provenance

GWoNmac reference commit `004194ff318320f854240fd227462b19889bef24`
provides GPL-3.0-only design and implementation lineage, recorded in source
headers, `REUSE.toml` and `THIRD-PARTY-NOTICES.md`. Reference offsets and synthetic
fixtures are leads, not proof for the current client. Native review already
found different frame-registry addresses, a falsely identified frame-hash
reader and a stale selected-name buffer; current proofs derive the relevant
semantics from the exact checked-in artifact instead.
