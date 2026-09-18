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

function inputHarness({ requestPointerLock = () => undefined, touchMode = 'off' } = {}) {
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

  const input = installGameInput({ canvas, touchMode, log() {} });
  return { window, document, canvas, input, classes, cleanup, exits: () => exits };
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

function dispatchCanvasMouse(harness, type, init) {
  const event = trusted(type, {
    target: harness.canvas,
    clientX: 10,
    clientY: 10,
    screenX: 10,
    screenY: 10,
    ...init,
  });
  // Browser capture path for an event targeted at the canvas. The client
  // receives it only after GWNative's window/document/canvas handlers.
  harness.window.dispatchEvent(event);
  if (!event.propagationStopped) harness.document.dispatchEvent(event);
  if (!event.propagationStopped) harness.canvas.dispatchEvent(event);
  return event;
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

  it('keeps client button state through continuous chord reanchors', async () => {
    let grant;
    const harness = setup({
      touchMode: 'dbltap',
      requestPointerLock: () => new Promise((resolve) => { grant = resolve; }),
    });
    let trackedButtons = 0;
    const moveStates = [];
    const buttonMask = (button) => [1, 4, 2, 8, 16][button] ?? 0;
    for (const type of ['mousedown', 'mouseup', 'mousemove']) {
      harness.canvas.addEventListener(type, (event) => {
        if (type === 'mousedown') trackedButtons |= buttonMask(event.button);
        if (type === 'mouseup') trackedButtons &= ~buttonMask(event.button);
        if (type === 'mousemove' && event.buttons) {
          moveStates.push(trackedButtons);
          assert.notEqual(trackedButtons, 0, 'client must retain a pressed button during motion');
        }
      });
    }

    dispatchCanvasMouse(harness, 'mousedown', { button: 2, buttons: 2, detail: 1 });
    harness.document.pointerLockElement = harness.canvas;
    grant();
    await Promise.resolve();
    harness.document.dispatchEvent(new FakeMouseEvent('pointerlockchange'));

    dispatchCanvasMouse(harness, 'mousedown', { button: 0, buttons: 3, detail: 2 });
    for (const [movementX, movementY] of [[2000, -2000], [-2000, 2000], [2000, 2000]]) {
      harness.document.dispatchEvent(trusted('mousemove', { movementX, movementY }));
    }
    dispatchCanvasMouse(harness, 'mouseup', { button: 0, buttons: 2, detail: 2 });
    dispatchCanvasMouse(harness, 'mouseup', { button: 2, buttons: 0, detail: 1 });

    assert.ok(moveStates.length > 3, 'large deltas should exercise multiple reanchors');
  });

  it('suppresses stale left-right chord events after pointer-lock loss until both buttons release', async () => {
    let grant;
    const harness = setup({
      touchMode: 'dbltap',
      requestPointerLock: () => new Promise((resolve) => { grant = resolve; }),
    });
    const clientEvents = [];
    for (const type of ['mousedown', 'mouseup', 'mousemove']) {
      harness.canvas.addEventListener(type, (event) => {
        clientEvents.push({ type, button: event.button, buttons: event.buttons });
      });
    }

    dispatchCanvasMouse(harness, 'mousedown', { button: 2, buttons: 2, detail: 1 });
    harness.document.pointerLockElement = harness.canvas;
    grant();
    await Promise.resolve();
    harness.document.dispatchEvent(new FakeMouseEvent('pointerlockchange'));
    dispatchCanvasMouse(harness, 'mousedown', { button: 0, buttons: 3, detail: 2 });
    harness.document.pointerLockElement = null;
    harness.document.dispatchEvent(new FakeMouseEvent('pointerlockchange'));
    dispatchCanvasMouse(harness, 'mousemove', { buttons: 3, movementX: 1, movementY: 0 });
    dispatchCanvasMouse(harness, 'mouseup', { button: 0, buttons: 2, detail: 2 });
    dispatchCanvasMouse(harness, 'mouseup', { button: 2, buttons: 0, detail: 1 });
    dispatchCanvasMouse(harness, 'mousemove', { buttons: 0, movementX: 1, movementY: 0 });
    dispatchCanvasMouse(harness, 'mousedown', { button: 0, buttons: 1, detail: 1 });

    assert.deepEqual(clientEvents, [
      { type: 'mousedown', button: 2, buttons: 2 },
      { type: 'mousedown', button: 0, buttons: 3 },
      { type: 'mouseup', button: 2, buttons: 1 },
      { type: 'mouseup', button: 0, buttons: 0 },
      { type: 'mousemove', button: undefined, buttons: 0 },
      { type: 'mousedown', button: 0, buttons: 1 },
    ]);
  });

  it('suppresses stale right release before left release, then allows hover', async () => {
    let grant;
    const harness = setup({
      requestPointerLock: () => new Promise((resolve) => { grant = resolve; }),
    });
    const clientEvents = [];
    for (const type of ['mousedown', 'mouseup', 'mousemove']) {
      harness.canvas.addEventListener(type, (event) => {
        clientEvents.push({ type, button: event.button, buttons: event.buttons });
      });
    }

    dispatchCanvasMouse(harness, 'mousedown', { button: 2, buttons: 2 });
    harness.document.pointerLockElement = harness.canvas;
    grant();
    await Promise.resolve();
    harness.document.dispatchEvent(new FakeMouseEvent('pointerlockchange'));
    dispatchCanvasMouse(harness, 'mousedown', { button: 0, buttons: 3 });
    harness.document.pointerLockElement = null;
    harness.document.dispatchEvent(new FakeMouseEvent('pointerlockchange'));
    dispatchCanvasMouse(harness, 'mouseup', { button: 2, buttons: 1 });
    dispatchCanvasMouse(harness, 'mousemove', { buttons: 1, movementX: 1, movementY: 0 });
    dispatchCanvasMouse(harness, 'mouseup', { button: 0, buttons: 0 });
    dispatchCanvasMouse(harness, 'mousemove', { buttons: 0, movementX: 1, movementY: 0 });

    assert.deepEqual(clientEvents, [
      { type: 'mousedown', button: 2, buttons: 2 },
      { type: 'mousedown', button: 0, buttons: 3 },
      { type: 'mouseup', button: 2, buttons: 1 },
      { type: 'mouseup', button: 0, buttons: 0 },
      { type: 'mousemove', button: undefined, buttons: 0 },
    ]);
  });

  it('allows a new left press while stale right remains suppressed', async () => {
    let grant;
    const harness = setup({
      requestPointerLock: () => new Promise((resolve) => { grant = resolve; }),
    });
    const clientEvents = [];
    for (const type of ['mousedown', 'mouseup', 'mousemove']) {
      harness.canvas.addEventListener(type, (event) => {
        clientEvents.push({ type, button: event.button, buttons: event.buttons });
      });
    }

    dispatchCanvasMouse(harness, 'mousedown', { button: 2, buttons: 2 });
    harness.document.pointerLockElement = harness.canvas;
    grant();
    await Promise.resolve();
    harness.document.dispatchEvent(new FakeMouseEvent('pointerlockchange'));
    dispatchCanvasMouse(harness, 'mousedown', { button: 0, buttons: 3 });
    harness.document.pointerLockElement = null;
    harness.document.dispatchEvent(new FakeMouseEvent('pointerlockchange'));
    dispatchCanvasMouse(harness, 'mouseup', { button: 0, buttons: 2 });
    dispatchCanvasMouse(harness, 'mousedown', { button: 0, buttons: 3 });
    dispatchCanvasMouse(harness, 'mousemove', { buttons: 3, movementX: 1, movementY: 0 });
    dispatchCanvasMouse(harness, 'mouseup', { button: 0, buttons: 2 });
    dispatchCanvasMouse(harness, 'mouseup', { button: 2, buttons: 0 });
    dispatchCanvasMouse(harness, 'mousemove', { buttons: 0, movementX: 1, movementY: 0 });

    assert.deepEqual(clientEvents, [
      { type: 'mousedown', button: 2, buttons: 2 },
      { type: 'mousedown', button: 0, buttons: 3 },
      { type: 'mouseup', button: 2, buttons: 1 },
      { type: 'mouseup', button: 0, buttons: 0 },
      { type: 'mousedown', button: 0, buttons: 3 },
      { type: 'mouseup', button: 0, buttons: 2 },
      { type: 'mousemove', button: undefined, buttons: 0 },
    ]);
  });

  it('bounds redacted mouse diagnostics across repeated blur resets', () => {
    const harness = setup();
    dispatchCanvasMouse(harness, 'mousedown', { button: 2, buttons: 2 });
    dispatchCanvasMouse(harness, 'mousedown', { button: 0, buttons: 3 });
    harness.window.dispatchEvent(new FakeMouseEvent('blur'));
    harness.window.dispatchEvent(new FakeMouseEvent('blur'));

    const reset = harness.input.getMouseDiagnostics().find((entry) => entry.transition === 'reset');
    assert.deepEqual(reset, {
      transition: 'reset', reason: 'blur', buttons: 3, staleButtons: 3,
    });

    for (let index = 0; index < 20; index += 1) {
      dispatchCanvasMouse(harness, 'mousedown', { button: 0, buttons: 1 });
      dispatchCanvasMouse(harness, 'mouseup', { button: 0, buttons: 0 });
    }
    const diagnostics = harness.input.getMouseDiagnostics();
    assert.equal(diagnostics.length, 32);
    assert.ok(diagnostics.every((entry) => Object.keys(entry).every((key) =>
      ['transition', 'reason', 'button', 'buttons', 'staleButtons', 'synthetic', 'locked'].includes(key))));
  });
});
