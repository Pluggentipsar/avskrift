param([string]$Dir)
# Run inside the MSVC environment (dumpbin). Every DLL that exe/DLLs in $Dir import must be in $Dir (or ffmpeg\), or be a system DLL.
$have = @{}; Get-ChildItem $Dir -Recurse -File -Include *.dll,*.exe | ForEach-Object { $have[$_.Name.ToLower()] = 1 }
$missing = @{}
foreach ($f in Get-ChildItem $Dir -Recurse -File -Include *.dll,*.exe) {
  $deps = (dumpbin /nologo /dependents $f.FullName) | Where-Object { $_ -match '^\s+\S+\.dll\s*$' } | ForEach-Object { $_.Trim().ToLower() }
  foreach ($d in $deps) {
    if ($have[$d]) { continue }
    if (Test-Path (Join-Path $env:WINDIR "System32\$d")) { continue }
    if ($d -like 'api-ms-*' -or $d -like 'ext-ms-*') { continue }
    $missing["$d <- $($f.Name)"] = 1
  }
}
if ($missing.Count) { 'MISSING:'; $missing.Keys; exit 1 } else { "OK: all imports resolved in $Dir" }
$v = (dumpbin /nologo /dependents (Join-Path $Dir 'ggml.dll')) -match 'ggml-vulkan'
"ggml.dll imports ggml-vulkan: $([bool]$v)"
