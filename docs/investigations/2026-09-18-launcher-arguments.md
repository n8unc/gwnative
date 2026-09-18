# WASM startup-argument feasibility

## Scope and artifact

Static inspection plus an offline exact-artifact parser probe. No game launch,
network request, credential read, or login simulation occurred. Constructor
locale/clock imports were satisfied with local stubs; no external host effects
occurred.

Inspected `web/Gw.wasm` SHA-256:

`373c65abcfe6c26161ffd59cb8ea298eb3d1814f4778aeaf24d8978c1fdfa8ef`

The runnable probe is
[`scripts/probe-wasm-login-arguments.mjs`](../../scripts/probe-wasm-login-arguments.mjs).
It checks this digest, adds diagnostic exports and one empty table slot only to
an in-memory module, and never writes the client artifact.

## Proven startup and parser flow

Generated glue exposes in-memory `Module.arguments` and passes its values into
`callMain(args)` (`web/Gw.js:11327`, `12584-12600`). This is Wasm-side argv
construction; it is not native process argv, an environment variable, or a
URL. The current harness does not assign `Module.arguments`.

Static tracing shows `__main_argc_argv` calls `func[10473]`. The probe does not
call main. It initializes constructors, then manually invokes these actual
client functions in sequence:

```text
func[10473](argc, argv)                        collect startup argv
func[10483](1452944, 52)                       create parser with client registry
func[10487](handle, callback, 0, 0, 0)          parse retained command string
func[10486](handle, 1 | 47 | 51)               read parsed option value
```

`1452944` (`0x162b90`) is the actual option-descriptor table base and contains
52 descriptors. The former `0x162c24` address was an interior table location
and is superseded. Descriptor IDs observed by the probe are `1` (`autologin`),
`47` (`email`), and `51` (`password`).

Fourteen fixed cases pass:

| case | observed result |
| --- | --- |
| `--email=value --password=value` | both values populated |
| `-email=value -password=value` | both values populated |
| `--email value --password value` | both values populated |
| `-email value -password value` | both values populated |
| `--autologin` | option value `1` |
| `-autologin` | option value `1` |
| combined credentials plus auto-login | all three values populated |
| unknown option control | parse result `0`; unknown callback receives the option |
| no flags | parse result `1`; empty values |
| ASCII punctuation (`-fixture=two\\three`) | round-trips as a password value |
| unquoted space | truncates value and rejects the remaining token |
| wrapping quotes | corrupts/rejects the value |
| embedded quote | corrupts/rejects the value |
| non-ASCII UTF-8 (`pāss🔒`) | byte-expanded UTF-16 code units |

Constructor locale/clock imports were the only imports observed, and were
handled by local stubs. The probe performed no network, credential, game, or
authenticated-runtime operation. It does not call any option value callback
handler; it only reads returned option values.

The punctuation, quoting, spacing, and UTF-8 cases describe this parser's
observed transport behavior. They do not prove that no serializer could
represent other values; they only bound this probe.

## Security and experiment boundary

`Module.arguments` is in-memory JavaScript/WASM state, so it does not itself
violate the narrower rule against OS arguments, environment variables, or URLs.
It does retain credential-bearing startup text inside the client realm, and the
parser's command-string path can retain an encoded representation. Keep this
experiment separate from production credential transport.

The next controlled test is limited to `--autologin` in-memory
`Module.arguments`, with existing `Module.secureStorage` supplying UTF-16
credentials. Live acceptance needs a user-chosen test account entered locally
and an independent login action/state signal. Current harness has no
`Module.arguments` assignment; no production hook is implied by this record.

## Verdict

WASM startup argv and semantic parsing are now proven for the tested spellings,
value forms, and `autologin` flag. Credential values can be recovered from
the parser, but this remains offline parser evidence only. It proves neither
login submission nor authentication, failure handling, character selection, or
world entry.
