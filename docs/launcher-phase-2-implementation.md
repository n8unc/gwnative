# Phase 2 implementation and acceptance record

Date: 2026-09-18. Scope: [confirmed brief](launcher-phase-2.md), execution: [parallel build plan](launcher-phase-2-build-plan.md).

**Supported Phase 2 scope passed final combined native acceptance.** Documented
format and observation limits remain outside that supported result.

## Delivered implementation

| Area | Delivered behavior |
| --- | --- |
| Groups and launch preferences | Catalog v2 stores ordered Account references. Groups support create, edit, reorder, delete, cleanup on Account removal, and per-member queued/started/skipped/failed feedback. Play, selected Accounts, groups and Auto-launch share one resolver. Each child receives immutable sound, FPS, mode, preferred-character and texture settings. |
| Window state | Version 2 separates observed state from fixed launch geometry. Migration, locked atomic merge, frozen queued geometry, display fitting and native request/acknowledged current-layout capture are implemented. |
| Texture discovery and sessions | Configurable source folder, debounced polling rescan, durable source identity, last-good recovery, ordered Account pack selection and per-child immutable revision manifests are implemented. Leases protect active revisions through launcher and child lifetime. |
| Texture runtime | Bounded TPF preparation plus PNG, supported DDS, mutable/immutable RGBA, partial subimage, mip and compatible DXT replacement paths are implemented. First selected pack wins; upload state and original-upload fallback are restored. |
| Character entry | Exact-build JSPI transform provides bounded roster/name/UUID reads and a closed callback-only Select/Play queue. Session IDs, frozen UUID rechecks, fresh selected-name queries, pre-Play waiting, independent world observation, cancellation, stale-result and timeout gates are enforced. Asyncify remains manual. |

## Review fixes included

Review and executable fixtures fixed launch-environment inheritance, runner-wide mute propagation, fixed-to-restore geometry, stale layout acknowledgements, group continuation after a failed member, and unsafe parallel-test environment mutation.

Texture review fixed source identity across atomic replacement, copy debounce and pending cleanup, queued-manifest pinning, launcher/child lease cleanup, bounded discovery reads, archive metadata ambiguity, upload heap lifetime, unpack state, partial-subimage cropping and Account isolation.

Character review replaced reference-derived frame assumptions with current-artifact semantic proofs, rejects stale selected-name buffers, proves fresh callback queries and current world/player state, and refuses malformed, duplicate or missing identities. Selector navigation now walks monotonically toward a uniquely resolved target, skips only null model slots, validates the first non-null row and requires its actual callback-reported index before another click. FPS limiting now admits every callback sharing an accepted native animation-frame timestamp, preventing helper callbacks from starving the game frame loop.

## Native acceptance evidence

| Check | Result |
| --- | --- |
| JSPI automatic entry | Cycone selected and entered after protected saved sign-in; party/world inspection passed in Ran Musu Gardens. |
| JSPI manual sign-in then entry | Auto-login disabled; native Log In followed by Cycone selection and entry passed. |
| Sparse Selector navigation | Main selected Spiritard through null Selector slots, logged `entered`, and showed Spiritard in Minister Cho's Estate. |
| Missing preferred name | Native refusal remained at character selection without a Play submission. |
| FPS and sound isolation | Separate children retained 60 FPS with sound and 30 FPS muted. The corrected 30 FPS limiter reached Cycone's world and kept rendering. |
| Texture replacement | Minimalus replacement passed at character selection and with Cycone visible in Ran Musu Gardens. Concurrent Accounts retained distinct original/Minimalus texture selections. |
| Asyncify | Protected saved sign-in reaches manual character selection and reports unsupported automatic entry. No Asyncify automation is claimed. |
| Combined saved group | Launcher reported both members started. Main entered Spiritard in Minister Cho's Estate with original beveled UI, sound and 60 FPS; Main Alt entered Cycone in Ran Musu Gardens with flat Minimalus UI, muted sound and 30 FPS. Both entered before Main closed; closing only the launcher did not end either child. |

## Validation

Focused Rust selector, world, lifecycle, launcher and texture regressions pass.
`cargo test -- --test-threads=1` passed 469 Rust tests with 4 ignored; its web
integration test also passed. `node --test web/*.test.js` passed 343 tests across
34 suites. The supplied Minimalus ignored parser test was run explicitly and
passed again. Formatting, JavaScript syntax checks and diff checks passed during
the final review cycle.

This record does not claim an unqualified parallel `cargo test` pass: its run
had 468 passes and one failure in the existing shell transaction-lock test,
which saw another `gwnative` PID despite native test apps already being closed.
The serial command above is the completed final Rust validation.

Synthetic browser WebGL2 fixtures validate texture bytes and isolated manifests. They do not establish game-client compatibility or native process behavior. Native observation supplies that evidence for the checks listed above.

## Known acceptance limits

- Representative uMod containers have no supplied samples; their format-specific support remains unvalidated.
- Total WebKit/GPU memory is unmeasured. Limited host/frame samples are not a total-memory result.
- Physical display removal has automated geometry coverage only; it has not had native acceptance.
- Live compressed texture upload paths remain unobserved.

## Test cleanup

Original group settings were restored. Temporary pack selection, group and
folder override were removed, and no test launcher or game process remained.

See [character evidence](launcher-phase-2-character.md) and [texture matrix](launcher-phase-2-textures.md) for capability-specific limits and provenance.
