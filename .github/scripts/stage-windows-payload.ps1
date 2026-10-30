param(
    [Parameter(Mandatory)][string]$Root,
    [Parameter(Mandatory)][string]$Out
)

# Stages the portable Windows layout used by Cinnabar's installer payload:
# binaries beside cinnabar.exe and Mojang-derived resources under resources/.
# The vanilla pack itself is downloaded and compiled on first run.

$ErrorActionPreference = "Stop"

$root = (Resolve-Path -LiteralPath $Root).Path
$out = [IO.Path]::GetFullPath($Out)
$profileDir = Join-Path $root "target/play"

New-Item -ItemType Directory -Force -Path $out | Out-Null

$binaries = @(
    "cinnabar.exe",
    "bedrock-core.exe",
    "bedrock-local-server.exe",
    "cinnabar-updater.exe"
)
foreach ($name in $binaries) {
    $source = Join-Path $profileDir $name
    if (-not (Test-Path -LiteralPath $source)) {
        throw "missing $source; the Rust/Go build did not produce it"
    }
    Copy-Item -LiteralPath $source -Destination (Join-Path $out $name)
}

$resources = Join-Path $out "resources"
$assets = Join-Path $resources "assets"
$licenses = Join-Path $resources "licenses"
$fonts = Join-Path $resources "fonts"
$prepKit = Join-Path $resources "prep-kit"
$prepKitBin = Join-Path $prepKit "bin"
foreach ($dir in @($assets, $licenses, $fonts, $prepKitBin)) {
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
}

Copy-Item -LiteralPath (Join-Path $root "crates/assets/data/block-physics-v2225.bin") -Destination $assets
Copy-Item -LiteralPath (Join-Path $root "THIRD_PARTY_NOTICES.md") -Destination (Join-Path $assets "THIRD_PARTY_NOTICES.md")
Copy-Item -LiteralPath (Join-Path $root "LICENSE") -Destination (Join-Path $licenses "Cinnabar-LICENSE.md")
Copy-Item -Path (Join-Path $root "assets/licenses/*") -Destination $licenses

$fontSource = Get-Content -Raw -LiteralPath (Join-Path $root "assets/cinnangles-sans-source.json") | ConvertFrom-Json
$font = Join-Path $root ("assets/fonts/" + $fontSource.font_file)
if (-not (Test-Path -LiteralPath $font)) { throw "missing $font" }
Copy-Item -LiteralPath $font -Destination $fonts

$assetc = Join-Path $profileDir "assetc.exe"
if (-not (Test-Path -LiteralPath $assetc)) { throw "missing $assetc" }
Copy-Item -LiteralPath $assetc -Destination (Join-Path $prepKitBin "assetc.exe")

$list = Get-Content -LiteralPath (Join-Path $root "packaging/common/prep-kit-files.txt")
foreach ($entry in $list) {
    $relative = $entry.Trim().Replace('/', [IO.Path]::DirectorySeparatorChar)
    if (-not $relative) { continue }
    $source = Join-Path $root $relative
    if (-not (Test-Path -LiteralPath $source)) { throw "missing prep-kit input $source" }
    $destination = Join-Path $prepKit $relative
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination) | Out-Null
    Copy-Item -LiteralPath $source -Destination $destination
}

# DX12 uses this pinned compiler when present; without it wgpu falls back to FXC.
& (Join-Path $root "packaging/windows/fetch-dxc.ps1") -Root $root -Out $out

$files = (Get-ChildItem -LiteralPath $out -Recurse -File | Measure-Object).Count
$bytes = (Get-ChildItem -LiteralPath $out -Recurse -File | Measure-Object -Property Length -Sum).Sum
Write-Output ("staged {0} files, {1:N1} MiB at {2}" -f $files, ($bytes / 1MB), $out)
