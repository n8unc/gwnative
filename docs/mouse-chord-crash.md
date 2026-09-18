# RMB + LMB movement crash

Reported 2026-09-18: holding RMB and LMB moves the character, then movement
after several seconds aborts with `evt.buttonState` at
`Engine/Frame/FrMouse.cpp:486`. The saved native log confirms that assertion;
it does not contain the preceding mouse transitions.

## Verified failure path

Current JSPI client disassembly shows function 6263 requires its internal
mouse event button state to be nonzero. Function 6275 populates that state
from the client-held button mask at 5911088. Press/release handlers update
that mask independently of the browser's `MouseEvent.buttons` field.
A DOM mask of 3 (LMB + RMB) is valid; it is not itself the assertion failure.

The production input-handler regression reproduces a mismatch: press both
buttons, lose pointer lock, let the host synthesize releases, then move while
physical buttons are still down. The browser still reports a drag while the
client's press/release-derived state is zero. The regression failed before
the fix. A separate continuous-drag control crosses multiple re-anchor
boundaries without losing tracked button state.

## Fix

After synthetic releases, the host remembers which physical buttons belong
to the cancelled gesture. It suppresses stale drag movement and duplicate
physical releases. Physical release or a fresh press ends that suppression;
ordinary hover and fresh gestures remain available. Synthetic releases carry
the progressively remaining button mask.

A bounded 32-entry mouse-transition history records button transitions,
capture changes, reset reasons and re-anchors. The existing abort handler logs
it only for `evt.buttonState` failures. It records no pointer coordinates,
keystrokes, account names or credentials.

## Validation

`node --test web/input.test.js`: 9 passed. Tests cover uninterrupted chord
re-anchors, capture loss, both release orders, fresh presses, hover recovery
and bounded diagnostics. `node --test web/*.test.js`: 348 passed. JavaScript
syntax and diff checks passed.

## Evidence limit

The reset/movement mismatch is reproduced through actual host input handlers
with a client-state model derived from the checked client. The user's precise
physical sequence has not been reproduced in a native game run; capture loss
is a demonstrated trigger, not a confirmed event in that run. If it recurs,
the crash-only history distinguishes reset/capture and re-anchor transitions.
The development launcher installs the updated shell on the next game launch;
already-running game pages retain their loaded input handlers.
