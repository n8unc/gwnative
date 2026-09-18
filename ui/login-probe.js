// Exact-build login diagnostics.
//
// This observes the client-owned OSK bridge and login request boundary without
// reading a field, request body, response body, header, cookie, or credential.
// It deliberately never focuses a field, synthesises Return, or submits login.

const LOGIN_PATHS = new Set([
  '/webgate/users/login.xml',
  '/webgate/my_account/upgrade_login.xml',
]);

const probePath = (url) => {
  try {
    return new URL(String(url), 'http://launcher.invalid').pathname;
  } catch {
    return '';
  }
};

const frozen = (event) => Object.freeze(event);

/**
 * Install an opt-in observation probe for one certified game artifact.
 *
 * `artifactHash` and `expectedArtifactHash` are supplied by the certified
 * launch descriptor. A mismatch makes this a no-op, so a changed client cannot
 * silently inherit assumptions about its login UI.
 *
 * @param {{
 *   module: { oskInput?: Record<string, EventTarget | undefined> },
 *   enabled?: boolean,
 *   artifactHash?: string,
 *   expectedArtifactHash?: string,
 *   report?: (event: Readonly<Record<string, string | number>>) => void,
 *   XMLHttpRequestCtor?: { prototype: XMLHttpRequest },
 * }} options
 */
export function installCertifiedLoginProbe({
  module,
  enabled = false,
  artifactHash,
  expectedArtifactHash,
  report = () => {},
  XMLHttpRequestCtor = globalThis.XMLHttpRequest,
}) {
  if (!enabled) return Object.freeze({ active: false, reason: 'disabled' });
  if (!artifactHash || artifactHash !== expectedArtifactHash) {
    return Object.freeze({ active: false, reason: 'uncertified-artifact' });
  }
  if (!module?.oskInput || !XMLHttpRequestCtor?.prototype) {
    return Object.freeze({ active: false, reason: 'bridge-unavailable' });
  }

  const emit = (event) => report(frozen(event));
  const cleanups = [];
  for (const type of ['email', 'password']) {
    const field = module.oskInput[type];
    if (!field?.addEventListener) continue;
    const onFocus = () => emit({ kind: 'field-opened', field: type });
    const onKeydown = (event) => {
      if (event.key === 'Enter') emit({ kind: 'submit-gesture', field: type });
    };
    field.addEventListener('focus', onFocus);
    field.addEventListener('keydown', onKeydown);
    cleanups.push(() => {
      field.removeEventListener('focus', onFocus);
      field.removeEventListener('keydown', onKeydown);
    });
  }

  const prototype = XMLHttpRequestCtor.prototype;
  const originalOpen = prototype.open;
  const originalSend = prototype.send;
  const requests = new WeakMap();
  prototype.open = function launcherLoginProbeOpen(method, url, ...rest) {
    requests.set(this, {
      method: String(method).toUpperCase(),
      path: probePath(url),
      attached: false,
    });
    return originalOpen.call(this, method, url, ...rest);
  };
  prototype.send = function launcherLoginProbeSend(...args) {
    const request = requests.get(this);
    if (request && request.method === 'POST' && LOGIN_PATHS.has(request.path) && !request.attached) {
      request.attached = true;
      emit({ kind: 'request-started', path: request.path });
      const finish = () => emit({
        kind: 'request-finished',
        path: request.path,
        status: Number(this.status) || 0,
      });
      this.addEventListener('loadend', finish, { once: true });
    }
    return originalSend.apply(this, args);
  };
  cleanups.push(() => {
    prototype.open = originalOpen;
    prototype.send = originalSend;
  });

  return Object.freeze({
    active: true,
    // A login HTTP response is not authentication success. The caller must
    // correlate it with a separately certified game-state transition.
    dispose: () => cleanups.splice(0).reverse().forEach((cleanup) => cleanup()),
  });
}

export const certifiedLoginProbePaths = Object.freeze([...LOGIN_PATHS]);
