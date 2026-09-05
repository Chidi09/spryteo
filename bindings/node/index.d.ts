/**
 * Spryteo: raster images in, clean animateable SVG plus a metadata
 * sidecar out.
 *
 * Backed by a prebuilt native addon where one exists for the platform,
 * and by the bundled WASM build everywhere else. Both return the same
 * `ConvertResult`.
 */
import type { ConvertResult } from './meta'

export * from './meta'

/** Anything byte-shaped: a Buffer, any typed array view, or a raw buffer. */
export type ImageBytes = Uint8Array | ArrayBuffer | ArrayBufferView

/**
 * Convert on the calling thread.
 *
 * Prefer {@link convert}: a conversion is CPU-bound and can take seconds,
 * and this one blocks the event loop for all of it.
 *
 * @param optionsJson JSON options object; defaults to `{}`.
 */
export declare function convertSync(
  bytes: ImageBytes,
  optionsJson?: string | null
): ConvertResult

/**
 * Convert off the event loop.
 *
 * On the native backend the work runs on a libuv worker thread, bounded
 * by `UV_THREADPOOL_SIZE`. On the WASM fallback there is no worker to
 * run on, so the returned Promise resolves from a synchronous call.
 *
 * @param optionsJson JSON options object; defaults to `{}`.
 */
export declare function convert(
  bytes: ImageBytes,
  optionsJson?: string | null
): Promise<ConvertResult>

/** Which backend a conversion would use, loading one if needed. */
export declare function loadedBackend(): 'native' | 'wasm'
