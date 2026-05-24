param(
    [string]$OutputDir = "dist",
    [string]$PlatformTag = "windows-x64",
    [switch]$Clean
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    if ($Clean -and (Test-Path $OutputDir)) {
        Remove-Item -Recurse -Force $OutputDir
    }

    $manifest = Join-Path $repoRoot "Cargo.toml"
    $manifestText = Get-Content -Raw -Path $manifest
    $versionMatch = [regex]::Match($manifestText, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $versionMatch.Success) {
        throw "Could not determine version from Cargo.toml"
    }
    $version = $versionMatch.Groups[1].Value

    cargo build --release --bin rustyserial --bin rustyserial-gui

    $releaseDir = Join-Path $repoRoot "target/release"
    $packageName = "RustySerial-v$version-$PlatformTag"
    $stageDir = Join-Path $repoRoot (Join-Path $OutputDir $packageName)
    $zipPath = Join-Path $repoRoot (Join-Path $OutputDir "$packageName.zip")

    New-Item -ItemType Directory -Force -Path $stageDir | Out-Null

    Copy-Item (Join-Path $releaseDir "rustyserial.exe") $stageDir -Force
    Copy-Item (Join-Path $releaseDir "rustyserial-gui.exe") $stageDir -Force
    Copy-Item (Join-Path $repoRoot "README.md") $stageDir -Force

    foreach ($licenseName in @("LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING")) {
        $licensePath = Join-Path $repoRoot $licenseName
        if (Test-Path $licensePath) {
            Copy-Item $licensePath $stageDir -Force
        }
    }

    if (Test-Path $zipPath) {
        Remove-Item -Force $zipPath
    }

    Compress-Archive -Path (Join-Path $stageDir "*") -DestinationPath $zipPath -Force

    Write-Host "Packaged RustySerial:" -ForegroundColor Green
    Write-Host "  Folder: $stageDir"
    Write-Host "  Zip:    $zipPath"
}
finally {
    Pop-Location
}
