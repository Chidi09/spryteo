// Publish one package directory, unless that exact version is already there.
//
// npm refuses to republish a version, and a release is exactly the situation
// where some packages landed and others did not: each AppVeyor job publishes
// what it built, so one failed job leaves the release half-finished. Without
// this check, re-running a job dies on the first package it already published
// and never reaches the ones still missing -- the release cannot be finished
// without hand-editing the pipeline.
//
// Used by every publish step so a re-run is always safe.

import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const dir = process.argv[2]
if (!dir) {
  console.error('usage: publish-package.mjs <package-directory>')
  process.exit(1)
}

const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'))
const { name, version } = pkg

/** Whether the registry already serves this exact version. */
async function published() {
  const response = await fetch(`https://registry.npmjs.org/${name}/${version}`, {
    headers: { accept: 'application/json' },
  })
  if (response.status === 200) return true
  if (response.status === 404) return false
  throw new Error(`registry returned ${response.status} for ${name}@${version}`)
}

if (await published()) {
  console.log(`${name}@${version} is already on the registry; skipping`)
  process.exit(0)
}

console.log(`publishing ${name}@${version} from ${dir}`)
// shell: true so this works with npm.cmd on Windows as well as npm on Unix.
execFileSync('npm', ['publish', dir, '--access', 'public'], {
  stdio: 'inherit',
  shell: true,
})
