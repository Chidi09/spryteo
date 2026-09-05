'use strict'

// Hand-written entry point. `native.js` is the NAPI-RS generated loader,
// which resolves one of the `spryteo-<platform>` optional dependencies.
// When no prebuilt binary matches this platform -- or the optional
// dependency was skipped, as it is behind `--no-optional` -- we fall back
// to the WASM build so `require('spryteo')` still converts (#1).

let impl = null
let backend = null
let nativeError = null
let wasmError = null

/** Accept anything byte-shaped and hand each backend the view it wants. */
function asUint8Array(bytes) {
  if (bytes instanceof Uint8Array) return bytes
  if (bytes instanceof ArrayBuffer) return new Uint8Array(bytes)
  if (ArrayBuffer.isView(bytes)) {
    return new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  }
  throw new TypeError(
    'spryteo: expected a Buffer, Uint8Array or ArrayBuffer of image bytes, got ' +
      (bytes === null ? 'null' : typeof bytes)
  )
}

function wrapNative(native) {
  return {
    convertSync(bytes, optionsJson) {
      return native.convertSync(Buffer.from(asUint8Array(bytes)), optionsJson)
    },
    convert(bytes, optionsJson) {
      // Already a Promise: the addon runs the conversion on a libuv
      // worker thread rather than the event loop.
      return native.convert(Buffer.from(asUint8Array(bytes)), optionsJson)
    },
  }
}

function wrapWasm(wasm) {
  // The WASM build is synchronous and takes a required options string,
  // so the async surface is a resolved Promise over the same call. It
  // runs on the event loop; that is the cost of having no native binary.
  const call = (bytes, optionsJson) =>
    wasm.convert(asUint8Array(bytes), optionsJson == null ? '{}' : optionsJson)
  return {
    convertSync: call,
    convert: (bytes, optionsJson) =>
      new Promise((resolve) => resolve(call(bytes, optionsJson))),
  }
}

function load() {
  if (impl) return impl
  try {
    impl = wrapNative(require('./native.js'))
    backend = 'native'
    return impl
  } catch (err) {
    nativeError = err
  }
  try {
    impl = wrapWasm(require('./wasm/spryteo_wasm.js'))
    backend = 'wasm'
    return impl
  } catch (err) {
    wasmError = err
  }
  const detail = [
    `native: ${nativeError && nativeError.message}`,
    `wasm: ${wasmError && wasmError.message}`,
  ].join('\n  ')
  const error = new Error(
    `spryteo: no usable backend for ${process.platform}-${process.arch}.\n  ${detail}`
  )
  error.nativeError = nativeError
  error.wasmError = wasmError
  throw error
}

/** Convert on the calling thread. Prefer `convert` unless you want that. */
function convertSync(bytes, optionsJson) {
  return load().convertSync(bytes, optionsJson)
}

/** Convert off the event loop where a native binary is available. */
function convert(bytes, optionsJson) {
  let backendImpl
  try {
    backendImpl = load()
  } catch (err) {
    return Promise.reject(err)
  }
  return backendImpl.convert(bytes, optionsJson)
}

/**
 * Which backend answered: `'native'` or `'wasm'`. Loads one if none has
 * been loaded yet, so it reports what a conversion would actually use.
 */
function loadedBackend() {
  load()
  return backend
}

module.exports.convert = convert
module.exports.convertSync = convertSync
module.exports.loadedBackend = loadedBackend
