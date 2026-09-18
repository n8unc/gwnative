import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import vm from 'node:vm';

const context = vm.createContext({});
vm.runInContext(readFileSync(new URL('../ui/launcher.js', import.meta.url), 'utf8'), context);
const model = context.GWLauncherModel;

function launcherForm(accounts = []) {
  const nodes = new Map();
  const created = [];
  const element = (tag = 'div') => ({
    tag, value: '', checked: false, disabled: false, hidden: false, textContent: '',
    append() {}, replaceChildren() {}, setAttribute() {}, focus() {}, showModal() {}, close() {},
    reset() {}, addEventListener() {}, classList: { toggle() {} },
  });
  for (const id of ['notice', 'launch-selected', 'selection-count', 'account-count', 'empty', 'accounts', 'update-status', 'check-updates', 'auto-login', 'remove-password', 'password', 'email', 'nickname', 'auto-launch', 'account-form', 'form-error', 'editor-title', 'remove-password-row', 'password-hint', 'remove-account', 'editor', 'save', 'cancel-edit', 'add', 'empty-add', 'adopt', 'quit-all', 'close-imports', 'confirmation', 'confirm-title', 'confirm-message', 'confirm-yes', 'confirm-cancel', 'confirm-options']) nodes.set(id, element());
  const posted = [];
  const document = {
    getElementById: (id) => nodes.get(id),
    createElement: (tag) => { const node = element(tag); created.push(node); return node; },
    createTextNode: (text) => ({ textContent: text }),
  };
  const sandbox = {
    document, setInterval() {}, setTimeout, clearTimeout, Promise, Map, Set, Error,
    window: { webkit: { messageHandlers: { launcher: { postMessage(raw) {
      const request = JSON.parse(raw);
      if (request.action === 'save') posted.push(request);
      queueMicrotask(() => sandbox.window.launcherReply({ id: request.id, result: request.action === 'snapshot' ? { accounts, retained: [], updateMessage: '' } : { ok: true } }));
    } } } } },
  };
  const context = vm.createContext(sandbox);
  vm.runInContext(readFileSync(new URL('../ui/launcher.js', import.meta.url), 'utf8'), context);
  return { nodes, created, posted, flush: () => new Promise((resolve) => setImmediate(resolve)) };
}

async function submit(form) {
  await form.nodes.get('account-form').onsubmit({ preventDefault() {} });
  await form.flush();
  return form.posted.at(-1);
}

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

test('auto-login support preserves existing saved choice while editing', () => {
  assert.equal(model.autoLoginSupported, true);
  assert.equal(model.savedAutoLogin({ editing: { autoLogin: true }, adopting: null }), true);
  assert.equal(model.savedAutoLogin({ editing: { autoLogin: false }, adopting: null }), false);
  assert.equal(model.savedAutoLogin({ editing: { autoLogin: true }, adopting: null, removed: true }), false);
  assert.equal(model.savedAutoLogin({ editing: null, adopting: { hasPassword: true } }), false);
});

test('new Account defaults auto-login on with password, and explicit off reaches native save', async () => {
  const form = launcherForm();
  await form.flush();
  form.nodes.get('add').onclick();
  form.nodes.get('nickname').value = 'Main';
  form.nodes.get('email').value = 'main@example.test';
  form.nodes.get('password').value = 'private password';
  form.nodes.get('password').oninput();
  assert.equal(form.nodes.get('auto-login').checked, true);
  form.nodes.get('auto-login').checked = false;
  form.nodes.get('auto-login').onchange();
  assert.equal((await submit(form)).autoLogin, false);
});

test('editing preserves off choice and submits user-enabled auto-login', async () => {
  const account = { id: 'main', nickname: 'Main', email: 'main@example.test', hasPassword: true, autoLogin: false, autoLaunch: false, status: 'ready' };
  const form = launcherForm([account]);
  await form.flush();
  const edit = form.created.find((node) => node.tag === 'button' && node.textContent === 'Edit');
  edit.onclick();
  assert.equal(form.nodes.get('auto-login').checked, false);
  form.nodes.get('nickname').value = 'Main renamed';
  assert.equal((await submit(form)).autoLogin, false);

  edit.onclick();
  form.nodes.get('auto-login').checked = true;
  form.nodes.get('auto-login').onchange();
  assert.equal((await submit(form)).autoLogin, true);
});

test('password removal and email change submit auto-login off', async () => {
  const account = { id: 'main', nickname: 'Main', email: 'main@example.test', hasPassword: true, autoLogin: true, autoLaunch: false, status: 'ready' };
  const form = launcherForm([account]);
  await form.flush();
  const edit = form.created.find((node) => node.tag === 'button' && node.textContent === 'Edit');
  edit.onclick();
  form.nodes.get('remove-password').checked = true;
  form.nodes.get('remove-password').onchange();
  assert.equal((await submit(form)).autoLogin, false);

  edit.onclick();
  form.nodes.get('email').value = 'changed@example.test';
  form.nodes.get('email').oninput();
  // Email changes present a confirmation; accept it through the generated dialog action.
  const pending = form.nodes.get('account-form').onsubmit({ preventDefault() {} });
  await form.flush();
  form.nodes.get('confirm-yes').onclick();
  await pending;
  await form.flush();
  assert.equal(form.posted.at(-1).autoLogin, false);
});
