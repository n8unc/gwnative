/** Must match `FEATURE_*` in `src/companion-kernel/lib.rs`. */
export const FEATURE_NATIVE_CURSOR = 1 << 0;
export const FEATURE_TARGET_READOUT = 1 << 1;
export const KNOWN_FEATURES = FEATURE_NATIVE_CURSOR | FEATURE_TARGET_READOUT;

/**
 * Intersect player settings with capabilities the selected exact client was
 * certified to observe. This happens before companion regions are allocated:
 * target readout must not receive a snapshot region or run a read for a
 * cursor-only certificate. Unsupported requested tools are reported to the
 * caller but do not suppress another reviewed tool.
 *
 * @param {{ featureMask?: unknown }} manifest
 * @param {{ nativeCursor: boolean, targetReadout: boolean }} selection
 */
export function selectEnhancementFeatures(manifest, selection) {
  const featureMask = manifest.featureMask === undefined
    ? KNOWN_FEATURES
    : manifest.featureMask;
  if (
    !Number.isInteger(featureMask)
    || featureMask <= 0
    || featureMask > KNOWN_FEATURES
  ) {
    throw new Error('the client manifest has unsupported enhancement capabilities');
  }
  const requested =
    (selection.nativeCursor ? FEATURE_NATIVE_CURSOR : 0)
    | (selection.targetReadout ? FEATURE_TARGET_READOUT : 0);
  const unavailable = requested & ~featureMask;
  const enabled = requested & featureMask;
  return Object.freeze({
    nativeCursor: (enabled & FEATURE_NATIVE_CURSOR) !== 0,
    targetReadout: (enabled & FEATURE_TARGET_READOUT) !== 0,
    flags: enabled,
    unavailable,
  });
}
