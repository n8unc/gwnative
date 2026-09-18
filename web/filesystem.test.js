import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { afterEach, describe, it } from 'node:test';

import { installGameFilesystem } from './filesystem.js';

const originalFS = globalThis.FS;
const originalIDBFS = globalThis.IDBFS;
const originalAddEventListener = globalThis.addEventListener;
const originalFlush = globalThis.gwFlushFilesystem;

afterEach(() => {
  globalThis.FS = originalFS;
  globalThis.IDBFS = originalIDBFS;
  globalThis.addEventListener = originalAddEventListener;
  globalThis.gwFlushFilesystem = originalFlush;
});

const generatedCalculateAt = (fs) => {
  const source = readFileSync(new URL('./fixtures/syscall-calculate-at.txt', import.meta.url), 'utf8');
  const method = source.slice(source.indexOf('calculateAt(dirfd, path, allowEmpty) {'))
    .trim()
    .replace(/^calculateAt/, 'function calculateAt')
    .replace(/}\s*$/, '}');
  return Function('PATH', 'FS', `${method}; return calculateAt;`)(
    { isAbs: (path) => path.startsWith('/') },
    fs,
  );
};

const runtime = (stored = []) => {
  let cwd = '/';
  const mounts = new Set();
  const calls = [];
  const directories = new Set(['/']);
  const files = new Set(stored);
  for (const file of files) {
    const parts = file.split('/').filter(Boolean);
    let current = '';
    for (const part of parts.slice(0, -1)) {
      current += `/${part}`;
      directories.add(current);
    }
  }
  const resolve = (path) =>
    path.startsWith('/')
      ? path
      : `${cwd}/${path}`.replaceAll('//', '/');
  const lookupPath = function (path) {
    calls.push(['lookupPath', path]);
    const absolute = resolve(path);
    if (directories.has(absolute)) {
      return { path: absolute, node: { mode: 16384 } };
    }
    throw Object.assign(new Error('missing'), { errno: 44 });
  };
  const fs = {
    analyzePath(path) {
      try {
        const lookup = this.lookupPath(path);
        return { error: 0, exists: true, path: lookup.path };
      } catch {
        return { error: 44, exists: false };
      }
    },
    lookupPath,
    mkdir(path) {
      const absolute = resolve(path);
      calls.push(['mkdir', path]);
      directories.add(absolute);
    },
    open(path, flags = 0) {
      const absolute = resolve(path);
      calls.push(['open', path]);
      const parent = absolute.slice(0, absolute.lastIndexOf('/')) || '/';
      if (!directories.has(parent)) throw Object.assign(new Error('missing parent'), { errno: 44 });
      if (flags & 64) files.add(absolute);
      if (!files.has(absolute)) throw Object.assign(new Error('missing file'), { errno: 44 });
      return { path: absolute };
    },
    unlink(path) { calls.push(['unlink', path]); },
    mknod(path) { calls.push(['mknod', path]); },
    rmdir(path) { calls.push(['rmdir', path]); },
    rename(from, to) { calls.push(['rename', from, to]); },
    symlink(from, to) { calls.push(['symlink', from, to]); },
    mount(_type, _options, path) {
      calls.push(['mount', _options, path]);
      mounts.add(path);
      directories.add(resolve(path));
    },
    mkdirTree(path) {
      const absolute = resolve(path);
      calls.push(['mkdirTree', path]);
      const parts = absolute.split('/').filter(Boolean);
      let current = '';
      for (const part of parts) {
        current += `/${part}`;
        directories.add(current);
      }
    },
    chdir(path) {
      calls.push(['chdir', path]);
      cwd = resolve(path);
    },
    cwd() { return cwd; },
    syncfs(_populate, callback) {
      calls.push(['syncfs', _populate]);
      callback();
    },
  };
  return { fs, mounts, calls };
};

describe('persistent filesystem path boundary', () => {
  it('keeps generated relative app: lookup at mounted root after host chdir', async () => {
    const { fs, mounts, calls } = runtime();
    globalThis.FS = fs;
    globalThis.IDBFS = {};
    globalThis.addEventListener = () => {};
    const module = {
      addRunDependency() {},
      removeRunDependency() {},
    };
    installGameFilesystem({ module, failed: assert.fail, log() {} });
    module.preRun();
    await new Promise((resolve) => setImmediate(resolve));

    assert.deepEqual([...mounts], ['app:']);
    assert.equal(fs.analyzePath('app:').error, 0);
    assert.deepEqual(calls.filter(([name]) => name === 'mount'), [
      ['mount', { autoPersist: true }, 'app:'],
    ]);
    assert.deepEqual(calls.filter(([name]) => name === 'syncfs'), [
      ['syncfs', true],
      ['syncfs', false],
    ]);
  });

  it('matches generated init callback without nesting a second app mount', async () => {
    const { fs, mounts, calls } = runtime();
    globalThis.FS = fs;
    globalThis.IDBFS = {};
    globalThis.addEventListener = () => {};
    const module = {
      addRunDependency() {},
      removeRunDependency() {},
    };
    installGameFilesystem({ module, failed: assert.fail, log() {} });
    module.preRun();
    await new Promise((resolve) => setImmediate(resolve));

    // This mirrors the generated client's mount decision (ASM_CONST 2658112):
    // an existing app: mount invokes completion and does not mount again.
    if (fs.analyzePath('app:').error) {
      fs.mkdir('app:');
      fs.mount(globalThis.IDBFS, { autoPersist: true }, 'app:');
    }

    assert.deepEqual([...mounts], ['app:']);
    assert.equal(calls.filter(([name]) => name === 'mount').length, 1);
  });

  it('normalizes Windows app paths while preserving ordinary relative paths', async () => {
    const { fs, calls } = runtime();
    globalThis.FS = fs;
    globalThis.IDBFS = {};
    globalThis.addEventListener = () => {};
    const module = {
      addRunDependency() {},
      removeRunDependency() {},
    };
    installGameFilesystem({ module, failed: assert.fail, log() {} });
    module.preRun();
    await new Promise((resolve) => setImmediate(resolve));

    fs.lookupPath('\\app:\\Templates\\Skills');
    fs.lookupPath('app:/Templates/Equipment');
    fs.lookupPath('Templates/Skills');
    fs.rename('\\app:\\old.dat', '\\app:\\new.dat');

    assert.deepEqual(calls.filter(([name]) => name === 'lookupPath').slice(-3), [
      ['lookupPath', '/app:/Templates/Skills'],
      ['lookupPath', '/app:/Templates/Equipment'],
      ['lookupPath', 'Templates/Skills'],
    ]);
    assert.deepEqual(calls.at(-1), ['rename', '/app:/old.dat', '/app:/new.dat']);
  });

  it('normalizes a direct app: Gw.dat open at mounted root after changing cwd', async () => {
    const { fs, calls } = runtime();
    globalThis.FS = fs;
    globalThis.IDBFS = {};
    globalThis.addEventListener = () => {};
    const module = {
      addRunDependency() {},
      removeRunDependency() {},
    };
    installGameFilesystem({ module, failed: assert.fail, log() {} });
    module.preRun();
    await new Promise((resolve) => setImmediate(resolve));

    fs.open('app:/Gw.dat', 64);

    assert.deepEqual(calls.at(-1), ['open', '/app:/Gw.dat']);
    assert.ok(!calls.some(([name, path]) => name === 'open' && path === '/app:/app:/Gw.dat'));
  });

  it('creates generated openat parent while retaining an existing nested Gw.dat', async () => {
    const stored = '/app:/app:/Gw.dat';
    const existing = runtime([stored]);
    globalThis.FS = existing.fs;
    globalThis.IDBFS = {};
    globalThis.addEventListener = () => {};
    const module = {
      addRunDependency() {},
      removeRunDependency() {},
    };
    installGameFilesystem({ module, failed: assert.fail, log() {} });
    module.preRun();
    await new Promise((resolve) => setImmediate(resolve));

    // Existing profiles already retain this generated-client path and must
    // continue to read it unchanged.
    const existingPath = generatedCalculateAt(existing.fs)(-100, 'app:/Gw.dat');
    assert.equal(existingPath, stored);
    assert.doesNotThrow(() => existing.fs.open(existingPath));

    const fresh = runtime();
    globalThis.FS = fresh.fs;
    installGameFilesystem({ module, failed: assert.fail, log() {} });
    module.preRun();
    await new Promise((resolve) => setImmediate(resolve));
    const freshPath = generatedCalculateAt(fresh.fs)(-100, 'app:/Gw.dat');
    assert.equal(freshPath, stored);
    assert.doesNotThrow(() => fresh.fs.open(freshPath, 64));
  });
});
