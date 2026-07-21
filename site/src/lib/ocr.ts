// Offline OCR for sheet-mode icon labels.
//
// The sheet engine reports, per icon, a `label_rect` in source-image pixel
// space — the bounding box of the label text it detected and stripped. We crop
// that region back out of the original image and run Tesseract on it to recover
// a human name ("home", "search", …) instead of the positional icon-NN-MM.
//
// Everything is served same-origin from /ocr/ (worker, wasm core, language
// data) — no CDN — so this stays true to the page's "entirely offline" promise.
// tesseract.js is imported dynamically so its ~4MB core is only fetched when a
// user actually asks to name icons.

export type LabelRect = [number, number, number, number]; // x0, y0, x1, y1

export interface LabelJob {
  index: number; // position in the icons array
  rect: LabelRect;
}

let workerPromise: Promise<any> | null = null;

async function getWorker(): Promise<any> {
  if (!workerPromise) {
    workerPromise = (async () => {
      const { createWorker } = await import('tesseract.js');
      // oem 1 = LSTM-only (loads the smaller *-lstm cores). All asset paths are
      // local; corePath is a directory so tesseract picks the SIMD variant when
      // the browser supports it and falls back otherwise.
      const worker = await createWorker('eng', 1, {
        workerPath: '/ocr/worker.min.js',
        corePath: '/ocr/',
        langPath: '/ocr/',
      });
      // Labels are a single short line of words.
      await worker.setParameters({ tessedit_pageseg_mode: '7' });
      return worker;
    })();
  }
  return workerPromise;
}

/** Load a File into an ImageBitmap for cropping. */
export async function loadSource(file: File): Promise<ImageBitmap> {
  return createImageBitmap(file);
}

/** Crop one padded, upscaled label region to a canvas Tesseract can read. */
function cropLabel(source: ImageBitmap, rect: LabelRect, pitch: number): HTMLCanvasElement {
  const [x0, y0, x1, y1] = rect;
  const cx = (x0 + x1) / 2;
  const w = x1 - x0;
  const h = y1 - y0;
  // Tight label_rect clips wide words; widen around the centre but never past
  // roughly half the column pitch so we don't swallow the neighbouring label.
  const half = Math.min(w * 1.15, pitch * 0.48);
  const nx0 = Math.max(0, Math.floor(cx - half));
  const nx1 = Math.min(source.width, Math.ceil(cx + half));
  const ny0 = Math.max(0, Math.floor(y0 - h * 0.35));
  const ny1 = Math.min(source.height, Math.ceil(y1 + h * 0.35));
  const sw = Math.max(1, nx1 - nx0);
  const sh = Math.max(1, ny1 - ny0);
  const scale = 3;
  const canvas = document.createElement('canvas');
  canvas.width = sw * scale;
  canvas.height = sh * scale;
  const ctx = canvas.getContext('2d')!;
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(source, nx0, ny0, sw, sh, 0, 0, canvas.width, canvas.height);
  return canvas;
}

/** Turn raw OCR text into a safe, kebab-case file stem, or '' if unusable. */
export function sanitizeName(raw: string): string {
  return raw
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48);
}

export interface RecognizeResult {
  names: Map<number, string>; // icon index -> deduped kebab name
  recognized: number; // how many produced a usable name
}

/**
 * OCR every label job. Names are sanitized and de-duplicated (home, home-2, …).
 * `taken` seeds the dedup set with names already in use (e.g. untouched icons).
 */
export async function recognizeLabels(
  source: ImageBitmap,
  cols: number,
  jobs: LabelJob[],
  onProgress?: (done: number, total: number) => void,
  taken: Set<string> = new Set(),
): Promise<RecognizeResult> {
  const worker = await getWorker();
  const pitch = cols > 0 ? source.width / cols : source.width;
  const names = new Map<number, string>();
  const used = new Set(taken);
  let recognized = 0;

  for (let i = 0; i < jobs.length; i++) {
    const job = jobs[i];
    let stem = '';
    try {
      const canvas = cropLabel(source, job.rect, pitch);
      const { data } = await worker.recognize(canvas);
      if (data.confidence >= 45) stem = sanitizeName(data.text.replace(/\n/g, ' ').trim());
    } catch {
      stem = '';
    }
    if (stem) {
      let unique = stem;
      let n = 2;
      while (used.has(unique)) unique = `${stem}-${n++}`;
      used.add(unique);
      names.set(job.index, unique);
      recognized++;
    }
    onProgress?.(i + 1, jobs.length);
  }

  return { names, recognized };
}

/** Free the worker and its memory. */
export async function disposeOcr(): Promise<void> {
  if (workerPromise) {
    const w = await workerPromise.catch(() => null);
    workerPromise = null;
    if (w) await w.terminate();
  }
}
