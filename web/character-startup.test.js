import assert from 'node:assert/strict';
import test from 'node:test';
import { characterCapabilityStatus, startPreferredCharacter } from './character-startup.js';

const sessionId = 'session-a';
const target = { id: 'char-a', name: 'Devona' };
const capability = { supported: true, runtime: 'jspi', build: 'exact-a', operations: [
  'observeReady', 'readRoster', 'selectCharacter', 'readSelected', 'enterCharacter', 'observeEntered',
] };

function bridge(overrides = {}) {
  return {
    observeReady: async ({ sessionId: id }) => ({ sessionId: id, state: 'ready' }),
    readRoster: async ({ sessionId: id }) => ({ sessionId: id, roster: [target] }),
    selectCharacter: async ({ sessionId: id }) => ({ sessionId: id, accepted: true }),
    readSelected: async ({ sessionId: id }) => ({ sessionId: id, character: target }),
    enterCharacter: async ({ sessionId: id }) => ({ sessionId: id, accepted: true }),
    observeEntered: async ({ sessionId: id }) => ({ sessionId: id, state: 'entered', character: target }),
    ...overrides,
  };
}

let runSequence = 0;
const run = (overrides = {}) => startPreferredCharacter({ sessionId: `session-${runSequence += 1}`, runtime: 'jspi', clientBuild: 'exact-a', capability, preferredCharacter: 'Devona', bridge: bridge(), deadlineMs: Infinity, sleep: async () => {}, ...overrides });

test('requires exact runtime, build, and complete certificate-backed bridge', () => {
  assert.deepEqual(characterCapabilityStatus({ sessionId, runtime: 'jspi', clientBuild: 'other', capability, bridge: bridge() }), { supported: false, reason: 'uncertified-build' });
  assert.deepEqual(characterCapabilityStatus({ sessionId, runtime: 'asyncify', clientBuild: 'exact-a', capability, bridge: bridge() }), { supported: false, reason: 'uncertified-build' });
  assert.deepEqual(characterCapabilityStatus({ sessionId, runtime: 'jspi', clientBuild: 'exact-a', capability, bridge: {} }), { supported: false, reason: 'incomplete-bridge' });
  for (const runtime of ['jspi', 'asyncify']) {
    assert.deepEqual(characterCapabilityStatus({ sessionId, runtime, clientBuild: 'exact-a', capability: { ...capability, runtime, supported: false }, bridge: bridge() }), { supported: false, reason: 'unsupported-build' });
  }
});

test('selects exact unique name, confirms it, enters once, then independently observes entry', async () => {
  let enters = 0;
  assert.deepEqual(await run({ bridge: bridge({ enterCharacter: async ({ sessionId: id }) => { enters += 1; return { sessionId: id, accepted: true }; } }) }), { outcome: 'entered', character: 'Devona' });
  assert.equal(enters, 1);
});

test('uses exact name when certified roster has no stable identity', async () => {
  const nameOnly = { name: 'Devona' };
  assert.deepEqual(await run({ bridge: bridge({
    readRoster: async ({ sessionId: id }) => ({ sessionId: id, roster: [nameOnly] }),
    readSelected: async ({ sessionId: id }) => ({ sessionId: id, character: nameOnly }),
    observeEntered: async ({ sessionId: id }) => ({ sessionId: id, state: 'entered', character: nameOnly }),
  }) }), { outcome: 'entered', character: 'Devona' });
});

test('fails closed for missing or ambiguous target without selecting', async () => {
  let selects = 0;
  const result = await run({ bridge: bridge({ readRoster: async ({ sessionId: id }) => ({ sessionId: id, roster: [target, { id: 'char-b', name: 'Devona' }] }), selectCharacter: async () => { selects += 1; } }) });
  assert.deepEqual(result, { outcome: 'manual', reason: 'missing-or-ambiguous-target' });
  assert.equal(selects, 0);
});

test('fails closed for stale state, manual interruption, failed confirmation, and no independent entry', async () => {
  assert.deepEqual(await run({ bridge: bridge({ observeReady: async () => ({ sessionId: 'other', state: 'ready' }) }) }), { outcome: 'manual', reason: 'stale-session' });
  assert.deepEqual(await run({ bridge: bridge({ selectCharacter: async () => ({ sessionId: 'other', accepted: true }) }) }), { outcome: 'manual', reason: 'stale-session' });
  assert.deepEqual(await run({ bridge: bridge({ observeReady: async ({ sessionId: id }) => ({ sessionId: id, state: 'manual' }) }) }), { outcome: 'manual', reason: 'interrupted' });
  assert.deepEqual(await run({ bridge: bridge({ readSelected: async ({ sessionId: id }) => ({ sessionId: id, character: { id: 'wrong', name: 'Devona' } }) }) }), { outcome: 'manual', reason: 'selection-unconfirmed' });
  assert.deepEqual(await run({ bridge: bridge({ observeEntered: async ({ sessionId: id }) => ({ sessionId: id, state: 'manual' }) }) }), { outcome: 'manual', reason: 'interrupted' });
  assert.deepEqual(await run({ bridge: bridge({ readSelected: async ({ sessionId: id }) => ({ sessionId: id, state: 'cancelled' }) }) }), { outcome: 'manual', reason: 'interrupted' });
});

test('coalesces concurrent and later requests for one launch session', async () => {
  const oneSession = 'one-launch-only';
  let selections = 0;
  const shared = bridge({ selectCharacter: async ({ sessionId: id }) => {
    selections += 1;
    await Promise.resolve();
    return { sessionId: id, accepted: true };
  } });
  const options = { sessionId: oneSession, runtime: 'jspi', clientBuild: 'exact-a', capability, preferredCharacter: 'Devona', bridge: shared, deadlineMs: Infinity, sleep: async () => {} };
  const first = startPreferredCharacter(options);
  assert.strictEqual(startPreferredCharacter(options), first);
  assert.deepEqual(await first, { outcome: 'entered', character: 'Devona' });
  assert.deepEqual(await startPreferredCharacter(options), { outcome: 'entered', character: 'Devona' });
  assert.equal(selections, 1);
});

test('keeps accounts isolated and rejects entry for another character', async () => {
  const accountB = { id: 'char-b', name: 'Cynn' };
  let observations = 0;
  const first = run({ preferredCharacter: 'Devona' });
  const second = run({ preferredCharacter: 'Cynn', bridge: bridge({
    readRoster: async ({ sessionId: id }) => ({ sessionId: id, roster: [accountB] }),
    readSelected: async ({ sessionId: id }) => ({ sessionId: id, character: accountB }),
    observeEntered: async ({ sessionId: id }) => ({ sessionId: id, state: observations++ ? 'manual' : 'entered', character: target }),
  }) });
  assert.deepEqual(await first, { outcome: 'entered', character: 'Devona' });
  assert.deepEqual(await second, { outcome: 'manual', reason: 'interrupted' });
});

test('bounds a bridge promise that never resolves', async () => {
  assert.deepEqual(await run({
    bridge: bridge({ observeReady: async () => new Promise(() => {}) }),
    deadlineMs: 1,
  }), { outcome: 'manual', reason: 'readiness-timeout' });
});

test('aborts a late action at deadline so bridge can refuse its side effect', async () => {
  let aborted = false;
  let waits = 0;
  const result = await run({
    deadlineMs: 1_000,
    now: () => 0,
    sleep: async () => { waits += 1; if (waits < 3) await new Promise(() => {}); },
    bridge: bridge({ selectCharacter: ({ signal }) => new Promise((resolve) => {
      signal.addEventListener('abort', () => { aborted = true; resolve({ sessionId, accepted: false }); }, { once: true });
    }) }),
  });
  assert.deepEqual(result, { outcome: 'manual', reason: 'selection-timeout' });
  assert.equal(aborted, true);
});
