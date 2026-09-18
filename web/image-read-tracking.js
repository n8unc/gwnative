// Host-owned image-read registry. Generated glue installs its Promise here;
// rejected reads are removed while preserving Promise identity and callbacks.

/**
 * @param {{ frameAudit?: { imageReadQueued?: Function, imageReadResolved?: Function, trackImageWait?: Function }, scheduleCleanup?: (callback: () => void) => unknown }} options
 */
export function createImageReadTracking({ frameAudit = null, scheduleCleanup = (cleanup) => setTimeout(cleanup, 0) } = {}) {
  const reads = new Map();
  const get = reads.get.bind(reads);
  const set = reads.set.bind(reads);
  const remove = reads.delete.bind(reads);

  reads.get = (id) => {
    const promise = get(id);
    return frameAudit?.trackImageWait ? frameAudit.trackImageWait(promise, id) : promise;
  };
  reads.set = (id, promise) => {
    frameAudit?.imageReadQueued?.(id, promise);
    set(id, promise);
    if (promise && typeof promise.then === 'function') {
      // Attach a rejection handler without replacing stored Promise. Returned
      // observation Promise always fulfills, so cleanup adds no unhandled error.
      promise.then(undefined, () => {
        try {
          // Run in next task after generated glue's fatal reaction, retaining
          // rejected Promise for late ImageWait callers in current turn.
          scheduleCleanup(() => {
            try {
              if (get(id) === promise) reads.delete(id);
            } catch {
              // Instrumentation must not create a second unhandled rejection.
            }
          });
        } catch {
          // Instrumentation must not create a second unhandled rejection.
        }
      });
    }
    return reads;
  };
  reads.delete = (id) => {
    const deleted = remove(id);
    if (deleted) frameAudit?.imageReadResolved?.(id);
    return deleted;
  };
  return reads;
}
