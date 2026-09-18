# Launcher login and application-update investigation

## Scope

Initial work used static inspection plus an offline exact-artifact argv/parser
probe. That probe opened no game and read no credentials. A subsequent saved
Account startup investigation opened diagnostic game processes using the normal
protected credential route; observations are recorded below. Probe scope and
result: [startup-argument feasibility](2026-09-18-launcher-arguments.md).

## Inspected client artifact

| file | SHA-256 |
| --- | --- |
| `web/Gw.wasm` | `373c65abcfe6c26161ffd59cb8ea298eb3d1814f4778aeaf24d8978c1fdfa8ef` |
| `web/Gw.js` | `e6d031a22d9047f30a1e2d1d2e25f6daca552df4a5dc1e3fad83b57bf404463c` |
| `web/harness.js` | `dd72b748c2903d7666cacf8ce041bc5201e2dfd3630ce6c392eed42659f9a8b3` |

Repeat this work for any changed digest.

## Verified credential flow

`web/harness.js:660-686` supplies `Module.secureStorage.getCredentials()`
from host-backed saved credentials. `web/Gw.js:11806` converts the returned
strings to UTF-16 and calls `OsCredentialsGetResult(username, password,
callback, userParam)`. This proves saved-value delivery and an asynchronous
continuation, not login submission or an authenticated session.

The current native launcher path offers invocation credentials to the same
protected host route; it does not serialize them into `window.__gwnativeLaunch`
or assign `Module.arguments`. The client can therefore receive credentials
through existing secure storage while startup arguments remain an independent
experiment.

The Wasm exports credential result continuations and
`EmscriptenInputOskInput`, `EmscriptenInputOskChange`, and
`EmscriptenInputOskComplete`. It exports no named portal-login submit,
login-status, or character-selection operation. Missing export names do not
prove an internal action is impossible: internal code can call unexported
functions or indirect callbacks.

`web/harness.js:1139-1149` creates hidden email/password proxies and sets
`Module.oskIsModal = false`. `ASM_CONSTS[2658622]` at `web/Gw.js:11794` copies
a non-modal key event to canvas. Only modal Return-plus-blur maps to
`EmscriptenInputOskComplete`; synthetic Return, focus, or blur remains blind
UI input without a certified action or success signal.

Generated glue also supports provider auth through
`Module.login.getAuthToken(...)` or `Module.nativeAccount.login(...)`, returning
provider values through `EmscriptenGcPlatformGetAuthTokenResult`. The harness
supplies only `login.hasProvider`, returning false; it supplies neither token
callback. This is a separate provider-token protocol.

`ui/login-probe.js` (covered by `web/account-login.test.js`) is an
exact-artifact-gated diagnostic observer. It records field category, a user
Return gesture, and two known login request paths/statuses. It never reads field
values, request bodies, responses, cookies, or headers; it never synthesizes
input or reports authentication success.

## Startup arguments and next controlled test

Generated glue accepts in-memory `Module.arguments` at `web/Gw.js:11327` and
passes them to `_main` through `callMain` at `:12584-12600`. The exact-artifact
probe's 14 checks prove the real parser accepts `--email=value`,
`--password=value`, separate-value forms, both dash spellings, and
`--autologin` / `-autologin`. Unknown options return parse result `0`; ASCII
punctuation round-trips, while spaces and quotes corrupt or reject values and
non-ASCII UTF-8 is byte-expanded. It resolves the actual descriptor table at
`1452944` (`0x162b90`), count 52; the former `0x162c24` address was an
interior location and is superseded.

`Module.arguments` is in-memory JavaScript/WASM state, not native process argv,
an environment variable, or a URL. It can still retain credential-bearing
startup text and encoded command-string data, so it must remain separate from
production secret transport. Current harness has no assignment to it.

Next controlled test: use only `--autologin` in-memory, let existing
`Module.secureStorage` provide the locally entered test account's UTF-16
credentials, and observe an independent login action/state signal. This avoids
adding a new secret transport. Credential retrieval, focus, Return, and process
liveness alone do not establish successful sign-in.

## Credential continuation

`wasm-objdump` resolves the host credential request to Wasm `func[10154]` at
`0xd14d6e`. It invokes `func[9975]` at `0xd14d7a` with callback-table index
`2371`; generated glue at data address `2666524` (`0xcfb998`) calls
`Module.secureStorage.getCredentials()` and then `OsCredentialsGetResult`.

The continuation resolves to `func[10155]` (`0xd14d89`), which copies returned
credentials into a temporary object and calls unlabelled `func[876]` at
`0xd14ee9`. Static code establishes delivery to an internal consumer only. It
does not identify a login-button action or prove network/auth success. No
separately callable submit transition, readiness predicate, error signal, or
authenticated-state signal has been certified.

## Saved Account startup and gwonmac comparison

The user reproduced blank fields by running `cargo run`, then launching an
Account with email and password stored. A diagnostic game using that Account's
profile logged a successful protected-storage read and reached its first frame.
No `secureStorage: returning the saved login` marker was observed. This proves
host retrieval, not that the WASM requested or consumed the result. Native UI
inspection timed out, so field population was not independently verified.

The user supplied `.references/gwonmac` for comparison. Its credential path is:

1. `src/renderer/harness.ts:857-869`: client calls
   `Module.secureStorage.getCredentials()`, which awaits native
   `credentials.load()` and returns `{ username, password }`.
2. `src/preload/preload.body.cjs:190-194`: credential operations cross Electron
   IPC.
3. `src/main/ipc.ts:463-475` and
   `src/main/multiple-accounts-controller.ts:140-143,529-540`: the requesting
   game window selects its own profile's Keychain store.

This is the same client-facing hook used here. The inspected reference path
does not assign credential-bearing `Module.arguments` or fill HTML inputs.
Its validator (`src/main/core/credentials.ts:24-38`) accepts strings, including
spaces, quotes and Unicode, up to 4096 code units per field.

Reference `automatic-character-return.ts:345-380` separately submits the saved
login after paint and focus. Crucially, `claimIntent()` at lines 302-325 must
first return an armed relog intent. This is a conditional relog continuation,
not unconditional startup autofill. Copying its Enter action would not explain
or fix a missing credential request.

Reference provider handling also differs: it offers Steam while this host
reports no federated providers. No controlled result establishes whether that
difference, client preferences, or startup ordering explains the missing hook.
No production login change was made on the basis of this comparison.

### Cache audit

The named Account's selected shell revision has byte-identical `harness.js`
and `index.html` to the source tree. Both official JS/WASM pairs in its live
client directory also match source SHA-256 hashes. The diagnostic runtime
selected JSPI, build 38888, generation `cf616d7e6056f3d1`; its official Wasm hash
is `1eb07332632e2fca8aabf5baa14fa1a1e6a2a59ec7134dfb8f6231d924c9fd7b`.

Shell revisions refresh by content hash on each new process launch. Static
responses use `Cache-Control: no-cache` with ETags; derived Wasm caches verify
the input/glue hashes, transform ABI and output hash. Focused Rust checks passed:
four path tests and five shell tests. This rules out an old copied artifact for
the inspected profile, but does not prove what an already-open page has loaded.

The runtime log also contains `Failed to sync file system: ErrnoError: Resource
busy`, immediately after persistent-filesystem readiness and runtime init.
This is a separate concrete startup failure to reproduce before attributing
blank fields to any particular internal login-state condition.

### Filesystem correction and sound-parameter experiment

`web/filesystem.js` changed the working directory to `/app:` during preRun.
The client's subsequent relative `app:` lookup could therefore resolve to
`/app:/app:` and attempt a second mount. Normalize explicit `app:` paths to the
existing absolute mount while retaining the original `app:` database identity.
Three regression tests fail against the original implementation and pass with
the fix. A fresh signed game run then reached its first frame without the
filesystem-restore error, but still showed no credential-delivery callback.

The native `-nosound` switch is translated to the host's mute option. A live
run recorded `gw.audio.muted = 1`. The double-dash `--nosound` spelling is
currently rejected by the native CLI. Account child creation originally
forwarded only offline/update policy; it now also forwards mute as `-nosound`.
A subprocess regression parses `-nosound` and checks the child argv, without
placing credentials there.

To isolate the WASM argument path, a temporary harness assigned
`Module.arguments = ['-nosound']`, with no native mute option. The normal
baseline created one audio context; the argument run created none. Both reached
the first frame and read credentials through protected host storage. Neither
logged a client credential-delivery callback. The temporary harness assignment
was removed after the test. This is live evidence for the sound argument, not
proof that arbitrary credential strings round-trip through argv or that login
succeeds.

An offline probe of JSPI's generated setting table found entry 95 remains zero
after initialization; a saved-credential request branch depends on this entry.
Its meaning has not been identified, so it must not be labelled or modified as
an account/password preference on this evidence alone.

## Application updates while games survive launcher exit

Local Sparkle 2.9.4 headers show installation can occur when its host
terminates; its delegate delays relaunch but does not provide a complete
download-now and external-lock-before-install path for this launcher policy.
The launcher therefore performs metadata checks only and does not start
Sparkle in this development build. No application archive is staged or
installed.

`updater::LauncherUpdateGate` remains a tested future-helper primitive: it holds
`profiles.lock` and every profile `gwnative.lock`, blocking existing direct
games, new profile allocation, and new direct starts through final handoff. A
future staged-update helper must retain that exclusion through replacement.

## Result

Saved credential delivery and startup option parsing are proven independently.
Automatic login, authentication outcome, challenge handling, character
selection, and world entry remain unproven; no implementation should claim them
from these offline results.

## Subsequent live auto-login verification

The earlier result above describes the original investigation. A controlled
2026-09-18 comparison now establishes automatic submission on the reviewed JSPI
artifact, using the user's authorized Main Account:

- Baseline: saved credentials delivered through the existing host bridge; game
  remained at the prefilled login screen.
- Flag-only variant: `Module.arguments = ['-autologin']`; same profile and
  credential route; game reached character selection without mouse or keyboard
  submission. The user independently confirmed the result.
- No email/password arguments, DOM field filling, timed Enter, character choice,
  or world-entry action was used.
- Final rebuilt integration, with the saved Auto-login preference enabled and
  conditional flag insertion, independently reached character selection again.
  Validation: 405 Rust tests passed (three ignored), the complete web-suite
  integration passed, and formatting, Clippy and diff checks passed.

Production integration captures the Account's Auto-login choice when the game
registers its profile and injects only a boolean into the private WebView
preamble. `Module.arguments` starts empty. After the exact-pair credential bridge
successfully prepares saved credentials, the harness appends `-autologin` if
that boolean is true, before calling the WASM success callback. Both official
glue files capture the arguments array before asynchronous instantiation and
consume it after that callback; replacing the array would lose the flag.

The harness regression executes this actual boundary for both glue variants,
including delayed delivery, opt-out, unmanaged profiles, absent/failed credential
reads, and missing reviewed exports. It checks the captured array at the success
callback, including that no credential strings enter it. Existing prefill probes
continue to check the real client functions for both artifacts.

GWOnMac reference commit `004194ff318320f854240fd227462b19889bef24` takes a different
route: [automatic-character-return.ts](https://github.com/Mat4m0/gwonmac/blob/004194ff318320f854240fd227462b19889bef24/src/renderer/automatic-character-return.ts)
claims an armed relog intent, waits for rendering and pre-game controls, then
[input.ts](https://github.com/Mat4m0/gwonmac/blob/004194ff318320f854240fd227462b19889bef24/src/renderer/input.ts)
sends canvas Enter. Its renderer does not set login arguments. GWNative uses the
client's existing auto-login flow instead of copying that input automation.

This proves successful sign-in on the tested JSPI client. It does not prove
wrong-password/security-challenge behavior, Asyncify live sign-in, or a launcher
authentication-result/Needs attention signal. The host adds no retry loop; the
client retains control of authentication errors and challenges.
