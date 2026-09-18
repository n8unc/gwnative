# Phase 2 native live checks

Status: acceptance procedure only. Record date, app build/client runtime, result and observation in `launcher-phase-2-implementation.md`; do not record Account names, email addresses, passwords, Keychain output, session nonces, or texture paths outside user-owned notes.

## Preconditions

Use two existing launcher Accounts whose owners approve interactive sign-in. They must have distinct Profiles and neither can be queued, starting or running. Confirm this in launcher UI only. Do not add, edit, remove, or inspect credentials for this check. Keep Character preference empty: current builds explicitly require manual character selection.

Before native work, run existing non-live checks:

```sh
cargo test launcher_accounts --no-fail-fast
cargo test launcher_sessions --no-fail-fast
cargo test launcher_preferences --no-fail-fast
cargo test window::state --no-fail-fast
node --test web/account-launcher.test.js
```

Record supplied client build/runtime. JSPI auto-login has prior live evidence only when saved launcher credentials and Auto-login were already enabled; Asyncify automatic sign-in remains unproven. Neither result proves character entry.

## Saved group and queue

1. In launcher, create temporary group containing two approved Accounts in visible order A then B. Give A muted/windowed with fixed layout and explicit FPS; give B sound/windowed with a different fixed layout and explicit FPS. Save.
2. Quit and reopen launcher. Verify group name, membership order and each Account's displayed next-launch preferences persist. This proves catalog persistence only.
3. Launch group once. Observe A starts first. When A game window appears, observe B starts; do not require authentication or character entry. Record per-member status and approximate start ordering.
4. Click group Launch again while both sessions live. Expect each member skipped/already running and no extra game window. If possible, repeat through overlapping group containing either Account.
5. Close only A through launcher. Verify B remains open. Reopen launcher and verify B reconnects as running. This checks session independence from launcher lifetime; do not close a game by quitting launcher.
6. After both sessions exit, delete temporary group. Verify Accounts and Profiles remain present. Do not remove Accounts to test membership cleanup on a live catalog.

Fail result: a duplicate process, B starting before A window is visible, later member never attempted after an A failure, changed queued settings, or group deletion changing Account/Profile data.

## Native current-layout and fullscreen

1. With one approved Account running windowed, move/resize its native window to distinctive logical-point bounds. Use **Use current window layout**. Wait for UI confirmation, then save Account settings. The action must capture only; it must not change a running window.
2. Close game normally and launch that Account. Observe restored bounds. Start a second Account with another fixed layout and verify each window retains its own geometry, mute state and FPS choice.
3. Set fullscreen for one Account, retain fixed frame, and launch it. It should open fullscreen. Change that Account back to windowed, launch again, and verify retained fixed frame restores. Do not infer physical fullscreen from a fake/test display.
4. With a removable second display available, save frame there, disconnect display while game is closed, then launch. Verify window is visibly fitted on remaining display. Reconnect display and repeat only if needed.

## Evidence and unresolved gates

Capture only screenshots/video that omit account identity and login fields. Record observed window bounds/modes, ordering, duplicate-process result, exact build/runtime and any failure text. Stop manual testing on authentication challenge, wrong-account state, or ambiguous session ownership.

Group and window checks alone do not establish character or texture support. The completed JSPI character bridge, combined native entry and Minimalus observations are recorded in [implementation acceptance](launcher-phase-2-implementation.md). Its matrix separates native evidence from synthetic coverage and remaining limits. Asyncify character entry remains manual; representative uMod containers remain unverified.
