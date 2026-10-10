# Packs the Microsoft Store bundle from release builds of both architectures:
#
#   pnpm tauri build --no-bundle --target x86_64-pc-windows-msvc
#   pnpm tauri build --no-bundle --target aarch64-pc-windows-msvc
#   pwsh scripts/package-msix.ps1 [-OutDir dir]
#
# Writes ZShell_<version>.msixbundle to OutDir (src-tauri/target/msix by default). The
# packages are not signed: the Store signs them on submission. For a local test, register
# the unpacked layout instead (Developer Mode required):
#
#   Add-AppxPackage -Register src-tauri/target/msix/x64/AppxManifest.xml
param(
  [string]$OutDir = "src-tauri/target/msix"
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

# The package's own version is the first `version =` line, as in scripts/release.sh.
$version = (Select-String -Path src-tauri/Cargo.toml -Pattern '^version = "(.+)"' |
  Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $version) { throw "could not read the version from src-tauri/Cargo.toml" }

# makeappx and makepri ship with the Windows SDK, one folder per SDK version; take the newest.
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" |
  Sort-Object { [version]$_.Directory.Parent.Name } |
  Select-Object -Last 1
if (-not $sdk) { throw "makeappx.exe not found; install the Windows SDK" }
$makeappx = $sdk.FullName
$makepri = Join-Path $sdk.Directory "makepri.exe"

if (Test-Path $OutDir) { Remove-Item $OutDir -Recurse -Force }
$packages = Join-Path $OutDir "packages"
New-Item -ItemType Directory -Path $packages | Out-Null

$targets = [ordered]@{ x64 = "x86_64-pc-windows-msvc"; arm64 = "aarch64-pc-windows-msvc" }
foreach ($arch in $targets.Keys) {
  $exe = "src-tauri/target/$($targets[$arch])/release/zshell.exe"
  if (-not (Test-Path $exe)) { throw "$exe not found; build it first" }

  $layout = Join-Path $OutDir $arch
  New-Item -ItemType Directory -Path (Join-Path $layout "Assets") | Out-Null
  Copy-Item $exe $layout
  Copy-Item src-tauri/icons/msix/*.png (Join-Path $layout "Assets")
  $manifest = Get-Content src-tauri/msix/AppxManifest.xml -Raw
  $manifest = $manifest.Replace("{version}", "$version.0").Replace("{arch}", $arch)
  Set-Content (Join-Path $layout "AppxManifest.xml") $manifest -Encoding utf8 -NoNewline

  # The images carry scale and size qualifiers (Square44x44Logo.scale-200.png), which
  # Windows finds through the resource index, for the names in the manifest without them.
  $priconfig = Join-Path $OutDir "priconfig-$arch.xml"
  & $makepri createconfig /cf $priconfig /dq en-US /pv 10.0.0 /o
  if ($LASTEXITCODE -ne 0) { throw "makepri createconfig failed for $arch" }
  & $makepri new /pr $layout /cf $priconfig /mn (Join-Path $layout "AppxManifest.xml") /of (Join-Path $layout "resources.pri") /o
  if ($LASTEXITCODE -ne 0) { throw "makepri new failed for $arch" }

  & $makeappx pack /o /d $layout /p (Join-Path $packages "ZShell_${version}_$arch.msix")
  if ($LASTEXITCODE -ne 0) { throw "makeappx pack failed for $arch" }
}

$bundle = Join-Path $OutDir "ZShell_$version.msixbundle"
& $makeappx bundle /o /d $packages /bv "$version.0" /p $bundle
if ($LASTEXITCODE -ne 0) { throw "makeappx bundle failed" }
Write-Host "Wrote $bundle"
