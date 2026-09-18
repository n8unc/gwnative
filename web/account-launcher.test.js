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
  for (const match of readFileSync(new URL('../ui/launcher.html', import.meta.url), 'utf8').matchAll(/id="([^"]+)"/g)) {
    if (!nodes.has(match[1])) nodes.set(match[1], element());
  }
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
  await edit.onclick();
  assert.equal(form.nodes.get('auto-login').checked, false);
  form.nodes.get('nickname').value = 'Main renamed';
  assert.equal((await submit(form)).autoLogin, false);

  await edit.onclick();
  form.nodes.get('auto-login').checked = true;
  form.nodes.get('auto-login').onchange();
  assert.equal((await submit(form)).autoLogin, true);
});

test('password removal and email change submit auto-login off', async () => {
  const account = { id: 'main', nickname: 'Main', email: 'main@example.test', hasPassword: true, autoLogin: true, autoLaunch: false, status: 'ready' };
  const form = launcherForm([account]);
  await form.flush();
  const edit = form.created.find((node) => node.tag === 'button' && node.textContent === 'Edit');
  await edit.onclick();
  form.nodes.get('remove-password').checked = true;
  form.nodes.get('remove-password').onchange();
  assert.equal((await submit(form)).autoLogin, false);

  await edit.onclick();
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


test('Account launch settings and fixed geometry serialize without changing live status', async () => {
  const account = { id: 'main', nickname: 'Main', email: 'main@example.test', status: 'running', busy: true, launchPreferences: { muted: true, frameRateLimit: { limit: 90 }, windowMode: 'fullscreen', preferredCharacter: 'Devona', texturePackIds: ['pack-one'] }, windowPreferences: { fixedLaunchFrame: { x: 12, y: 34, width: 1000, height: 700 } } };
  const form = launcherForm([account]); await form.flush();
  await form.created.find(node => node.tag === 'button' && node.textContent === 'Edit').onclick();
  assert.equal(form.nodes.get('launch-sound').value, 'muted');
  assert.equal(form.nodes.get('launch-fps').value, '90');
  assert.equal(form.nodes.get('capture-layout').disabled, false);
  assert.match(form.nodes.get('next-launch-hint').textContent, /next launch/);
  form.nodes.get('launch-sound').value = 'on';
  form.nodes.get('launch-fps').value = '';
  form.nodes.get('launch-mode').value = 'windowed';
  form.nodes.get('preferred-character').value = '';
  const saved = await submit(form);
  assert.deepEqual(saved.launchPreferences, { muted: false, frameRateLimit: 'default', windowMode: 'windowed', preferredCharacter: null, texturePackIds: ['pack-one'] });
  assert.deepEqual(saved.fixedLaunchFrame, { x: 12, y: 34, width: 1000, height: 700 });
  assert.equal(account.status, 'running');
});

test('profile-scoped last-seen names populate preferred-character suggestions only', async () => {
  const account = { id: 'main', nickname: 'Main', email: 'main@example.test', status: 'ready', lastSeenCharacters: ['Devona', 'Cynn'] };
  const form = launcherForm([account]); await form.flush();
  await form.created.find(node => node.tag === 'button' && node.textContent === 'Edit').onclick();
  const suggestions = form.nodes.get('last-seen-characters');
  assert.equal(suggestions.children?.length ?? 0, 0);
  assert.deepEqual(
    form.created.filter(node => node.tag === 'option').map(node => [node.value, node.label]),
    [['Devona', 'Last seen — Devona'], ['Cynn', 'Last seen — Cynn']]
  );
});

test('Restore last layout clears fixed preference on next save', async () => {
  const account = { id: 'main', nickname: 'Main', email: 'main@example.test', status: 'ready', windowPreferences: { fixedLaunchFrame: { x: 0, y: 0, width: 900, height: 600 } } };
  const form = launcherForm([account]); await form.flush();
  await form.created.find(node => node.tag === 'button' && node.textContent === 'Edit').onclick();
  form.nodes.get('launch-layout').value = 'restore'; form.nodes.get('launch-layout').onchange();
  assert.equal(form.nodes.get('layout-values').hidden, true);
  assert.equal((await submit(form)).fixedLaunchFrame, null);
});
