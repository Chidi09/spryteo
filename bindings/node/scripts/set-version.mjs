// Stamp one version across the main package and all seven platform
// packages, including the optionalDependency pins that must match them
// exactly. Defaults to the workspace crate version so npm and crates.io
// never drift apart.

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { readdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const root = dirname(dirname(fileURLToPath(import.meta.url)))

function workspaceVersion() {
  const toml = readFileSync(join(root, '..', '..', 'Cargo.toml'), 'utf8')
  const match = toml.match(/^version\s*=\s*"([^"]+)"/m)
  if (!match) throw new Error('no version in the workspace Cargo.toml')
  return match[1]
}

const version = (process.argv[2] ?? workspaceVersion()).replace(/^v/, '')
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`not a semver version: ${version}`)
}

function edit(path, change) {
  const pkg = JSON.parse(readFileSync(path, 'utf8'))
  change(pkg)
  writeFileSync(path, JSON.stringify(pkg, null, 2) + '\n')
}

const platforms = readdirSync(join(root, 'npm'))
for (const name of platforms) {
  edit(join(root, 'npm', name, 'package.json'), (pkg) => {
    pkg.version = version
  })
}

edit(join(root, 'package.json'), (pkg) => {
  pkg.version = version
  for (const dep of Object.keys(pkg.optionalDependencies ?? {})) {
    pkg.optionalDependencies[dep] = version
  }
})

console.log(`spryteo and ${platforms.length} platform packages set to ${version}`)
