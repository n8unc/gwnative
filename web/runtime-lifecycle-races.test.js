import assert from 'node:assert/strict';
import test from 'node:test';

import { createRuntimeLifecycle } from './client-runtime.js';

const client = { mode: 'jspi', wasm: 'Gw.jspi.wasm' };

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function target() {
  return {
    __gwnativeClientBuild: 'build-7',
    __gwnativeTemplateSave: 'ready',
    __gwnativeEnhancements: 'ready',
    __gwnativeEnhancementManifest: { version: 1 },
    __gwnativeLaunchNonce: 'nonce-7',
  };
}

test('cancels original retry when failure races its pending original proof', async () => {
  const originalEntered = deferred();
  const originalGate = deferred();
  const trace = [];
  const relaunches = [];
  let originalLoads = 0;
  let runtimeFailures = 0;
  const state = target();
  const proofOptions = {
    delays: [0, 0],
    post: async (path, body) => {
      trace.push([path, body]);
      if (path === '__runtime' && body.transformed === false) {
        originalEntered.resolve();
        await originalGate.promise;
        return null;
      }
      if (path === '__runtime-failed') {
        runtimeFailures += 1;
        return { outcome: 'predecessor-restored' };
      }
      return null;
    },
  };
  const lifecycle = createRuntimeLifecycle({
    client,
    target: state,
    relaunch: async () => relaunches.push('relaunch'),
    proofOptions,
  });

  await lifecycle.start(() => trace.push(['glue']));
  const instantiate = lifecycle.instantiate(async (url) => {
    trace.push(['load', url]);
    if (url === client.wasm) throw new Error('transformed failed');
    originalLoads += 1;
    return { instance: 'stale' };
  });
  await originalEntered.promise;
  const failure = lifecycle.fail('client stopped');
  assert.equal(trace.filter(([path]) => path === '__transform-failed').length, 1);
  assert.equal(relaunches.length, 0);
  assert.equal(originalLoads, 0);
  originalGate.resolve();
  await assert.rejects(instantiate, /client stopped/);
  await failure;

  assert.equal(runtimeFailures, 1);
  assert.equal(relaunches.length, 1);
  const failedClaim = trace.find(([path]) => path === '__runtime-failed')[1].launch;
  assert.deepEqual(failedClaim, {
    runtime: 'jspi',
    build: null,
    transformed: false,
    nonce: 'nonce-7',
  });
  assert.equal(originalLoads, 0);
});

test('coalesces transform proof when failure races transformed fallback', async () => {
  const transformEntered = deferred();
  const transformGate = deferred();
  const trace = [];
  const relaunches = [];
  let originalLoads = 0;
  const state = target();
  const lifecycle = createRuntimeLifecycle({
    client,
    target: state,
    relaunch: async () => relaunches.push('relaunch'),
    proofOptions: {
      delays: [0, 0],
      post: async (path, body) => {
        trace.push([path, body]);
        if (path === '__transform-failed') {
          transformEntered.resolve();
          await transformGate.promise;
        }
        return null;
      },
    },
  });

  await lifecycle.start(() => {});
  const instantiate = lifecycle.instantiate(async (url) => {
    trace.push(['load', url]);
    if (url === client.wasm) throw new Error('transformed failed');
    originalLoads += 1;
    return { instance: 'stale' };
  });
  await transformEntered.promise;
  const failure = lifecycle.fail('client stopped');
  assert.equal(relaunches.length, 0);
  assert.equal(trace.filter((entry) => entry[0] === '__runtime' && entry[1]?.transformed === false).length, 0);
  assert.equal(trace.filter((entry) => entry[0] === 'load' && entry[1] !== client.wasm).length, 0);
  transformGate.resolve();
  await assert.rejects(instantiate, /client stopped/);
  await failure;

  assert.equal(trace.filter(([path]) => path === '__transform-failed').length, 1);
  assert.equal(trace.filter((entry) => entry[0] === '__runtime' && entry[1]?.transformed === false).length, 0);
  assert.equal(trace.filter((entry) => entry[0] === 'load' && entry[1] !== client.wasm).length, 0);
  assert.equal(originalLoads, 0);
  assert.equal(relaunches.length, 1);
});

test('does not retry original runtime after original identity proof rejection', async () => {
  const trace = [];
  const relaunches = [];
  let originalLoads = 0;
  let fallbackNotices = 0;
  let transitions = 0;
  const state = target();
  const lifecycle = createRuntimeLifecycle({
    client,
    target: state,
    relaunch: async () => relaunches.push('relaunch'),
    onOriginalFallback: () => { fallbackNotices += 1; },
    onTransition: () => { transitions += 1; },
    proofOptions: {
      delays: [0, 0],
      post: async (path, body) => {
        trace.push([path, body]);
        if (path === '__runtime' && body.transformed === false) {
          throw new Error('original proof failed');
        }
        return null;
      },
    },
  });

  await lifecycle.start(() => {});
  await assert.rejects(lifecycle.instantiate(async (url) => {
    trace.push(['load', url]);
    if (url === client.wasm) throw new Error('transformed failed');
    originalLoads += 1;
    return { instance: 'stale' };
  }), /original proof failed/);

  assert.equal(fallbackNotices, 1);
  assert.equal(transitions, 0);
  assert.equal(relaunches.length, 0);
  assert.equal(originalLoads, 0);
  assert.equal(state.__gwnativeTemplateSave, 'failed');
  assert.equal(state.__gwnativeEnhancements, 'off');
  assert.equal(state.__gwnativeEnhancementManifest, null);
  assert.deepEqual(state.__gwnativeLaunchIdentity, {
    runtime: 'jspi',
    build: 'build-7',
    transformed: true,
    nonce: 'nonce-7',
  });
  assert.equal(trace.filter((entry) => entry[0] === '__runtime' && entry[1]?.transformed === false).length, 2);
});
