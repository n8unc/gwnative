import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import vm from 'node:vm';

const context = vm.createContext({});
vm.runInContext(readFileSync(new URL('../ui/launcher.js', import.meta.url), 'utf8'), context);
const model = context.GWLauncherModel;

test('Play cannot duplicate a queued, starting, running, recovering or closing game', () => {
  for (const status of ['queued', 'preparing', 'starting', 'running', 'closing', 'recovering', 'attention', 'unavailable']) {
    assert.equal(model.canPlay({ status }), false, status);
  }
  assert.equal(model.canPlay({ status: 'ready' }), true);
  assert.equal(model.canPlay({ status: 'failed', busy: true }), false);
  assert.equal(model.canPlay({ status: 'failed', busy: false }), true);
});

test('batch selection skips running games even if selected before their state changed', () => {
  const rows = [{ id: 'a', status: 'ready' }, { id: 'b', status: 'running' }, { id: 'c', status: 'ready' }];
  assert.deepEqual(Array.from(model.selectedLaunches(rows, new Set(['a', 'b']))), ['a']);
});

test('removing password or changing email always disables credential eligibility', () => {
  assert.equal(model.passwordEligible({ saved: true, removed: true }), false);
  assert.equal(model.passwordEligible({ saved: true, typed: true, emailChanged: true }), false);
  assert.equal(model.passwordEligible({ saved: false, typed: false }), false);
  assert.equal(model.passwordEligible({ saved: true }), true);
  assert.equal(model.passwordEligible({ typed: true }), true);
});

test('unsupported auto-login stays disabled while existing saved choice is preserved', () => {
  assert.equal(model.autoLoginSupported, false);
  assert.equal(model.savedAutoLogin({ editing: { autoLogin: true }, adopting: null }), true);
  assert.equal(model.savedAutoLogin({ editing: { autoLogin: false }, adopting: null }), false);
  assert.equal(model.savedAutoLogin({ editing: { autoLogin: true }, adopting: null, removed: true }), false);
  assert.equal(model.savedAutoLogin({ editing: null, adopting: { hasPassword: true } }), false);
});
