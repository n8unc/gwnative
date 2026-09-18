import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  applyClientLimits,
  createRuntimeLifecycle,
  deliverRuntimeProof,
  postRuntimeState,
  readRuntimePlan,
  selectClient,
  supportsJspi,
  transitionRuntimeFailure,
} from './client-runtime.js';

const workingJspi = {
  Module: class {},
  Instance: class {
    constructor(_module, imports) {
      this.exports = { g: () => imports.e.f.operation() };
    }
  },
  Suspending: class {
    constructor(operation) {
      this.operation = operation;
    }
  },
  promising: (operation) => async () => operation(),
};

describe('client runtime selection', () => {
  it('uses Asyncify when the WKWebView has no JSPI', async () => {
    assert.equal(await supportsJspi({}), false);
    assert.deepEqual(
      await selectClient({}),
      { mode: 'asyncify', glue: 'Gw.js', wasm: 'Gw.wasm' },
    );
  });

  it('uses JSPI only after a functional suspend/resume round trip', async () => {
    assert.equal(await supportsJspi(workingJspi), true);
    assert.deepEqual(
      await selectClient(workingJspi),
      { mode: 'jspi', glue: 'Gw.jspi.js', wasm: 'Gw.jspi.wasm' },
    );
  });

  it('falls back when JSPI is present but does not work', async () => {
    const broken = { ...workingJspi, promising: () => async () => 41 };
    assert.equal(await supportsJspi(broken), false);
    assert.equal((await selectClient(broken)).mode, 'asyncify');
  });

  it('falls back when a partial JSPI implementation never resumes', async () => {
    const stuck = {
      ...workingJspi,
      promising: () => () => new Promise(() => {}),
    };
    assert.equal(await supportsJspi(stuck, 1), false);
  });

  it('can force Asyncify for runner coverage', async () => {
    assert.equal((await selectClient({}, 'asyncify')).mode, 'asyncify');
  });

  it('refuses to force JSPI in an incompatible WKWebView', async () => {
    await assert.rejects(
      selectClient({}, 'jspi'),
      /failed its suspend\/resume probe/,
    );
  });

  it('selects Asyncify in a fresh realm after the exact JSPI runtime failed', async () => {
    assert.equal(
      (await selectClient(workingJspi, undefined, { failedOfficial: ['jspi'] })).mode,
      'asyncify',
    );
    assert.equal(
      (await selectClient(workingJspi, 'jspi', { failedOfficial: ['jspi'] })).mode,
      'asyncify',
      'a bring-up preference cannot create a persisted crash loop',
    );
  });

  it('refuses an exhausted or force-selected failed official runtime', async () => {
    await assert.rejects(
      selectClient(workingJspi, undefined, { failedOfficial: ['jspi', 'asyncify'] }),
      /No compatible official runtime remains/,
    );
    await assert.rejects(
      selectClient(workingJspi, 'asyncify', { failedOfficial: ['asyncify'] }),
      /forced Asyncify runtime already failed/,
    );
  });

  it('validates the host runtime plan before using it', async () => {
    const response = (status) => ({
      ok: true,
      status,
    });
    assert.deepEqual(
      await readRuntimePlan({
        fetch: async () => response(221),
      }),
      { failedOfficial: ['jspi'] },
    );
    await assert.rejects(
      readRuntimePlan({ fetch: async () => response(227) }),
      /invalid runtime plan/,
    );
  });

  it('decodes bodyless runtime transition acknowledgements', async () => {
    const send = (status) => async () => ({ ok: true, status });
    assert.deepEqual(
      await postRuntimeState('__runtime-failed', {}, { fetch: send(224) }),
      { outcome: 'try-runtime', runtime: 'asyncify' },
    );
    assert.deepEqual(
      await postRuntimeState('__runtime-failed', {}, { fetch: send(225) }),
      { outcome: 'predecessor-restored' },
    );
    assert.deepEqual(
      await postRuntimeState('__runtime-failed', {}, { fetch: send(226) }),
      { outcome: 'exhausted' },
    );
  });

  it('persists an official failure before requesting a fresh WKWebView', async () => {
    const order = [];
    const launch = { runtime: 'jspi', mode: 'original', nonce: 'exact' };
    const result = await transitionRuntimeFailure(launch, {
      post: async (path, body) => {
        order.push(['persist', path, body]);
        return { outcome: 'try-runtime', runtime: 'asyncify' };
      },
      relaunch: async () => order.push(['relaunch']),
    });
    assert.deepEqual(result, { outcome: 'try-runtime', runtime: 'asyncify' });
    assert.deepEqual(order, [
      ['persist', '__runtime-failed', { launch }],
      ['relaunch'],
    ]);
  });

  it('retries the exact runtime failure after a lost acknowledgement', async () => {
    const launch = { runtime: 'jspi', transformed: false, nonce: 'exact' };
    const claims = [];
    let relaunched = 0;
    await transitionRuntimeFailure(launch, {
      delays: [0, 0],
      post: async (path, body) => {
        claims.push([path, body]);
        if (claims.length === 1) throw new Error('acknowledgement lost');
        return { outcome: 'try-runtime', runtime: 'asyncify' };
      },
      relaunch: async () => { relaunched += 1; },
    });
    assert.equal(claims.length, 2);
    assert.ok(claims.every(([path, body]) => path === '__runtime-failed' && body.launch === launch));
    assert.equal(relaunched, 1);
  });

  it('does not relaunch when both runtimes are exhausted without a predecessor', async () => {
    let relaunched = false;
    await assert.rejects(
      transitionRuntimeFailure({}, {
        post: async () => ({ outcome: 'exhausted' }),
        relaunch: async () => { relaunched = true; },
      }),
      /no predecessor was removed/,
    );
    assert.equal(relaunched, false);
  });

  it('applies only the independently selected JSPI certificate', () => {
    const state = {
      __gwnativeRuntimeCapabilities: {
        jspi: {
          build: 'certified-jspi-build',
          templateSave: 'ready',
          enhancements: 'ready',
          enhancementManifest: { familyId: 'jspi-asyncify-pair' },
        },
        asyncify: {
          build: 'certified-asyncify-build',
          templateSave: 'ready',
          enhancements: 'ready',
          enhancementManifest: { familyId: 'jspi-asyncify-pair' },
        },
      },
    };
    applyClientLimits(
      { mode: 'jspi', glue: 'Gw.jspi.js', wasm: 'Gw.jspi.wasm' },
      { nativeCursor: true, targetReadout: true },
      state,
    );
    assert.equal(state.__gwnativeTemplateSave, 'ready');
    assert.equal(state.__gwnativeClientBuild, 'certified-jspi-build');
    assert.equal(state.__gwnativeEnhancements, 'ready');
    assert.deepEqual(state.__gwnativeEnhancementManifest, {
      familyId: 'jspi-asyncify-pair',
    });
  });

  it('does not inherit JSPI facts when Asyncify is selected', () => {
    const state = {
      __gwnativeRuntimeCapabilities: {
        jspi: {
          build: 'jspi',
          templateSave: 'ready',
          enhancements: 'ready',
          enhancementManifest: { runtime: 'jspi' },
        },
        asyncify: {
          build: 'asyncify',
          templateSave: 'ready',
          enhancements: 'ready',
          enhancementManifest: { runtime: 'asyncify' },
        },
      },
    };
    applyClientLimits(
      { mode: 'asyncify', glue: 'Gw.js', wasm: 'Gw.wasm' },
      { nativeCursor: true, targetReadout: false },
      state,
    );
    assert.equal(state.__gwnativeTemplateSave, 'ready');
    assert.equal(state.__gwnativeClientBuild, 'asyncify');
    assert.equal(state.__gwnativeEnhancements, 'ready');
    assert.deepEqual(state.__gwnativeEnhancementManifest, { runtime: 'asyncify' });
  });

  it('fails closed when the selected artifact has no certificate', () => {
    const state = {};
    applyClientLimits(
      { mode: 'asyncify', glue: 'Gw.js', wasm: 'Gw.wasm' },
      { nativeCursor: true, targetReadout: false },
      state,
    );
    assert.equal(state.__gwnativeTemplateSave, 'uncertified');
    assert.equal(state.__gwnativeEnhancements, 'uncertified');
    assert.equal(state.__gwnativeEnhancementManifest, null);
  });

  it('does not let runtime-state persistence hold client startup', async () => {
    const neverAnswers = (_path, { signal }) => new Promise((_resolve, reject) => {
      signal.addEventListener('abort', () => {
        reject(new DOMException('timed out', 'AbortError'));
      });
    });
    await assert.rejects(
      postRuntimeState('__runtime', {}, {
        fetch: neverAnswers,
        token: 'test',
        deadlineMs: 1,
      }),
      { name: 'AbortError' },
    );
  });

  it('retries a lost proof with identical data and bounded backoff', async () => {
    const calls = [];
    const waits = [];
    const body = { launch: { nonce: 'exact' } };
    const result = await deliverRuntimeProof('__booted', body, {
      delays: [0, 10, 20],
      wait: async (delay) => waits.push(delay),
      post: async (path, sent) => {
        calls.push({ path, sent });
        if (calls.length < 3) throw new Error('reply lost');
        return null;
      },
    });
    assert.equal(result, null);
    assert.deepEqual(waits, [10, 20]);
    assert.equal(calls.length, 3);
    assert.ok(calls.every(({ path, sent }) => path === '__booted' && sent === body));
  });

  it('retries a durably recorded initial attempt before executing glue', async () => {
    const claim = { runtime: 'jspi', build: null, transformed: false, nonce: 'exact' };
    const calls = [];
    await deliverRuntimeProof('__runtime', claim, {
      delays: [0, 0],
      post: async (path, body) => {
        calls.push([path, body]);
        if (calls.length === 1) throw new Error('204 lost');
        return null;
      },
    });
    assert.equal(calls.length, 2);
    assert.ok(calls.every(([path, body]) => path === '__runtime' && body === claim));
  });

  it('stops proof retry after the bounded attempt set', async () => {
    let attempts = 0;
    await assert.rejects(
      deliverRuntimeProof('__booted', {}, {
        delays: [0, 0, 0],
        post: async () => {
          attempts += 1;
          throw new Error('offline');
        },
      }),
      /offline/,
    );
    assert.equal(attempts, 3);
  });
});

describe('runtime lifecycle', () => {
  const deferred = () => {
    let resolve;
    let reject;
    const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
    return { promise, resolve, reject };
  };

  const lifecycleFixture = (overrides = {}) => {
    const target = {
      __gwnativeTemplateSave: 'ready',
      __gwnativeClientBuild: 'build-a',
      __gwnativeLaunchNonce: 'nonce-a',
      ...overrides.target,
    };
    const calls = [];
    const proofOptions = {
      delays: [0, 0],
      post: async (path, body) => {
        calls.push([path, body]);
        return null;
      },
      ...overrides.proofOptions,
    };
    const lifecycle = createRuntimeLifecycle({
      client: { mode: 'jspi', wasm: 'Gw.jspi.wasm' },
      target,
      relaunch: overrides.relaunch ?? (async () => {}),
      proofOptions,
      onTransition: overrides.onTransition,
      onOriginalFallback: overrides.onOriginalFallback,
    });
    return { lifecycle, target, calls };
  };

  it('records exact attempt before invoking glue and coalesces starts', async () => {
    const { lifecycle, target, calls } = lifecycleFixture();
    const events = [];
    const first = lifecycle.start(() => events.push('glue'));
    const second = lifecycle.start(() => events.push('duplicate-glue'));
    const launch = await first;
    assert.equal(second, first);
    assert.deepEqual(events, ['glue']);
    assert.deepEqual(calls, [['__runtime', launch]]);
    assert.equal(target.__gwnativeLaunchIdentity, launch);
  });

  it('does not run glue when initial proof cannot be recorded', async () => {
    const { lifecycle, target } = lifecycleFixture({
      proofOptions: { post: async () => { throw new Error('offline'); } },
    });
    let glued = false;
    await assert.rejects(lifecycle.start(() => { glued = true; }), /offline/);
    assert.equal(glued, false);
  });

  it('does not release stale glue when failure races pending start proof', async () => {
    const gate = deferred();
    let glued = false;
    let relaunched = 0;
    const { lifecycle, calls } = lifecycleFixture({
      relaunch: async () => { relaunched += 1; },
      proofOptions: { post: async (path, body) => { calls.push([path, body]); return gate.promise; } },
    });
    const started = lifecycle.start(() => { glued = true; });
    const failed = lifecycle.fail(new Error('startup failure'));
    gate.resolve(null);
    await assert.rejects(started, /startup failure/);
    await assert.rejects(failed, /startup failure/);
    assert.equal(glued, false);
    assert.equal(relaunched, 0);
    assert.deepEqual(calls.map(([path]) => path), ['__runtime']);
  });

  it('uses official failure transition for an untransformed attempt', async () => {
    let relaunches = 0;
    const { lifecycle } = lifecycleFixture({
      target: { __gwnativeTemplateSave: 'off', __gwnativeClientBuild: null },
      relaunch: async () => { relaunches += 1; },
      proofOptions: {
        post: async (path) => path === '__runtime'
          ? null
          : { outcome: 'predecessor-restored' },
      },
    });
    await lifecycle.start();
    await lifecycle.fail(new Error('official failure'));
    assert.equal(relaunches, 1);
  });

  it('does not relaunch or notify transition for exhausted, invalid, or failed persistence', async () => {
    for (const response of [{ outcome: 'exhausted' }, { outcome: 'unexpected' }]) {
      let relaunches = 0;
      let transitions = 0;
      const { lifecycle } = lifecycleFixture({
        target: { __gwnativeTemplateSave: 'off', __gwnativeClientBuild: null },
        onTransition: () => { transitions += 1; },
        relaunch: async () => { relaunches += 1; },
        proofOptions: { post: async (path) => path === '__runtime' ? null : response },
      });
      await lifecycle.start();
      await assert.rejects(lifecycle.fail(new Error('official failure')));
      assert.equal(relaunches, 0);
      assert.equal(transitions, 0);
    }
    let relaunches = 0;
    let transitions = 0;
    const { lifecycle } = lifecycleFixture({
      target: { __gwnativeTemplateSave: 'off', __gwnativeClientBuild: null },
      onTransition: () => { transitions += 1; },
      relaunch: async () => { relaunches += 1; },
      proofOptions: { post: async (path) => path === '__runtime' ? null : (() => { throw new Error('proof failed'); })() },
    });
    await lifecycle.start();
    await assert.rejects(lifecycle.fail(new Error('official failure')));
    assert.equal(relaunches, 0);
    assert.equal(transitions, 0);
  });

  it('coalesces duplicate pre-frame transformed failures and transitions once', async () => {
    const transitions = [];
    let relaunched = 0;
    const { lifecycle } = lifecycleFixture({
      onTransition: (error) => transitions.push(error.message),
      relaunch: async () => { relaunched += 1; },
      proofOptions: {
        post: async (path) => path === '__runtime' ? null : { outcome: 'try-runtime', runtime: 'asyncify' },
      },
    });
    await lifecycle.start();
    const first = lifecycle.fail(new Error('transformed failure'));
    const second = lifecycle.fail(new Error('duplicate failure'));
    await Promise.all([first, second]);
    assert.deepEqual(transitions, ['transformed failure']);
    assert.equal(relaunched, 1);
  });

  it('records transform failure and retries exact original wasm with captured identity', async () => {
    const fallbacks = [];
    const { lifecycle, target, calls } = lifecycleFixture({
      onOriginalFallback: (error) => fallbacks.push(error.message),
    });
    await lifecycle.start();
    const seen = [];
    const instantiation = lifecycle.instantiate(async (wasm) => {
      seen.push(wasm);
      if (seen.length === 1) throw new Error('transform rejected');
      return 'instance';
    });
    target.__gwnativeLaunchNonce = 'mutated';
    target.__gwnativeClientBuild = 'mutated-build';
    assert.equal(await instantiation, 'instance');
    assert.deepEqual(seen, ['Gw.jspi.wasm', 'Gw.jspi.wasm?gwnative-original=1']);
    assert.deepEqual(calls.map(([path, body]) => [path, body.nonce ?? body.launch?.nonce]), [
      ['__runtime', 'nonce-a'],
      ['__transform-failed', 'nonce-a'],
      ['__runtime', 'nonce-a'],
    ]);
    assert.deepEqual(fallbacks, ['transform rejected']);
    assert.equal(target.__gwnativeEnhancements, 'off');
    assert.equal(target.__gwnativeEnhancementManifest, null);
  });

  it('keeps valid fallback protocol when presentation callback throws', async () => {
    const seen = [];
    let relaunched = 0;
    const { lifecycle } = lifecycleFixture({
      onOriginalFallback: () => { throw new Error('presentation only'); },
      relaunch: async () => { relaunched += 1; },
    });
    await lifecycle.start();
    await lifecycle.instantiate(async (wasm) => {
      seen.push(wasm);
      if (seen.length === 1) throw new Error('transform rejected');
      return 'instance';
    });
    assert.deepEqual(seen, ['Gw.jspi.wasm', 'Gw.jspi.wasm?gwnative-original=1']);
    assert.equal(relaunched, 0);
  });

  it('does not retry original when transform proof cannot be recorded', async () => {
    const seen = [];
    let notified = 0;
    const { lifecycle, target } = lifecycleFixture({
      target: { __gwnativeTemplateSave: 'ready', __gwnativeEnhancements: 'ready', __gwnativeEnhancementManifest: { familyId: 'x' } },
      proofOptions: { post: async (path) => {
        if (path === '__transform-failed') throw new Error('transform proof failed');
        return null;
      } },
      onOriginalFallback: () => { notified += 1; },
    });
    await lifecycle.start();
    await assert.rejects(lifecycle.instantiate(async (wasm) => {
      seen.push(wasm);
      throw new Error('transform rejected');
    }), /transform proof failed/);
    assert.deepEqual(seen, ['Gw.jspi.wasm']);
    assert.equal(target.__gwnativeTemplateSave, 'ready');
    assert.equal(target.__gwnativeEnhancements, 'ready');
    assert.deepEqual(target.__gwnativeEnhancementManifest, { familyId: 'x' });
    assert.equal(notified, 0);
  });

  it('uses current identity for first-frame proof and never starts fallback after frame', async () => {
    const boot = deferred();
    const { lifecycle, calls } = lifecycleFixture({
      proofOptions: {
        post: async (path, body) => {
          calls.push([path, body]);
          if (path === '__booted') return boot.promise;
          return null;
        },
      },
    });
    await lifecycle.start();
    const frame = lifecycle.firstFrame();
    const failure = lifecycle.fail(new Error('late failure'));
    await assert.rejects(failure, /late failure/);
    assert.deepEqual(calls.map(([path]) => path), ['__runtime', '__booted']);
    boot.resolve(null);
    await frame;
    assert.equal(calls[1][1].launch, calls[0][1]);
  });

  it('coalesces duplicate first-frame notifications and rejects fallback after proof failure', async () => {
    let relaunches = 0;
    const { lifecycle } = lifecycleFixture({
      relaunch: async () => { relaunches += 1; },
      proofOptions: { post: async (path) => path === '__booted' ? Promise.reject(new Error('boot proof failed')) : null },
    });
    await lifecycle.start();
    const first = lifecycle.firstFrame();
    const second = lifecycle.firstFrame();
    assert.equal(first, second);
    await assert.rejects(first, /boot proof failed/);
    await assert.rejects(lifecycle.fail(new Error('late failure')), /late failure/);
    assert.equal(relaunches, 0);
  });

  it('fails before start without recording a later attempt', async () => {
    const calls = [];
    const { lifecycle, target } = lifecycleFixture({
      proofOptions: { post: async (path, body) => { calls.push([path, body]); return null; } },
    });
    await assert.rejects(lifecycle.fail(new Error('before start')), /before start/);
    let glued = false;
    await assert.rejects(lifecycle.start(() => { glued = true; }), /before start/);
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(calls, []);
    assert.equal(glued, false);
  });

  it('captures build and nonce before asynchronous proof can observe mutations', async () => {
    const gate = deferred();
    const { lifecycle, target, calls } = lifecycleFixture({
      proofOptions: { post: async (_path, body) => { calls.push(body); await gate.promise; } },
    });
    const started = lifecycle.start();
    target.__gwnativeClientBuild = 'drifted';
    target.__gwnativeLaunchNonce = 'drifted';
    gate.resolve();
    await started;
    assert.deepEqual(calls[0], {
      runtime: 'jspi', build: 'build-a', transformed: true, nonce: 'nonce-a',
    });
  });

  it('keeps valid transformed and official transitions when onTransition throws', async () => {
    for (const transformed of [true, false]) {
      let relaunches = 0;
      let transitions = 0;
      const { lifecycle } = lifecycleFixture({
        target: transformed ? {} : { __gwnativeTemplateSave: 'off', __gwnativeClientBuild: null },
        onTransition: () => { transitions += 1; throw new Error('presentation only'); },
        relaunch: async () => { relaunches += 1; },
        proofOptions: { post: async (path) => path === '__runtime' ? null : (transformed ? null : { outcome: 'predecessor-restored' }) },
      });
      await lifecycle.start();
      await lifecycle.fail(new Error('failure'));
      assert.equal(transitions, 1);
      assert.equal(relaunches, 1);
    }
  });

  it('records original identity on first frame after successful fallback', async () => {
    const calls = [];
    const { lifecycle, target } = lifecycleFixture({
      proofOptions: { post: async (path, body) => { calls.push([path, body]); return null; } },
    });
    await lifecycle.start();
    await lifecycle.instantiate(async (wasm) => wasm.includes('?') ? 'original' : Promise.reject(new Error('reject')));
    const frame = lifecycle.firstFrame();
    await frame;
    const original = calls.find(([path, body]) => path === '__runtime' && body.transformed === false)[1];
    assert.deepEqual(calls.at(-1)[1].launch, original);
    assert.equal(original.runtime, 'jspi');
    assert.equal(original.nonce, 'nonce-a');
    assert.equal(original.build, null);
    assert.equal(original.transformed, false);
    assert.equal(target.__gwnativeTemplateSave, 'failed');
    assert.equal(target.__gwnativeEnhancements, 'off');
  });
});
