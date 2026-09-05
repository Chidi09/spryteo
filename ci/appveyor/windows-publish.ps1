# Publish spryteo-win32-x64-msvc. Nothing publishes except on a `v*` tag.
$ErrorActionPreference = "Continue"

if ($env:APPVEYOR_REPO_TAG -ne "true") {
  Write-Host "not a tag; nothing to publish"
  exit 0
}
if (-not $env:NPM_TOKEN) {
  Write-Host "NPM_TOKEN is not set"
  exit 1
}

node bindings/node/scripts/set-version.mjs $env:APPVEYOR_REPO_TAG_NAME
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

"//registry.npmjs.org/:_authToken=$env:NPM_TOKEN" | Out-File -Encoding ascii "$env:USERPROFILE\.npmrc"
npm publish bindings/node/npm/win32-x64-msvc --access public
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
exit 0
