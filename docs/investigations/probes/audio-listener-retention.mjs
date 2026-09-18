import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

class ListenerTarget {
  #listeners = new Map();

  addEventListener(type, listener, options) {
    const entries = this.#listeners.get(type) ?? [];
    entries.push({ listener, once: options?.once === true });
    this.#listeners.set(type, entries);
  }

  get listenerCount() {
    return [...this.#listeners.values()].reduce((count, entries) => count + entries.length, 0);
  }

  dispatch(type) {
    const entries = this.#listeners.get(type) ?? [];
    this.#listeners.set(type, entries.filter((entry) => !entry.once));
    for (const { listener } of entries) listener();
  }
}

for (const name of ['Gw.js', 'Gw.jspi.js']) {
  const source = readFileSync(new URL(`../../../web/${name}`, import.meta.url), 'utf8');
  const autoResume = source.match(/var autoResumeAudioContext = \(ctx\) => \{[\s\S]*?\n  \};/);
  const destroy = source.match(/var _alcDestroyContext = \(contextId\) => \{[\s\S]*?\n  \};/);
  assert.ok(autoResume, `${name}: expected generated autoResumeAudioContext source`);
  assert.ok(destroy, `${name}: expected generated _alcDestroyContext source`);

  const document = new ListenerTarget();
  const canvas = new ListenerTarget();
  document.getElementById = (id) => id === 'canvas' ? canvas : null;
  const resume = new Function('document', `${autoResume[0]}; return autoResumeAudioContext;`)(document);

  for (let replacement = 0; replacement < 100; replacement++) {
    const context = { state: 'closed', resume() {} };
    resume(context);
    const AL = {
      contexts: { [replacement]: { audioCtx: context, deviceId: 1, interval: setInterval(() => {}, 1) } },
      currentCtx: null,
      alcErr: 0,
      deviceRefCounts: { 1: 1 },
      freeIds: [],
    };
    new Function('AL', `${destroy[0]}; return _alcDestroyContext;`)(AL)(replacement);
    assert.equal(AL.contexts[replacement], undefined, `${name}: destroy removed AL context`);
  }

  assert.equal(document.listenerCount, 300);
  assert.equal(canvas.listenerCount, 300);
  document.dispatch('keydown');
  canvas.dispatch('keydown');
  assert.equal(document.listenerCount + canvas.listenerCount, 400);
  document.dispatch('mousedown');
  canvas.dispatch('mousedown');
  assert.equal(document.listenerCount + canvas.listenerCount, 200);
  document.dispatch('touchstart');
  canvas.dispatch('touchstart');
  assert.equal(document.listenerCount + canvas.listenerCount, 0);
  console.log(`${name}: destroy removes AL bookkeeping but retains 400 DOM listener closures after 100 replacements plus desktop keydown; only all three event types release them`);
}
