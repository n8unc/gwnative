/**
 * Prepare the client-side launcher credential capability before WASM starts.
 *
 * Password stays in the host-provided read function. Managed account name is
 * copied into client memory so its existing credential callback can match the
 * returned login to visible account field; account name remains protected from
 * diagnostics and only crosses this private in-process boundary.
 */
export async function prepareLauncherCredentials({ managed, readSaved, exports, log, timeoutMs = 5_000 }) {
  const setter = exports && typeof exports.GwnativeSetLauncherCredentialsAvailable === 'function'
    ? exports.GwnativeSetLauncherCredentialsAvailable
    : null;
  const nameSetter = exports && typeof exports.GwnativeSetLauncherAccountName === 'function'
    ? exports.GwnativeSetLauncherAccountName
    : null;
  let namePointer = 0;

  if (setter) {
    try {
      setter(0);
    } catch {
      log?.('launcher credentials unavailable');
      return false;
    }
  } else if (managed) {
    log?.('launcher credentials unavailable');
  }

  if (managed !== true || typeof readSaved !== 'function') return false;

  let saved;
  try {
    const pending = Promise.resolve().then(readSaved);
    saved = await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('credential read timed out')), timeoutMs);
      pending.then((value) => { clearTimeout(timeout); resolve(value); }, (error) => {
        clearTimeout(timeout);
        reject(error);
      });
    });
  } catch {
    log?.('launcher credentials unavailable');
    return false;
  }
  if (typeof saved?.username !== 'string' || saved.username.length === 0
    || typeof saved?.password !== 'string' || saved.password.length === 0) {
    log?.('launcher credentials unavailable');
    return false;
  }
  if (!setter || !nameSetter || typeof exports.malloc !== 'function' || !exports.memory?.buffer) return false;

  try {
    const encoded = [];
    for (const character of saved.username) {
      const code = character.codePointAt(0);
      if (code <= 0xffff) encoded.push(code);
      else {
        const value = code - 0x10000;
        encoded.push(0xd800 + (value >> 10), 0xdc00 + (value & 0x3ff));
      }
    }
    namePointer = Number(exports.malloc((encoded.length + 1) * 2));
    if (!Number.isInteger(namePointer) || namePointer <= 0 || namePointer % 2 !== 0
      || namePointer + (encoded.length + 1) * 2 > exports.memory.buffer.byteLength) throw new Error('credential name allocation failed');
    const name = new Uint16Array(exports.memory.buffer, namePointer, encoded.length + 1);
    name.set(encoded); name[encoded.length] = 0;
    nameSetter(namePointer);
    setter(1);
    log?.('launcher credentials ready');
    return true;
  } catch {
    let cleared = false;
    try { setter?.(0); nameSetter(0); cleared = true; } catch {}
    if (cleared && namePointer && typeof exports.free === 'function') {
      try { exports.free(namePointer); } catch {}
    }
    log?.('launcher credentials unavailable');
    return false;
  }
}
