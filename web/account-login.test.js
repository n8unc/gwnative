import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { certifiedLoginProbePaths, installCertifiedLoginProbe } from '../ui/login-probe.js';

class Field extends EventTarget {}

class Request extends EventTarget {
  open(method, url) {
    this.method = method;
    this.url = url;
  }

  send() {}

  finish(status) {
    this.status = status;
    this.dispatchEvent(new Event('loadend'));
  }
}

const setup = (options = {}) => {
  const events = [];
  const module = { oskInput: { email: new Field(), password: new Field() } };
  const probe = installCertifiedLoginProbe({
    module,
    enabled: true,
    artifactHash: 'exact-build',
    expectedArtifactHash: 'exact-build',
    report: (event) => events.push(event),
    XMLHttpRequestCtor: Request,
    ...options,
  });
  return { events, module, probe };
};

describe('certified login probe', () => {
  it('does nothing unless explicitly enabled for exact artifact', () => {
    const disabled = installCertifiedLoginProbe({ module: {}, enabled: false });
    assert.deepEqual(disabled, { active: false, reason: 'disabled' });
    const mismatch = installCertifiedLoginProbe({
      module: {}, enabled: true, artifactHash: 'old', expectedArtifactHash: 'new',
    });
    assert.deepEqual(mismatch, { active: false, reason: 'uncertified-artifact' });
  });

  it('reports field type and Return gesture without inspecting a value', () => {
    const { events, module, probe } = setup();
    module.oskInput.email.dispatchEvent(new Event('focus'));
    const enter = new Event('keydown');
    Object.defineProperty(enter, 'key', { value: 'Enter' });
    module.oskInput.password.dispatchEvent(enter);
    assert.deepEqual(events, [
      { kind: 'field-opened', field: 'email' },
      { kind: 'submit-gesture', field: 'password' },
    ]);
    probe.dispose();
  });

  it('reports only exact login request path and status, never payload or response', () => {
    const { events, probe } = setup();
    const login = new Request();
    login.open('POST', '/webgate/users/login.xml');
    login.send({ password: 'not observed' });
    login.finish(200);
    const unrelated = new Request();
    unrelated.open('POST', '/webgate/other.xml');
    unrelated.send({ password: 'not observed' });
    unrelated.finish(200);
    assert.deepEqual(events, [
      { kind: 'request-started', path: '/webgate/users/login.xml' },
      { kind: 'request-finished', path: '/webgate/users/login.xml', status: 200 },
    ]);
    probe.dispose();
  });

  it('restores XMLHttpRequest methods on disposal', () => {
    const originalOpen = Request.prototype.open;
    const originalSend = Request.prototype.send;
    const { probe } = setup();
    probe.dispose();
    assert.equal(Request.prototype.open, originalOpen);
    assert.equal(Request.prototype.send, originalSend);
  });

  it('names only the two protected login endpoints', () => {
    assert.deepEqual(certifiedLoginProbePaths, [
      '/webgate/users/login.xml',
      '/webgate/my_account/upgrade_login.xml',
    ]);
  });
});
