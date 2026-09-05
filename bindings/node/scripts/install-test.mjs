// Pack, install into a directory that shares nothing with the checkout,
// and run the smoke test against the result -- the "CI packs and
// installs the tarball in a clean directory" half of #1.
//
// All of it goes through Node rather than shell so the same one-line
// step works on Linux, macOS and Windows runners.
//
//   node scripts/install-test.mjs --main <dir> [--platform <dir>]
//   node scripts/install-test.mjs --main <dir> --fallback
//
// A directory is packed if it holds no tarball yet, so this works both
// from a plain checkout and from CI artifacts that were packed earlier.

import { spawnSync } from 'node:child_process'
import { mkdtempSync, readdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))

function arg(name) {
  const i = process.argv.indexOf(`--${name}`)
  return i === -1 ? null : process.argv[i + 1]
}

const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm'

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...options })
  if (result.status !== 0) {
    console.error(`\nfailed: ${command} ${args.join(' ')}`)
    process.exit(result.status ?? 1)
  }
}

/** The tarball in `dir`, packing one first if none is there yet. */
function tarball(dir) {
  const path = resolve(dir)
  const find = () => readdirSync(path).find((f) => f.endsWith('.tgz'))
  if (!find()) run(npm, ['pack'], { cwd: path, shell: process.platform === 'win32' })
  const name = find()
  if (!name) throw new Error(`no tarball in ${path}`)
  return join(path, name)
}

const fallback = process.argv.includes('--fallback')
const packages = [tarball(arg('main') ?? join(here, '..'))]
const platform = arg('platform')
if (platform) packages.push(tarball(platform))

const work = mkdtempSync(join(tmpdir(), 'spryteo-install-'))
console.log(`installing into ${work}`)
run(npm, ['init', '-y'], { cwd: work, stdio: 'ignore', shell: process.platform === 'win32' })
// --omit=optional: at this version the other platform packages are not
// on the registry, and the one for this platform is installed directly.
run(npm, ['install', '--omit=optional', '--no-audit', '--no-fund', ...packages], {
  cwd: work,
  shell: process.platform === 'win32',
})

const installed = join(work, 'node_modules', 'spryteo')
run(process.execPath, [join(here, 'smoke.mjs'), ...(fallback ? ['--fallback'] : []), installed])
