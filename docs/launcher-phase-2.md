# Launcher Phase 2: groups, launch settings, characters and texture packs

Status: implementation plan. Feature scope and the two choices below are user-confirmed; remaining product defaults are recommendations for implementation.

## Scope and priorities

Phase 2 adds:

1. Saved launch groups.
2. Per-account window size/position, sound and frame-rate settings.
3. A preferred character that is selected and automatically enters the game.
4. Texture overrides, prioritising existing TexMod/uMod packs.

User decisions:

- Selecting a preferred character also means entering the game. There is no separate auto-enter toggle.
- Existing TexMod/uMod packs take priority over inventing a GWNative-only authoring format.
- Automatically discover `.tpf` files in `texturepacks`; manual import is not required for files placed there. The supplied sample is `texturepacks/Minimalus.UI.v3.2.tpf`.
- Toolbox features are a later phase.
- The previous MVP completion programme is deferred. Application-update installation, comprehensive authentication/challenge reporting, full multibox release acceptance and broad adoption/recovery validation remain documented gaps, not prerequisites for starting this phase.

Feature-specific checks below still apply. For example, two accounts using different texture packs must be checked to establish texture isolation; that does not reopen the entire deferred release programme.

The accepted [MVP specification](launcher.md) and [implementation record](launcher-implementation.md) remain historical and current baselines. Groups, character entry and texture overrides deliberately extend that original scope. Account/Profile identity, launcher-independent game lifetime, and launcher-owned credentials remain unchanged. This phase does not introduce multiple Profiles per Account.

## 1. Launch groups

### User experience

- Create a named group from existing Accounts, choose members and arrange launch order.
- Save, rename, edit, delete and launch a group from the launcher.
- An Account can belong to several groups. Groups contain references, not copies of Accounts or credentials.
- Launching a group uses each Account's own settings, preferred character and texture selection.
- Already queued, starting or running Accounts are skipped. Show which members were started, skipped or failed.
- Preserve existing staggered startup: start the next member when the previous game window opens, without waiting for login or character entry.
- Repeated clicks and overlapping groups still produce at most one live instance per Account.
- Editing a group does not change launches already queued. Deleting a group never closes games or removes Accounts.

Recommended initial limits: groups are manually launched; no group-specific settings overrides or separate group Auto-launch. Existing Account Auto-launch continues independently. Removing an Account removes its membership references atomically; renaming an Account preserves membership. An empty group remains editable with Launch disabled.

### Implementation and acceptance

Store stable group IDs, names and ordered Account IDs in versioned launcher metadata. Extend the existing launch-selected path and session queue rather than introduce another process controller. Resolve members and take their launch-setting snapshots when the group launch is requested.

Checks: create/edit/reorder/restart persistence; overlapping groups; already-running members; missing members in imported/older metadata; one failed member with later members continuing; deletion without Account/Profile changes. Group persistence and Account removal must not leave dangling membership after an interrupted write.

## 2. Per-account launch settings

### User experience

Add a Launch settings section to Account editing:

- Sound: on or muted.
- Frame-rate limit: use the current default, or an explicit validated limit.
- Window mode: windowed or fullscreen, using existing supported modes.
- Window layout: restore the last layout by default, or use a saved fixed size and position.
- A Use current window layout action captures a running Account's native window geometry without restarting it.

Settings edits apply to the next launch. Clearly mark this while a game is running; do not unexpectedly move, resize or mute a live game. Fullscreen temporarily ignores windowed geometry but retains it for a later windowed launch. If a saved display disappears, fit the window visibly on an available display.

### Implementation and acceptance

Keep one authoritative Profile window-state store; do not duplicate geometry in Account metadata. Extend `src/window/state.rs` with a versioned `window.json` containing `lastObservedFrame` and a separate optional `fixedLaunchFrame` preference. Migrate existing geometry to the last-observed value. The game owns observed-frame updates; the launcher owns explicit preferences. Merge writes under the same store lock, and stage changes made during play so the game's shutdown write cannot overwrite the new preference.

Persist nonsecret launch preferences separately from runtime observations. Resolve one launch descriptor for Play, Launch selected, groups and Auto-launch. Recommended precedence: explicitly supplied invocation overrides, then per-account launch preferences, then existing profile/application defaults. Absence must be distinguishable from an explicit false/default value. Credentials remain in Keychain and never enter this descriptor's process arguments.

Existing mute/FPS and window-mode code can provide the runtime implementation. Put each Account's resolved options into its child `LaunchSpec`; replace the current runner-wide `game_options` treatment for these preferences so one Account's mute/FPS choices cannot bleed into another. Explicit invocation overrides are resolved before that per-child snapshot. Keep UI dimensions in macOS logical points; display scaling is not a second render-resolution setting.

Checks: different settings on two Accounts; preference persistence; consistent behaviour across launch entry points; queued settings remaining stable; running-game edits preserved for next launch; display removal, scaling and fullscreen transitions; stale or malformed settings falling back to usable defaults.

## 3. Preferred character and automatic entry

### User experience

- Default: Stop at character selection.
- Optional: choose a preferred character for that Account. After authentication, select that character and enter the game automatically.
- Keep Auto-login independent. A configured character can follow successful manual sign-in as well as automatic sign-in, once the correct character list is available.
- Present a list observed from that Account's game when available. Any remembered list is labelled as last seen and is revalidated against the live list before acting.
- Missing, renamed, ambiguous or unavailable targets leave the game open for manual choice. Never substitute the first character or whichever character happens to be highlighted.
- Authentication challenges pause progression naturally. Character automation does not retry passwords or manufacture login success.

### Capability work before feature delivery

The native `-character` switch is currently parsed but explicitly unsupported. A successful `-nosound` or `-autologin` test does not establish character selection.

First prove, for supported client builds:

1. Observe authenticated character-selection readiness and the live roster.
2. Identify the intended character reliably, using stable identity if available and verified exact-name matching otherwise; never a saved row index.
3. Ask the client to select it and confirm the selected identity.
4. Invoke the client's Enter/Play action once.
5. Observe entry of the intended character independently of the submission action.

Use exact-build client integration, following the existing reviewed credential-bridge approach. Do not rely on screen coordinates, fixed delays or blind keyboard sequences. Bind the operation to its launch/session identity and stop on cancellation, stale state, unsupported builds or a bounded readiness timeout. A later sign-out must not unexpectedly replay startup automation.

The reference project's pre-game and character-switch modules provide investigation leads, not proof that their integration ports unchanged. This is a focused character capability, not a general Toolbox port. Document the new capability in the game API contract rather than silently widen existing read-only guarantees.

Checks: successful automatic and manual sign-in followed by correct character entry; reordered roster; missing/renamed target; slow loading; manual interruption; unsupported client; two concurrent Accounts with different targets. Confirm both JSPI and Asyncify behaviour before claiming support for both.

## 4. Existing texture-pack support

### Compatibility milestone first

Separate three questions:

- Can we read the pack's container and mapping definitions?
- Can its texture identifiers be matched to this client's texture uploads?
- Can replacement pixels be rendered correctly, including orientation, alpha, compression and mip levels?

The local gwonmac reference contains a legacy TexMod TPF reader, DDS decoding, texture identifier calculations and upload interception. Its importer explicitly accepts `.tpf`; uMod-style matching does not by itself prove arbitrary uMod package/container support. Its TPF reader also rejects unsupported hash widths. Produce a compatibility matrix from representative TexMod and uMod packs before advertising formats as supported.

Prefer adapting proven format rules and matching logic after reviewing the reference's reuse terms. Prove the integration against GWNative's WKWebView/WebGL path. Start with a visible, known texture replacement and an unchanged baseline. If an existing pack needs conversion, preserve the original and make the supported conversion explicit; an internal compiled cache is not a replacement user-facing pack format.

### User experience

- A shared texture library automatically lists `.tpf` files discovered in the watched `texturepacks` folder, with validation progress, details and compatibility/error information.
- Scan at launcher startup and watch for additions, changes, renames and removals while open. Rescan on watcher recovery and provide Refresh and Open folder actions. Discovery does not enable a pack on any Account automatically.
- Validate and prepare a discovered pack once, then enable it independently for any Account. An optional Add files action copies chosen files into the watched folder; it is a convenience, not the required discovery path.
- Recommended selection model: an ordered enabled-pack list per Account. The first enabled pack, displayed at the top, wins when multiple packs replace the same texture; persist this order and show conflicts and precedence.
- No packs selected means original game appearance.
- Selection/order changes apply on next launch. Live reloading is deferred.
- Import failures explain unsupported variants or malformed contents without silently marking an incomplete pack as fully supported.
- Removal explains affected Accounts. Existing game sessions retain the exact pack revision they started with until they exit.

### Folder discovery

Add a dedicated texture-source resolver in `src/paths.rs`: explicit saved folder choice first, otherwise the compile-time project root plus `texturepacks` for development runs, or a writable shared `Application Support/gwnative/texturepacks` directory for packaged builds. Never resolve against the launcher's current working directory. Offer Change folder for an explicitly selected external directory. Do not store player packs inside the application bundle or one Account's private Profile.

Initial discovery covers top-level regular files with a case-insensitive `.tpf` extension. Additional uMod formats are added to discovery only after their container support is established. Wait until a newly copied file is stable before validation; combine duplicate filesystem events and retry when an incomplete file changes. Work runs off the launcher UI thread and survives launcher restart through a rescan.

Keep pack identity distinct from content revision. A valid replacement at the same source updates the pack for subsequent launches while preserving Account selections; running games retain their pinned revision. An invalid replacement reports its error and retains the last known-good revision. A removed source is shown as missing and bypassed on subsequent launches with a visible explanation; existing sessions remain unaffected. Never automatically delete a user's source file as a consequence of disabling a pack or pruning compiled cache data.

The supplied `Minimalus.UI.v3.2.tpf` is the first local discovery and rendering acceptance sample. Its presence establishes test input, not compatibility. Pack files remain outside source control under the existing `texturepacks/.gitignore` policy. CI uses small synthetic format fixtures; it must not assume this ignored local file exists or silently report its live compatibility check as passed when absent.

### Storage and runtime

Discovery records the external source path and content identity. Bounded background preparation creates managed immutable source revisions and compiled assets once in a shared library, with per-Account selections; the watched originals remain user-owned and unchanged. Deduplicate identical imports by content identity. Compilation keys include the conversion/matching version and any client-specific dependency; profile identity is not needed in a key unless output actually differs by profile.

Substitute textures at a verified upload seam. Do not rewrite shared game-image chunks, official client files or the original pack. Pin selected revisions for each game session so removal/reimport cannot change a running client's files. Reclaim unused revisions after their last session releases them.

Validate archive paths, expansion/resource limits, mappings, image sizes and supported encodings before publishing a pack. Failed imports leave the existing library usable. Missing or unmatched replacements retain the original texture. Broken pack data must not prevent an otherwise usable game from launching; surface which pack was bypassed.

Checks: the supplied TPF appears without manual import; discovery before and after startup; partial copies and subsequent completion; case-insensitive extensions; rename/removal/replacement; watcher recovery; representative TexMod and uMod formats; known target replacement; transparency/orientation/mips; unmatched targets; pack conflicts; duplicate content; malformed/truncated pack; removal during play; restart persistence; different selections in simultaneous Accounts; frame-time and memory comparison with and without packs. Shared disk assets do not imply shared GPU memory.

## Delivery sequence

| Milestone | Deliverable | Completion evidence |
| --- | --- | --- |
| A | Shared launch-settings resolution, per-account controls and group catalog/UI | All launch paths use one settings snapshot; groups preserve ordering and session exclusion |
| B | Character and texture feasibility proofs, run in parallel with A | Correct character identified/selected/entered; existing pack identifiers produce a visible correct replacement; explicit format/runtime support matrix |
| C | Preferred-character UI and bounded startup progression | Configured target enters reliably; missing targets and unsupported builds remain manual |
| D | Watched pack folder, shared library, per-account selection/order and session-pinned runtime replacement | Supported existing packs are discovered automatically and render; isolation, fallback and removal checks pass |
| E | Combined Phase 2 acceptance | A saved group launches Accounts with distinct layouts, sound/FPS, preferred characters and texture selections; behaviour survives restart |

Within A, implement the shared settings resolver first; settings controls and group persistence/UI can then proceed independently. Keep B early so unknown client actions or texture formats cannot be hidden behind completed UI. C and D can proceed independently after their respective proofs. Integration checks use the real launcher path, not just standalone client flags.

If one capability is blocked, report that specific limitation and continue independent features. Do not claim the whole Phase 2 scope complete while character entry or prioritised pack compatibility remains unproven.

## Implementation inputs and exclusions

The supplied `texturepacks/Minimalus.UI.v3.2.tpf` is available for implementation. Additional representative uMod packs and locally configured test Accounts with known characters will establish the remaining compatibility cases. Credentials stay out of chat, fixtures and diagnostics. Record supported pack variants and tested client builds alongside results.

Deferred: Toolbox ports, additional Profiles per Account, group-specific settings overrides, group scheduling/Auto-launch, texture-pack authoring, live texture reload, and the prior broad MVP release-gate programme. These can be planned separately without blocking this feature phase.
