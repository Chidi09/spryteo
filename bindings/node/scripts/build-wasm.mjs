// Build the WASM fallback that `index.js` loads when no native binary
// matches the platform (#1).
//
// wasm-pack writes a self-contained npm package into the output
// directory. We want the module, not the package: a nested package.json
// makes npm-packlist treat `wasm/` as a separate package and drop it
// from the tarball, and the nested .gitignore hides it from git. Both
// are pruned here so the fallback actually ships.

import { spawnSync } from 'node:child_process'
import { existsSync, rmSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const out = join(root, 'wasm')

const build = spawnSync(
  'wasm-pack',
  ['build', join(root, '..', 'wasm'), '--release', '--target', 'nodejs',
   '--out-dir', out, '--out-name', 'spryteo_wasm'],
  { stdio: 'inherit', shell: process.platform === 'win32' }
)
if (build.status !== 0) process.exit(build.status ?? 1)

for (const name of ['package.json', 'README.md', 'LICENSE', '.gitignore']) {
  const path = join(out, name)
  if (existsSync(path)) rmSync(path)
}
console.log(`wasm fallback ready in ${out}`)
