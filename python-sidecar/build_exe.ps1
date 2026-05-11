# Build a single-file Windows executable with PyInstaller.
# Usage: pwsh -File build_exe.ps1
# Output: target/sidecar-vendor/windows-x86_64/lmt-vba-sidecar.exe
$ErrorActionPreference = 'Stop'
$root = Resolve-Path "$PSScriptRoot/.."
$venv = "$PSScriptRoot/.venv-build"

if (-not (Test-Path $venv)) {
    python -m venv $venv
}
& "$venv/Scripts/Activate.ps1"
pip install -e "$PSScriptRoot[dev]"

$out = Join-Path $root "target/sidecar-vendor/windows-x86_64"
New-Item -ItemType Directory -Force -Path $out | Out-Null

pyinstaller `
    --onefile `
    --name lmt-vba-sidecar `
    --distpath $out `
    --workpath "$PSScriptRoot/build" `
    --specpath "$PSScriptRoot/build" `
    --collect-all cv2 `
    --collect-submodules scipy `
    --collect-submodules lmt_vba_sidecar `
    --paths "$PSScriptRoot/src" `
    "$PSScriptRoot/src/lmt_vba_sidecar/__main__.py"

Write-Host "Built: $out/lmt-vba-sidecar.exe"
