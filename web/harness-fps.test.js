import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

function cappedScheduler({ fps = 30 } = {}) {
  const source = fs.readFileSync(new URL('./harness.js', import.meta.url), 'utf8');
  const start = source.indexOf('const launchOptions =');
  const end = source.indexOf('\nconst statusEl =', start);
  assert.ok(start >= 0 && end > start, 'harness frame scheduler bounds');
  const scheduler = source
    .slice(start, end)
    // Exercise steady-state cap directly; boot rescue has separate timing.
    .replace('let bootRescueActive = true;', 'let bootRescueActive = false;');
  let next = 1;
  const pending = new Map();
  const window = {
    __gwnativeLaunch: { fps },
    requestAnimationFrame(callback) {
      const handle = next++;
      pending.set(handle, callback);
      return handle;
    },
  };
  vm.runInNewContext(scheduler, {
    window,
    frameAudit: undefined,
    performance: { now: () => 0 },
    setTimeout: () => 0,
    clearTimeout: () => {},
  });
  return { pending, window };
}

test('capped steady-state RAF admits every callback from accepted native frame', () => {
  for (const fps of [30, 60]) {
    const { pending, window } = cappedScheduler({ fps });
    const main = [];
    const helper = [];
    const chain = (calls) => (timestamp) => {
      calls.push(timestamp);
      if (calls.length < 2) window.requestAnimationFrame(chain(calls));
    };
    const mainFirst = window.requestAnimationFrame(chain(main));
    const helperFirst = window.requestAnimationFrame(chain(helper));
    assert.equal(mainFirst, 1);
    assert.equal(helperFirst, 2);
    pending.get(mainFirst)(0);
    pending.get(helperFirst)(0);
    assert.deepEqual(main, [0]);
    assert.deepEqual(helper, [0]);

    // Both chains are deferred below the cap interval and each reschedules.
    pending.get(3)(10);
    pending.get(4)(10);
    assert.deepEqual(main, [0]);
    assert.deepEqual(helper, [0]);
    assert.ok(pending.has(5));
    assert.ok(pending.has(6));

    // A later accepted timestamp again runs both chains, regardless of order.
    pending.get(6)(34);
    pending.get(5)(34);
    assert.deepEqual(main, [0, 34]);
    assert.deepEqual(helper, [0, 34]);
  }
});

test('steady-state default hands native RAF handle through unchanged', () => {
  const { pending, window } = cappedScheduler({ fps: 0 });
  const handle = window.requestAnimationFrame(() => {});
  assert.equal(handle, 1);
  assert.ok(pending.has(handle));
});
