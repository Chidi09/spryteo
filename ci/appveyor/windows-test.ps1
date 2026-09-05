# Pack and install the two packages into a scratch project, then run the
# smoke test against them. The main package is packed without the WASM
# fallback here: this job is proving the native addon and the CLI shim.
# The fallback is covered on Ubuntu, which is also the job that
# publishes it.
$ErrorActionPreference = "Continue"

node bindings/node/scripts/install-test.mjs --main bindings/node --platform bindings/node/npm/win32-x64-msvc
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
exit 0
