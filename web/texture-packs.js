// Bounded replacement seam for prepared legacy TexMod mappings.
//
// Runtime consumes child-pinned manifest only. It never opens player source
// paths and always delegates unchanged upload when a condition is unknown.

const GL_TEXTURE_2D = 0x0DE1;
const GL_TEXTURE0 = 0x84C0;
const GL_RGBA = 0x1908;
const GL_RGBA8 = 0x8058;
const GL_UNSIGNED_BYTE = 0x1401;
const GL_UNPACK_ALIGNMENT = 0x0CF5;
const GL_UNPACK_ROW_LENGTH = 0x0CF2;
const GL_UNPACK_SKIP_ROWS = 0x0CF3;
const GL_UNPACK_SKIP_PIXELS = 0x0CF4;
const GL_PIXEL_UNPACK_BUFFER_BINDING = 0x88EF;
const MAX_ENTRIES = 1024;
const MAX_BYTES = 64 * 1024 * 1024;

const table = Uint32Array.from({ length: 256 }, (_, value) => {
  let crc = value;
  for (let bit = 0; bit < 8; bit += 1) crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
  return crc >>> 0;
});

function hash(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) crc = table[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return crc >>> 0;
}

function transformed(source, width, height, flip, swap) {
  if (!flip && !swap) return source;
  const output = new Uint8Array(source.byteLength);
  for (let y = 0; y < height; y += 1) for (let x = 0; x < width; x += 1) {
    const from = (y * width + x) * 4;
    const to = ((flip ? height - y - 1 : y) * width + x) * 4;
    output[to] = source[from + (swap ? 2 : 0)];
    output[to + 1] = source[from + 1];
    output[to + 2] = source[from + (swap ? 0 : 2)];
    output[to + 3] = source[from + 3];
  }
  return output;
}
function mip(source, width, height, level) {
  while (level > 0) {
    const nextWidth = Math.max(1, width >> 1); const nextHeight = Math.max(1, height >> 1);
    const output = new Uint8Array(nextWidth * nextHeight * 4);
    for (let y = 0; y < nextHeight; y += 1) for (let x = 0; x < nextWidth; x += 1) for (let c = 0; c < 4; c += 1) {
      let total = 0;
      for (let dy = 0; dy < 2; dy += 1) for (let dx = 0; dx < 2; dx += 1) total += source[(Math.min(height - 1, y * 2 + dy) * width + Math.min(width - 1, x * 2 + dx)) * 4 + c];
      output[(y * nextWidth + x) * 4 + c] = Math.round(total / 4);
    }
    source = output; width = nextWidth; height = nextHeight; level -= 1;
  }
  return source;
}

function region(source, sourceWidth, x, y, width, height) {
  const output = new Uint8Array(width * height * 4);
  for (let row = 0; row < height; row += 1) {
    const start = ((y + row) * sourceWidth + x) * 4;
    output.set(source.subarray(start, start + width * 4), row * width * 4);
  }
  return output;
}

function compressedBytes(mode, width, height) {
  const blockBytes = mode === 'DXT1' ? 8 : mode === 'DXT3' || mode === 'DXT5' ? 16 : 0;
  const blocks = Math.ceil(width / 4) * Math.ceil(height / 4);
  const bytes = blocks * blockBytes;
  return Number.isSafeInteger(bytes) && bytes > 0 && bytes <= MAX_BYTES ? bytes : 0;
}

function compatibleCompressedFormat(mode, format) {
  return mode === 'DXT1' ? format === 0x83F0 || format === 0x83F1
    : mode === 'DXT3' ? format === 0x83F2 : mode === 'DXT5' && format === 0x83F3;
}

function decode(value) {
  if (typeof value !== 'string' || value.length === 0 || value.length > Math.ceil(MAX_BYTES * 4 / 3)) return null;
  try {
    const binary = atob(value);
    if (binary.length > MAX_BYTES) return null;
    return Uint8Array.from(binary, (byte) => byte.charCodeAt(0));
  } catch { return null; }
}

function mappings(manifest) {
  if (!manifest || manifest.format !== 1 || !Array.isArray(manifest.packs)) return new Map();
  const result = new Map();
  let decodedBytes = 0;
  for (const pack of manifest.packs) {
    if (!Array.isArray(pack?.entries)) continue;
    for (const entry of pack.entries) {
      if (result.size >= MAX_ENTRIES || !Number.isInteger(entry?.target) || entry.target < 0 || entry.target > 0xffffffff
        || !Number.isInteger(entry.width) || !Number.isInteger(entry.height) || entry.width < 1 || entry.height < 1
        || entry.width > 4096 || entry.height > 4096 || result.has(entry.target)) continue;
      const expectedBytes = entry.width * entry.height * 4;
      if (!Number.isSafeInteger(expectedBytes) || expectedBytes > MAX_BYTES || expectedBytes > MAX_BYTES - decodedBytes) continue;
      const rgba = decode(entry.rgbaBase64);
      if (rgba?.byteLength !== expectedBytes) continue;
      decodedBytes += rgba.byteLength;
      let compressed = null;
      if (entry.compressed && typeof entry.compressed === 'object'
        && (entry.compressed.mode === 'DXT1' || entry.compressed.mode === 'DXT3' || entry.compressed.mode === 'DXT5')
        && Array.isArray(entry.compressed.levels)) {
        const levels = [];
        let valid = entry.compressed.levels.length > 0;
        for (let level = 0; valid && level < entry.compressed.levels.length; level += 1) {
          const levelWidth = Math.max(1, entry.width >> level); const levelHeight = Math.max(1, entry.height >> level);
          const expected = compressedBytes(entry.compressed.mode, levelWidth, levelHeight);
          const bytes = decode(entry.compressed.levels[level]);
          if (!expected || !bytes || bytes.byteLength !== expected || bytes.byteLength > MAX_BYTES - decodedBytes) valid = false;
          else { decodedBytes += bytes.byteLength; levels.push(bytes); }
        }
        if (valid) compressed = { mode: entry.compressed.mode, levels };
      }
      result.set(entry.target >>> 0, { width: entry.width, height: entry.height, rgba, compressed });
    }
  }
  return result;
}

// Emscripten publishes Module.ctx after it creates WebGL context. A non-default
// unpack state changes pointer semantics, so replacement needs proven default.
function packedHeapUpload(module) {
  const context = module?.ctx;
  if (!context || typeof context.getParameter !== 'function') return false;
  try {
    if (typeof context.isContextLost === 'function' && context.isContextLost()) return false;
    if (context.getParameter(GL_UNPACK_ALIGNMENT) !== 4
      || context.getParameter(GL_PIXEL_UNPACK_BUFFER_BINDING) != null) return false;
    // WebGL 1 rejects these WebGL 2-only parameter enums. On WebGL 2, require
    // the tightly packed source view the CRC and temporary heap write assume.
    if (typeof context.texStorage2D !== 'function') return true;
    return context.getParameter(GL_UNPACK_ROW_LENGTH) === 0
      && context.getParameter(GL_UNPACK_SKIP_ROWS) === 0
      && context.getParameter(GL_UNPACK_SKIP_PIXELS) === 0;
  } catch { return false; }
}

/**
 * Install after imports exist; Emscripten may publish HEAPU8 only after Wasm
 * instantiation, so heap/context validation deliberately happens on each
 * upload. Compressed uploads and later mip levels remain baseline until
 * separately certified against WebKit/WebGL.
 */
export function installTexturePacks({ manifest, imports, module, log = () => {}, diagnostics = false }) {
  const byTarget = mappings(manifest);
  const env = imports?.env;
  const original = env?.glTexImage2D;
  if (!byTarget.size || typeof original !== 'function' || !module) return null;
  let replacements = 0;
  const bindings = new Map(); const matchedTextures = new Map(); const storages = new Map(); let activeUnit = 0;
  const counts = {
    uploads: 0, eligibleUploads: 0, hashMatches: 0, mip: 0,
    texStorage2D: 0, texSubImage2D: 0,
    compressedTexImage2D: 0, compressedTexSubImage2D: 0,
  };
  const activeTexture = env.glActiveTexture;
  if (typeof activeTexture === 'function') env.glActiveTexture = function (unit) { const value = activeTexture.call(this, unit); if (Number.isInteger(unit) && unit >= GL_TEXTURE0) activeUnit = unit - GL_TEXTURE0; return value; };
  const bindingKey = (target) => `${activeUnit}:${target}`;
  const bind = env.glBindTexture;
  if (typeof bind === 'function') env.glBindTexture = function (target, texture) { const value = bind.call(this, target, texture); if (target === GL_TEXTURE_2D) bindings.set(bindingKey(target), texture); return value; };
  const deleted = env.glDeleteTextures;
  if (typeof deleted === 'function') env.glDeleteTextures = function (count, pointer) { const heap = module.HEAPU8; if (heap && Number.isInteger(count) && count >= 0 && count <= Math.floor(heap.byteLength / 4) && Number.isInteger(pointer) && pointer >= 0 && pointer <= heap.byteLength - count * 4) { const view = new DataView(heap.buffer, heap.byteOffset + pointer, count * 4); for (let index = 0; index < count; index += 1) matchedTextures.delete(view.getUint32(index * 4, true)); } return deleted.call(this, count, pointer); };
  const storage = env.glTexStorage2D;
  if (typeof storage === 'function') env.glTexStorage2D = function (target, levels, internalFormat, width, height) {
    counts.texStorage2D += 1;
    const value = storage.call(this, target, levels, internalFormat, width, height);
    const texture = target === GL_TEXTURE_2D ? bindings.get(bindingKey(target)) : null;
    if (texture) { matchedTextures.delete(texture); storages.set(texture, { levels, internalFormat, width, height }); }
    return value;
  };
  env.glTexImage2D = function (target, level, internalFormat, width, height, border, format, type, pointer) {
    counts.uploads += 1;
    const call = () => original.call(this, target, level, internalFormat, width, height, border, format, type, pointer);
    let source = null;
    let replacement = null;
    let heap = null;
    try {
      const bytes = width * height * 4;
      heap = module.HEAPU8;
      const eligible = target === GL_TEXTURE_2D && border === 0 && format === GL_RGBA && type === GL_UNSIGNED_BYTE
        && (internalFormat === GL_RGBA || internalFormat === GL_RGBA8) && Number.isSafeInteger(pointer) && pointer > 0
        && Number.isInteger(width) && width > 0 && Number.isInteger(height) && height > 0
        && heap && heap instanceof Uint8Array && Number.isSafeInteger(bytes) && bytes >= 1 && bytes <= MAX_BYTES
        && pointer <= heap.byteLength - bytes && packedHeapUpload(module);
      const texture = bindings.get(bindingKey(GL_TEXTURE_2D));
      if (eligible && level > 0 && texture && matchedTextures.has(texture)) {
        source = heap.slice(pointer, pointer + bytes);
        const association = matchedTextures.get(texture); const expectedWidth = Math.max(1, association.match.width >> level); const expectedHeight = Math.max(1, association.match.height >> level);
        if (width === expectedWidth && height === expectedHeight) replacement = transformed(mip(association.match.rgba, association.match.width, association.match.height, level), width, height, association.flip, false);
        else matchedTextures.delete(texture);
      } else if (eligible) {
        counts.eligibleUploads += 1;
        source = heap.slice(pointer, pointer + bytes);
        let match = byTarget.get(hash(source));
        let flip = false;
        if (!match) {
          for (const candidate of [[false, true], [true, false], [true, true]]) {
            match = byTarget.get(hash(transformed(source, width, height, candidate[0], candidate[1])));
            if (match) { flip = candidate[0]; break; }
          }
        }
        if (match?.width === width && match.height === height) {
          counts.hashMatches += 1;
          // R/B swapping discovers Direct3D-style source identifiers only.
          // Prepared replacements are RGBA and remain RGBA for this upload.
          replacement = transformed(match.rgba, width, height, flip, false);
          if (texture) matchedTextures.set(texture, { kind: 'rgba', match, flip });
        } else if (texture && level === 0) {
          matchedTextures.delete(texture);
        }
      }
    } catch (error) {
      log('[textures] replacement bypassed:', error?.message ?? error);
    }
    if (level > 0) counts.mip += 1;
    if (!heap || !source || !replacement) return call();
    heap.set(replacement, pointer);
    try { const value = call(); replacements += 1; return value; }
    finally { heap.set(source, pointer); }
  };
  const subImage = env.glTexSubImage2D;
  if (typeof subImage === 'function') env.glTexSubImage2D = function (target, level, x, y, width, height, format, type, pointer) {
    counts.texSubImage2D += 1;
    const texture = target === GL_TEXTURE_2D ? bindings.get(bindingKey(target)) : null;
    const call = () => subImage.call(this, target, level, x, y, width, height, format, type, pointer);
    const storageInfo = texture && storages.get(texture);
    try {
      const bytes = width * height * 4; const heap = module.HEAPU8;
      const levelWidth = storageInfo && Math.max(1, storageInfo.width >> level); const levelHeight = storageInfo && Math.max(1, storageInfo.height >> level);
      const within = storageInfo && Number.isInteger(level) && level >= 0 && level < storageInfo.levels
        && Number.isInteger(x) && x >= 0 && Number.isInteger(y) && y >= 0
        && Number.isInteger(width) && width > 0 && Number.isInteger(height) && height > 0
        && x <= levelWidth - width && y <= levelHeight - height;
      const complete = within && x === 0 && y === 0 && width === levelWidth && height === levelHeight;
      const eligible = within && target === GL_TEXTURE_2D && format === GL_RGBA && type === GL_UNSIGNED_BYTE
        && Number.isSafeInteger(pointer) && pointer > 0
        && heap instanceof Uint8Array && Number.isSafeInteger(bytes) && bytes >= 1 && bytes <= MAX_BYTES
        && pointer <= heap.byteLength - bytes && packedHeapUpload(module);
      if (!eligible || !texture) { if (texture) matchedTextures.delete(texture); return call(); }
      const source = heap.slice(pointer, pointer + bytes); let replacement = null;
      if (complete && level === 0) {
        let match = byTarget.get(hash(source)); let flip = false;
        if (!match) for (const candidate of [[false, true], [true, false], [true, true]]) {
          match = byTarget.get(hash(transformed(source, width, height, candidate[0], candidate[1])));
          if (match) { flip = candidate[0]; break; }
        }
        if (match?.width === width && match.height === height) {
          replacement = transformed(match.rgba, width, height, flip, false);
          matchedTextures.set(texture, { kind: 'rgba', match, flip }); counts.hashMatches += 1;
        }
      } else {
        const association = matchedTextures.get(texture);
        if (association?.kind === 'rgba') {
          const image = transformed(mip(association.match.rgba, association.match.width, association.match.height, level), levelWidth, levelHeight, association.flip, false);
          replacement = complete ? image : region(image, levelWidth, x, y, width, height);
        }
      }
      if (!replacement) { if (level === 0) matchedTextures.delete(texture); return call(); }
      heap.set(replacement, pointer);
      try { const value = call(); replacements += 1; return value; }
      finally { heap.set(source, pointer); }
    } catch (error) { log('[textures] subimage replacement bypassed:', error?.message ?? error); return call(); }
  };
  const compressedImage = env.glCompressedTexImage2D;
  if (typeof compressedImage === 'function') env.glCompressedTexImage2D = function (target, level, internalFormat, width, height, border, imageBytes, pointer) {
    counts.compressedTexImage2D += 1;
    const texture = target === GL_TEXTURE_2D ? bindings.get(bindingKey(target)) : null;
    if (texture && level === 0) matchedTextures.delete(texture);
    return compressedUpload(compressedImage, this, target, level, width, height, border, internalFormat, imageBytes, pointer, texture, 0, 0);
  };
  const compressedSubImage = env.glCompressedTexSubImage2D;
  if (typeof compressedSubImage === 'function') env.glCompressedTexSubImage2D = function (target, level, x, y, width, height, format, imageBytes, pointer) {
    counts.compressedTexSubImage2D += 1;
    const texture = target === GL_TEXTURE_2D ? bindings.get(bindingKey(target)) : null;
    if (texture && level === 0) matchedTextures.delete(texture);
    return compressedUpload(compressedSubImage, this, target, level, width, height, null, format, imageBytes, pointer, texture, x, y);
  };
  function compressedUpload(originalCompressed, receiver, target, level, width, height, border, format, imageBytes, pointer, texture, x, y) {
    const call = () => border === null
      ? originalCompressed.call(receiver, target, level, x, y, width, height, format, imageBytes, pointer)
      : originalCompressed.call(receiver, target, level, format, width, height, border, imageBytes, pointer);
    try {
      const heap = module.HEAPU8;
      if (!texture || target !== GL_TEXTURE_2D || (border !== null && border !== 0) || x !== 0 || y !== 0
        || !Number.isInteger(level) || level < 0 || !Number.isInteger(width) || !Number.isInteger(height)
        || width < 1 || height < 1 || !Number.isInteger(imageBytes) || imageBytes < 1
        || !Number.isInteger(pointer) || pointer <= 0 || !heap || !(heap instanceof Uint8Array)
        || pointer > heap.byteLength - imageBytes || !packedHeapUpload(module)) return call();
      const source = heap.slice(pointer, pointer + imageBytes);
      let association = matchedTextures.get(texture);
      let replacement = null;
      if (level === 0) {
        const match = byTarget.get(hash(source));
        if (match?.compressed && match.width === width && match.height === height
          && compatibleCompressedFormat(match.compressed.mode, format)
          && match.compressed.levels[0]?.byteLength === imageBytes) {
          association = { kind: 'compressed', match }; replacement = match.compressed.levels[0];
          matchedTextures.set(texture, association); counts.hashMatches += 1;
        }
      } else if (association?.kind === 'compressed') {
        const expectedWidth = Math.max(1, association.match.width >> level); const expectedHeight = Math.max(1, association.match.height >> level);
        const compressed = association.match.compressed;
        if (compressed && width === expectedWidth && height === expectedHeight && compatibleCompressedFormat(compressed.mode, format)
          && compressed.levels[level]?.byteLength === imageBytes) replacement = compressed.levels[level];
      }
      if (!replacement) return call();
      heap.set(replacement, pointer);
      try { const value = call(); replacements += 1; return value; }
      finally { heap.set(source, pointer); }
    } catch (error) {
      log('[textures] compressed replacement bypassed:', error?.message ?? error);
      return call();
    }
  }
  log(`texture packs armed (${byTarget.size} mappings)`);
  return Object.freeze({ dispose: () => { if (env.glTexImage2D !== original) env.glTexImage2D = original; if (bind && env.glBindTexture !== bind) env.glBindTexture = bind; if (activeTexture && env.glActiveTexture !== activeTexture) env.glActiveTexture = activeTexture; if (storage && env.glTexStorage2D !== storage) env.glTexStorage2D = storage; if (subImage && env.glTexSubImage2D !== subImage) env.glTexSubImage2D = subImage; if (compressedImage && env.glCompressedTexImage2D !== compressedImage) env.glCompressedTexImage2D = compressedImage; if (compressedSubImage && env.glCompressedTexSubImage2D !== compressedSubImage) env.glCompressedTexSubImage2D = compressedSubImage; if (deleted && env.glDeleteTextures !== deleted) env.glDeleteTextures = deleted; }, snapshot: () => Object.freeze({ mappings: byTarget.size, replacements, ...(diagnostics ? { uploads: counts.uploads, eligibleUploads: counts.eligibleUploads, hashMatches: counts.hashMatches, bypassedMipUploads: counts.mip, texStorage2D: counts.texStorage2D, texSubImage2D: counts.texSubImage2D, compressedTexImage2D: counts.compressedTexImage2D, compressedTexSubImage2D: counts.compressedTexSubImage2D, matchedTextures: matchedTextures.size } : {}) }) });
}
