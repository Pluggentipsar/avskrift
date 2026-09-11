param(
    [ValidateSet('Vulkan', 'CPU')][string]$Backend = 'Vulkan',
    [Parameter(Mandatory = $true)][string]$TargetDir,
    [string]$LibClangPath = $env:LIBCLANG_PATH,
    [ValidatePattern('^[a-zA-Z0-9-]*$')][string]$PackageSuffix = ''
)
$ErrorActionPreference = 'Stop'
$taskRepo = Split-Path -Parent $PSScriptRoot
$taskTarget = [IO.Path]::GetFullPath($TargetDir)
if ($taskTarget.Length -gt 32) { throw 'Välj en kort byggsökväg, till exempel C:\avb, för att undvika Windows sökvägsgräns.' }
$taskVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$taskVs = & $taskVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$taskVs) { throw 'Visual Studio C++ Build Tools saknas.' }
$taskVcvars = Join-Path $taskVs 'VC/Auxiliary/Build/vcvars64.bat'
$taskEnvironment = & cmd.exe /c "`"$taskVcvars`" >nul && set"
if ($LASTEXITCODE -ne 0) { throw 'Kunde inte aktivera Visual Studio-miljön.' }
foreach ($taskLine in $taskEnvironment) {
    if ($taskLine -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
}
$taskCmake = Join-Path $taskVs 'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin'
$taskNinja = Join-Path $taskVs 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja'
$env:PATH = "$taskCmake;$taskNinja;" + $env:PATH
if (!$LibClangPath -or !(Test-Path -LiteralPath (Join-Path $LibClangPath 'libclang.dll'))) {
    throw 'Ange -LibClangPath till katalogen som innehåller libclang.dll.'
}
$env:LIBCLANG_PATH = $LibClangPath
# cmake-rs with an implicit Visual Studio generator can replace CMAKE_*_FLAGS_RELEASE
# without /O2. An explicit Ninja generator preserves CMake's release optimization flags.
$env:CMAKE_GENERATOR = 'Ninja'
if ($Backend -eq 'Vulkan' -and !$env:VULKAN_SDK) { throw 'Installera Vulkan SDK och sätt VULKAN_SDK först.' }
$taskBuildDir = Join-Path $taskTarget 'release/build'
foreach ($taskDir in @(Get-ChildItem -LiteralPath $taskBuildDir -Directory -Filter 'whisper-rs-sys-*' -ErrorAction SilentlyContinue)) {
    $taskCache = Join-Path $taskDir.FullName 'out/build/CMakeCache.txt'
    if (Test-Path -LiteralPath $taskCache) {
        $taskConfig = Get-Content -LiteralPath $taskCache
        if (!($taskConfig -match '^CMAKE_GENERATOR:[^=]+=Ninja$') -or !($taskConfig -match '^CMAKE_CXX_FLAGS_RELEASE:[^=]+=.*[/\-]O2')) {
            throw "Byggcachen $taskCache använder andra eller ooptimerade inställningar. Välj en ny, kort -TargetDir."
        }
    }
}
Push-Location $taskRepo
try {
    & npm.cmd run build
    if ($LASTEXITCODE -ne 0) { throw 'Frontend-bygget misslyckades.' }
    $taskFeatures = if ($Backend -eq 'Vulkan') { 'tauri/custom-protocol,vulkan' } else { 'tauri/custom-protocol' }
    $taskNativeDirs = @()
    & cargo build --release --no-default-features --features $taskFeatures --target-dir $taskTarget --manifest-path src-tauri/Cargo.toml --message-format=json-render-diagnostics | ForEach-Object {
        $taskEvent = $_ | ConvertFrom-Json
        if ($taskEvent.reason -eq 'compiler-message') { Write-Host $taskEvent.message.rendered }
        if ($taskEvent.reason -eq 'build-script-executed' -and $taskEvent.package_id -match 'llama-cpp-sys-2') { $taskNativeDirs += $taskEvent.out_dir }
    }
    if ($LASTEXITCODE -ne 0) { throw 'Rust-bygget misslyckades.' }
    $taskRelease = Join-Path $taskTarget 'release'
    $taskOutput = Join-Path $taskRepo "dist/Avskrift-$Backend"
    if ($PackageSuffix) { $taskOutput += "-$PackageSuffix" }
    New-Item -ItemType Directory -Path $taskOutput -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $taskRelease 'avskrift.exe') -Destination $taskOutput -Force
    Get-ChildItem -LiteralPath (Join-Path $taskRepo 'src-tauri/runtime') -File -Filter '*.dll' |
        Where-Object { $_.Name -notmatch '^(llama|ggml.*)\.dll$' } |
        ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $taskOutput -Force }
    # Package matching native DLLs directly. Cargo may hard-link these to target/release;
    # copying back there can attempt to overwrite a file with itself.
    if (!$taskNativeDirs.Count) { throw 'Kunde inte hitta de aktiva native-biblioteken i Cargo-resultatet.' }
    foreach ($taskNative in $taskNativeDirs) {
        $taskDlls = Get-ChildItem -LiteralPath $taskNative -Recurse -File -Filter '*.dll' |
            Where-Object { $_.Name -match '^(llama|ggml.*)\.dll$' } | Sort-Object LastWriteTime -Descending |
            Group-Object Name | ForEach-Object { $_.Group[0] }
        foreach ($taskDll in $taskDlls) { Copy-Item -LiteralPath $taskDll.FullName -Destination $taskOutput -Force }
    }
    if (Test-Path -LiteralPath (Join-Path $taskRelease 'resources')) {
        Copy-Item -LiteralPath (Join-Path $taskRelease 'resources') -Destination $taskOutput -Recurse -Force
    }
    Write-Host "Klart: $taskOutput\avskrift.exe"
} finally { Pop-Location }
