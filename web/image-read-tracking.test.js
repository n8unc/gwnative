import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { createImageReadTracking } from './image-read-tracking.js';

describe('image read tracking', () => {
  it('retains original Promise and removes rejected reads', async () => {
    const events = [];
    const reads = createImageReadTracking({ frameAudit: {
      imageReadQueued: (id, promise) => events.push(['queued', id, promise]),
      imageReadResolved: (id) => events.push(['resolved', id]),
    }});
    let reject;
    const promise = new Promise((resolve, fail) => { reject = fail; });
    reads.set(4, promise);
    assert.equal(reads.get(4), promise);
    reject(new Error('read failed'));
    await Promise.resolve();
    await Promise.resolve();
    await new Promise((done) => setTimeout(done, 0));
    assert.equal(reads.has(4), false);
    assert.deepEqual(events.map(([name, id]) => [name, id]), [['queued', 4], ['resolved', 4]]);
  });

  it('does not delete newer read reusing same id', async () => {
    const reads = createImageReadTracking();
    let reject;
    const old = new Promise((resolve, fail) => { reject = fail; });
    const current = new Promise(() => {});
    reads.set(1, old);
    reads.set(1, current);
    reject(new Error('old failure'));
    await Promise.resolve();
    assert.equal(reads.get(1), current);
  });

  for (const glue of ['Gw.js', 'Gw.jspi.js']) {
    it(`cleans rejected and completes successful generated callback in ${glue}`, async () => {
      const source = readFileSync(new URL(`./fixtures/${glue}.txt`, import.meta.url), 'utf8');
      const line = source.split('\n').find((entry) => entry.includes('const readPromise = Module.image.readAsync'));
      assert.ok(line);
      const events = [];
      let resolve;
      const pending = new Promise((done) => { resolve = done; });
      const Module = { imageReads: createImageReadTracking({ frameAudit: {
        imageReadQueued: (id, promise) => events.push(['queued', id, promise]),
        imageReadResolved: (id) => events.push(['resolved', id]),
      }}), imageReadsSequence: 1, image: { readAsync: () => pending } };
      let completed = 0;
      // Actual callback closes over Module and callbacks supplied as globals.
      const run = runInNewContext(`(${line.slice(line.indexOf(':') + 1).replace(/,\s*$/, '')})`, {
        Module, _EmscriptenExeFileOnImageAsyncReadComplete: () => { completed += 1; },
        _EmscriptenExeFileFatalImageReadError: () => {},
      });
      const id = run(1, 0, 0, 16, 9, 0);
      assert.equal(Module.imageReads.get(id), pending);
      resolve();
      await Promise.resolve();
      await Promise.resolve();
      assert.equal(Module.imageReads.has(id), false);
      assert.equal(completed, 1);
      let reject;
      const failed = new Promise((done, fail) => { reject = fail; });
      Module.image.readAsync = () => failed;
      const failedId = run(1, 0, 0, 16, 9, 0);
      assert.equal(Module.imageReads.get(failedId), failed);
      reject(new Error('synthetic failure'));
      await Promise.resolve();
      await Promise.resolve();
      await new Promise((done) => setTimeout(done, 0));
      assert.equal(Module.imageReads.has(failedId), false);
      assert.deepEqual(events.map(([name]) => name), ['queued', 'resolved', 'queued', 'resolved']);
    });

    it(`keeps rejected ImageWait pending until fatal reaction then cleans ${glue}`, async () => {
      const source = readFileSync(new URL(`./fixtures/${glue}.txt`, import.meta.url), 'utf8');
      const callbackLine = source.split('\n').find((entry) => entry.includes('const readPromise = Module.image.readAsync'));
      const waitLine = source.split('\n').find((entry) => entry.startsWith('function __asyncjs__EmscriptenExeFileImageWait'));
      assert.ok(callbackLine && waitLine);
      let reject;
      const failed = new Promise((resolve, fail) => { reject = fail; });
      let fatalCalls = 0;
      const Module = { imageReads: createImageReadTracking(), imageReadsSequence: 1, image: { readAsync: () => failed } };
      const wait = runInNewContext(`(${waitLine})`, {
        Module, Asyncify: { handleAsync: (fn) => fn() },
      });
      const run = runInNewContext(`(${callbackLine.slice(callbackLine.indexOf(':') + 1).replace(/,\s*$/, '')})`, {
        Module,
        _EmscriptenExeFileOnImageAsyncReadComplete: () => {},
        _EmscriptenExeFileFatalImageReadError: () => { fatalCalls += 1; assert.equal(Module.imageReads.has(1), true); },
      });
      const id = run(1, 0, 0, 16, 9, 0);
      const early = wait(id);
      reject(new Error('failed image'));
      await Promise.resolve();
      const late = wait(id);
      await assert.rejects(early);
      await assert.rejects(late);
      assert.equal(fatalCalls, 1);
      assert.equal(Module.imageReads.has(id), true);
      await new Promise((done) => setTimeout(done, 0));
      assert.equal(Module.imageReads.has(id), false);
    });

    it(`does not add unhandled rejection when audit cleanup throws (${glue})`, async () => {
      const source = readFileSync(new URL(`./fixtures/${glue}.txt`, import.meta.url), 'utf8');
      const line = source.split('\n').find((entry) => entry.includes('const readPromise = Module.image.readAsync'));
      let reject;
      const failure = new Promise((resolve, fail) => { reject = fail; });
      const reads = createImageReadTracking({ frameAudit: {
        imageReadResolved: () => { throw new Error('audit failure'); },
      }});
      const Module = { imageReads: reads, imageReadsSequence: 1, image: { readAsync: () => failure } };
      const run = runInNewContext(`(${line.slice(line.indexOf(':') + 1).replace(/,\s*$/, '')})`, {
        Module, _EmscriptenExeFileOnImageAsyncReadComplete: () => {},
        _EmscriptenExeFileFatalImageReadError: () => {},
      });
      const unhandled = [];
      const capture = (reason) => unhandled.push(reason);
      process.on('unhandledRejection', capture);
      try {
        run(1, 0, 0, 16, 9, 0);
        reject(new Error('synthetic failure'));
        await new Promise((done) => setTimeout(done, 10));
        assert.equal(reads.size, 0);
        assert.deepEqual(unhandled, []);
      } finally {
        process.off('unhandledRejection', capture);
      }
    });
  }
});
