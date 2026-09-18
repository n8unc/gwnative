// Session-bound preferred-character progression. This module has no fallback
// input path: a certificate-backed bridge must expose every observation and
// action below for the exact selected runtime before it can do anything.

const REQUIRED_OPERATIONS = Object.freeze([
  'observeReady', 'readRoster', 'selectCharacter', 'readSelected',
  'enterCharacter', 'observeEntered',
]);
const SESSION_RUNS = new Map();

export const CHARACTER_REQUIRED_OPERATIONS = REQUIRED_OPERATIONS;

export function characterCapabilityStatus({ sessionId, runtime, clientBuild, capability, bridge }) {
  if (typeof sessionId !== 'string' || sessionId.length === 0) return { supported: false, reason: 'missing-session' };
  if (runtime !== 'jspi' && runtime !== 'asyncify') return { supported: false, reason: 'unknown-runtime' };
  if (typeof clientBuild !== 'string' || clientBuild.length === 0) return { supported: false, reason: 'missing-build' };
  if (capability?.runtime !== runtime || capability?.build !== clientBuild) return { supported: false, reason: 'uncertified-build' };
  if (capability?.supported !== true) return { supported: false, reason: 'unsupported-build' };
  if (!REQUIRED_OPERATIONS.every((operation) => capability.operations?.includes(operation)
    && typeof bridge?.[operation] === 'function')) return { supported: false, reason: 'incomplete-bridge' };
  return { supported: true };
}

function targetFromRoster(targetName, roster) {
  const matches = roster.filter((entry) => entry && entry.name === targetName);
  return matches.length === 1 ? matches[0] : null;
}

function sameCharacter(expected, observed) {
  if (!observed || observed.name !== expected.name) return false;
  return typeof expected.id !== 'string' || observed.id === expected.id;
}

function sessionMatches(value, sessionId) {
  return value && value.sessionId === sessionId;
}

/**
 * Progress one fresh launch at most once. All calls carry session ID so a
 * page from another Account or a stale WebView cannot advance this launch.
 */
export function startPreferredCharacter(options) {
  const sessionId = options?.sessionId;
  if (typeof sessionId !== 'string' || sessionId.length === 0) return progressPreferredCharacter(options);
  const prior = SESSION_RUNS.get(sessionId);
  if (prior) return prior;
  const run = progressPreferredCharacter(options);
  SESSION_RUNS.set(sessionId, run);
  return run;
}

async function progressPreferredCharacter(options) {
  const {
    sessionId, runtime, clientBuild, capability, bridge, preferredCharacter,
    now = () => performance.now(), sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
    deadlineMs = 120_000, pollMs = 100,
  } = options ?? {};
  const status = characterCapabilityStatus({ sessionId, runtime, clientBuild, capability, bridge });
  if (!status.supported) return { outcome: 'manual', reason: status.reason };
  if (typeof preferredCharacter !== 'string' || preferredCharacter.length === 0) {
    return { outcome: 'manual', reason: 'missing-target' };
  }
  const deadline = now() + deadlineMs;
  const expired = () => now() >= deadline;
  const timedOut = Symbol('character bridge timeout');
  const stale = Symbol('character bridge stale session');
  const withinDeadline = async (operation) => {
    const remaining = deadline - now();
    if (remaining <= 0) return timedOut;
    if (!Number.isFinite(remaining)) return Promise.resolve().then(operation);
    const controller = new AbortController();
    let timer;
    let settled = false;
    const pending = Promise.resolve().then(() => operation(controller.signal));
    const timeout = options.sleep
      ? sleep(remaining).then(() => {
        if (!settled) controller.abort();
        return timedOut;
      })
      : new Promise(resolve => {
        timer = setTimeout(() => { controller.abort(); resolve(timedOut); }, remaining);
      });
    try {
      return await Promise.race([
        pending,
        timeout,
      ]);
    } finally {
      settled = true;
      if (timer !== undefined) clearTimeout(timer);
    }
  };
  const observe = async (operation) => {
    const value = await withinDeadline((signal) => bridge[operation]({ sessionId, signal }));
    if (value === timedOut) return timedOut;
    return sessionMatches(value, sessionId) ? value : stale;
  };
  try {
    while (!expired()) {
      const ready = await observe('observeReady');
      if (ready === timedOut) return { outcome: 'manual', reason: 'readiness-timeout' };
      if (ready === stale) return { outcome: 'manual', reason: 'stale-session' };
      if (ready?.state === 'manual' || ready?.state === 'cancelled') return { outcome: 'manual', reason: 'interrupted' };
      if (ready?.state === 'ready') break;
      await sleep(pollMs);
    }
    if (expired()) return { outcome: 'manual', reason: 'readiness-timeout' };

    const rosterResult = await observe('readRoster');
    if (rosterResult === timedOut) return { outcome: 'manual', reason: 'readiness-timeout' };
    if (rosterResult === stale) return { outcome: 'manual', reason: 'stale-session' };
    if (!Array.isArray(rosterResult?.roster)) return { outcome: 'manual', reason: 'invalid-roster' };
    const target = targetFromRoster(preferredCharacter, rosterResult.roster);
    if (!target) return { outcome: 'manual', reason: 'missing-or-ambiguous-target' };

    const selected = await withinDeadline((signal) => bridge.selectCharacter({ sessionId, character: target, signal }));
    if (selected === timedOut) return { outcome: 'manual', reason: 'selection-timeout' };
    if (!sessionMatches(selected, sessionId)) return { outcome: 'manual', reason: 'stale-session' };
    if (selected.accepted !== true) return { outcome: 'manual', reason: 'selection-rejected' };
    const confirmation = await observe('readSelected');
    if (confirmation === timedOut) return { outcome: 'manual', reason: 'selection-timeout' };
    if (confirmation === stale) return { outcome: 'manual', reason: 'stale-session' };
    if (confirmation?.state === 'manual' || confirmation?.state === 'cancelled') return { outcome: 'manual', reason: 'interrupted' };
    if (!sameCharacter(target, confirmation?.character)) {
      return { outcome: 'manual', reason: 'selection-unconfirmed' };
    }
    const entered = await withinDeadline((signal) => bridge.enterCharacter({ sessionId, character: target, signal }));
    if (entered === timedOut) return { outcome: 'manual', reason: 'enter-timeout' };
    if (!sessionMatches(entered, sessionId)) return { outcome: 'manual', reason: 'stale-session' };
    if (entered.accepted !== true) return { outcome: 'manual', reason: 'enter-rejected' };
    while (!expired()) {
      const observation = await observe('observeEntered');
      if (observation === timedOut) return { outcome: 'manual', reason: 'entry-timeout' };
      if (observation === stale) return { outcome: 'manual', reason: 'stale-session' };
      if (observation?.state === 'manual' || observation?.state === 'cancelled') return { outcome: 'manual', reason: 'interrupted' };
      if (observation?.state === 'entered'
        && sameCharacter(target, observation.character)) {
        return { outcome: 'entered', character: target.name };
      }
      await sleep(pollMs);
    }
    return { outcome: 'manual', reason: 'entry-timeout' };
  } catch {
    return { outcome: 'manual', reason: 'bridge-failed' };
  }
}
