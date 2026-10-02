param([ValidateSet('build', 'test')][string]$Action = 'build')
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
& cargo $Action --locked --release --manifest-path (Join-Path $PSScriptRoot 'pianissimo-rust-probe/Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'Rust-kommandot misslyckades.' }
