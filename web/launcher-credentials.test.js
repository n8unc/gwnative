import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import vm from 'node:vm';

import { prepareLauncherCredentials } from './launcher-credentials.js';

function fakeExports(setter) {
  const memory = new WebAssembly.Memory({ initial: 1 });
  let next = 16;
  return { memory, malloc(bytes) { const pointer = next; next += bytes; return pointer; }, free() {},
    GwnativeSetLauncherCredentialsAvailable: setter, GwnativeSetLauncherAccountName() {} };
}

describe('launcher credential capability', () => {
  it('resets before awaiting saved credentials and enables after a valid read', async () => {
    const events = [];
    let release;
    const pending = new Promise((resolve) => { release = resolve; });
    const result = prepareLauncherCredentials({
      managed: true,
      readSaved: async () => { events.push('read'); return pending; },
        exports: fakeExports((value) => events.push(value)),
      log: (message) => events.push(message),
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(events, [0, 'read']);
    release({ username: 'user with spaces"', password: 'p äss\'word' });
    assert.equal(await result, true);
    assert.deepEqual(events, [0, 'read', 1, 'launcher credentials ready']);
  });

  it('does not read or enable for unmanaged games', async () => {
    let reads = 0;
    const values = [];
    assert.equal(await prepareLauncherCredentials({
      managed: false,
      readSaved: async () => { reads += 1; return { username: 'u', password: 'p' }; },
      exports: fakeExports((value) => values.push(value)),
      log() {},
    }), false);
    assert.equal(reads, 0);
    assert.deepEqual(values, [0]);
  });

  it('allocates retained UTF-16 account name before enabling the gate', async () => {
    const events = [];
    const exports = fakeExports((value) => events.push(value));
    let pointer = 0;
    exports.GwnativeSetLauncherAccountName = (value) => { pointer = value; };
    assert.equal(await prepareLauncherCredentials({
      managed: true,
      readSaved: async () => ({ username: '𝄞 user\\"', password: 'dummy' }),
      exports,
      log: (message) => events.push(message),
    }), true);
    const heap = new Uint16Array(exports.memory.buffer);
    let value = '';
    for (let index = pointer / 2; heap[index]; index += 1) value += String.fromCharCode(heap[index]);
    assert.equal(value, '𝄞 user\\"');
    assert.deepEqual(events, [0, 1, 'launcher credentials ready']);
  });

  it('clears the native name before freeing it after setup failure', async () => {
    const calls = [];
    const exports = fakeExports((value) => { calls.push(['available', value]); if (value === 1) throw new Error('fixture'); });
    exports.GwnativeSetLauncherAccountName = (value) => calls.push(['name', value]);
    let freed = 0;
    exports.free = () => { freed += 1; };
    assert.equal(await prepareLauncherCredentials({
      managed: true,
      readSaved: async () => ({ username: 'fixture', password: 'dummy' }),
      exports,
      log() {},
    }), false);
    assert.deepEqual(calls, [
      ['available', 0], ['name', 16], ['available', 1], ['available', 0], ['name', 0],
    ]);
    assert.equal(freed, 1);
  });

  it('stays disabled for missing or invalid credentials', async () => {
    for (const saved of [null, {}, { username: '', password: 'p' }, { username: 'u', password: '' }, { username: 1, password: 'p' }]) {
      const values = [];
      assert.equal(await prepareLauncherCredentials({
        managed: true,
        readSaved: async () => saved,
        exports: fakeExports((value) => values.push(value)),
        log() {},
      }), false);
      assert.deepEqual(values, [0]);
    }
  });

  it('handles failed reads, missing exports, and setter failures without exposing errors', async () => {
    const logs = [];
    assert.equal(await prepareLauncherCredentials({
      managed: true,
      readSaved: async () => { throw new Error('secret-shaped failure'); },
      exports: {},
      log: (message) => logs.push(message),
    }), false);
    assert.equal(await prepareLauncherCredentials({
      managed: true,
      readSaved: async () => ({ username: 'u', password: 'p' }),
      exports: { GwnativeSetLauncherCredentialsAvailable() { throw new Error('secret-shaped failure'); } },
      log: (message) => logs.push(message),
    }), false);
    assert.ok(logs.every((message) => message === 'launcher credentials unavailable'));
  });

  it('falls back to manual login when the host read stalls', async () => {
    const values = [];
    const logs = [];
    assert.equal(await prepareLauncherCredentials({
      managed: true,
      readSaved: () => new Promise(() => {}),
      exports: fakeExports((value) => values.push(value)),
      log: (message) => logs.push(message),
      timeoutMs: 0,
    }), false);
    assert.deepEqual(values, [0]);
    assert.deepEqual(logs, ['launcher credentials unavailable']);
  });

  it('keeps the real harness boundary ordered and protects managed store/clear callbacks', () => {
    const source = readFileSync(new URL('./harness.js', import.meta.url), 'utf8');
    const prepare = source.indexOf('await host.prepareLauncherCredentials({');
    const success = source.indexOf('success(result.instance, result.module);', prepare);
    assert.ok(prepare >= 0);
    assert.ok(success > prepare, 'WASM success must wait for credential preparation');
    const storeGuard = source.indexOf('if (window.__gwnativeManagedAccount === true) {', source.indexOf('async storeCredentials'));
    const clearGuard = source.indexOf('if (window.__gwnativeManagedAccount === true) return;', source.indexOf('async clearCredentials'));
    assert.ok(storeGuard > source.indexOf('async storeCredentials'));
    assert.ok(clearGuard > source.indexOf('async clearCredentials'));
    assert.match(source.slice(storeGuard, storeGuard + 180), /protectCredentials\(\{ username, password \}\)/);
  });

  it('awaits the exact harness instantiateWasm boundary before success', async () => {
    const source = readFileSync(new URL('./harness.js', import.meta.url), 'utf8');
    const start = source.indexOf('instantiateWasm(imports, success) {');
    const end = source.indexOf('\n  // Both generated glue files', start);
    const method = source.slice(start, end).replace(/,\s*$/, '');
    const events = [];
    let release;
    const pending = new Promise((resolve) => { release = resolve; });
    let success;
    const sandbox = {
      Module: { canvas: {} },
      window: { __gwnativeManagedAccount: true },
      performance: {
        mark() {},
        measure() { return { duration: 0 }; },
      },
      host: {
        installGameAudioResumeLifecycle() {},
        installGraphics() {},
        installMemorySensor() { return () => {}; },
        installTemplateSave() {},
        prepareLauncherCredentials,
      },
      runtimeLifecycle: {
        instantiate: async () => ({ instance: {
          exports: fakeExports((value) => events.push(value)),
        }, module: {} }),
      },
      readSaved: async () => pending,
      success: (...args) => { success = args; events.push('success'); },
      frameAudit: {},
      log() {}, status() {}, releaseStage() {},
      runtimeFailedBeforeProof() {},
      diag: null, readHeap: null, bootProof: null, bootRescueActive: true,
      document: {}, fetch: async () => { throw new Error('network disabled'); },
      WebAssembly,
    };
    const context = vm.createContext(sandbox);
    const instantiate = vm.runInContext(`({${method}}).instantiateWasm`, context);
    instantiate({}, sandbox.success);
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(events, [0]);
    assert.equal(success, undefined);
    release({ username: 'dummy user', password: 'dummy password' });
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(events, [0, 1, 'success']);
    assert.ok(success);
  });

  it('executes managed secureStorage store and clear without host writes', async () => {
    const source = readFileSync(new URL('./harness.js', import.meta.url), 'utf8');
    const start = source.indexOf('secureStorage: {');
    const end = source.indexOf('\n\n  // No federated auth', start);
    const object = source.slice(start, end).replace(/,\s*$/, '');
    const writes = [];
    const sandbox = {
      window: { __gwnativeManagedAccount: true },
      performance: { now: () => 0 },
      saved: Promise.resolve({ username: 'dummy user', password: 'dummy password' }),
      readSaved: () => sandbox.saved,
      protectCredentials(value) { return value; },
      credentials() { writes.push('host-write'); throw new Error('network disabled'); },
      log() {},
    };
    const context = vm.createContext(sandbox);
    const storage = vm.runInContext(`({${object}}).secureStorage`, context);
    await storage.storeCredentials('replacement', 'replacement');
    await storage.clearCredentials();
    assert.deepEqual(await storage.getCredentials(), { username: 'dummy user', password: 'dummy password' });
    assert.deepEqual(writes, []);
  });
});
