# Character startup argument feasibility

## Scope

Offline inspection of checked-in `web/Gw.jspi.wasm`, plus
`scripts/probe-wasm-login-arguments.mjs` with fixed fixture strings. The probe
checks the exact Wasm SHA-256, instantiates only an in-memory diagnostic module,
does not call `main`, and blocks every host import except constructor locale and
clock stubs. It performs no game launch, credential read, network access, login
or character-selection action.

## Parser evidence

`Gw.jspi.js` takes `Module.arguments` at line 11102 and builds Wasm argv in
`callMain` at lines 12214-12227. This is client-memory startup argv, not an OS
argument, environment value or URL.

The official descriptor table starts at linear-memory address `1452944`
(`0x162b90`) and has 52 twelve-byte entries. Entry 14 is:

| descriptor index | kind | UTF-16 name | value ID |
| --- | ---: | --- | ---: |
| 14 | 768 | `character` | 46 |

The offline parser accepts canonical `--character=Fixture` and
`-character Fixture`: both return parse result `1`, no unknown callback, and
value ID 46 returns `Fixture`. Do not treat `charactername` as an alias: its
double-dash spelling happens to produce a value, while its single-dash form
parses `name` as the character value and rejects `Fixture`. `--charname` and
`--char` are rejected and delivered to the unknown callback.

Names containing spaces are not safely transported by this path. A single
`Module.arguments` item `--character=Fixture User` produces character value
`Fixture`, parse result `0`, and unknown token `User`. The existing probe also
shows wrapping or embedded quotes corrupt/reject values. No supported quoting
form has been demonstrated.

## Native consumer trace

Disassembly shows official `func[9441]` calls the parser bootstrap
`func[10490]`, then calls `func[10491]` with descriptor index `14`. If nonzero,
it copies the parsed UTF-16 value through a bounded 260-byte temporary and
stores resulting object pointer at linear-memory global location `5925952`.
This is evidence that official startup initialization consumes the parsed
`character` field; it is stronger than a host CLI label or parser-only match.

The trace does not establish what later code does with that stored object. No
offline call followed its downstream consumers, and no game session was run.
Therefore native requested-character selection and world entry remain
unproven. The current character-read observer remains read-only and separate.

## Verdict

Current exact JSPI supports a nonsecret `-character` startup option and retains
its value in native startup state. It cannot safely carry names with spaces
using the demonstrated `Module.arguments` serialization. Do not replace any
character-selection implementation with this option until a bounded live test
shows selection and entry for a fixture account, including a name with spaces.
