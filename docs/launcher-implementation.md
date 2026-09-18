# Launcher implementation status

Status: development implementation; not ready for release.

The [accepted specification](launcher.md) remains the target. No feature has been
removed from that target to make the implementation appear complete.

## Implemented boundaries

- Bare application startup opens a native account window. Explicit game and
  maintenance commands retain separate paths.
- Account metadata has its own catalog. Passwords use protected macOS Keychain
  storage. Games cannot save or clear launcher-owned credentials.
- Accounts select private Profiles. The existing shared immutable chunk cache
  remains the game-content source; the launcher does not clone the game image.
- A per-profile queue starts separate game processes and waits for each native
  window before starting the next. Profile locks and authenticated local control
  prevent duplicate starts and permit reconnect after launcher exit.
- Play, selection, per-account toggles, Show, Close, explicit force quit, import
  review, and removal flows are wired to native operations.
- Game-client revalidation offers pending generations without replacing active
  game files. Application release checks currently fetch metadata only.

## Unresolved acceptance gates

**Automatic login is not implemented.** The saved-credential bridge can provide
credentials, but a reliable submit action and authentication/challenge state have
not been certified. An offline test now proves that the actual WASM option parser
accepts `--email=value`, `--password=value`, and `--autologin`. It also exposes
password encoding/quoting failures in that argv path. The next controlled live
test should pass only `--autologin` and retain the existing protected credential
bridge; its interaction with that flag is not yet proven. Accounts can launch regardless of their saved Auto-login choice. This build
prefills saved credentials and requires the user to click Log In in game; the
Auto-login control is unavailable until automatic submission is implemented. The diagnostic probe is not connected to a game and does not submit
or infer authentication success. See [login investigation](investigations/2026-09-18-launcher-login.md).

**Application update download/install is not implemented.** Sparkle can install
when its host terminates, including while independent game processes survive.
Consequently no game or launcher starts Sparkle in this development build; stored
preferences are preserved. A separate staged-update helper must retain exclusion
through replacement before automatic application installation can be enabled.
Game-client generation updates remain a separate working path.

**Live two-account acceptance remains pending.** Passing process and storage
unit tests does not establish simultaneous authenticated gameplay, persistent
in-game isolation, login failure/challenge handling, or live update recovery.

## Validation

- Native account window opened in an isolated test application bundle; empty
  list and Add Account form inspected visually and through accessibility.
- Password field enables Auto-login; changing another field preserves an explicit
  off choice. Test form cancelled without saving credentials or an Account.
- Final `cargo test --quiet`: 403 passed, three ignored, no failures; the web-suite
  integration test passed. Ignored live checks were not claimed as evidence.
- `cargo clippy --all-targets -- -D warnings`, debug build, and `git diff --check`
  pass.
- Rebuilt native launcher opened successfully after UI assets moved under `ui/`.
  A second invocation exited rather than creating another launcher process.
- `node scripts/probe-wasm-login-arguments.mjs`: 14 exact-artifact offline checks
  pass, including syntax, option values, unknown-option rejection, and observed
  encoding/quoting hazards. This is parser evidence, not authenticated gameplay.

## Review corrections

- Kept the existing game download-choice module intact. Native account UI lives
  under `ui/`, separately from the certified game shell under `web/`.
- Combined password and metadata edits into one validated account operation.
- Checked profile exclusion during sensitive edits and asynchronous deletion.
- Prevented update checks from racing explicit private-file deletion. Deletion
  retains a named profile's WebKit descriptor until other private files are
  removed, and failed cleanup can be retried without restoring the Account.
- Preserved saved application-update preferences and the existing automatic
  check interval; the launcher's explicit Check now action requests a check.
- Forwarded offline/update policy to child game processes.
- Reconnected preparing games and checked process birth identity before explicit
  force quit; a PID file alone is insufficient.
- Reserved each profile through child spawn and process-identity publication.
  Reopening during startup or recovery preserves the pending session. Failed
  child cleanup keeps the Account busy until its process has actually ended.

## Launcher credential prefill

Managed sessions now wait for their Keychain credential read before starting the
client. For the two reviewed client artifacts, a guarded derived module enables
the existing saved-login request and seeds the account-name field during login
screen creation without changing persisted game preferences. This name is needed
because the client only accepts a returned password for a matching visible name.
The existing secure-storage callback delivers email and password; no credential
arguments are generated. In-game save/clear calls preserve launcher ownership.

This is prefill only. Play and Auto-launch accept accounts with saved Auto-login
choices; automatic submission remains unavailable.
Unknown client versions and unavailable credentials retain manual login.
