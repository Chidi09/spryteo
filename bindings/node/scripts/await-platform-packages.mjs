// Wait until every `spryteo-<platform>` package exists on the registry
// at the given version.
//
// The main package pins each platform package as an exact-version
// optional dependency, so it must not be published before they are
// resolvable. On AppVeyor each job publishes the packages it built --
// there is no cross-job artifact transfer -- so the job that publishes
// the main package waits here for the others to land first.

import { setTimeout as sleep } from 'node:timers/promises'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'))

const version = (process.argv[2] ?? pkg.version).replace(/^v/, '')
const timeoutMs = Number(process.argv[3] ?? 45 * 60 * 1000)
const names = Object.keys(pkg.optionalDependencies ?? {})

/** Whether the registry serves this exact version yet. */
async function published(name) {
  const response = await fetch(
    `https://registry.npmjs.org/${name}/${version}`,
    { headers: { accept: 'application/json' } }
  )
  if (response.status === 200) return true
  if (response.status === 404) return false
  throw new Error(`${name}: registry answered ${response.status}`)
}

const deadline = Date.now() + timeoutMs
const pending = new Set(names)

console.log(`waiting for ${names.length} platform packages at ${version}`)

while (pending.size > 0) {
  for (const name of [...pending]) {
    let ok = false
    try {
      ok = await published(name)
    } catch (err) {
      // A registry blip should not fail the release; try again.
      console.log(`  ${name}: ${err.message}`)
    }
    if (ok) {
      pending.delete(name)
      console.log(`  ${name} ok (${pending.size} left)`)
    }
  }
  if (pending.size === 0) break
  if (Date.now() > deadline) {
    console.error(`\nstill missing at ${version}: ${[...pending].join(', ')}`)
    console.error('the main package pins these exactly, so it is not safe to publish')
    process.exit(1)
  }
  await sleep(15000)
}

console.log('all platform packages are on the registry')
