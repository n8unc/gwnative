import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  asyncifyStateReader,
  createPassiveObserver,
} from './passive-observer.js';
import { selectEnhancementFeatures } from './enhancement-capabilities.js';

describe('enhancement capability selection', () => {
  const cursorOnly = { featureMask: 1 };

  it('keeps an absent capability mask compatible with existing full manifests', () => {
    assert.deepEqual(
      selectEnhancementFeatures({}, { nativeCursor: true, targetReadout: true }),
      { nativeCursor: true, targetReadout: true, flags: 3, unavailable: 0 },
    );
  });

  it('keeps cursor active and withholds target readout on a cursor-only manifest', () => {
    assert.deepEqual(
      selectEnhancementFeatures(cursorOnly, { nativeCursor: true, targetReadout: true }),
      { nativeCursor: true, targetReadout: false, flags: 1, unavailable: 2 },
    );
  });

  it('selects only the reviewed cursor capability', () => {
    assert.deepEqual(
      selectEnhancementFeatures(cursorOnly, { nativeCursor: true, targetReadout: false }),
      { nativeCursor: true, targetReadout: false, flags: 1, unavailable: 0 },
    );
  });

  it('rejects an oversized capability mask instead of truncating it to known bits', () => {
    assert.throws(
      () => selectEnhancementFeatures(
        { featureMask: 0x1_0000_0001 },
        { nativeCursor: true, targetReadout: false },
      ),
      /unsupported enhancement capabilities/,
    );
  });

  it('does not treat an explicit null mask as a legacy full-capability manifest', () => {
    assert.throws(
      () => selectEnhancementFeatures(
        { featureMask: null },
        { nativeCursor: true, targetReadout: false },
      ),
      /unsupported enhancement capabilities/,
    );
  });

  it('does not allocate a target snapshot when a cursor-only manifest receives both requests', async () => {
    const original = {
      document: globalThis.document,
      window: globalThis.window,
      fetch: globalThis.fetch,
      setInterval: globalThis.setInterval,
    };
    const allocations = [];
    try {
      globalThis.document = {
        createElement: () => ({ getContext: () => ({}) }),
      };
      globalThis.window = { addEventListener() {} };
      globalThis.setInterval = () => 0;
      globalThis.fetch = async () => ({ ok: false });
      const { installEnhancements } = await import('./enhancements.js?cursor-capability-test');
      const memory = new WebAssembly.Memory({ initial: 2 });
      let nextPointer = 64;
      const instance = {
        exports: {
          memory,
          malloc(bytes) {
            allocations.push(bytes);
            const pointer = nextPointer;
            nextPointer += bytes + 16;
            return pointer;
          },
          free() {},
        },
      };
      const manifest = {
        snapshotAbi: 1,
        snapshotBytes: 64,
        cursorSnapshotAbi: 1,
        cursorSnapshotBytes: 4160,
        configBytes: 116,
        layoutWords: Array(29).fill(0),
        familyId: '0'.repeat(64),
        featureMask: 1,
      };
      await assert.rejects(
        installEnhancements(instance, manifest, {
          nativeCursor: true,
          targetReadout: true,
          runtime: 'jspi',
        }),
        /companion module is unavailable/,
      );
      assert.ok(allocations.includes(4160), 'cursor region is allocated');
      assert.ok(!allocations.includes(64), 'target snapshot is not allocated');
    } finally {
      globalThis.document = original.document;
      globalThis.window = original.window;
      globalThis.fetch = original.fetch;
      globalThis.setInterval = original.setInterval;
    }
  });
});

describe('passive enhancement observer', () => {
  it('observes JSPI without requiring an Asyncify export', () => {
    let reads = 0;
    const observe = createPassiveObserver(null, () => { reads += 1; });
    assert.equal(observe(), true);
    assert.equal(reads, 1);
  });

  it('never enters the companion while Asyncify unwinds or rewinds', () => {
    let state = 1;
    let reads = 0;
    const observe = createPassiveObserver(
      () => state,
      () => { reads += 1; },
    );
    assert.equal(observe(), false);
    state = 2;
    assert.equal(observe(), false);
    assert.equal(reads, 0);
  });

  it('does not mistake a missing Asyncify state export for JSPI', () => {
    assert.throws(
      () => asyncifyStateReader({}, 'asyncify'),
      /does not export asyncify_get_state/,
    );
    assert.equal(asyncifyStateReader({}, 'jspi'), null);
    assert.throws(() => asyncifyStateReader({}, 'later-runtime'), /unknown client runtime/);
  });

  it('requires Asyncify to remain Normal across the read', () => {
    let state = 0;
    const observe = createPassiveObserver(
      () => state,
      () => { state = 1; },
    );
    assert.equal(observe(), false);
  });

  it('fails closed when a state getter or companion read traps', () => {
    assert.equal(
      createPassiveObserver(
        () => { throw new Error('state'); },
        () => {},
      )(),
      false,
    );
    assert.equal(
      createPassiveObserver(null, () => { throw new Error('read'); })(),
      false,
    );
  });
});
