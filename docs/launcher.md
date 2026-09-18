# Launcher MVP specification

Status: shared understanding confirmed; implementation authorized.
Development implementation underway. Auto-login failure/challenge acceptance and
application-update installation remain unresolved; this is not a completed MVP. Required evidence
below remains distinct from [implementation results](launcher-implementation.md).

## Accepted scope

- A GUI appears before game launch.
- Accounts represent existing Guild Wars logins, each with a default private
  context for mutable state. Account creation is local launcher registration.
- MVP includes saved account credentials, automatic login, and simultaneous
  game instances.
- MVP manages accounts, startup, and keeping game clients up to date.
- Launcher stays available as a control centre during play. Closing its window
  leaves running games alive. Quitting one game and quitting everything are
  separate actions.
- Texture overrides and GWToolbox++ feature conversions/ports are future
  possibilities, outside this MVP. Additional configurations per Account are
  outside the current scope.

Terminology: [glossary](../CONTEXT.md).
Identity rationale: [Account and Profile identities](adr/0001-account-and-profile-identities.md).
Lifetime rationale: [game sessions outlive the launcher](adr/0002-game-sessions-outlive-launcher.md).
Credential ownership: [launcher-managed credentials](adr/0003-launcher-managed-credentials.md).
Storage evidence: [multibox viability](investigations/2026-09-18-multibox-storage.md).

## Accepted behaviour

### Credentials and login

- Account form contains nickname, login email, optional password, Auto-login and
  Auto-launch. Nickname labels the Account in the launcher and game window;
  changing it preserves existing state.
- Password is optional when adding an Account. Auto-login requires a saved
  password; its option is disabled without one. Creating an Account with a
  password defaults Auto-login on, visible before saving. Subsequent edits
  follow the specific password rules below.
- Adding a password later makes Auto-login available but leaves it off until
  explicitly selected. Replacing a password preserves the current toggle.
  Removing a password disables and unticks Auto-login.
- Editing login email requires confirmation that private context stays attached.
  It clears the old password and disables Auto-login. Reject an email already
  assigned to another Account; a separate game account should use Add Account.
- Launcher manages its saved credentials. Do not read game-entered credentials,
  sync passwords back from the game, or infer an Account reassignment from game
  login changes. User explicitly rejected that feature in Q25. Credential edits
  happen in the launcher.
- While an Account is running, nickname and next-launch toggles may be edited.
  Changing login email, replacing/removing its password, or removing the Account
  requires its game to close first.
- Auto-login signs in; automatic character selection is a future enhancement.
- Failed credentials or additional verification leave the game open for manual
  completion. Launcher flags the Account as needing attention. Other requested
  launches continue; credentials are not repeatedly retried automatically.

### Launch and lifetime

- Opening GWNativeLauncher shows Accounts. Per-account Auto-launch is in MVP,
  off by default. Auto-launch runs once per fresh launcher application start,
  skips running Accounts, and does not run when an existing launcher window is
  merely brought forward. Auto-login separately controls signing in.
- Play stays disabled while that Account's game instance is running. It does
  not focus, restart or launch another instance. A second launch is possible
  only after the previous instance closes. The launch lifecycle must enforce
  this continuously through startup, recovery and shutdown, including rapid
  repeated clicks and launcher reconnection.
- Launch selected starts selected Accounts, skipping already-running Accounts.
  Saved launch groups are outside MVP.
- Stagger startup: open one game window before starting the next. Login flows
  may overlap; an Account awaiting manual login does not hold up the queue.
- Queued/preparing launches can be cancelled before the game starts; afterward
  use Close game. Quitting the launcher cancels pending launches and leaves
  already-started games alive.
- Closing only the launcher window hides it while queued launches continue.
  This differs from quitting the launcher, which cancels pending launches.
- Running Accounts have separate Show window and Close game actions. Show window
  brings the game forward; Close game requests orderly shutdown so mutable
  files can finish saving.
- Closing the launcher window or quitting GWNativeLauncher with Command-Q leaves
  game instances running. A separate Quit launcher and all games action confirms
  first. Reopening the launcher reconnects to running instances.
- No repeated automatic relaunch after a crash or failed start. Show the failure
  and restore Play only once the old instance has fully ended. Existing bounded
  client recovery may finish first; Play remains disabled during recovery.
- If orderly shutdown stalls, show Waiting for game to close, then offer Keep
  waiting or Force quit. Explain potential unsaved-state loss. Force quit is
  always an explicit user action, never an automatic timeout response.

### Updates

- Check for game-client updates in the background on launcher startup. Each
  launch verifies its required client files before opening the game; show shared
  progress and affected Accounts as Preparing. Download game-content chunks on
  demand rather than requiring the entire game image before play.
- Prepare updates without interrupting running games. A subsequent game launch
  uses the required updated version. Running games are never automatically
  restarted to update them.
- If an update check fails, attempt the installed version and explain that the
  game service may require an update. With no usable installed client, show a
  recoverable preparation failure rather than claiming it can launch.
- Launcher owns GWNative application-update checks. Application updates may be
  downloaded during play, but installing them waits until every game instance
  has closed. Do not automatically interrupt games to install an app update.

### Existing data and removal

- First-run review offers adoption of existing profiles and saved logins while
  preserving state. Profiles without an identifiable login ask which Account
  they belong to.
- Adoption is skippable and available later. Adopted Accounts start with
  Auto-launch off. Review nickname, login and Auto-login before saving. Importing
  an Account never launches it by itself.
- One launcher Account per login; duplicate additions are blocked. If adoption
  finds several profiles for one login, the user chooses which becomes its
  context. Preserve other profiles' files without merging them.
- Removing an Account forgets its saved credentials and retains private game
  files by default. Offer a clearly labelled option to delete those files too.
  Its running game must close before removal proceeds.
- Re-adding an Account with retained data offers Reuse previous settings and
  files or Start fresh. Do not silently merge/discard retained data. Credentials
  must be supplied again.

### Interface and installation

- One installed application: opening GWNative shows the launcher, which starts
  independent game windows. No separate launcher installation is required.
- Compact account list with nickname, login email, status, toggles and launch
  controls. Add/Edit opens a focused form; selection enables Launch selected.
  Shared update progress appears above the list.

## Baseline facts recorded during design

These describe the pre-launcher code inspected during the interview. Current
implementation and validation are tracked in [implementation status](launcher-implementation.md).

- [Profile layout](profiles.md) already separates mutable native/browser state
  and credentials while sharing game-image chunks.
- [Application entry](../src/main.rs) selects a profile before starting the game;
  no pre-game account-management GUI currently exists.
- [Application lifecycle](../src/app.rs) currently terminates when its last game
  window closes and flushes persistent game files on orderly quit. Launcher
  lifecycle requires a new boundary.
- [Credential API](../src/server/api.rs) and
  [client adapter](../web/harness.js) already support profile-scoped saved login
  retrieval/storage. [CLI](../src/cli.rs) lists `-autologin` as unsupported;
  saving credentials does not establish automatic submission. Existing game
  credential-write callbacks must not silently overwrite launcher-owned records
  under the accepted one-way credential policy.
- The host currently has no dedicated authentication-result/challenge callback.
  Auto-submit and reliable Needs attention reporting need client integration
  evidence; do not treat a running process or prefilled form as login success.
  Q25's rejection concerns live game-entered credential sync. It does not revoke
  Q10's accepted adoption of existing host-saved credentials.
- [Client update and cache handling](../src/main.rs) currently runs before the
  game window. Launcher presentation and coordination still need implementation;
  accepted update policy is recorded above.
- [Instance locks](../src/instance.rs) enforce exclusion but do not expose a
  launcher session lifecycle. Stored PID alone can be stale after a crash;
  startup, stopping and replacement-process recovery need reliable observation.
- The legacy default profile uses the historical WebKit store and support root;
  named profiles have separate identities. Adoption must preserve those bindings.
  [Profile data](../src/profile.rs) does not currently enforce unique saved login
  names, so multiple existing profiles may refer to the same game login.

## Decision coverage

| Branch | Agreed boundary |
| --- | --- |
| Account identity | Existing game login, default private Profile, one launcher entry per login |
| Credentials | Optional password, explicit Auto-login rules, launcher-managed edits only |
| Login outcome | Automatic sign-in without character selection; manual completion on failure/challenge |
| Startup | Individual/batch/Auto-launch; staggered, cancellable, no duplicate live instance |
| Lifetime | Launcher exit leaves games; reconnect; orderly close; explicit force quit |
| Updates | Required client files prepared before launch; shared chunks; app installation waits |
| Existing data | Optional adoption, explicit duplicate choice, retained-state reuse or fresh start |
| Interface | Compact list, focused forms, status/actions, shared preparation progress, one installed app |
| Future scope | Character selection, saved groups, additional configurations, texture overrides, Toolbox ports |

## Implementation constraints and acceptance evidence

These checks derive from accepted behaviours; they do not introduce additional
user-facing features. Technical implementation choices remain subject to source
and runtime validation.

1. **Prove automatic login first.** On the supported client generation, launch
   with saved credentials and reach character selection without manually
   submitting login. Separately verify disabled Auto-login, missing password,
   wrong password and manual challenge handoff. Establish a reliable attention
   signal without reading game-entered credentials. If client integration cannot
   support required behaviour, report the specific blocker and revisit it with
   the user rather than silently delivering credential prefill as Auto-login.
2. **Enforce credential ownership.** Passwords stay in protected credential
   storage and out of account-list records, logs and process arguments. Game
   save/clear callbacks cannot overwrite or delete launcher-owned credentials.
   Test creation, later addition, replacement, removal and email changes against
   the agreed toggle rules, including write failures.
3. **Verify session exclusion and lifecycle.** Rapid Play clicks, Launch selected,
   Auto-launch and launcher restart cannot create a second instance for the same
   Account. Reconcile actual process/lock state; stale PID files alone do not
   establish ownership. Track startup through crash/recovery and final exit.
   Existing direct-launch paths must preserve profile exclusion and cannot bypass
   it by opening another launcher. Resolve this integration without expanding
   Account management to unrelated processes.
4. **Exercise queue and shutdown boundaries.** Stagger until each window opens;
   do not wait for its login. A failed launch does not strand later Accounts.
   Cancellation racing with process creation must leave either no game or one
   correctly tracked instance. Hiding the launcher preserves queue work; quitting
   cancels pending work. Games survive launcher termination and reconnect on the
   next launch. Orderly shutdown flushes mutable files; force quit requires action.
5. **Prove isolation and shared content with two accounts.** Verify distinct
   credentials, persistent settings and browser storage through quit/restart;
   both games reuse the common game-image chunks. Auto-launch skips surviving
   games. Do not claim memory sharing merely from shared disk storage.
6. **Exercise updates and failure recovery.** Cover warm/offline installed-client
   launch, no usable installed client, interrupted preparation, and a new game
   generation while another game remains open. Keep active generation files and
   chunks usable. Application installation and new game starts must coordinate
   so the all-games-closed prerequisite remains true during installation.
7. **Preserve legacy and retained data.** Check default and named profile adoption,
   skipped adoption, duplicate-login choice, account rename/email change, removal
   with both retention choices and re-adding an Account. Preserve original data
   when adoption or persistence fails. Nickname changes do not change storage
   identity. Validate which legacy data can actually be reused before offering it.
8. **Review the actual interface.** Verify empty, preparing, running, attention,
   failed and closing states; keyboard access, accessible labels, narrow-window
   layout and readable errors. The list must not equate Running with authenticated
   login success. Check disabled actions against real state, not optimistic UI.

## Completion boundary

Implementation follows final shared-understanding confirmation. Work should
first establish automatic-login and independent-session feasibility, then build
and validate the account interface and update/adoption flows. Completion requires
behavioural checks and live two-account evidence; unit tests or a visual mockup
alone are insufficient. Report any user-dependent live validation separately.
