// Choose the official client build that the WebKit in this WKWebView can run.
//
// Safari, Safari Technology Preview and WKWebView do not necessarily use the
// same WebKit. In particular, installing a browser with JSPI does not add JSPI
// to an application's system WKWebView. Capability detection has to happen in
// this realm, and presence alone is not enough: exercise a suspended import and
// its promised export before selecting ArenaNet's JSPI build.

const PROBE_WASM = new Uint8Array([
  0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
  0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f,
  0x02, 0x07, 0x01, 0x01, 0x65, 0x01, 0x66, 0x00, 0x00,
  0x03, 0x02, 0x01, 0x00,
  0x07, 0x05, 0x01, 0x01, 0x67, 0x00, 0x01,
  0x0a, 0x06, 0x01, 0x04, 0x00, 0x10, 0x00, 0x0b,
]);

const CLIENTS = Object.freeze({
  jspi: Object.freeze({
    mode: 'jspi',
    glue: 'Gw.jspi.js',
    wasm: 'Gw.jspi.wasm',
  }),
  asyncify: Object.freeze({
    mode: 'asyncify',
    glue: 'Gw.js',
    wasm: 'Gw.wasm',
  }),
});

const RUNTIME_STATE_DEADLINE_MS = 1_500;
const PROOF_RETRY_DELAYS_MS = Object.freeze([0, 50, 150, 350]);
const JSPI_PROBE_DEADLINE_MS = 1_500;
const JSPI_PROBE_TIMED_OUT = Symbol('JSPI probe timed out');

/**
 * Prove that this realm implements the JSPI operation the client needs.
 *
 * @param {typeof WebAssembly} wasm
 * @param {number} deadlineMs
 * @returns {Promise<boolean>}
 */
export async function supportsJspi(
  wasm = WebAssembly,
  deadlineMs = JSPI_PROBE_DEADLINE_MS,
) {
  if (
    typeof wasm?.Suspending !== 'function'
    || typeof wasm?.promising !== 'function'
  ) {
    return false;
  }
  try {
    const module = new wasm.Module(PROBE_WASM);
    const imports = {
      e: {
        f: new wasm.Suspending(async () => {
          await 0;
          return 42;
        }),
      },
    };
    const instance = new wasm.Instance(module, imports);
    let deadline;
    const result = await Promise.race([
      wasm.promising(instance.exports.g)(),
      new Promise((resolve) => {
        deadline = setTimeout(() => resolve(JSPI_PROBE_TIMED_OUT), deadlineMs);
      }),
    ]).finally(() => clearTimeout(deadline));
    return result === 42;
  } catch {
    return false;
  }
}

/**
 * Select the matching official glue and module pair.
 *
 * `forced` is a bring-up hook injected by the native host from
 * `GWNATIVE_CLIENT_RUNTIME`. It lets a runner exercise both paths, but forcing
 * JSPI still has to pass the functional probe: an override must not turn a
 * compatibility test into an avoidable crash.
 *
 * @param {typeof WebAssembly} wasm
 * @param {unknown} forced
 * @returns {Promise<(typeof CLIENTS)[keyof typeof CLIENTS]>}
 */
export async function selectClient(
  wasm = WebAssembly,
  forced = globalThis.__gwnativeClientRuntime,
  plan = { failedOfficial: [] },
) {
  const failed = new Set(plan.failedOfficial ?? []);
  if (forced === 'asyncify') {
    if (failed.has('asyncify')) {
      throw new Error('The forced Asyncify runtime already failed for these exact official bytes.');
    }
    return CLIENTS.asyncify;
  }

  const jspi = await supportsJspi(wasm);
  if (forced === 'jspi' && !jspi) {
    throw new Error(
      'JSPI was requested for this test, but this WKWebView failed its suspend/resume probe.',
    );
  }
  if (jspi && !failed.has('jspi')) return CLIENTS.jspi;
  if (!failed.has('asyncify')) return CLIENTS.asyncify;
  throw new Error('No compatible official runtime remains after the exact recorded failures.');
}

/** Read the durable launch plan before selecting any glue in this realm. */
export async function readRuntimePlan(options = {}) {
  const send = options.fetch ?? fetch;
  const token = options.token ?? globalThis.__gwnativeToken ?? '';
  const deadlineMs = options.deadlineMs ?? RUNTIME_STATE_DEADLINE_MS;
  const controller = new AbortController();
  const deadline = setTimeout(() => controller.abort(), deadlineMs);
  try {
    const response = await send('__runtime-plan', {
      headers: { 'X-Gwnative-Token': token },
      signal: controller.signal,
    });
    if (!response.ok) {
      throw new Error((await response.text()) || `runtime plan failed: ${response.status}`);
    }
    if (response.status < 220 || response.status > 223) {
      throw new Error('the host returned an invalid runtime plan');
    }
    const mask = response.status - 220;
    return {
      failedOfficial: [
        ...(mask & 1 ? ['jspi'] : []),
        ...(mask & 2 ? ['asyncify'] : []),
      ],
    };
  } finally {
    clearTimeout(deadline);
  }
}

/**
 * Apply the independently certified facts for the selected official runtime.
 *
 * The host prepares both modules before this realm can perform the JSPI probe.
 * Keeping the facts keyed by runtime prevents a macOS 26 Asyncify selection
 * from inheriting JSPI hashes, while preserving the JSPI certificate unchanged
 * on macOS 27.
 *
 * @param {(typeof CLIENTS)[keyof typeof CLIENTS]} client
 * @param {{ nativeCursor?: unknown, targetReadout?: unknown }} settings
 * @param {Record<string, unknown>} target
 */
export function applyClientLimits(client, settings, target = globalThis) {
  const selected = target.__gwnativeRuntimeCapabilities?.[client.mode];
  const wanted = settings.nativeCursor === true || settings.targetReadout === true;
  target.__gwnativeClientBuild = selected?.build ?? null;
  target.__gwnativePreparedTransform = selected?.preparedTransform
    ?? (selected?.templateSave === 'ready' || selected?.characterStartup === true);
  target.__gwnativeCharacterCapability = {
    supported: selected?.characterStartup === true && typeof selected?.build === 'string',
    runtime: client.mode, build: selected?.build ?? '',
    operations: selected?.characterStartup === true ? [
      'observeReady', 'readRoster', 'selectCharacter', 'readSelected', 'enterCharacter', 'observeEntered',
    ] : [],
  };
  target.__gwnativeTemplateSave = selected?.templateSave ?? 'uncertified';
  target.__gwnativeEnhancements = selected?.enhancements ?? (wanted ? 'uncertified' : 'off');
  target.__gwnativeEnhancementManifest = selected?.enhancementManifest ?? null;
}

/**
 * Persist launch/fallback state without allowing an auxiliary loopback write
 * to hold the client boot indefinitely.
 *
 * @param {string} path
 * @param {object} body
 * @param {{ fetch?: typeof fetch, token?: string, deadlineMs?: number }} options
 */
export async function postRuntimeState(path, body, options = {}) {
  const send = options.fetch ?? fetch;
  const token = options.token ?? globalThis.__gwnativeToken ?? '';
  const deadlineMs = options.deadlineMs ?? RUNTIME_STATE_DEADLINE_MS;
  const controller = new AbortController();
  const deadline = setTimeout(() => controller.abort(), deadlineMs);
  try {
    const response = await send(path, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Gwnative-Token': token,
      },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
    if (!response.ok) {
      throw new Error((await response.text()) || `${path} failed: ${response.status}`);
    }
    if (path === '__runtime-failed') {
      if (response.status === 224) return { outcome: 'try-runtime', runtime: 'asyncify' };
      if (response.status === 225) return { outcome: 'predecessor-restored' };
      if (response.status === 226) return { outcome: 'exhausted' };
    }
    if (response.status !== 204) {
      throw new Error(`the host returned an invalid ${path} acknowledgement`);
    }
    return null;
  } finally {
    clearTimeout(deadline);
  }
}

/**
 * Deliver an idempotent launch proof in the background. A reply can disappear
 * after the host has committed the proof, so retries send byte-for-byte the
 * same launch identity and let the host distinguish that from stale evidence.
 *
 * @param {string} path
 * @param {object} body
 * @param {{ post?: Function, wait?: Function, delays?: number[] }} options
 */
export async function deliverRuntimeProof(path, body, options = {}) {
  const post = options.post ?? ((proofPath, proofBody) =>
    postRuntimeState(proofPath, proofBody, options));
  const wait = options.wait ?? ((milliseconds) =>
    new Promise((resolve) => setTimeout(resolve, milliseconds)));
  const delays = options.delays ?? PROOF_RETRY_DELAYS_MS;
  let failure = new Error('proof delivery had no attempts');
  for (const delay of delays) {
    if (delay) await wait(delay);
    try {
      return await post(path, body);
    } catch (error) {
      failure = error;
    }
  }
  throw failure;
}

/**
 * Persist an exact original-runtime failure, then leave the contaminated
 * WKWebView behind. The host starts the successor before this page disappears.
 */
export async function transitionRuntimeFailure(launch, options = {}) {
  const post = options.post ?? ((path, body) => postRuntimeState(path, body, options));
  const relaunch = options.relaunch;
  const result = await deliverRuntimeProof('__runtime-failed', { launch }, {
    post,
    wait: options.wait,
    delays: options.delays,
  });
  if (result?.outcome === 'exhausted') {
    throw new Error('Both usable official runtimes are exhausted; no predecessor was removed.');
  }
  if (
    result?.outcome !== 'predecessor-restored'
    && !(result?.outcome === 'try-runtime' && result.runtime === 'asyncify')
  ) {
    throw new Error('the host returned an invalid runtime failure transition');
  }
  if (typeof relaunch !== 'function') {
    throw new Error('runtime transition has no fresh-realm relaunch');
  }
  await relaunch();
  return result;
}

/**
 * Own launch identity and startup recovery for the generated-client adapter.
 * The four returned operations are intentionally the only lifecycle surface:
 * persistence, fallback, first-frame proof, and failure policy stay here while
 * the harness remains responsible for DOM and generated glue details.
 *
 * @param {{ mode: string, wasm: string }} options.client
 * @param {Record<string, unknown>} options.target
 * @param {Function} options.relaunch
 * @param {Record<string, unknown>} options.proofOptions
 * @param {Function} options.onTransition
 * @param {Function} options.onOriginalFallback
 */
export function createRuntimeLifecycle({
  client,
  target = globalThis,
  relaunch,
  proofOptions = {},
  onTransition = () => {},
  onOriginalFallback = () => {},
}) {
  let active;
  let startPromise;
  let instantiationPromise;
  let firstFrameSeen = false;
  let transitionPromise;
  let bootProofPromise;
  let stoppedError;
  let terminal = false;
  let attemptInFlight;
  let transformFailurePromise;

  const proof = (path, body) => deliverRuntimeProof(path, body, proofOptions);
  const identity = (transformed, source = active) => Object.freeze({
    runtime: source?.runtime ?? client.mode,
    build: transformed ? source?.build ?? target.__gwnativeClientBuild : null,
    transformed,
    nonce: source?.nonce ?? target.__gwnativeLaunchNonce,
  });

  const persistAttempt = async (transformed, source = active) => {
    const launch = identity(transformed, source);
    const operation = proof('__runtime', launch);
    attemptInFlight = operation;
    await operation;
    active = launch;
    target.__gwnativeLaunchIdentity = launch;
    return launch;
  };

  const start = (loadGlue) => {
    if (startPromise) return startPromise;
    if (terminal) return Promise.reject(stoppedError ?? new Error('runtime launch stopped'));
    startPromise = persistAttempt(
      (target.__gwnativePreparedTransform
        ?? (target.__gwnativeTemplateSave === 'ready' || target.__gwnativeCharacterCapability?.supported === true))
        && typeof target.__gwnativeClientBuild === 'string',
    ).then((launch) => {
      if (terminal || stoppedError) throw stoppedError ?? new Error('runtime launch stopped');
      if (typeof loadGlue === 'function') loadGlue();
      return launch;
    });
    return startPromise;
  };

  const instantiate = (load) => {
    if (instantiationPromise) return instantiationPromise;
    instantiationPromise = (async () => {
      const source = client.wasm;
      if (terminal || firstFrameSeen) throw stoppedError ?? new Error('runtime launch stopped');
      try {
        const result = await load(source);
        if (terminal || firstFrameSeen) throw stoppedError ?? new Error('runtime launch stopped');
        return result;
      } catch (error) {
        if (!active?.transformed || firstFrameSeen || terminal) throw error;
        transformFailurePromise ??= proof('__transform-failed', { launch: active });
        await transformFailurePromise;
        if (terminal || firstFrameSeen) throw stoppedError ?? error;
        target.__gwnativeTemplateSave = 'failed';
        target.__gwnativePreparedTransform = false;
        target.__gwnativeEnhancements = 'off';
        target.__gwnativeEnhancementManifest = null;
        target.__gwnativeCharacterCapability = { supported: false, runtime: client.mode, build: '', operations: [] };
        try { onOriginalFallback(error); } catch { /* notification cannot alter policy */ }
        const original = await persistAttempt(false, active);
        if (terminal || firstFrameSeen) throw stoppedError ?? error;
        const result = await load(`${source}?gwnative-original=1`, original);
        if (terminal || firstFrameSeen) throw stoppedError ?? error;
        return result;
      }
    })();
    return instantiationPromise;
  };

  const firstFrame = () => {
    if (firstFrameSeen) return bootProofPromise;
    firstFrameSeen = true;
    if (!active) {
      bootProofPromise = Promise.reject(new Error('first frame has no launch identity'));
      return bootProofPromise;
    }
    bootProofPromise ??= proof('__booted', { launch: active }).catch((error) => {
      throw error;
    });
    return bootProofPromise;
  };

  const fail = (reason) => {
    const error = reason instanceof Error ? reason : new Error(String(reason));
    if (firstFrameSeen) {
      const stopped = new Error(`The game client stopped unexpectedly: ${error.message}`);
      stoppedError ??= stopped;
      return Promise.reject(stoppedError);
    }
    if (transitionPromise) return transitionPromise;
    const acknowledgedAtInvocation = Boolean(active);
    terminal = true;
    stoppedError = new Error(`The game client stopped unexpectedly: ${error.message}`);
    transitionPromise = (async () => {
      let launch;
      try {
        if (!acknowledgedAtInvocation) throw stoppedError;
        if (attemptInFlight) await attemptInFlight;
        launch = active;
        if (!launch) throw stoppedError;
      } catch (cause) {
        throw cause;
      }
      if (firstFrameSeen) throw stoppedError;
      if (launch.transformed) {
        transformFailurePromise ??= proof('__transform-failed', { launch });
        await transformFailurePromise;
        if (firstFrameSeen) throw stoppedError;
        if (typeof relaunch !== 'function') throw new Error('runtime transition has no fresh-realm relaunch');
        try { onTransition(error); } catch { /* notification cannot alter policy */ }
        await relaunch();
        return;
      }
      if (typeof relaunch !== 'function') throw new Error('runtime transition has no fresh-realm relaunch');
      if (firstFrameSeen) throw stoppedError;
      await transitionRuntimeFailure(launch, {
        ...proofOptions,
        relaunch: async () => {
          if (firstFrameSeen) throw stoppedError;
          try { onTransition(error); } catch { /* notification cannot alter policy */ }
          return relaunch();
        },
      });
    })().catch((failure) => {
      if (failure === stoppedError) throw failure;
      const wrapped = new Error(`The game client could not start: ${failure?.message ?? failure}`);
      stoppedError = wrapped;
      throw wrapped;
    });
    return transitionPromise;
  };

  return Object.freeze({ start, instantiate, firstFrame, fail });
}
