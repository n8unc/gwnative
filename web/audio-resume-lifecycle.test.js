import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { describe, it } from 'node:test';

const extract = (source, marker) => {
  const start = source.indexOf(marker);
  assert.notEqual(start, -1, `missing ${marker}`);
  const equals = source.indexOf('=', start) + 1;
  let braces = 0;
  let opened = false;
  for (let i = equals; i < source.length; i += 1) {
    if (source[i] === '{') { braces += 1; opened = true; }
    if (source[i] === '}') {
      braces -= 1;
      if (opened && braces === 0) return source.slice(equals, i + 1);
    }
  }
  throw new Error(`unterminated ${marker}`);
};

describe('generated audio resume seam', () => {
  it('executes both generated create functions through lifecycle hook', async () => {
    class Target extends EventTarget {
      constructor() { super(); this.live = new Set(); }
      addEventListener(type, listener, options) { this.live.add(listener); return super.addEventListener(type, listener, options); }
      removeEventListener(type, listener, options) { this.live.delete(listener); return super.removeEventListener(type, listener, options); }
    }
    const document = new Target();
    const canvas = new Target();
    document.getElementById = () => canvas;
    globalThis.window = globalThis;
    globalThis.addEventListener ??= () => {};
    const realSetInterval = globalThis.setInterval;
    globalThis.setInterval = (...args) => realSetInterval(...args).unref();
    globalThis.document = document;
    globalThis.navigator ??= {};
    class Context extends EventTarget {
      static built = [];
      constructor() { super(); this.state = 'suspended'; this.currentTime = 0; this.sampleRate = 48000; this.destination = {}; this.resumes = 0; this.reject = false; Context.built.push(this); }
      createGain() { return { gain: { value: 1, setTargetAtTime() {} }, connect() {} }; }
      resume() { this.resumes += 1; if (this.reject) { this.reject = false; return Promise.reject(new Error('blocked')); } this.state = 'running'; return Promise.resolve(); }
      close() { this.state = 'closed'; return Promise.resolve(); }
    }
    globalThis.AudioContext = Context;
    globalThis.autoResumeAudioContext = () => {};
    const audio = await import('./audio.js');
    audio.installGameAudio();
    audio.installGameAudioResumeLifecycle();

    for (const glue of ['Gw.js', 'Gw.jspi.js']) {
      const source = readFileSync(new URL('./fixtures/alc-create-context.txt', import.meta.url), 'utf8');
      const sandbox = vm.createContext({
        document,
        AudioContext: globalThis.AudioContext,
        HEAP32: new Int32Array(1),
        setInterval: () => 1,
        clearInterval,
        AL: {
          deviceRefCounts: { 1: 1 }, contexts: {}, currentCtx: null, freeIds: [],
          newId: (() => { let id = 1; return () => id++; })(),
          scheduleContextAudio() {}, updateContextGlobal() {}, alcErr: 0,
        },
      });
      sandbox.window = sandbox;
      globalThis.window = sandbox;
      vm.runInContext(`var autoResumeAudioContext = ${extract(source, 'var autoResumeAudioContext =')}`, sandbox);
      const original = sandbox.autoResumeAudioContext;
      audio.installGameAudioResumeLifecycle({ target: sandbox });
      assert.notEqual(sandbox.autoResumeAudioContext, original, `${glue}: global hook replaced`);
      const create = vm.runInContext(`(${extract(source, 'var _alcCreateContext =')})`, sandbox);
      const context = create(1, 0);
      assert.notEqual(context, 0, `${glue}: generated create returned context`);
      const actual = Context.built.at(-1);
      assert.equal(document.live.size + canvas.live.size, 6, `${glue}: six listeners installed`);
      document.dispatchEvent(new Event('keydown'));
      await Promise.resolve();
      assert.equal(actual.resumes, 1, `${glue}: gesture resumes context`);
      await Promise.resolve();
      assert.equal(document.live.size + canvas.live.size, 0, `${glue}: success removes listeners`);

      const retryId = create(1, 0);
      const retry = Context.built.at(-1);
      retry.reject = true;
      assert.equal(document.live.size + canvas.live.size, 6, `${glue}: retry listeners installed`);
      document.dispatchEvent(new Event('keydown'));
      await Promise.resolve();
      await Promise.resolve();
      assert.equal(retry.resumes, 1, `${glue}: failed resume attempted once`);
      assert.equal(document.live.size + canvas.live.size, 6, `${glue}: failed resume remains retriable`);
      document.dispatchEvent(new Event('keydown'));
      await Promise.resolve();
      await Promise.resolve();
      assert.equal(retry.resumes, 2, `${glue}: same gesture retries resume`);
      assert.equal(document.live.size + canvas.live.size, 0, `${glue}: retry success cleans listeners`);

      create(1, 0);
      const closed = Context.built.at(-1);
      assert.equal(document.live.size + canvas.live.size, 6, `${glue}: closed context listeners installed`);
      closed.state = 'closed';
      closed.dispatchEvent(new Event('statechange'));
      assert.equal(document.live.size + canvas.live.size, 0, `${glue}: closed context cleans listeners`);
    }
    globalThis.window = globalThis;
    globalThis.setInterval = realSetInterval;
  });

  it('keeps instantiateWasm lifecycle hook before graphics installation', () => {
    const source = readFileSync(new URL('./harness.js', import.meta.url), 'utf8');
    const instantiate = source.indexOf('instantiateWasm(imports, success) {');
    const graphics = source.indexOf('host.installGraphics({', instantiate);
    const hook = source.indexOf('host.installGameAudioResumeLifecycle?.();', instantiate);
    assert.ok(instantiate >= 0 && hook > instantiate && hook < graphics);
  });
});
