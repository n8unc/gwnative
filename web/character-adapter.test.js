import test from 'node:test';
import assert from 'node:assert/strict';
import { readCharacterRoster, readSelectedCharacterName } from './character-adapter.js';

function fixture() {
  const module = { HEAPU8: new Uint8Array(256) };
  const write = (at, value) => {
    const view = new DataView(module.HEAPU8.buffer);
    for (let i = 0; i < value.length; i += 1) view.setUint16(at + i * 2, value.charCodeAt(i), true);
  };
  write(40, 'First Character'); write(100, 'Second Character');
  const exports = {
    GwnativeCharacterRosterCount: () => 2,
    GwnativeCharacterNameAt: index => [40, 100][index] ?? 0,
    GwnativeSelectedCharacterName: () => 100,
  };
  return { module, exports };
}

test('reads bounded typed names and selected character from current heap', () => {
  const { module, exports } = fixture();
  assert.deepEqual(readCharacterRoster(exports, module), [
    { index: 0, name: 'First Character' }, { index: 1, name: 'Second Character' },
  ]);
  assert.equal(readSelectedCharacterName(exports, module), 'Second Character');
  module.HEAPU8 = new Uint8Array(8);
  assert.equal(readSelectedCharacterName(exports, module), null);
});

test('rejects malformed bounds, stale roster, unterminated and invalid Unicode names', () => {
  const { module, exports } = fixture();
  for (const pointer of [0, -2, 41, 240, NaN]) {
    assert.equal(readSelectedCharacterName({ ...exports, GwnativeSelectedCharacterName: () => pointer }, module), null);
  }
  let reads = 0;
  assert.equal(readCharacterRoster({ ...exports, GwnativeCharacterRosterCount: () => ++reads === 1 ? 2 : 3 }, module), null);
  assert.equal(readCharacterRoster({ ...exports, GwnativeCharacterRosterCount: () => 65 }, module), null);
  module.HEAPU8.fill(65, 100, 140);
  assert.equal(readSelectedCharacterName(exports, module), null);
  module.HEAPU8.fill(0, 100, 140);
  new DataView(module.HEAPU8.buffer).setUint16(100, 0xd800, true);
  assert.equal(readSelectedCharacterName(exports, module), null);
});

import { createCharacterAdapter } from './character-adapter.js';

function actionFixture() {
  const { module, exports } = fixture();
  const view = new DataView(module.HEAPU8.buffer);
  view.setInt32(160, 123, true); view.setInt32(176, 456, true);
  const calls = [];
  const listeners = new Map();
  let selected = 40;
  let status = 0;
  let world = 0;
  Object.assign(exports, {
    GwnativeCharacterUuidAt: index => [160, 176][index] ?? 0,
    GwnativeSelectedCharacterName: () => selected,
    GwnativeCharacterReadiness: () => 3,
    GwnativeCharacterUiReady: () => 1,
    GwnativeCharacterWorldEntered: (...words) => { calls.push(['world', ...words]); return world; },
    GwnativeCharacterActionConfigure: enabled => { calls.push(['configure', enabled]); return 1; },
    GwnativeCharacterActionTarget: (...words) => { calls.push(['target', ...words]); return 1; },
    GwnativeCharacterActionCancel: () => { calls.push(['cancel']); status = 0; return 1; },
    GwnativeCharacterAction: (kind, index) => {
      calls.push(['action', kind, index]); status = -1;
      if (kind === 1) selected = [40, 100][index];
      return 1;
    },
    GwnativeCharacterActionStatus: () => { const current = status; status = 1; return current; },
  });
  const bridge = createCharacterAdapter({ exports, module, sessionId: 'session', pause: async () => {},
    inputTarget: { addEventListener: (type, fn) => listeners.set(type, fn),
      removeEventListener: type => listeners.delete(type) } });
  return { bridge, exports, module, calls, listeners, request: { sessionId: 'session' }, setWorld: value => { world = value; } };
}

test('queues select and Play once, independently checks frozen UUID in world', async () => {
  const { bridge, calls, request, setWorld } = actionFixture();
  assert.equal(bridge.observeReady(request).state, 'ready');
  const character = bridge.readRoster(request).roster[1];
  assert.equal((await bridge.selectCharacter({ ...request, character })).accepted, true);
  assert.equal((await bridge.readSelected(request)).character.id, character.id);
  assert.equal((await bridge.enterCharacter({ ...request, character })).accepted, true);
  assert.equal((await bridge.enterCharacter({ ...request, character })).accepted, false);
  assert.equal(bridge.observeEntered(request).state, 'waiting');
  setWorld(1);
  assert.equal(bridge.observeEntered(request).character.id, character.id);
  assert.deepEqual(calls.filter(call => call[0] === 'action'), [['action', 1, 1], ['action', 2, 0]]);
  assert.deepEqual(calls.filter(call => call[0] === 'target'), [['target', 456, 0, 0, 0]]);
  assert.deepEqual(calls.at(-1), ['world', 456, 0, 0, 0]);
  bridge.dispose();
});

test('manual sign-in input is allowed, manual selection input cancels queued automation', async () => {
  const { bridge, request, listeners, calls, exports } = actionFixture();
  exports.GwnativeCharacterReadiness = () => 0;
  listeners.get('keydown')({ type: 'keydown', isTrusted: true });
  exports.GwnativeCharacterReadiness = () => 3;
  assert.equal(bridge.observeReady(request).state, 'ready');
  const character = bridge.readRoster(request).roster[0];
  listeners.get('pointerdown')({ type: 'pointerdown', isTrusted: true });
  assert.equal((await bridge.selectCharacter({ ...request, character })).accepted, false);
  assert.equal(bridge.observeReady(request).state, 'cancelled');
  assert.equal(calls.some(call => call[0] === 'action'), false);
  assert.equal(listeners.size, 0);
});

test('roster availability waits for UI readiness and rechecks before either action', async () => {
  const { bridge, request, exports, calls } = actionFixture();
  exports.GwnativeCharacterUiReady = () => 0;
  assert.equal(bridge.observeReady(request).state, 'waiting');
  const character = bridge.readRoster(request).roster[1];
  assert.equal((await bridge.selectCharacter({ ...request, character })).accepted, false);
  assert.equal(calls.some(call => call[0] === 'action'), false);
  exports.GwnativeCharacterUiReady = () => 1;
  assert.equal(bridge.observeReady(request).state, 'ready');
  assert.equal((await bridge.selectCharacter({ ...request, character })).accepted, true);
  exports.GwnativeCharacterUiReady = () => 0;
  assert.equal((await bridge.enterCharacter(request)).accepted, false);
  assert.equal(calls.filter(call => call[0] === 'action').length, 1);
  bridge.dispose();
});

test('refuses stale session, duplicate UUID and identity changed after selection', async () => {
  const { bridge, request, exports, module, calls } = actionFixture();
  assert.equal(bridge.observeReady({ sessionId: 'different' }).state, 'cancelled');
  bridge.observeReady(request);
  const character = bridge.readRoster(request).roster[1];
  await bridge.selectCharacter({ ...request, character });
  new DataView(module.HEAPU8.buffer).setInt32(176, 999, true);
  assert.equal((await bridge.enterCharacter({ ...request, character })).accepted, false);
  assert.equal(calls.filter(call => call[0] === 'action').length, 1);
  exports.GwnativeCharacterUuidAt = () => 160;
  assert.equal(bridge.readRoster(request).roster, null);
  bridge.dispose();
});

test('bounds stalled callback and cancels it before returning to manual control', async () => {
  const { bridge, request, exports, calls } = actionFixture();
  bridge.observeReady(request);
  exports.GwnativeCharacterActionStatus = () => -1;
  const character = bridge.readRoster(request).roster[0];
  assert.equal((await bridge.selectCharacter({ ...request, character })).accepted, false);
  assert.equal(bridge.observeReady(request).state, 'cancelled');
  assert.ok(calls.some(call => call[0] === 'cancel'));
});

import { observeCharacterNames } from './character-adapter.js';

test('last-seen publication contains only current names and session identity', async () => {
  const { module, exports } = fixture();
  let reads = 0;
  exports.GwnativeCharacterReadiness = () => ++reads > 1 ? 3 : 0;
  const reports = [];
  assert.equal(await observeCharacterNames({ exports, module, sessionId: 'one',
    pause: async () => {}, publish: async body => reports.push(body) }), true);
  assert.deepEqual(reports, [{ sessionId: 'one', names: ['First Character', 'Second Character'] }]);
  const controller = new AbortController(); controller.abort();
  assert.equal(await observeCharacterNames({ exports, module, sessionId: 'one',
    signal: controller.signal, publish: () => assert.fail('aborted publication') }), false);
});

test('reordered roster resolves frozen identity to its new live index', async () => {
  const { bridge, request, exports, calls } = actionFixture();
  bridge.observeReady(request);
  const character = bridge.readRoster(request).roster[1];
  exports.GwnativeCharacterNameAt = index => [100, 40][index] ?? 0;
  exports.GwnativeCharacterUuidAt = index => [176, 160][index] ?? 0;
  assert.equal((await bridge.selectCharacter({ ...request, character })).accepted, true);
  assert.deepEqual(calls.find(call => call[0] === 'action'), ['action', 1, 0]);
  bridge.dispose();
});

test('aborting a queued selection cancels it without affecting another account bridge', async () => {
  const first = actionFixture(); const second = actionFixture();
  first.bridge.observeReady(first.request); second.bridge.observeReady(second.request);
  const controller = new AbortController();
  first.exports.GwnativeCharacterActionStatus = () => { controller.abort(); return -1; };
  const character = first.bridge.readRoster(first.request).roster[0];
  assert.equal((await first.bridge.selectCharacter({ ...first.request, character, signal: controller.signal })).accepted, false);
  assert.equal(first.bridge.observeReady(first.request).state, 'cancelled');
  assert.equal(second.bridge.observeReady(second.request).state, 'ready');
  assert.equal(second.calls.length, 0);
  second.bridge.dispose();
});

test('manual input at character selection cancels even before first readiness poll', () => {
  const { bridge, request, listeners } = actionFixture();
  listeners.get('pointerdown')({ type: 'pointerdown', isTrusted: true });
  assert.equal(bridge.observeReady(request).state, 'cancelled');
});

test('new duplicate target name after selection refuses Play despite frozen UUID', async () => {
  const { bridge, request, exports, module, calls } = actionFixture();
  bridge.observeReady(request);
  const character = bridge.readRoster(request).roster[1];
  await bridge.selectCharacter({ ...request, character });
  module.HEAPU8.copyWithin(40, 100, 140);
  assert.equal((await bridge.enterCharacter({ ...request, character })).accepted, false);
  assert.equal(calls.filter(call => call[0] === 'action').length, 1);
  bridge.dispose();
});

test('Play refuses pre-existing entered state and malformed world observations', async () => {
  for (const state of [1, -1, undefined]) {
    const { bridge, request, calls, setWorld } = actionFixture();
    bridge.observeReady(request);
    const character = bridge.readRoster(request).roster[1];
    await bridge.selectCharacter({ ...request, character });
    setWorld(state);
    assert.equal((await bridge.enterCharacter(request)).accepted, false);
    assert.deepEqual(calls.filter(call => call[0] === 'action'), [['action', 1, 1]]);
    bridge.dispose();
  }
});
