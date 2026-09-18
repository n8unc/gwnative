# Saved-credential request gate

## Finding

Saved credentials in the host do not, by themselves, trigger game prefill.
The inspected JSPI login UI requests them only when setting 95 is enabled.
That same setting initializes the control named `BtnRememberPass`.

Further inspection identifies the setting by schema, not only UI label:
boolean descriptor `1456208 + 95 * 12` contains name pointer `1461568`,
whose UTF-16 value is `SavePassword`. Settings loader `func[10845]` uses section
`Prefs` at `1463970`. The probe now asserts both names.

This identifies a concrete prefill prerequisite, not a confirmed reading of
the affected live profile. No production behavior was changed in this investigation.

## Reproduction

Run `node scripts/probe-wasm-credential-request.mjs`:

```text
{"setting95":0,"requests":0}
{"setting95":1,"requests":1}
```

Run `node scripts/probe-wasm-credential-request.mjs --expect-request`
to assert that the reduced zero-setting login fixture requests saved credentials. This intentionally
fails with `Zero-setting login fixture never asks host for saved credentials` (`0 !== 1`).
This assertion covers the missing request prerequisite, not rendered fields.

The probe checks `web/Gw.jspi.wasm` SHA-256
`1eb07332632e2fca8aabf5baa14fa1a1e6a2a59ec7134dfb8f6231d924c9fd7b`.
It adds exports and replaces selected UI-service functions and settings-source operation `func[980]`
only in an in-memory copy. The actual login event handler `func[11302]`, settings
initializer/accessor bodies and credential request functions remain unchanged.
Both settings initializer `func[10786]` and loader `func[10845]` now run.
Only the settings-source operation is mocked to return failure, which makes
the loader retain defaults. A missing source is consistent with this result,
but unreadable or rejected data may also fail. This establishes the value
without loaded overrides, not the affected existing profile's persisted value. Earlier probe versions
stubbed the loader entirely and did not establish this default.
Imports are fail-closed except local clock/locale and credential-hook observation.
No profile, Keychain, network, game process or original client artifact is changed.

## Binary evidence

- Login handler `func[11302]`, original offset `0x3bbb2e`: control name at
  address `1493662` decodes as UTF-16 `BtnRememberPass`.
- At `0x3bbb3e`, setting 95 is read through `func[10797]`; its value is assigned
  to the control through `func[11093]` and retained in login UI state.
- At `0x3bbba3`, zero skips the credential request. Nonzero reaches
  `func[10154]` at `0x3bbbc0`, then `func[9975]`, then generated glue's
  `Module.secureStorage.getCredentials()` operation at address `2666524`.
- Settings initialization without loaded overrides yields zero. Changing only that fixture value
  yields one request. This is a diagnostic input, not a proposed memory patch.

## Host and sister-project comparison

`web/harness.js:488-510` already starts credential retrieval before game startup;
`:660-686` exposes the client-facing secure-storage hook. Sister project
`.references/gwonmac/src/renderer/harness.ts:857-869` uses the same hook, loading
credentials on request. Inspected reference credential/startup code supplies no
remember-password preference seed to port. Its automatic character return is a
separate, armed relog operation, not unconditional startup prefill.

The important difference is **how the Keychain entry is created**.
Reference `docs/user-guide.md:279-284` explicitly says its launcher does not ask
for credentials; the game owns sign-in. Reference `harness.ts:870-875` persists
credentials when the game calls `secureStorage.storeCredentials()`.
GWNative's `src/launcher_accounts.rs:339-361` instead saves credentials directly
from the launcher account form. That path does not set client `Prefs/SavePassword`.
Keychain content and game preference are independent pieces of state.
`src/profile.rs:84-123` creates an isolated WebKit store identity and profile
descriptor; it does not copy an existing client's preferences into the new Account.

In the inspected client, the save path also consults setting 95 at `0x3e0479`
in `func[11641]`; the enabled path with a password reaches `func[10158]` at
`0x3e04f3`, then `func[9977]` and the `storeCredentials` glue at `2667659`.
Thus the reference's ordinary game-driven save workflow differs from seeding
Keychain alone through a launcher. Its Keychain fixture tests do not prove
that independently seeded credentials populate a fresh game login screen.

The original launcher also rejected Play when Auto-login was enabled. That
restriction was removed after the prefill fix so these accounts can launch for
manual submission. It explained refused launches, not blank fields in a running game. The account editor deliberately leaves saved passwords
blank (`ui/launcher.js:124-131`); its email field is populated from account metadata.

## Implemented prefill path

`src/wasm/launcher_prefill.rs` adds a per-instance boolean capability for the
reviewed JSPI and Asyncify artifact pairs. When enabled, the existing settings
getter reports SavePassword enabled; otherwise its original behavior remains.
It does not rewrite persisted preferences or submit login. Official files stay
unchanged; the host serves a guarded derived copy. Unknown pairs retain normal
client behavior.

`web/launcher-credentials.js` waits for nonempty launcher credentials before the
Wasm startup callback resumes, then enables that capability. Only the boolean
crosses the new bridge. The game's existing secureStorage callback receives the
Keychain values. Missing credentials, timeout, or an unsupported artifact leave
manual login available. Managed game save/clear callbacks cannot replace or
clear the launcher's cached credentials or Keychain record.

`node --test web/*.test.js` covers startup ordering and managed credential
ownership. `scripts/probe-wasm-launcher-prefill.mjs` exercises the production
getter and original login handler on both reviewed client artifacts. UI services
and settings-source loading are mocked; this proves the request path, not
rendered fields or authentication. Live field verification is recorded separately.

Auto-login remains a separate unsupported option; this patch only enables
prefill. No credentials are passed through command-line arguments.

## Fix validation

- 405 Rust tests passed; three live checks ignored.
- 286 web tests passed, including actual harness startup and managed cache checks.
- Final debug build, strict Clippy check, and whitespace check passed.
- Production-derived JSPI and Asyncify probes pass request-gate, persisted-on,
  disabled-override, and adjacent-setting checks.
- Rebuilt signed development binary launched the existing managed profile:
  Keychain read, launcher gate ready, client secureStorage delivery, and first
  frame all observed. No filesystem restore failure. Process exited successfully.
- Computer-use inspector could not identify the unbundled development window;
  rendered field population remains visually unverified. Login was not submitted.

## Follow-up: callback requires a matching account name

The user's live screenshot disproved the initial prefill claim: both fields
remained empty even with Remember Password enabled. The host return log precedes
the generated glue callback and was insufficient evidence of field population.

Exact client `func11302` handles outer event79/nested event210 by reading the
current account-name widget (`func11188`) and comparing it with the Keychain
result (`func354`). A mismatch exits before copying the password. This callback
is a password lookup for an existing name, not an email-and-password field fill.

Prefill ABI2 therefore retains a UTF-16 launcher account name and seeds it only
during the initial account-name setter (`func11195`), before the credential
request. Full body hashes and distinct JSPI/Asyncify instruction anchors guard
that insertion; Asyncify inserts inside its normal-execution guard. The original
callback's name-match condition remains unchanged. The password still travels
only through the existing secure-storage callback.

The default production probe now allocates the synthetic account name before
stack initialization/constructors and checks that actual client initialization
passes that exact name to its field setter. Both reviewed runtimes pass. This
checks the client assignment boundary, not merely the host's credential return.

Live acceptance after ABI2: the corrected signed development build reached its
first frame and delivered the Keychain callback; the user explicitly confirmed
“IT PRE-FILLED” after seeing the game window. The diagnostic session then closed
orderly. Automatic login submission was not added or tested. Full Rust suite
(405 passed, three ignored), web suite, Clippy, and debug build passed.

The final offline consumer regression also invokes the real event79/event210
branch on both runtimes. Original comparison and password-copy functions remain
unchanged; only surrounding UI services are mocked. Empty or changed visible
account names leave password state empty, while a matching name accepts the
synthetic password. This regression passes alongside the initialization tests.
