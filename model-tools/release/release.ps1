# Build a complete Windows release locally: for Vulkan and CPU run the tests, collect the exact runtime
# DLLs, build the signed installer (NSIS + updater .sig) and the portable folder; then zip, write
# SHA256SUMS.txt and the updater manifests, and verify every installer signature against the public
# key in tauri.conf.json. Publishing is a separate, explicit step (see docs/UPPDATERINGAR.md).
#
#   powershell -ExecutionPolicy Bypass -File model-tools\release\release.ps1 -LibClangPath <dir>
#
# Needs: VS C++ Build Tools, Vulkan SDK (VULKAN_SDK), libclang, Python 3 with `cryptography`,
# src-tauri/ffmpeg (model-tools/fetch-ffmpeg.ps1) and the bundled resources. Output: dist\release-<version>\.
param(
    [string]$LibClangPath = $env:LIBCLANG_PATH,
    [string]$TargetDir = 'C:\avb',
    [string]$KeyPath = "$env:USERPROFILE\.tauri\avskrift-updater.key",
    [string]$PasswordPath = "$env:USERPROFILE\.tauri\avskrift-updater.password",
    [string]$Dist = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'dist'),
    [ValidateSet('beta', 'stable')][string]$Channel = '',
    [switch]$SkipTests
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$version = (Get-Content -Raw (Join-Path $repo 'src-tauri\tauri.conf.json') | ConvertFrom-Json).version
if (!$Channel) { $Channel = if ($version -match '-') { 'beta' } else { 'stable' } }
$dashed = $version -replace '\.', '-'
$rel = Join-Path $Dist "release-$version"
$log = Join-Path $rel 'logs'
New-Item -ItemType Directory $rel, $log -Force | Out-Null
Write-Host "Avskrift $version ($Channel) -> $rel"

# ---- Build environment: MSVC, bundled CMake + Ninja, libclang, Vulkan SDK, signing key ----
$vs = & (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe') -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vs) { throw 'Visual Studio C++ Build Tools saknas.' }
foreach ($line in (& cmd.exe /c "`"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && set")) {
    if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
}
$cmake = "$vs\Common7\IDE\CommonExtensions\Microsoft\CMake"
$env:PATH = "$cmake\CMake\bin;$cmake\Ninja;$env:PATH"
$env:CMAKE_GENERATOR = 'Ninja'   # MSBuild drops /O2 for whisper.cpp and chokes on long ggml-vulkan paths
if (!$LibClangPath -or !(Test-Path (Join-Path $LibClangPath 'libclang.dll'))) { throw 'Ange -LibClangPath till mappen med libclang.dll.' }
$env:LIBCLANG_PATH = $LibClangPath
if (!$env:VULKAN_SDK) { throw 'VULKAN_SDK saknas (installera Vulkan SDK).' }
if (!(Test-Path $KeyPath) -or !(Test-Path $PasswordPath)) { throw "Signeringsnyckel eller lösenord saknas: $KeyPath" }
$env:TAURI_SIGNING_PRIVATE_KEY = $KeyPath
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content -Raw $PasswordPath).Trim()
$env:CARGO_TARGET_DIR = $TargetDir
if (!(Test-Path (Join-Path $repo 'src-tauri\ffmpeg\ffmpeg.exe'))) { throw 'src-tauri\ffmpeg saknas; kör model-tools\fetch-ffmpeg.ps1.' }

function Run([string]$name, [string]$command) {
    # cmd redirection: cargo progress on stderr must not become PowerShell errors.
    Write-Host "  $name"
    & cmd.exe /c "$command > `"$log\$name.log`" 2>&1"
    if ($LASTEXITCODE -ne 0) { throw "$name misslyckades; se $log\$name.log" }
}

Push-Location $repo
try {
    $runtime = Join-Path $repo 'src-tauri\runtime'
    foreach ($backend in 'Vulkan', 'CPU') {
        Write-Host "== $backend"
        $features = if ($backend -eq 'Vulkan') { '--features vulkan' } else { '' }
        if (!$SkipTests) { Run "test-$backend" "cd src-tauri && cargo test --release $features" }

        # Exact runtime DLLs: sherpa/onnxruntime/DirectML from the target dir, llama/ggml from the build
        # script that actually ran (target\release can hold stale copies from another feature set).
        $cargoFeatures = if ($backend -eq 'Vulkan') { 'tauri/custom-protocol,vulkan' } else { 'tauri/custom-protocol' }
        $native = @()
        $ErrorActionPreference = 'Continue'
        & cargo build --release --features $cargoFeatures --manifest-path src-tauri\Cargo.toml --message-format=json 2>$null | ForEach-Object {
            $e = $_ | ConvertFrom-Json
            if ($e.reason -eq 'build-script-executed' -and $e.package_id -match 'llama-cpp-sys-2') { $native += $e.out_dir }
        }
        $ErrorActionPreference = 'Stop'
        if (!$native.Count) { throw 'Hittade inte llama-cpp-sys-2 i bygget.' }
        New-Item -ItemType Directory $runtime -Force | Out-Null
        Remove-Item "$runtime\*.dll" -ErrorAction SilentlyContinue
        Get-ChildItem "$TargetDir\release\*.dll" | Where-Object { $_.Name -notmatch '^(llama|ggml.*|avskrift_lib)\.dll$' } | Copy-Item -Destination $runtime
        foreach ($d in $native) {
            Get-ChildItem -LiteralPath $d -Recurse -File -Filter '*.dll' | Where-Object { $_.Name -match '^(llama|ggml.*)\.dll$' } |
                Sort-Object LastWriteTime -Descending | Group-Object Name | ForEach-Object { Copy-Item $_.Group[0].FullName $runtime }
        }

        Run "build-$backend" "npm run tauri build -- $features"

        $suffix = $backend.ToLower()
        $setup = "$TargetDir\release\bundle\nsis\Avskrift_${version}_x64-setup.exe"
        Copy-Item $setup (Join-Path $rel "Avskrift_${version}_x64-setup-$suffix.exe") -Force
        Copy-Item "$setup.sig" (Join-Path $rel "Avskrift_${version}_x64-setup-$suffix.exe.sig") -Force
        $out = Join-Path $Dist "Avskrift-$backend-$dashed"
        if (Test-Path $out) { Remove-Item $out -Recurse -Force }
        New-Item -ItemType Directory "$out\docs" | Out-Null
        Copy-Item "$TargetDir\release\avskrift.exe", "$runtime\*.dll", "$repo\NOTICE.md" $out
        Copy-Item "$TargetDir\release\resources" $out -Recurse
        Copy-Item "$repo\src-tauri\ffmpeg" $out -Recurse
        Copy-Item "$repo\docs\TEXTKLIPP.md", "$repo\docs\MALLFLODE-MVP.md", "$repo\docs\PIANISSIMO.md", "$repo\docs\UPPDATERINGAR.md" "$out\docs"
        Copy-Item "$repo\docs\demo-support" "$out\docs" -Recurse
        $notes = Join-Path $repo "docs\RELEASE-$version.md"
        if (Test-Path $notes) { Copy-Item $notes (Join-Path $out ('L' + [char]0xC4 + 'S-MIG.md')) }
        & powershell -NoProfile -ExecutionPolicy Bypass -File "$PSScriptRoot\deps.ps1" -Dir $out
        if ($LASTEXITCODE -ne 0) { throw "Saknade beroenden i $out" }
    }

    Write-Host '== Paketering'
    Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
    foreach ($backend in 'Vulkan', 'CPU') {
        $dir = Join-Path $Dist "Avskrift-$backend-$dashed"; $top = Split-Path $dir -Leaf
        $zip = Join-Path $rel "Avskrift-$version-Windows-$backend.zip"
        if (Test-Path $zip) { Remove-Item $zip }
        $fs = [IO.File]::Open($zip, 'CreateNew')
        $za = New-Object IO.Compression.ZipArchive($fs, [IO.Compression.ZipArchiveMode]::Create, $false, [Text.Encoding]::UTF8)
        foreach ($f in Get-ChildItem $dir -Recurse -File) {
            [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($za, $f.FullName, $top + '/' + $f.FullName.Substring($dir.Length + 1).Replace('\', '/'), 'Optimal') | Out-Null
        }
        $za.Dispose(); $fs.Dispose()
    }
    $sums = Get-ChildItem $rel -File | Where-Object Name -ne 'SHA256SUMS.txt' | Sort-Object Name |
        ForEach-Object { '{0}  {1}' -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name }
    [IO.File]::WriteAllLines((Join-Path $rel 'SHA256SUMS.txt'), $sums)

    Write-Host '== Signaturer och uppdateringsfiler'
    & python "$PSScriptRoot\verify_sig.py" "$repo\src-tauri\tauri.conf.json" (Join-Path $rel "Avskrift_${version}_x64-setup-vulkan.exe") (Join-Path $rel "Avskrift_${version}_x64-setup-cpu.exe")
    if ($LASTEXITCODE -ne 0) { throw 'Signaturkontrollen misslyckades.' }
    $channels = if ($Channel -eq 'stable') { 'stable,beta' } else { 'beta' }
    & python "$PSScriptRoot\make_manifests.py" $version $rel (Join-Path $repo "docs\RELEASE-$version.md") $channels
    if ($LASTEXITCODE -ne 0) { throw 'Uppdateringsfilerna kunde inte skrivas.' }
    Write-Host "Klart: $rel"
} finally { Pop-Location }
