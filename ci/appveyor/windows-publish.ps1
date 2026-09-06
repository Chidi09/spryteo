# Publish @chidi09/spryteo-win32-x64-msvc. Nothing publishes except on a `v*` tag.
$ErrorActionPreference = "Continue"

if ($env:APPVEYOR_REPO_TAG -ne "true") {
  Write-Host "not a tag; nothing to publish"
  exit 0
}
if (-not $env:NPM_TOKEN) {
  Write-Host "NPM_TOKEN is not set"
  exit 1
}

# npm answers an unauthorized publish with 404, not 401, so a malformed
# token looks exactly like a missing package and costs an hour to chase.
# This has already happened twice by pasting the whole `NPM_TOKEN=npm_...`
# dotenv line into the variable instead of just the value. Never print the
# token itself, only what is wrong with it.
if ($env:NPM_TOKEN -match '[=\s]') {
  Write-Host "NPM_TOKEN is malformed (length $($env:NPM_TOKEN.Length))."
  Write-Host "It contains '=' or whitespace, which an npm token never does."
  Write-Host "Set the variable to the token value alone -- the part after"
  Write-Host "NPM_TOKEN= -- with no quotes and no trailing spaces."
  exit 1
}
if ($env:NPM_TOKEN -notmatch '^npm_') {
  Write-Host "warning: NPM_TOKEN does not start with npm_; publishing anyway"
}

node bindings/node/scripts/set-version.mjs $env:APPVEYOR_REPO_TAG_NAME
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

"//registry.npmjs.org/:_authToken=$env:NPM_TOKEN" | Out-File -Encoding ascii "$env:USERPROFILE\.npmrc"
node bindings/node/scripts/publish-package.mjs bindings/node/npm/win32-x64-msvc
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
exit 0
