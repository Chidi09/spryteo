# spryteo

Raster images in, clean animateable SVG out: quantized colour layers,
traced contours, fitted curves, semantically grouped `<g>` trees, and a
metadata sidecar describing every node.

```
npm install spryteo
```

Node 18 or newer.

## Library

```js
const { convert } = require('spryteo')

const { svg, meta } = await convert(
  await fs.promises.readFile('icon.png'),
  JSON.stringify({ mode: 'icon', colors: 8 })
)

console.log(meta.stats) // { node_count, path_count, byte_count }
```

- `convert(bytes, optionsJson?)` runs the conversion off the event loop
  and resolves to `{ svg, meta }`.
- `convertSync(bytes, optionsJson?)` does the same on the calling thread.
- `loadedBackend()` reports `'native'` or `'wasm'`.

`bytes` is a Buffer, typed array or ArrayBuffer. `optionsJson` is a JSON
object, identical to the one the CLI, WASM and MCP surfaces accept, and
defaults to `{}`. Types for `meta` are in `meta.d.ts`, generated from the
Rust schema.

## CLI

```
npx spryteo convert ./icon.png -o ./icon.svg --mode icon --colors 8
```

Install it globally with `npm install -g spryteo` for a `spryteo` on the
PATH.

## What gets installed

The package itself is JavaScript. The engine arrives one of two ways:

- **Native addon.** One `spryteo-<platform>` optional dependency matching
  your machine, carrying the `.node` addon and the `spryteo` executable.
  Prebuilt for macOS x64/arm64, Linux x64/arm64 on glibc >= 2.28 and on
  musl, and Windows x64.
- **WASM fallback.** Bundled in this package and loaded when no native
  binary matches, or when optional dependencies were skipped. The library
  API works unchanged; it is slower, and the `spryteo` command is not
  available because it is a native executable.

## Development

From a checkout of the [repository](https://github.com/Chidi09/spryteo):

```
npm run build       # native addon for this platform
npm run build:cli   # the spryteo executable, next to the package
npm run build:wasm  # the WASM fallback into wasm/
npm test            # convert a real PNG through the library and the CLI
npm run test:install # pack, install into a clean dir, convert via WASM
```

MIT OR Apache-2.0.
