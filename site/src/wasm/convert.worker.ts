// Web Worker that runs Spryteo WASM conversions off the main thread.
//
// WASM executes synchronously, so converting a large photo directly on
// the main thread freezes the whole page for the duration of the
// conversion. Running it here keeps the UI responsive no matter how
// long a conversion takes.
import init, { convert_default } from './spryteo_wasm.js';

let ready: Promise<unknown> | null = null;

function ensureInit(): Promise<unknown> {
  if (!ready) {
    // The .wasm binary is served as-is from public/ and fetched by URL,
    // same as the previous main-thread loader did.
    ready = init('/wasm/spryteo_wasm_bg.wasm');
  }
  return ready;
}

self.onmessage = async (e: MessageEvent) => {
  const { id, bytes } = e.data as { id: number; bytes: Uint8Array };
  try {
    await ensureInit();
    const result = convert_default(bytes);
    self.postMessage({ id, ok: true, result });
  } catch (err) {
    self.postMessage({ id, ok: false, error: String(err) });
  }
};
