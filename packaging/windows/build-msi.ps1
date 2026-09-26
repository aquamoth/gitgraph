# Builds the Windows installer, parterre-<version>-x86_64-pc-windows-msvc.msi, from
# parterre.wxs next to this script. Needs WiX v7 on PATH:
#
#   dotnet tool install --global wix --version 7.0.0
#
#   packaging\windows\build-msi.ps1 [-Stage DIR] [-Out FILE]
#
# -Stage is a folder holding parterre.exe, LICENSE, NOTICE and THIRD-PARTY-NOTICES.html, as the
# release workflow packages them. Without it the script gathers them in target\msi\stage, from
# target\release\parterre.exe (run `cargo build --release` first) and cargo-about.
# -Out defaults to target\msi\ with the name above.
#
# The MSI version is the Cargo.toml version without its pre-release part: MSI versions are
# numbers only.
[CmdletBinding()]
param(
    [string]$Stage,
    [string]$Out
)
$ErrorActionPreference = 'Stop'
# Native commands that fail stop the script too (PowerShell 7.3 and newer).
$PSNativeCommandUseErrorActionPreference = $true

$root = (Resolve-Path "$PSScriptRoot\..\..").Path
Push-Location $root
try {
    $metadata = cargo metadata --no-deps --format-version 1 --locked | ConvertFrom-Json
    $version = ($metadata.packages | Where-Object name -EQ 'parterre').version
    $msiVersion = ($version -split '[-+]')[0]

    if (-not $Stage) {
        $Stage = "$root\target\msi\stage"
        New-Item -ItemType Directory -Force $Stage | Out-Null
        Copy-Item "$root\target\release\parterre.exe", "$root\LICENSE", "$root\NOTICE" $Stage
        cargo about generate --locked -c packaging/about.toml packaging/about.hbs `
            -o "$Stage\THIRD-PARTY-NOTICES.html"
    }
    $Stage = (Resolve-Path $Stage).Path
    if (-not $Out) {
        New-Item -ItemType Directory -Force "$root\target\msi" | Out-Null
        $Out = "$root\target\msi\parterre-$version-x86_64-pc-windows-msvc.msi"
    }

    # -acceptEula wix7: WiX v7's Open Source Maintenance Fee EULA, which asks a fee only of
    # users with revenue from it (docs/distribution.md). Accepted per run, so no file is left
    # behind.
    # No .wixpdb (debug information for patches, which parterre doesn't ship).
    wix build -acceptEula wix7 -arch x64 -pdbtype none `
        -d "Version=$msiVersion" `
        -d "Stage=$Stage" `
        -d "Icon=$root\packaging\icon\parterre.ico" `
        -o $Out `
        packaging\windows\parterre.wxs
    Write-Host "Built $Out"
}
finally {
    Pop-Location
}
