import assert from 'node:assert/strict';
import { afterEach, describe, it } from 'node:test';

const initialUIEvent = globalThis.UIEvent;
globalThis.UIEvent = class {};
const { installGameInput } = await import('./input.js');

class FakeMouseEvent {
  constructor(type, init = {}) {
    this.type = type;
    Object.assign(this, init);
    this.defaultPrevented = false;
    this.propagationStopped = false;
    this.immediatePropagationStopped = false;
  }

  preventDefault() { this.defaultPrevented = true; }
  stopPropagation() { this.propagationStopped = true; }
  stopImmediatePropagation() {
    this.immediatePropagationStopped = true;
    this.propagationStopped = true;
  }
}

class FakeTarget {
  constructor() { this.listeners = new Map(); }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  dispatchEvent(event) {
    for (const listener of this.listeners.get(event.type) ?? []) {
      listener(event);
      if (event.immediatePropagationStopped) break;
    }
    return !event.defaultPrevented;
  }
}

function trusted(type, init = {}) {
  return new FakeMouseEvent(type, { isTrusted: true, ...init });
}

function inputHarness({ requestPointerLock = () => undefined } = {}) {
  const original = {
    MouseEvent: globalThis.MouseEvent,
    UIEvent: initialUIEvent,
    WheelEvent: globalThis.WheelEvent,
    document: globalThis.document,
    window: globalThis.window,
  };
  const window = new FakeTarget();
  window.screenX = 0;
  const document = new FakeTarget();
  document.pointerLockElement = null;
  document.visibilityState = 'visible';
  document.documentElement = new FakeTarget();
  const canvas = new FakeTarget();
  canvas.dataset = {};
  const classes = new Set();
  canvas.classList = {
    add: (name) => classes.add(name),
    remove: (name) => classes.delete(name),
    toggle: (name, force) => force ? classes.add(name) : classes.delete(name),
    contains: (name) => classes.has(name),
  };
  canvas.getBoundingClientRect = () => ({ left: 0, top: 0, width: 100, height: 100 });
  canvas.requestPointerLock = requestPointerLock;
  let exits = 0;
  document.exitPointerLock = () => { exits += 1; document.pointerLockElement = null; };

  globalThis.MouseEvent = FakeMouseEvent;
  globalThis.UIEvent = FakeMouseEvent;
  globalThis.WheelEvent = { DOM_DELTA_PIXEL: 0 };
  globalThis.document = document;
  globalThis.window = window;
  const cleanup = () => {
    globalThis.MouseEvent = original.MouseEvent;
    globalThis.WheelEvent = original.WheelEvent;
    globalThis.document = original.document;
    globalThis.window = original.window;
    if (original.UIEvent === undefined) delete globalThis.UIEvent;
    else globalThis.UIEvent = original.UIEvent;
  };

  installGameInput({ canvas, log() {} });
  return { window, document, canvas, classes, cleanup, exits: () => exits };
}

let cleanups = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
});

function setup(options) {
  const harness = inputHarness(options);
  cleanups.push(harness.cleanup);
  return harness;
}

function rightDown(harness) {
  const event = trusted('mousedown', {
    target: harness.canvas, button: 2, buttons: 2,
    clientX: 10, clientY: 10, screenX: 10, screenY: 10,
  });
  harness.window.dispatchEvent(event);
  harness.canvas.dispatchEvent(event);
}

function rightUp(harness) {
  const event = trusted('mouseup', {
    target: harness.canvas, button: 2, buttons: 0,
    clientX: 10, clientY: 10, screenX: 10, screenY: 10,
  });
  harness.window.dispatchEvent(event);
  harness.document.dispatchEvent(event);
}

describe('right-drag pointer lock', () => {
  it('uses relative motion after lock, then releases it', () => {
    let requests = 0;
    const harness = setup({ requestPointerLock: () => { requests += 1; } });
    const mousemoves = [];
    harness.canvas.addEventListener('mousemove', (event) => mousemoves.push(event));

    rightDown(harness);
    assert.equal(requests, 1);
    harness.document.pointerLockElement = harness.canvas;
    harness.document.dispatchEvent(new FakeMouseEvent('pointerlockchange'));
    const movement = trusted('mousemove', { movementX: 600, movementY: -100 });
    harness.document.dispatchEvent(movement);

    assert.equal(movement.defaultPrevented, true);
    assert.equal(mousemoves.reduce((sum, event) => sum + event.movementX, 0), 600);
    assert.equal(mousemoves.reduce((sum, event) => sum + event.movementY, 0), -100);
    assert.ok(mousemoves.every((event) => event.clientX >= 0));
    assert.ok(mousemoves.every((event) => event.clientY >= 0));
    assert.equal(harness.classes.has('cursor-hidden'), true);

    rightUp(harness);
    assert.equal(harness.exits(), 1);
    assert.equal(harness.classes.has('cursor-hidden'), false);
  });

  it('keeps client coordinates nonnegative across repeated reanchors and reversal', () => {
    const harness = setup();
    const mousemoves = [];
    harness.canvas.addEventListener('mousemove', (event) => mousemoves.push(event));

    rightDown(harness);
    harness.document.pointerLockElement = harness.canvas;
    harness.document.dispatchEvent(new FakeMouseEvent('pointerlockchange'));

    const movements = Array.from({ length: 120 }, (_, index) =>
      index < 60 ? [-80, -30] : [80, 30]);
    const batches = [];
    for (const [movementX, movementY] of movements) {
      const start = mousemoves.length;
      harness.document.dispatchEvent(trusted('mousemove', { movementX, movementY }));
      batches.push(mousemoves.slice(start));
    }

    assert.ok(mousemoves.length > movements.length, 'long drags should re-anchor');
    assert.ok(mousemoves.every((event) => event.clientX >= 0 && event.clientY >= 0));
    for (const [batch, [movementX, movementY]] of movements.map((movement, index) =>
      [batches[index], movement])) {
      assert.equal(batch.reduce((sum, event) => sum + event.movementX, 0), movementX);
      assert.equal(batch.reduce((sum, event) => sum + event.movementY, 0), movementY);
    }
    assert.equal(harness.classes.has('cursor-hidden'), true);

    rightUp(harness);
    assert.equal(harness.exits(), 1);
    assert.equal(harness.classes.has('cursor-hidden'), false);
  });

  it('keeps denied right click pressed instead of synthesising an early mouseup', async () => {
    const refusal = new Error('focus required');
    const harness = setup({ requestPointerLock: () => Promise.reject(refusal) });
    const received = [];
    harness.canvas.addEventListener('mousedown', (event) => received.push(event.type));
    harness.canvas.addEventListener('mouseup', (event) => received.push(event.type));

    rightDown(harness);
    await Promise.resolve();
    await Promise.resolve();

    assert.deepEqual(received, ['mousedown']);
    assert.equal(harness.classes.has('cursor-hidden'), false);
  });

  it('exits a lock granted after the right button was released', async () => {
    let grant;
    const harness = setup({
      requestPointerLock: () => new Promise((resolve) => { grant = resolve; }),
    });

    rightDown(harness);
    rightUp(harness);
    harness.document.pointerLockElement = harness.canvas;
    grant();
    await Promise.resolve();

    assert.equal(harness.exits(), 1);
  });
});
