// Private typed observations from exact-build character exports. Never accept
// caller-selected memory addresses or expose these helpers through host routes.
const MAX_CHARACTERS = 64;
const MAX_NAME_UNITS = 20;

function nameAt(module, pointer) {
  const heap = module?.HEAPU8;
  if (!(heap instanceof Uint8Array) || !Number.isInteger(pointer) || pointer <= 0
      || pointer % 2 || pointer > heap.byteLength - MAX_NAME_UNITS * 2) return null;
  const view = new DataView(heap.buffer, heap.byteOffset + pointer, MAX_NAME_UNITS * 2);
  let units = 0;
  while (units < MAX_NAME_UNITS && view.getUint16(units * 2, true) !== 0) units += 1;
  if (units === 0 || units === MAX_NAME_UNITS) return null;
  try {
    return new TextDecoder('utf-16le', { fatal: true }).decode(heap.subarray(pointer, pointer + units * 2));
  } catch { return null; }
}

export function readCharacterRoster(exports, module) {
  if (typeof exports?.GwnativeCharacterRosterCount !== 'function'
      || typeof exports?.GwnativeCharacterNameAt !== 'function') return null;
  try {
    const count = exports.GwnativeCharacterRosterCount();
    if (!Number.isInteger(count) || count < 1 || count > MAX_CHARACTERS) return null;
    const roster = [];
    for (let index = 0; index < count; index += 1) {
      const name = nameAt(module, exports.GwnativeCharacterNameAt(index));
      if (name === null) return null;
      roster.push(Object.freeze({ index, name }));
    }
    // Refresh count after reads; a changing roster is not a coherent snapshot.
    if (exports.GwnativeCharacterRosterCount() !== count) return null;
    return Object.freeze(roster);
  } catch { return null; }
}

export function readSelectedCharacterName(exports, module) {
  if (typeof exports?.GwnativeSelectedCharacterName !== 'function') return null;
  try { return nameAt(module, exports.GwnativeSelectedCharacterName()); }
  catch { return null; }
}

function identityAt(exports, module, index) {
  const pointer = exports.GwnativeCharacterUuidAt(index);
  const heap = module?.HEAPU8;
  if (!(heap instanceof Uint8Array) || !Number.isInteger(pointer) || pointer <= 0
      || pointer % 4 || pointer > heap.byteLength - 16) return null;
  const view = new DataView(heap.buffer, heap.byteOffset + pointer, 16);
  const words = Object.freeze(Array.from({ length: 4 }, (_, i) => view.getInt32(i * 4, true)));
  if (words.every(word => word === 0)) return null;
  return { words, id: words.map(word => (word >>> 0).toString(16).padStart(8, '0')).join('') };
}

const ACTION_EXPORTS = [
  'GwnativeCharacterReadiness', 'GwnativeCharacterUiReady', 'GwnativeCharacterUuidAt',
  'GwnativeCharacterWorldEntered', 'GwnativeCharacterActionConfigure',
  'GwnativeCharacterActionTarget',
  'GwnativeCharacterAction', 'GwnativeCharacterActionCancel', 'GwnativeCharacterActionStatus',
];

/** A private bridge for one instantiation and launch. No generic memory/action API. */
export function createCharacterAdapter({ exports, module, sessionId, inputTarget = globalThis, log = () => {},
  pause = ms => new Promise(resolve => setTimeout(resolve, ms)) }) {
  if (!sessionId || !ACTION_EXPORTS.every(name => typeof exports?.[name] === 'function')) return null;
  let cancelled = false;
  let armed = false;
  let target = null;
  let playSubmitted = false;
  const result = values => ({ sessionId, ...values });
  const stop = () => {
    cancelled = true;
    exports.GwnativeCharacterActionCancel();
    exports.GwnativeCharacterActionConfigure(0);
    for (const type of ['pointerdown', 'keydown', 'pagehide']) inputTarget.removeEventListener?.(type, onInput, true);
  };
  const onInput = event => {
    if (event.type === 'pagehide' || (event.isTrusted
        && (armed || exports.GwnativeCharacterReadiness() === 3))) {
      if (event.isTrusted) log('[character] manual input cancelled startup');
      stop();
    }
  };
  for (const type of ['pointerdown', 'keydown', 'pagehide']) inputTarget.addEventListener?.(type, onInput, true);
  const valid = request => !cancelled && request.sessionId === sessionId && !request.signal?.aborted;
  const snapshot = () => {
    const roster = readCharacterRoster(exports, module);
    if (!roster) return null;
    const identified = roster.map(entry => {
      const identity = identityAt(exports, module, entry.index);
      return identity && Object.freeze({ ...entry, ...identity });
    });
    if (identified.some(entry => !entry) || new Set(identified.map(entry => entry.id)).size !== identified.length) return null;
    // Re-read names/identities to refuse a roster which changed during snapshot.
    const second = readCharacterRoster(exports, module);
    if (!second || second.length !== identified.length || identified.some((entry, i) =>
      second[i].name !== entry.name || identityAt(exports, module, i)?.id !== entry.id)) return null;
    return identified;
  };
  const currentTarget = () => {
    const matches = snapshot()?.filter(entry => entry.name === target?.name);
    return matches?.length === 1 && matches[0].id === target?.id ? matches[0] : null;
  };
  const action = async (request, kind, index) => {
    if (!valid(request)) return false;
    const abort = () => stop();
    request.signal?.addEventListener('abort', abort, { once: true });
    try {
      if (exports.GwnativeCharacterAction(kind, index) !== 1) {
        log('[character] queue refused', kind, exports.GwnativeCharacterActionStatus());
        return false;
      }
      for (let attempts = 0; attempts < 4800 && valid(request); attempts += 1) {
        const status = exports.GwnativeCharacterActionStatus();
        if (status !== -1) {
          if (status !== 1) log('[character] action refused', kind, status,
            typeof exports.GwnativeCharacterActionStage === 'function' ? exports.GwnativeCharacterActionStage() : 0);
          return status === 1;
        }
        await pause(25);
      }
      stop();
      return false;
    } finally { request.signal?.removeEventListener('abort', abort); }
  };
  return {
    dispose: stop,
    observeReady(request) {
      if (!valid(request)) return result({ state: 'cancelled' });
      const ready = exports.GwnativeCharacterReadiness() === 3
        && exports.GwnativeCharacterUiReady() === 1;
      if (ready) armed = true;
      return result({ state: ready ? 'ready' : 'waiting' });
    },
    readRoster(request) {
      return result({ roster: valid(request) ? snapshot() : null });
    },
    async selectCharacter(request) {
      if (!valid(request) || !armed || target) return result({ accepted: false });
      const matches = snapshot()?.filter(entry => entry.id === request.character?.id && entry.name === request.character?.name);
      if (matches?.length !== 1 || exports.GwnativeCharacterReadiness() !== 3
          || exports.GwnativeCharacterUiReady() !== 1) return result({ accepted: false });
      target = matches[0];
      if (exports.GwnativeCharacterActionConfigure(1) !== 1) return result({ accepted: false });
      if (exports.GwnativeCharacterActionTarget(...target.words) !== 1) return result({ accepted: false });
      const accepted = await action(request, 1, target.index);
      return result({ accepted });
    },
    async readSelected(request) {
      // Client may update selection after queued dispatch returns. Observe it,
      // never infer completion from the submission result.
      for (let attempts = 0; attempts < 200 && valid(request); attempts += 1) {
        if (currentTarget() && readSelectedCharacterName(exports, module) === target.name) {
          return result({ character: target });
        }
        await pause(25);
      }
      return result({ state: cancelled ? 'cancelled' : 'waiting' });
    },
    async enterCharacter(request) {
      if (!valid(request) || playSubmitted || !currentTarget()
          || exports.GwnativeCharacterReadiness() !== 3
          || exports.GwnativeCharacterUiReady() !== 1
          || readSelectedCharacterName(exports, module) !== target.name) return result({ accepted: false });
      // Require a fresh waiting-to-enter transition. A pre-existing world or
      // malformed observation cannot be credited to this launch's Play request.
      if (exports.GwnativeCharacterWorldEntered(...target.words) !== 0) return result({ accepted: false });
      playSubmitted = true;
      return result({ accepted: await action(request, 2, 0) });
    },
    observeEntered(request) {
      if (!valid(request)) return result({ state: 'cancelled' });
      const state = target && exports.GwnativeCharacterWorldEntered(...target.words);
      if (state === 1) return result({ state: 'entered', character: target });
      if (state === -1) stop();
      return result({ state: cancelled ? 'cancelled' : 'waiting' });
    },
  };
}

/** Publish names only; never persist UUIDs, indexes or client addresses. */
export async function observeCharacterNames({ exports, module, sessionId, publish,
  pause = ms => new Promise(resolve => setTimeout(resolve, ms)), signal }) {
  for (let attempt = 0; attempt < 1200 && !signal?.aborted; attempt += 1) {
    if (typeof exports?.GwnativeCharacterReadiness !== 'function') return false;
    if (exports.GwnativeCharacterReadiness() === 3) {
      const roster = readCharacterRoster(exports, module);
      if (roster) {
        await publish({ sessionId, names: roster.map(entry => entry.name) });
        return true;
      }
    }
    await pause(100);
  }
  return false;
}
