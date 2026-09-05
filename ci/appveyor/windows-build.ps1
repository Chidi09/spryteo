# Build the native addon and the CLI for win32-x64-msvc, and lay them
# out in the platform package. See windows-install.ps1 for why this is a
# script file rather than a `ps:` block.
$ErrorActionPreference = "Continue"
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

cargo build --release --target x86_64-pc-windows-msvc -p spryteo-node -p spryteo-cli
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$out = "bindings/node/npm/win32-x64-msvc"
$built = "target/x86_64-pc-windows-msvc/release"

# -ErrorAction Stop: a missing artifact must fail the step, and the
# preference above would otherwise let it through.
Copy-Item "$built/spryteo_node.dll" "$out/spryteo.win32-x64-msvc.node" -ErrorAction Stop
Copy-Item "$built/spryteo.exe" "$out/spryteo.exe" -ErrorAction Stop
Get-ChildItem $out
exit 0
