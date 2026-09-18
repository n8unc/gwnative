import assert from 'node:assert/strict';
import test from 'node:test';
import { installTexturePacks } from './texture-packs.js';

const encoded = (bytes) => Buffer.from(bytes).toString('base64');
const crc32 = (bytes) => {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
  }
  return crc >>> 0;
};
const transform = (bytes, width, height, flip, swap) => {
  const output = new Uint8Array(bytes.byteLength);
  for (let y = 0; y < height; y += 1) for (let x = 0; x < width; x += 1) {
    const from = (y * width + x) * 4;
    const to = ((flip ? height - y - 1 : y) * width + x) * 4;
    output[to] = bytes[from + (swap ? 2 : 0)]; output[to + 1] = bytes[from + 1];
    output[to + 2] = bytes[from + (swap ? 0 : 2)]; output[to + 3] = bytes[from + 3];
  }
  return output;
};
const context = ({ alignment = 4, unpack = null, rowLength = 0, skipRows = 0, skipPixels = 0, webgl2 = false, throwOnRead = false } = {}) => ({
  getParameter(value) {
    if (throwOnRead) throw new Error('state unavailable');
    if (value === 0x0CF5) return alignment;
    if (value === 0x88EF) return unpack;
    if (value === 0x0CF2) return rowLength;
    if (value === 0x0CF3) return skipRows;
    if (value === 0x0CF4) return skipPixels;
    return null;
  },
  ...(webgl2 ? { texStorage2D() {} } : {}),
});
const manifest = (entries) => ({ format: 1, packs: [{ entries }] });

test('replaces direct exact mapping, restores heap, and treats pointer zero as null allocation', () => {
  const heap = Uint8Array.from([0, 0, 0, 0, 1, 2, 3, 4]); let observed = [];
  const env = { glTexImage2D: (_t, _l, _i, _w, _h, _b, _f, _y, pointer) => { observed.push([...heap.slice(pointer, pointer + 4)]); return 9; } };
  const original = env.glTexImage2D;
  const seam = installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, manifest: manifest([{ target: crc32([1, 2, 3, 4]), width: 1, height: 1, rgbaBase64: encoded([9, 8, 7, 6]) }]) });
  assert.ok(seam);
  env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 0);
  env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4);
  assert.deepEqual(observed, [[0, 0, 0, 0], [9, 8, 7, 6]]);
  assert.deepEqual([...heap], [0, 0, 0, 0, 1, 2, 3, 4]); assert.equal(seam.snapshot().replacements, 1);
  seam.dispose(); assert.equal(env.glTexImage2D, original);
});

test('uses BGRA only to identify source and applies vertical orientation only', () => {
  const width = 1; const height = 2;
  const source = Uint8Array.from([30, 20, 10, 1, 60, 50, 40, 2]);
  const replacement = Uint8Array.from([3, 4, 5, 6, 7, 8, 9, 10]);
  const heap = new Uint8Array(32); heap.set(source, 8); let observed;
  const env = { glTexImage2D: (...args) => { observed = heap.slice(args[8], args[8] + source.byteLength); } };
  const seam = installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, manifest: manifest([{ target: crc32(transform(source, width, height, true, true)), width, height, rgbaBase64: encoded(replacement) }]) });
  env.glTexImage2D(0x0DE1, 0, 0x1908, width, height, 0, 0x1908, 0x1401, 8);
  assert.deepEqual([...observed], [...transform(replacement, width, height, true, false)]);
  assert.deepEqual([...heap.slice(8, 16)], [...source]); assert.equal(seam.snapshot().replacements, 1);
});

test('BGRA target hash leaves decoded RGBA replacement channels unchanged', () => {
  const source = Uint8Array.from([30, 20, 10, 255]); const replacement = Uint8Array.from([200, 15, 5, 255]);
  const heap = new Uint8Array(16); heap.set(source, 4); let observed;
  const env = { glTexImage2D: (...args) => { observed = heap.slice(args[8], args[8] + 4); } };
  installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, manifest: manifest([{ target: crc32(transform(source, 1, 1, false, true)), width: 1, height: 1, rgbaBase64: encoded(replacement) }]) });
  env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4);
  assert.deepEqual([...observed], [...replacement]);
});

test('first pack wins and aggregate decoded bytes are bounded', () => {
  const source = Uint8Array.from([1, 2, 3, 4]); const heap = new Uint8Array(16); heap.set(source, 4); let observed;
  const entries = [
    { target: crc32(source), width: 1, height: 1, rgbaBase64: encoded([9, 9, 9, 9]) },
    { target: crc32(source), width: 1, height: 1, rgbaBase64: encoded([8, 8, 8, 8]) },
  ];
  const env = { glTexImage2D: (...args) => { observed = heap.slice(args[8], args[8] + 4); } };
  const seam = installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, manifest: { format: 1, packs: [{ entries }] } });
  env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4);
  assert.deepEqual([...observed], [9, 9, 9, 9]); assert.equal(seam.snapshot().mappings, 1);
  const chunk = encoded(new Uint8Array(256 * 256 * 4));
  const many = Array.from({ length: 260 }, (_, index) => ({ target: index + 1, width: 256, height: 256, rgbaBase64: chunk }));
  const capped = installTexturePacks({ imports: { env: { glTexImage2D() {} } }, module: { HEAPU8: heap, ctx: context() }, manifest: manifest([
    ...many,
    { target: 2, width: 1, height: 1, rgbaBase64: encoded([1, 2, 3, 4]) },
  ]) });
  assert.equal(capped.snapshot().mappings, 256, 'aggregate cap rejects later entries');
});

test('late heap/context, malformed dimensions, and altered pixel store all bypass', () => {
  const module = {}; let calls = 0;
  const env = { glTexImage2D: () => { calls += 1; } };
  const seam = installTexturePacks({ imports: { env }, module, manifest: manifest([{ target: crc32([1, 2, 3, 4]), width: 1, height: 1, rgbaBase64: encoded([9, 8, 7, 6]) }]) });
  assert.ok(seam);
  env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4);
  module.HEAPU8 = new Uint8Array(16); module.HEAPU8.set([1, 2, 3, 4], 4); module.ctx = context();
  env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4);
  env.glTexImage2D(0x0DE1, 0, 0x1908, -1, 1, 0, 0x1908, 0x1401, 4);
  module.ctx = context({ alignment: 1 });
  env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4);
  module.ctx = context({ unpack: {} }); env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4);
  module.ctx = context({ webgl2: true, rowLength: 2 }); env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4);
  module.ctx = context({ throwOnRead: true }); env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4);
  assert.equal(calls, 7); assert.equal(seam.snapshot().replacements, 1);
});

test('original WebGL exception restores heap and seam errors fail open once', () => {
  const heap = new Uint8Array(16); heap.set([1, 2, 3, 4], 4); let calls = 0; const logs = [];
  const env = { glTexImage2D: () => { calls += 1; throw new Error('gl failed'); } };
  installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, manifest: manifest([{ target: crc32([1, 2, 3, 4]), width: 1, height: 1, rgbaBase64: encoded([9, 8, 7, 6]) }]), log: (...message) => logs.push(message) });
  assert.throws(() => env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4), /gl failed/);
  assert.deepEqual([...heap.slice(4, 8)], [1, 2, 3, 4]); assert.equal(calls, 1);
  assert.throws(() => env.glTexImage2D(0x0DE1, 1, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4), /gl failed/);
  assert.equal(calls, 2, 'baseline exception is not retried by bypass wrapper');
  const throwingHeap = new Proxy(heap, { get(target, key, receiver) { if (key === 'slice') throw new Error('heap read'); return Reflect.get(target, key, receiver); } });
  const fallback = { glTexImage2D: () => { calls += 1; } };
  installTexturePacks({ imports: { env: fallback }, module: { HEAPU8: throwingHeap, ctx: context() }, manifest: manifest([{ target: 1, width: 1, height: 1, rgbaBase64: encoded([1, 2, 3, 4]) }]), log: (...message) => logs.push(message) });
  fallback.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 4);
  assert.equal(calls, 3); assert.ok(logs.some(([message]) => String(message).includes('replacement bypassed')));
});

test('bound matched texture receives generated complete mip replacement', () => {
  const base = Uint8Array.from([1,2,3,4, 5,6,7,8, 9,10,11,12, 13,14,15,16]);
  const replacement = Uint8Array.from([20,0,0,255, 40,0,0,255, 60,0,0,255, 80,0,0,255]);
  const heap = new Uint8Array(64); heap.set(base, 8); heap.set([0,0,0,0], 32); const seen = [];
  const env = { glBindTexture() {}, glTexImage2D: (...args) => seen.push([...heap.slice(args[8], args[8] + args[3] * args[4] * 4)]) };
  const seam = installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, diagnostics: true, manifest: manifest([{ target: crc32(base), width: 2, height: 2, rgbaBase64: encoded(replacement) }]) });
  env.glBindTexture(0x0DE1, 7);
  env.glTexImage2D(0x0DE1, 0, 0x1908, 2, 2, 0, 0x1908, 0x1401, 8);
  env.glTexImage2D(0x0DE1, 1, 0x1908, 1, 1, 0, 0x1908, 0x1401, 32);
  assert.deepEqual(seen[1], [50, 0, 0, 255]); assert.equal(seam.snapshot().replacements, 2);
});

test('incompatible later mip clears association before baseline upload', () => {
  const base = Uint8Array.from([1,2,3,4, 5,6,7,8, 9,10,11,12, 13,14,15,16]);
  const replacement = Uint8Array.from([20,0,0,255, 40,0,0,255, 60,0,0,255, 80,0,0,255]);
  const heap = new Uint8Array(96); heap.set(base, 8); heap.set([7,7,7,7, 8,8,8,8], 32); heap.set([9,9,9,9], 48);
  const seen = [];
  const env = { glBindTexture() {}, glTexImage2D: (...args) => seen.push([...heap.slice(args[8], args[8] + args[3] * args[4] * 4)]) };
  installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, manifest: manifest([{ target: crc32(base), width: 2, height: 2, rgbaBase64: encoded(replacement) }]) });
  env.glBindTexture(0x0DE1, 7);
  env.glTexImage2D(0x0DE1, 0, 0x1908, 2, 2, 0, 0x1908, 0x1401, 8);
  env.glTexImage2D(0x0DE1, 1, 0x1908, 2, 1, 0, 0x1908, 0x1401, 32);
  env.glTexImage2D(0x0DE1, 1, 0x1908, 1, 1, 0, 0x1908, 0x1401, 48);
  assert.deepEqual(seen[1], [7,7,7,7, 8,8,8,8]);
  assert.deepEqual(seen[2], [9,9,9,9], 'mismatch removes association for later mips');
});

test('texture units and cube bindings do not alias a matched 2D texture', () => {
  const base = Uint8Array.from([1,2,3,4, 5,6,7,8, 9,10,11,12, 13,14,15,16]);
  const replacement = Uint8Array.from([20,0,0,255, 40,0,0,255, 60,0,0,255, 80,0,0,255]);
  const heap = new Uint8Array(96); heap.set(base, 8); heap.set([1,1,1,1], 32); heap.set([2,2,2,2], 48);
  const seen = [];
  const env = { glActiveTexture() {}, glBindTexture() {}, glTexImage2D: (...args) => seen.push([...heap.slice(args[8], args[8] + args[3] * args[4] * 4)]) };
  installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, manifest: manifest([{ target: crc32(base), width: 2, height: 2, rgbaBase64: encoded(replacement) }]) });
  env.glActiveTexture(0x84C0); env.glBindTexture(0x0DE1, 7);
  env.glTexImage2D(0x0DE1, 0, 0x1908, 2, 2, 0, 0x1908, 0x1401, 8);
  env.glBindTexture(0x8513, 99);
  env.glActiveTexture(0x84C1); env.glBindTexture(0x0DE1, 8);
  env.glTexImage2D(0x0DE1, 1, 0x1908, 1, 1, 0, 0x1908, 0x1401, 32);
  env.glActiveTexture(0x84C0);
  env.glTexImage2D(0x0DE1, 1, 0x1908, 1, 1, 0, 0x1908, 0x1401, 48);
  assert.deepEqual(seen[1], [1, 1, 1, 1]);
  assert.deepEqual(seen[2], [50, 0, 0, 255]);
});

test('unmatched redefinition, deletion, and a subimage invalidate matched mip state', () => {
  const base = Uint8Array.from([1,2,3,4, 5,6,7,8, 9,10,11,12, 13,14,15,16]);
  const replacement = Uint8Array.from([20,0,0,255, 40,0,0,255, 60,0,0,255, 80,0,0,255]);
  const heap = new Uint8Array(160); heap.set(base, 8); heap.set([9,9,9,9], 32); heap.set([3,3,3,3], 64); heap.set(base, 80); heap.set([4,4,4,4], 112); heap.set(base, 120); heap.set([5,5,5,5], 144);
  const seen = []; const subimages = [];
  const env = {
    glBindTexture() {}, glDeleteTextures() {},
    glTexSubImage2D: (...args) => subimages.push(args),
    glTexImage2D: (...args) => seen.push([...heap.slice(args[8], args[8] + args[3] * args[4] * 4)]),
  };
  installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, manifest: manifest([{ target: crc32(base), width: 2, height: 2, rgbaBase64: encoded(replacement) }]) });
  env.glBindTexture(0x0DE1, 7);
  env.glTexImage2D(0x0DE1, 0, 0x1908, 2, 2, 0, 0x1908, 0x1401, 8);
  env.glTexImage2D(0x0DE1, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, 32);
  env.glTexImage2D(0x0DE1, 1, 0x1908, 1, 1, 0, 0x1908, 0x1401, 64);
  env.glTexImage2D(0x0DE1, 0, 0x1908, 2, 2, 0, 0x1908, 0x1401, 80);
  new DataView(heap.buffer).setUint32(40, 7, true); env.glDeleteTextures(1, 40);
  env.glTexImage2D(0x0DE1, 1, 0x1908, 1, 1, 0, 0x1908, 0x1401, 112);
  env.glTexImage2D(0x0DE1, 0, 0x1908, 2, 2, 0, 0x1908, 0x1401, 120);
  env.glTexSubImage2D(0x0DE1, 0, 1, 0, 1, 1, 0x1908, 0x1401, 144);
  env.glTexImage2D(0x0DE1, 1, 0x1908, 1, 1, 0, 0x1908, 0x1401, 144);
  assert.deepEqual(seen[2], [3, 3, 3, 3], 'unmatched level zero clears association');
  assert.deepEqual(seen[4], [4, 4, 4, 4], 'glDeleteTextures clears association through Emscripten heap IDs');
  assert.deepEqual(seen[6], [5, 5, 5, 5], 'baseline subimage clears association');
  assert.equal(subimages.length, 1);
});

test('odd replacement dimensions generate a non-zero complete mip', () => {
  const base = Uint8Array.from({ length: 3 * 3 * 4 }, (_, index) => index + 1);
  const replacement = Uint8Array.from({ length: 3 * 3 * 4 }, (_, index) => index + 20);
  const heap = new Uint8Array(64); heap.set(base, 4); heap.set([0, 0, 0, 0], 48); const seen = [];
  const env = { glBindTexture() {}, glTexImage2D: (...args) => seen.push([...heap.slice(args[8], args[8] + args[3] * args[4] * 4)]) };
  installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, manifest: manifest([{ target: crc32(base), width: 3, height: 3, rgbaBase64: encoded(replacement) }]) });
  env.glBindTexture(0x0DE1, 7); env.glTexImage2D(0x0DE1, 0, 0x1908, 3, 3, 0, 0x1908, 0x1401, 4);
  env.glTexImage2D(0x0DE1, 1, 0x1908, 1, 1, 0, 0x1908, 0x1401, 48);
  assert.equal(seen[1].length, 4); assert.deepEqual(seen[1], [28, 29, 30, 31]);
});

test('diagnostics counts storage, subimage, and compressed import paths without changing them', () => {
  const source = Uint8Array.from([1, 2, 3, 4]); const heap = new Uint8Array(16); heap.set(source, 4);
  const calls = [];
  const env = {
    glTexImage2D() {}, glTexStorage2D: (...args) => calls.push(['storage', args]),
    glTexSubImage2D: (...args) => calls.push(['subimage', args]),
    glCompressedTexImage2D: (...args) => calls.push(['compressed-image', args]),
    glCompressedTexSubImage2D: (...args) => calls.push(['compressed-subimage', args]),
  };
  const seam = installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, diagnostics: true, manifest: manifest([{ target: crc32(source), width: 1, height: 1, rgbaBase64: encoded([5, 6, 7, 8]) }]) });
  env.glTexStorage2D(0x0DE1, 1, 0x8058, 1, 1);
  env.glTexSubImage2D(0x0DE1, 0, 0, 0, 1, 1, 0x1908, 0x1401, 4);
  env.glCompressedTexImage2D(0x0DE1, 0, 0x83F1, 1, 1, 0, 8, 4);
  env.glCompressedTexSubImage2D(0x0DE1, 0, 0, 0, 1, 1, 0x83F1, 8, 4);
  assert.deepEqual(calls.map(([kind]) => kind), ['storage', 'subimage', 'compressed-image', 'compressed-subimage']);
  assert.deepEqual(seam.snapshot(), { mappings: 1, replacements: 0, uploads: 0, eligibleUploads: 0, hashMatches: 0, bypassedMipUploads: 0, texStorage2D: 1, texSubImage2D: 1, compressedTexImage2D: 1, compressedTexSubImage2D: 1, matchedTextures: 0 });
});

test('compressed DXT subimages swap only an exact compatible raw block chain', () => {
  const source = Uint8Array.from([1, 2, 3, 4, 5, 6, 7, 8]);
  const replacement = Uint8Array.from([11, 12, 13, 14, 15, 16, 17, 18]);
  const nextSource = Uint8Array.from([21, 22, 23, 24, 25, 26, 27, 28]);
  const nextReplacement = Uint8Array.from([31, 32, 33, 34, 35, 36, 37, 38]);
  const heap = new Uint8Array(64); heap.set(source, 4); heap.set(nextSource, 20); const seen = [];
  const env = { glBindTexture() {}, glTexImage2D() {}, glCompressedTexSubImage2D: (...args) => seen.push([...heap.slice(args[8], args[8] + args[7])]) };
  const seam = installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, diagnostics: true, manifest: manifest([{
    target: crc32(source), width: 4, height: 4, rgbaBase64: encoded(new Uint8Array(64)),
    compressed: { mode: 'DXT1', levels: [encoded(replacement), encoded(nextReplacement)] },
  }]) });
  env.glBindTexture(0x0DE1, 7);
  env.glCompressedTexSubImage2D(0x0DE1, 0, 0, 0, 4, 4, 0x83F1, 8, 4);
  env.glCompressedTexSubImage2D(0x0DE1, 1, 0, 0, 2, 2, 0x83F1, 8, 20);
  assert.deepEqual(seen, [[...replacement], [...nextReplacement]]);
  assert.deepEqual([...heap.slice(4, 12)], [...source]); assert.deepEqual([...heap.slice(20, 28)], [...nextSource]);
  assert.equal(seam.snapshot().replacements, 2);
});

test('immutable storage keeps a replaced base coherent through RGBA subimages and mips', () => {
  const base = Uint8Array.from([1,2,3,4, 5,6,7,8, 9,10,11,12, 13,14,15,16]);
  const replacement = Uint8Array.from([20,0,0,255, 40,0,0,255, 60,0,0,255, 80,0,0,255]);
  const heap = new Uint8Array(80); heap.set(base, 4); heap.set([0,0,0,0], 32); heap.set(base, 48); const seen = [];
  const env = { glBindTexture() {}, glTexImage2D() {}, glTexStorage2D() {}, glTexSubImage2D: (...args) => seen.push([...heap.slice(args[8], args[8] + args[4] * args[5] * 4)]) };
  const seam = installTexturePacks({ imports: { env }, module: { HEAPU8: heap, ctx: context() }, manifest: manifest([{ target: crc32(base), width: 2, height: 2, rgbaBase64: encoded(replacement) }]) });
  env.glBindTexture(0x0DE1, 7); env.glTexStorage2D(0x0DE1, 2, 0x8058, 2, 2);
  env.glTexSubImage2D(0x0DE1, 0, 0, 0, 2, 2, 0x1908, 0x1401, 4);
  env.glTexSubImage2D(0x0DE1, 1, 0, 0, 1, 1, 0x1908, 0x1401, 32);
  env.glTexSubImage2D(0x0DE1, 0, 1, 0, 1, 1, 0x1908, 0x1401, 48);
  assert.deepEqual(seen[0], [...replacement]); assert.deepEqual(seen[1], [50, 0, 0, 255]);
  assert.deepEqual(seen[2], [40, 0, 0, 255], 'partial update receives matching replacement region');
  assert.equal(seam.snapshot().replacements, 3);
});
