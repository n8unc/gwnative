# Asyncify pre-game character investigation

Date: 2026-09-18. Status: static exact-artifact evidence; no transform, action, or live character claim.

## Artifact and method

Inspected current `web/Gw.wasm`, SHA-256:

```text
373c65abcfe6c26161ffd59cb8ea298eb3d1814f4778aeaf24d8978c1fdfa8ef
```

Used `wasm2wat --generate-names` plus a temporary section reader to enumerate imported/defined functions, raw code-body SHA-256 values and signatures. Values below were derived from Asyncify function instruction flow, then compared with source-level GWoNmac pre-game semantics. They were not copied from JSPI addresses.

## Read-only roster evidence

Asyncify has exactly one function matching bounded roster-copy structure: it loads a count, copies one 132-byte record from a pointer plus `index * 132`, and rejects out-of-range indexes. A second function matches selected-name search structure: it tests the count, scans those 132-byte records at offset 24 and copies a selected record.

| role | Asyncify function | signature | SHA-256 | derived static value |
| --- | --- | --- | --- |
| roster record reader | `10128` | `(i32, i32) -> i32` | `474c21373217e7de11fa262b70fe4a9e6713a8ec874a48fc96a8c4124f22b5d1` | count `0x5a75f0`, pointer `0x5a75e8`, stride `0x84` |
| selected record reader | `10129` | `(i32, i32) -> ()` | `bca1d0e9972117bf942f62e26f85f09eadba98c54b4b0246e1bf5dab668694fd` | same count/pointer/stride; selected-name location `0x5a7760` |

Both bodies include Asyncify state guards around otherwise matching read-only logic. These proofs can support an Asyncify-only bounded roster/readiness transform after new certificate schema and fixture tests. Readiness must bound count to `1..=64`, reject null/out-of-memory pointer plus `count * 132`, and expose no names or heap pointer to page code.

No independent entered observation follows from these readers. Pre-game controls can distinguish character-select/reconnect/loading only after separate label/frame proof; loading or a missing selector is not proof that requested character entered world.

## Control-path candidates: not authority

Asyncify functions at same source positions as GWoNmac's certified JSPI frame/action participants have distinct transformed bodies. Structural equivalence and signature alone do not certify use:

| candidate role | Asyncify function | signature | SHA-256 |
| --- | --- | --- | --- |
| frame child | `6796` | `(i32, i32) -> i32` | `0717c04e72539454ad7e59adf3c1568a02ebbca6454b9d516aa48c85a16b6568` |
| frame parent | `6797` | `(i32) -> i32` | `37d1ca3063284b481ba0bf821424857619a9ff6f75e813fafa3ab2c6acee8735` |
| frame message | `6841` | `(i32, i32, i32, i32) -> ()` | `23e2fdef7b0bd11c913af82ccdc0f5e9ae2a1d8967b31ac75b13333fec53602e` |
| frame hash reader | `9811` | `() -> i32` | `bbe651c66c774ac016200f50f803d1e162ea6e6643e5ea20e084840b4149d554` |
| logout producer | `12434` | `(i32, i32, i32) -> ()` | `664a93028581eb6cbdb9777c9fdd816a82b3272e99a9025f84cd24d9d4bdf738` |

Required before any action adapter: derive Asyncify frame resolver/dispatcher and label data from this artifact, prove all call edges/operands/body hashes, design a closed select/confirm/Play transaction, and independently observe matching entered character. Reuse no JSPI certificate or blind Enter path. Until then publish `supported: false` for Asyncify.

## Dispatcher and recurring callback

`6841` proves only a guarded frame-message forwarding edge. Its successful
receiver/message-bound path resolves through `6534`, adds `168`, then calls
`6508` with all four message arguments. `6508` walks a 12-byte callback row backward and
performs a four-argument `call_indirect`; it is dispatcher machinery, not an
exportable generic page-to-game control surface.

| role | Asyncify function | signature | SHA-256 | static evidence |
| --- | --- | --- | --- | --- |
| frame callback dispatcher | `6508` | `(i32, i32, i32, i32) -> ()` | `bf9a34390620eafbfcb249843194f78dd79e632145822768e5ac1f03885c513a` | reverse 12-byte callback-row scan, indirect four-argument call |
| frame receiver resolver | `6534` | `(i32) -> i32` | `12c4a65202993868ee5b360fe734b926c6d0795d0e7e76c359fcc2b9d99751c4` | immediate caller of `6841` before dispatcher offset `168` |

This supplies one required edge for a later select/Play certificate. It does
not identify selector or Play frame hashes, callback payload type, target row,
or valid message sequence. Those must be derived from this artifact, frozen in
certificate, then exercised against fixture and live proof before control code
exists.

JSPI callback `6661`, current raw-body SHA-256
`4168ff3e2a37bb36a94d1028f8abf0d0cc199115974646ae23aa253655650eea`, is
also not enough to host actions. Its `(i32, i32) -> ()` function sits at active
table slot `1721` (element base `1`); `6659` registers `(event=2, slot=1721,
context=0)` through `861`. Its body is a 544-byte stack-frame timing/render
update path, with no demonstrated pending-character command branch. Table
registration establishes recurring engine callback only. A bounded wrapper is
unsafe until event-2 thread/order and JSPI resumption are proven, original
callback behavior is retained, and exact-once private transaction proof exists.

## Parity result

Static evidence currently supports candidate read-only roster structure only.
No independently derived state traversal or target-character identity proves
that a requested character has entered world. Selector disappearance, loading,
or frame-context state remain ambiguous. Required six adapter operations therefore
remain unproven: `observeReady`, `readRoster`, `readSelected`,
`selectCharacter`, `enterCharacter`, and `observeEntered`. Keep Asyncify
capability disabled; do not infer success from `6661`, an indirect dispatcher,
or absence of pre-game UI.

## JSPI entered-world observer (separate exact-build proof)

This section concerns current raw JSPI `web/Gw.jspi.wasm` (SHA-256
`1eb07332632e2fca8aabf5baa14fa1a1e6a2a59ec7134dfb8f6231d924c9fd7b`)
and its credential-prefill output (`e2ad01cc8f3ccb82263399b72ec596f47e9a5a7439e640f7089463cb5abe07db`).
Prefill changes only its certified login/getter path and appends private state;
the readers below retain their raw bodies. The later character wrapper only
replaces callback `6661`, so it also does not alter them.

Exact raw JSPI role bodies found in current artifact:

| role | function | signature | SHA-256 | instruction evidence |
| --- | --- | --- | --- | --- |
| game-to-character context | `2705` | `(i32) -> i32` | `f234e09fc78c540418d7ee1e02bb339caf4e95b91dad5803dd87f1b1f229eede` | `i32.load offset=68` |
| character current-map reader | `9517` | `() -> i32` | `c6e2e54332d133eb208974890b255570fb2fcc9a802ea52957b4a64116f7e72c` | context accessor `228(17)`, load `564` |
| character player-number reader | `9524` | `() -> i32` | `3656f5655d9704c608247be52aaf1654141bce2db8f085c6cef4c967c7068acf` | context accessor `228(17)`, load `684` |
| character instance-type reader | `12962` | `(i32) -> i32` | `b9898076fd712f74e586f60df2a7e551c68db82b3726025815f46b78f32b8413` | `i32.load offset=572` |

These are independently located by exact hashes/signatures, then joined by the
reference's matching `derivePlayRegionLayout` proof. It derives current client
context traversal from a unique context-root writer: `load(0x5a0e70)`, context
slot `6`, game `load(... + 24)`, character `load(game + 68)`. This is not a
blind reuse of an address: that proof requires unique semantic role, exact role
hashes above, operand locations, and six code occurrences of context-root
operand. It derives character UUID as official fixed 16-byte `player_uuid` at
`character + 100` and instance type at `character + 572`.

Ready/loading meaning is also defined by matching source code, rather than UI
visibility: instance type `2` is Loading. Ready requires type `0` or `1`, a
nonzero map within `1..=2000`, base-map equality, `is_explorable == (type ==
1)`, and player number `1..=65535`. Thus an entered observer may publish
`loading`, `unavailable`, or `ready(key)` only after all bounds and state checks
pass. It must never classify missing selector frame as entered.

A privacy-safe entered identity is feasible: hash exactly 16 nonzero UUID bytes
with FNV-1a-64 inside Wasm, compare only that key to target's roster UUID key,
and export a boolean/result only. This uses the same bounded key routine for
in-world `character + 100` and an account record `record + 8`; current
reference kernel identifies those as `CharContext::player_uuid` and 132-byte
account-record UUID respectively.

Remaining specific gap: current GWNative reader exports count/readiness only;
it neither reads account record UUID nor retains a chosen record key. Therefore
no target-key comparison can yet run. A later closed select transaction must
validate target index against frozen roster, derive/store its 16-byte UUID key
privately, then observe `ready(current-key == expected-key)` over two stable
ticks. Add a certificate/fixture proving account-record UUID offset `8` before
using it: roster-copy body alone proves record size and bounds, not UUID field
semantics. No local display-name or agent scan is needed for entry proof, and
none has been certified here.
