// A real conversion against an installed package: the library API on
// whichever backend loaded, and the `spryteo` CLI through its bin shim.
//
// Takes the package root -- `node_modules/spryteo` in a clean install,
// or this directory in a checkout -- so CI can run the very tarball it
// is about to publish (#1).
//
// With `--fallback`, asserts the other half of the contract instead: an
// install with no platform package still converts through WASM, and the
// CLI shim says so plainly rather than crashing.

import { createRequire } from 'node:module'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

const args = process.argv.slice(2)
const fallback = args.includes('--fallback')
const pkgDir = resolve(args.find((a) => !a.startsWith('--')) ?? '.')
const require = createRequire(join(pkgDir, 'index.js'))

function check(ok, what) {
  if (!ok) {
    console.error(`FAIL  ${what}`)
    process.exitCode = 1
    return false
  }
  console.log(`ok    ${what}`)
  return true
}

// Three concentric discs on white: the same fixture the Rust surfaces
// use, inlined so the script needs nothing from the repo.
const FIXTURE_B64 =
  "iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAACCklEQVR4nO1by3KDMAx0Oj30" +
  "N3qkf5cv6d+FY34jh86QE5MOBVuPXckdvMfWQbtrWRYYLsuyLOXEeMsmkI1hQDaBbAwDsglk" +
  "4z0y2DxNonHTPJOZvHBhboNSwS0wDaEYgBK+BcMIqAEs4VsgjYAVwSjx6FjuDIgUvgdvNrgy" +
  "IFs8goPZgB7Er/BwMRnQk/gVVk5qA3oUv8LCTVUEEeK/Pj6r/7897u4YmsIY0gq3RB+NRZjR" +
  "gngJWGdfIx71Ww1X0RKwiPcI34MlGyRLgXI7jBbPumYpAgO0s88iarm2hDs0A5jiWTGqBvS8" +
  "50vR0gDLgIjZZ8SC9AEWQj/fGyJXfUxEnxD6TLCUv8K3f9ca4cXhEmCs/yPx2jFa1LS4a4A0" +
  "/TXCpGMRteD05wIhBljSmrEU9jAyIJtANoYBEUEse3tUPzAywHsBaTuqmVHpWEQrfGgA4yBS" +
  "IoyR+jUt4fcCq0DvzRAKEANuj7u6LfUKRj0xhhXBiEfYjFhVAyJfVWGhpQG6DUZkATpG0wBt" +
  "FjBN0F477VyAYQLLWPHhaMbRWCl24dLMFWeAtSB6Zo4tvpSgRui3kIjjcQ3UL0n1fliizVR1" +
  "Eey5N7BwM+0CPZpg5WTeBnsywcPF1Qf0YIKXA+xd4ejiiDIf1glGZgMy1nhdfnwwEfjh5Ok+" +
  "mfkPGOcC2QSyMQzIJpCN0xvwBJpW6L4T6qfNAAAAAElFTkSuQmCC"

const png = Buffer.from(FIXTURE_B64, 'base64')
const work = mkdtempSync(join(tmpdir(), 'spryteo-smoke-'))
const input = join(work, 'input.png')
const output = join(work, 'output.svg')
writeFileSync(input, png)

console.log(`package: ${pkgDir}`)

// --- library ---------------------------------------------------------
const spryteo = require(pkgDir)
const backend = spryteo.loadedBackend()
console.log(`backend: ${backend}`)
check(backend === (fallback ? 'wasm' : 'native'), `loaded the ${fallback ? 'WASM' : 'native'} backend`)

const sync = spryteo.convertSync(png, JSON.stringify({ mode: 'icon', colors: 8 }))
check(sync.svg.includes('<svg'), 'convertSync returns an SVG document')
check(sync.meta.stats.node_count >= 3, `convertSync traces the three discs (${sync.meta.stats.node_count} nodes)`)
check(sync.meta.schema_version === 1, 'the sidecar carries its schema version')

const async_ = await spryteo.convert(png, JSON.stringify({ mode: 'icon', colors: 8 }))
check(async_.svg === sync.svg, 'convert agrees with convertSync')

// --- CLI -------------------------------------------------------------
const cli = join(pkgDir, 'bin', 'spryteo.js')

if (fallback) {
  // No platform package, so no CLI binary. The shim has to fail
  // usefully: name the package to install, and say the library still
  // works.
  const refused = spawnSync(process.execPath, [cli, '--version'], { encoding: 'utf8' })
  check(refused.status === 1, 'the CLI shim exits 1 with no platform package')
  check(/no CLI binary/.test(refused.stderr), 'the CLI shim explains what is missing')
  check(/npm install spryteo-/.test(refused.stderr), 'the CLI shim names the package to install')
  console.log(process.exitCode ? '\nsmoke test failed' : '\nsmoke test passed')
  process.exit(process.exitCode ?? 0)
}

const version = spawnSync(process.execPath, [cli, '--version'], { encoding: 'utf8' })
check(version.status === 0, `spryteo --version exits 0 (${(version.stdout || version.stderr).trim()})`)

const converted = spawnSync(
  process.execPath,
  [cli, 'convert', input, '-o', output, '--mode', 'icon', '--colors', '8'],
  { encoding: 'utf8' }
)
if (check(converted.status === 0, 'spryteo convert exits 0')) {
  const svg = readFileSync(output, 'utf8')
  check(svg.startsWith('<svg'), 'spryteo convert wrote an SVG document')
  check(svg.length > 200, `the SVG has real content (${svg.length} bytes)`)
} else {
  console.error(converted.stdout)
  console.error(converted.stderr)
}

if (process.exitCode) console.error('\nsmoke test failed')
else console.log('\nsmoke test passed')
