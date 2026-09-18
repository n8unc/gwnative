#!/usr/bin/env node
// Offline probe of the actual client parser. Fixed dummy values only; never
// accept credentials or launch the game. Original client file stays untouched.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../web/Gw.wasm', import.meta.url));
assert.equal(createHash('sha256').update(source).digest('hex'),
  '373c65abcfe6c26161ffd59cb8ea298eb3d1814f4778aeaf24d8978c1fdfa8ef',
  'Client changed: re-establish internal parser addresses before running probe');
assert.equal(process.argv.length, 2, 'Probe takes no arguments; fixtures are embedded');

function leb(value) {
  const bytes = [];
  do {
    let byte = value & 127;
    value >>>= 7;
    if (value) byte |= 128;
    bytes.push(byte);
  } while (value);
  return Buffer.from(bytes);
}
function readLeb(bytes, offset) {
  let value = 0, shift = 0, byte;
  do {
    byte = bytes[offset++];
    value |= (byte & 127) << shift;
    shift += 7;
  } while (byte & 128);
  return [value >>> 0, offset];
}
function name(value) {
  return Buffer.concat([leb(Buffer.byteLength(value)), Buffer.from(value)]);
}

// Add diagnostic exports and one empty table slot only in memory. Executable
// instructions, data, and all existing function/table indices stay unchanged.
const sections = [source.subarray(0, 8)];
for (let offset = 8; offset < source.length;) {
  const id = source[offset++];
  const [length, start] = readLeb(source, offset);
  let body = source.subarray(start, start + length);
  offset = start + length;
  if (id === 4) {
    const [count, cursor] = readLeb(body, 0);
    assert.equal(count, 1);
    assert.equal(body[cursor], 0x70); // funcref
    assert.equal(body[cursor + 1], 1); // bounded table
    const [initial, next] = readLeb(body, cursor + 2);
    const [maximum, end] = readLeb(body, next);
    assert.equal(end, body.length);
    assert.equal(initial, maximum);
    body = Buffer.concat([leb(1), Buffer.from([0x70, 1]), leb(initial + 1), leb(maximum + 1)]);
  }
  if (id === 7) {
    const [count, cursor] = readLeb(body, 0);
    const exports = [['collectArgv', 10473], ['createParser', 10483],
      ['parseOptions', 10487], ['optionValue', 10486]];
    body = Buffer.concat([leb(count + exports.length), body.subarray(cursor),
      ...exports.map(([label, index]) => Buffer.concat([name(label), Buffer.from([0]), leb(index)]))]);
  }
  sections.push(Buffer.from([id]), leb(body.length), body);
}
const module = new WebAssembly.Module(Buffer.concat(sections));
// Typed (i32, i32) -> i32 wrapper lets us observe unknown-option fallback.
const callbackModule = new WebAssembly.Module(Uint8Array.from([
  0, 97, 115, 109, 1, 0, 0, 0,
  1, 7, 1, 96, 2, 127, 127, 1, 127,
  2, 7, 1, 1, 109, 1, 102, 0, 0,
  7, 5, 1, 1, 102, 0, 0,
]));

function probe(args, inspect = false) {
  let exports;
  const imports = {};
  let importCalls = 0;
  const allocate = bytes => {
    const pointer = exports.malloc(bytes.length);
    new Uint8Array(exports.memory.buffer, pointer, bytes.length).set(bytes);
    return pointer;
  };
  for (const item of WebAssembly.Module.imports(module)) {
    assert.equal(item.kind, 'function');
    (imports[item.module] ??= {})[item.name] = (...values) => {
      importCalls++;
      if (item.name === 'emscripten_get_now') return 0;
      if (item.name === 'emscripten_asm_const_ptr' && values[0] === 2656343) {
        return allocate(Buffer.from('en\0')); // constructor locale, no host access
      }
      throw new Error(`Blocked host import: ${item.name}`);
    };
  }
  exports = new WebAssembly.Instance(module, imports).exports;
  exports.emscripten_stack_init();
  exports.__wasm_call_ctors();
  const constructorCalls = importCalls;
  const utf16 = pointer => {
    const heap = new Uint16Array(exports.memory.buffer);
    let value = '';
    assert.ok(pointer > 0 && pointer % 2 === 0);
    for (let i = pointer / 2; i < heap.length; i++) {
      if (!heap[i]) return value;
      assert.ok(value.length < 4096, 'Unexpected unbounded string');
      value += String.fromCharCode(heap[i]);
    }
    throw new Error('Unterminated string');
  };
  const unknown = [];
  const callback = new WebAssembly.Instance(callbackModule, { m: {
    f: pointer => { unknown.push(utf16(pointer)); return 0; },
  } }).exports.f;
  const table = exports.__indirect_function_table;
  const callbackIndex = table.length - 1;
  assert.equal(table.get(callbackIndex), null);
  table.set(callbackIndex, callback);

  const pointers = ['gwnative', ...args].map(value => allocate(Buffer.from(value + '\0')));
  const argv = exports.malloc(4 * (pointers.length + 1));
  new Uint32Array(exports.memory.buffer, argv, pointers.length + 1).set([...pointers, 0]);
  exports.collectArgv(pointers.length, argv);
  // Same descriptor base/count used by the client's own Cmd.cpp bootstrap.
  const parser = exports.createParser(1452944, 52);
  assert.ok(parser);
  const parseResult = exports.parseOptions(parser, callbackIndex, 0, 0, 0);
  const result = {
    parseResult,
    email: utf16(exports.optionValue(parser, 47)),
    password: utf16(exports.optionValue(parser, 51)),
    autoLogin: exports.optionValue(parser, 1),
    unknown,
  };
  if (inspect) {
    result.character = utf16(exports.optionValue(parser, 46));
    const words = new Uint32Array(exports.memory.buffer);
    result.descriptors = Array.from({ length: 52 }, (_, id) => {
      const at = 1452944 / 4 + id * 3;
      const pointer = words[at + 1];
      let name = null;
      try { name = pointer ? utf16(pointer) : null; } catch {}
      return [id, words[at], pointer, words[at + 2], name];
    });
  }
  assert.equal(importCalls, constructorCalls, 'Parser unexpectedly requested a host operation');
  return result;
}

const email = 'launcher-probe@example.invalid';
const password = 'fixture-only';
const credentials = { parseResult: 1, email, password, autoLogin: 0, unknown: [] };
const empty = { parseResult: 1, email: '', password: '', autoLogin: 0, unknown: [] };
const cases = [
  ['no flags', [], empty],
  ['double dash, equals', [`--email=${email}`, `--password=${password}`], credentials],
  ['single dash, equals', [`-email=${email}`, `-password=${password}`], credentials],
  ['double dash, separate values', ['--email', email, '--password', password], credentials],
  ['single dash, separate values', ['-email', email, '-password', password], credentials],
  ['double dash autologin', ['--autologin'], { ...empty, autoLogin: 1 }],
  ['single dash autologin', ['-autologin'], { ...empty, autoLogin: 1 }],
  ['combined options', [`--email=${email}`, `--password=${password}`, '--autologin'],
    { ...credentials, autoLogin: 1 }],
  ['unknown option control', ['--invalid-fixture=value'],
    { ...empty, parseResult: 0, unknown: ['--invalid-fixture=value'] }],
  ['ASCII punctuation round trip', ['--password=-fixture=two\\three'],
    { ...empty, password: '-fixture=two\\three' }],
  ['space transport hazard', ['--password=fixture two'],
    { ...empty, parseResult: 0, password: 'fixture', unknown: ['two'] }],
  ['quote wrapping transport hazard', ['--password="fixture two"'],
    { ...empty, parseResult: 0, password: '\\', unknown: ['fixture two\\'] }],
  ['embedded quote transport hazard', ['--password=fixture"two'],
    { ...empty, parseResult: 0, password: 'fixture\\', unknown: ['two'] }],
  ['UTF-8 transport hazard', ['--password=pāss🔒'],
    { ...empty, password: 'p\uffc4\uff81ss\ufff0\uff9f\uff94\uff92' }],
];
for (const [label, args, expected] of cases) {
  assert.deepEqual(probe(args), expected, label);
  console.log(`PASS ${label}`);
}
for (const option of ['character', 'charname', 'charactername', 'char']) {
  const plain = probe([`--${option}=Fixture`], true);
  const spaced = probe([`--${option}=Fixture User`], true);
  const single = probe([`-${option}`, 'Fixture'], true);
  console.log(`OPTION ${option} plain=${plain.parseResult}/${JSON.stringify(plain.unknown)}/${JSON.stringify(plain.character)} single=${single.parseResult}/${JSON.stringify(single.unknown)}/${JSON.stringify(single.character)} spaced=${spaced.parseResult}/${JSON.stringify(spaced.unknown)}/${JSON.stringify(spaced.character)}`);
}
console.log(`DESCRIPTORS ${JSON.stringify(probe([], true).descriptors)}`);
console.log(`${cases.length} parser checks passed. No game startup, credentials read, or network access.`);
