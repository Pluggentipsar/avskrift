param(
    [Parameter(Mandatory = $true)][string]$TargetDir,
    [ValidateSet('CPU', 'Vulkan')][string]$Backend = 'CPU',
    [Parameter(Mandatory = $true)][string]$LibClangPath
)
$ErrorActionPreference = 'Stop'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vs) { throw 'Visual Studio C++ Build Tools saknas.' }
$vcvars = Join-Path $vs 'VC/Auxiliary/Build/vcvars64.bat'
$buildEnv = & cmd.exe /c "`"$vcvars`" >nul && set"
if ($LASTEXITCODE -ne 0) { throw 'Kunde inte aktivera Visual Studio.' }
foreach ($line in $buildEnv) {
    if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
}
$env:PATH = (Join-Path $vs 'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin') + ';' +
    (Join-Path $vs 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja') + ';' + $env:PATH
$env:CMAKE_GENERATOR = 'Ninja'
if (!(Test-Path -LiteralPath (Join-Path $LibClangPath 'libclang.dll'))) { throw 'libclang.dll saknas.' }
$env:LIBCLANG_PATH = [IO.Path]::GetFullPath($LibClangPath)
Remove-Item Env:WHISPER_DONT_GENERATE_BINDINGS -ErrorAction SilentlyContinue
$features = @()
if ($Backend -eq 'Vulkan') {
    if (!$env:VULKAN_SDK) { throw 'VULKAN_SDK måste vara satt för GPU-bygge.' }
    $features = @('--features', 'vulkan')
}
& cargo build --locked --release --manifest-path (Join-Path $PSScriptRoot 'whisper-probe/Cargo.toml') --target-dir $TargetDir @features
if ($LASTEXITCODE -ne 0) { throw 'Bygget misslyckades.' }
