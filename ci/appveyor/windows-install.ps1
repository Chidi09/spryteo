# Windows toolchain setup for AppVeyor.
#
# These scripts live in files, not in `ps:` blocks, because AppVeyor's
# PowerShell host treats a native command's stderr as an exception and
# fails the step -- cargo failed the job on its own "Finished `release`
# profile" line, and $ErrorActionPreference = "Continue" was not enough
# to stop it. AppVeyor's own answer is to go through cmd, which judges a
# step by exit code alone, so appveyor.yml runs each of these as
# `cmd: powershell -File ...` and they report by exiting.
$ErrorActionPreference = "Continue"

# Rust is not on the Visual Studio 2022 image at all -- unlike the Linux
# and macOS workers, rustup is not even on PATH.
if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
  Invoke-WebRequest https://win.rustup.rs/x86_64 -OutFile "$env:TEMP\rustup-init.exe"
  & "$env:TEMP\rustup-init.exe" -y --default-toolchain stable --profile minimal
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

# Each step is its own process, so every script sets this for itself.
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

rustup default stable
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
rustup target add x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

node --version
cargo --version
exit 0
