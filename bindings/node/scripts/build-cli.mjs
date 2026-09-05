// Build the Rust CLI and drop it next to the package, where the `bin`
// shim looks after the platform packages -- so `spryteo ...` works from
// a checkout exactly as it does from an install.

import { spawnSync } from 'node:child_process'
import { copyFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const exe = process.platform === 'win32' ? 'spryteo.exe' : 'spryteo'

const build = spawnSync('cargo', ['build', '--release', '-p', 'spryteo-cli'], {
  cwd: join(root, '..', '..'),
  stdio: 'inherit',
  shell: process.platform === 'win32',
})
if (build.status !== 0) process.exit(build.status ?? 1)

const from = join(root, '..', '..', 'target', 'release', exe)
copyFileSync(from, join(root, exe))
console.log(`${exe} ready in ${root}`)
