import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

const context = {
  lastPixels: '',
  createImageData(width, height) {
    return { data: new Uint8ClampedArray(width * height * 4) };
  },
  putImageData(image) {
    this.lastPixels = `${image.data[0]}:${image.data[1]}:${image.data[2]}:${image.data[3]}`;
  },
};

const listeners = new Map();
const addListener = (target, type, listener) => {
  const key = `${target}:${type}`;
  const entries = listeners.get(key) ?? new Set();
  entries.add(listener);
  listeners.set(key, entries);
};
const removeListener = (target, type, listener) => {
  listeners.get(`${target}:${type}`)?.delete(listener);
};
const dispatch = (target, type) => {
  for (const listener of listeners.get(`${target}:${type}`) ?? []) listener();
};

const body = {
  children: [],
  append(element) {
    this.children.push(element);
  },
};

globalThis.document = {
  createElement(name) {
    if (name === 'div') {
      return {
        style: { cssText: '', width: '1px' },
        offsetWidth: 0,
        remove() {
          this.removed = true;
        },
      };
    }
    assert.equal(name, 'canvas');
    return {
      width: 0,
      height: 0,
      getContext(kind) {
        assert.equal(kind, '2d');
        return context;
      },
      toDataURL(kind) {
        assert.equal(kind, 'image/png');
        return `data:image/png;base64,${context.lastPixels}`;
      },
    };
  },
  body,
  addEventListener(type, listener) {
    addListener('document', type, listener);
  },
  removeEventListener(type, listener) {
    removeListener('document', type, listener);
  },
};
globalThis.window = {
  addEventListener(type, listener) {
    addListener('window', type, listener);
  },
  removeEventListener(type, listener) {
    removeListener('window', type, listener);
  },
};

const { buildCursorCss } = await import('./enhancement-cursor.js');

describe('game cursor presentation', () => {
  it('gives WebKit one stable cursor URL rather than a rebuilt image-set', () => {
    const css = buildCursorCss(new Uint8ClampedArray(32 * 32 * 4), 3, 7);

    assert.equal(
      css,
      'url("data:image/png;base64,0:0:0:0") 3 7, default',
    );
    assert.equal(css.includes('image-set('), false);
  });

  it('applies published pixels and hotspot, then switches image on generation change', async () => {
    const { createCursorConsumer } = await import('./enhancement-cursor.js');
    const memory = new WebAssembly.Memory({ initial: 1 });
    const element = { style: { cursor: '' } };
    const consumer = createCursorConsumer({ element, memory, cursorPointer: 128 });
    writeCursor(memory.buffer, 128, { generation: 1, pixelHash: 11, hotspotX: 2, hotspotY: 3, pixel: 17 });

    consumer.poll();
    const first = element.style.cursor;
    assert.match(first, /17:17:17:17"\) 2 3, default$/);
    assert.deepEqual(consumer.state, {
      generation: 1,
      pixelHash: 11,
      hidden: false,
      valid: true,
      cssLength: first.length,
    });

    writeCursor(memory.buffer, 128, { generation: 2, pixelHash: 12, hotspotX: 4, hotspotY: 5, pixel: 23 });
    consumer.poll();
    assert.match(element.style.cursor, /23:23:23:23"\) 4 5, default$/);
    assert.notEqual(element.style.cursor, first);
    consumer.dispose();
  });

  it('hides and restores the last cursor, preserving it during a writing publish', async () => {
    const { createCursorConsumer } = await import('./enhancement-cursor.js');
    const memory = new WebAssembly.Memory({ initial: 1 });
    const element = { style: { cursor: '' } };
    const consumer = createCursorConsumer({ element, memory, cursorPointer: 128, fallback: 'crosshair' });
    writeCursor(memory.buffer, 128, { generation: 4, pixelHash: 14, pixel: 31 });
    consumer.poll();
    const visible = element.style.cursor;

    writeCursor(memory.buffer, 128, { sequence: 5, generation: 4, pixelHash: 14, pixel: 99 });
    consumer.poll();
    assert.equal(element.style.cursor, visible);
    assert.equal(consumer.state.generation, 4);

    writeCursor(memory.buffer, 128, { flags: 3, generation: 4, pixelHash: 14, pixel: 31 });
    consumer.poll();
    assert.equal(element.style.cursor, 'none');
    assert.equal(consumer.state.hidden, true);

    writeCursor(memory.buffer, 128, { flags: 1, generation: 4, pixelHash: 14, pixel: 31 });
    consumer.poll();
    assert.equal(element.style.cursor, visible);
    assert.equal(consumer.state.hidden, false);
    consumer.dispose();
  });

  it('falls back for an invalid publish and reasserts ownership after focus or pointer lock', async () => {
    const { createCursorConsumer } = await import('./enhancement-cursor.js');
    const memory = new WebAssembly.Memory({ initial: 1 });
    const element = { style: { cursor: '' } };
    const consumer = createCursorConsumer({ element, memory, cursorPointer: 128, fallback: 'auto' });
    writeCursor(memory.buffer, 128, { generation: 6, pixelHash: 16, pixel: 41 });
    consumer.poll();
    const visible = element.style.cursor;

    element.style.cursor = 'auto';
    dispatch('window', 'focus');
    assert.equal(element.style.cursor, visible);
    element.style.cursor = 'auto';
    dispatch('document', 'pointerlockchange');
    assert.equal(element.style.cursor, visible);

    writeCursor(memory.buffer, 128, { flags: 0, generation: 7, pixelHash: 0, pixel: 0 });
    consumer.poll();
    assert.equal(element.style.cursor, 'auto');
    assert.equal(consumer.state.valid, false);

    consumer.dispose();
    assert.equal(element.style.cursor, '');
    element.style.cursor = 'auto';
    dispatch('window', 'focus');
    assert.equal(element.style.cursor, 'auto');
  });
});

function writeCursor(buffer, pointer, overrides = {}) {
  const fields = {
    sequence: 4,
    flags: 1,
    generation: 1,
    hotspotX: 5,
    hotspotY: 6,
    pixelHash: 1,
    pixel: 0,
    ...overrides,
  };
  const view = new DataView(buffer, pointer, 4160);
  view.setUint32(0, 0x43545747, true);
  view.setUint16(4, 1, true);
  view.setUint16(6, 4160, true);
  view.setUint32(8, fields.sequence, true);
  view.setUint32(12, fields.flags, true);
  view.setUint32(16, fields.generation, true);
  view.setUint32(20, 32, true);
  view.setUint32(24, 32, true);
  view.setUint32(28, fields.hotspotX, true);
  view.setUint32(32, fields.hotspotY, true);
  view.setUint32(36, fields.pixelHash, true);
  for (let offset = 40; offset < 64; offset += 4) view.setUint32(offset, 0, true);
  for (let offset = 64; offset < 4160; offset += 4) {
    view.setUint32(offset, fields.pixel | (fields.pixel << 8) | (fields.pixel << 16) | (fields.pixel << 24), true);
  }
}
