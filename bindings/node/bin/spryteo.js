#!/usr/bin/env node
'use strict'

// The `spryteo` command documented in the README. The CLI itself is the
// Rust binary: each `spryteo-<platform>` package ships it next to the
// `.node` addon, and this shim finds it and hands over argv (#1).

const { chmodSync, existsSync } = require('fs')
const { spawnSync } = require('child_process')
const { join } = require('path')

/** Mirrors the platform detection in the generated `native.js` loader. */
function isMusl() {
  if (process.platform !== 'linux') return false
  if (!process.report || typeof process.report.getReport !== 'function') {
    try {
      const ldd = require('child_process').execSync('which ldd').toString().trim()
      return require('fs').readFileSync(ldd, 'utf8').includes('musl')
    } catch {
      return true
    }
  }
  return !process.report.getReport().header.glibcVersionRuntime
}

function platformPackage() {
  const { platform, arch } = process
  if (platform === 'darwin' && (arch === 'x64' || arch === 'arm64')) {
    return `spryteo-darwin-${arch}`
  }
  if (platform === 'win32' && arch === 'x64') {
    return 'spryteo-win32-x64-msvc'
  }
  if (platform === 'linux' && (arch === 'x64' || arch === 'arm64')) {
    return `spryteo-linux-${arch}-${isMusl() ? 'musl' : 'gnu'}`
  }
  return null
}

const exeName = process.platform === 'win32' ? 'spryteo.exe' : 'spryteo'

/**
 * Look in the platform package first, then next to this package -- the
 * layout a local `napi build` leaves behind, so a checkout runs the same
 * command a published install does.
 */
function findExecutable() {
  const pkg = platformPackage()
  if (pkg) {
    try {
      const dir = join(require.resolve(`${pkg}/package.json`), '..')
      const exe = join(dir, exeName)
      if (existsSync(exe)) return exe
    } catch {
      // Optional dependency not installed; fall through.
    }
  }
  const local = join(__dirname, '..', exeName)
  if (existsSync(local)) return local
  return null
}

function fail(message) {
  process.stderr.write(`spryteo: ${message}\n`)
  process.exit(1)
}

const exe = findExecutable()
if (!exe) {
  const pkg = platformPackage()
  fail(
    `no CLI binary for ${process.platform}-${process.arch}.\n` +
      (pkg
        ? `  Install the platform package: npm install ${pkg}\n` +
          '  (npm normally installs it for you; --no-optional or --omit=optional skips it.)\n'
        : '  This platform has no prebuilt binary. See https://github.com/Chidi09/Spryteo#install\n') +
      '  The library API still works: require("spryteo") falls back to WASM.'
  )
}

let result = spawnSync(exe, process.argv.slice(2), { stdio: 'inherit' })
if (result.error && result.error.code === 'EACCES') {
  // npm preserves the executable bit, but a tarball unpacked by other
  // means may not. Set it once and retry rather than failing outright.
  try {
    chmodSync(exe, 0o755)
    result = spawnSync(exe, process.argv.slice(2), { stdio: 'inherit' })
  } catch {
    // Fall through to the error report below.
  }
}
if (result.error) fail(`could not run ${exe}: ${result.error.message}`)
if (result.signal) fail(`${exe} was killed by ${result.signal}`)
process.exit(result.status === null ? 1 : result.status)
