import assert from 'node:assert/strict';
import { describe, it, beforeEach, afterEach } from 'node:test';

const originalWindow = globalThis.window;
const originalSetInterval = globalThis.setInterval;
globalThis.window = { addEventListener() {}, removeEventListener() {} };
globalThis.setInterval = (...args) => originalSetInterval(...args).unref();
const { installTemplateSave } = await import('./template-save.js');
globalThis.setInterval = originalSetInterval;
const markers = { ensureDirectory: 1, findFiles: 2, fileBaseName: 3, deleteFile: 4, fileExists: 5 };

function setup({ heap = new Uint8Array(4096), malloc = () => 128, free, onCarrier } = {}) {
  globalThis.window = { __gwnativeBridgeMarkers: markers };
  const env = { __syscall_newfstatat: onCarrier ?? (() => 99) };
  const module = { HEAPU8: heap };
  installTemplateSave({ imports: { env }, module, exports: () => ({ malloc, free }), log() {} });
  return { env, module };
}

function putWide(heap, pointer, value) {
  new Uint16Array(heap.buffer)[pointer / 2] = value.charCodeAt(0);
  for (let i = 1; i < value.length; i++) new Uint16Array(heap.buffer)[pointer / 2 + i] = value.charCodeAt(i);
  new Uint16Array(heap.buffer)[pointer / 2 + value.length] = 0;
}

describe('template save bridge allocation ownership', () => {
  beforeEach(() => { globalThis.FS = { readdir: () => ['Test.txt'], stat: () => ({ mode: 0 }), isDir: () => false }; });
  afterEach(() => { globalThis.window = originalWindow; delete globalThis.FS; });

  it('publishes valid listing and leaves ownership with game', () => {
    const heap = new Uint8Array(4096);
    putWide(heap, 16, 'Templates/Skills/*.txt');
    let freed = 0;
    const { env } = setup({ heap, free: () => { freed += 1; } });
    const out = 1024;
    assert.equal(env.__syscall_newfstatat(markers.findFiles, 16, out, 1), 0);
    assert.equal(new Uint32Array(heap.buffer)[out / 4], 128);
    assert.equal(new Uint32Array(heap.buffer)[out / 4 + 2], 1);
    assert.equal(freed, 0);
  });

  it('requires matching allocator and free', () => {
    const heap = new Uint8Array(4096); putWide(heap, 16, 'Templates/Skills/*.txt');
    let allocations = 0;
    const { env } = setup({ heap, malloc: () => { allocations += 1; return 128; }, free: undefined });
    assert.equal(env.__syscall_newfstatat(markers.findFiles, 16, 1024, 1), 0);
    assert.equal(allocations, 0);
  });

  it('frees once when heap disappears after allocation', () => {
    const heap = new Uint8Array(4096); putWide(heap, 16, 'Templates/Skills/*.txt');
    let freed = 0;
    let module;
    const { env, module: actualModule } = setup({ heap, free: () => { freed += 1; }, malloc: () => { module.HEAPU8 = undefined; return 128; } });
    module = actualModule;
    assert.equal(env.__syscall_newfstatat(markers.findFiles, 16, 1024, 1), 0);
    assert.equal(freed, 1);
  });

  it('frees once when record writing throws', () => {
    const heap = new Uint8Array(4096); putWide(heap, 16, 'Templates/Skills/*.txt');
    let freed = 0;
    let module;
    const { env, module: actualModule } = setup({ heap, free: () => { freed += 1; }, malloc: () => { module.HEAPU8 = new Proxy(heap, { get() { throw new Error('heap'); } }); return 128; } });
    module = actualModule;
    assert.equal(env.__syscall_newfstatat(markers.findFiles, 16, 1024, 1), 0);
    assert.equal(freed, 1);
  });

  it('rejects invalid output header before malloc', () => {
    const heap = new Uint8Array(4096); putWide(heap, 16, 'Templates/Skills/*.txt');
    let allocations = 0;
    const { env } = setup({ heap, malloc: () => { allocations += 1; return 128; }, free: () => {} });
    assert.equal(env.__syscall_newfstatat(markers.findFiles, 16, 4092, 1), 0);
    assert.equal(allocations, 0);
  });

  for (const invalidPointer of [-1, 1.5, Infinity, 0x100000000]) {
    it(`rejects malformed malloc result ${String(invalidPointer)}`, () => {
      const heap = new Uint8Array(4096); putWide(heap, 16, 'Templates/Skills/*.txt');
      let freed = 0;
      const { env } = setup({ heap, malloc: () => invalidPointer, free: () => { freed += 1; } });
      assert.equal(env.__syscall_newfstatat(markers.findFiles, 16, 1024, 1), 0);
      const header = new Uint32Array(heap.buffer);
      assert.equal(freed, 0);
      assert.equal(header[1024 / 4], 0);
      assert.equal(header[1024 / 4 + 2], 0);
    });
  }

  it('frees once when allocated region exceeds heap', () => {
    const heap = new Uint8Array(4096); putWide(heap, 16, 'Templates/Skills/*.txt');
    let freed = 0;
    const { env } = setup({ heap, malloc: () => 4090, free: () => { freed += 1; } });
    assert.equal(env.__syscall_newfstatat(markers.findFiles, 16, 1024, 1), 0);
    assert.equal(freed, 1);
  });

  it('uses fresh heap after allocator growth', () => {
    const initial = new Uint8Array(4096); putWide(initial, 16, 'Templates/Skills/*.txt');
    const grown = new Uint8Array(8192); grown.set(initial);
    let module;
    const { env, module: actualModule } = setup({ heap: initial, malloc: () => { module.HEAPU8 = grown; return 128; }, free: () => {} });
    module = actualModule;
    assert.equal(env.__syscall_newfstatat(markers.findFiles, 16, 1024, 1), 0);
    assert.equal(new Uint32Array(grown.buffer)[1024 / 4], 128);
  });
});
