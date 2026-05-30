# Preflight: kontrollerar att alla verktyg för att bygga Avskrift finns.
# Kör: model-tools\preflight.ps1
# Stannar inte vid första felet — listar ALLT som saknas så du kan fixa allt på en gång.

$ErrorActionPreference = "Continue"
$missing = @()
$ok = @()

function Test-Tool {
    param([string]$Name, [string]$Cmd, [string]$VersionArg = "--version", [string]$Hint)
    $exe = Get-Command $Cmd -ErrorAction SilentlyContinue
    if ($exe) {
        $ver = ""
        try { $ver = (& $Cmd $VersionArg 2>&1 | Select-Object -First 1) } catch {}
        $script:ok += "  [OK] $Name  ($ver)"
        return $true
    } else {
        $script:missing += "  [SAKNAS] $Name`n           -> $Hint"
        return $false
    }
}

Write-Host "`n=== Avskrift preflight ===`n" -ForegroundColor Cyan

Test-Tool "Rust (rustc)" "rustc" "--version" "https://rustup.rs  (installerar rustc + cargo)" | Out-Null
Test-Tool "Cargo"        "cargo" "--version" "Ingår i Rust via rustup.rs" | Out-Null
Test-Tool "Node.js"      "node"  "--version" "https://nodejs.org  (LTS, 18 eller senare)" | Out-Null
Test-Tool "npm"          "npm"   "--version" "Ingår i Node.js" | Out-Null
Test-Tool "CMake"        "cmake" "--version" "https://cmake.org/download/  (krävs av whisper.cpp/sherpa-onnx)" | Out-Null

# Node-major >= 18?
$node = Get-Command node -ErrorAction SilentlyContinue
if ($node) {
    $nv = (& node --version) -replace 'v',''
    $major = [int]($nv.Split('.')[0])
    if ($major -lt 18) { $missing += "  [FÖR GAMMAL] Node $nv -> uppgradera till 18+ (https://nodejs.org)" }
}

# C++-kompilator (MSVC på Windows). cl.exe finns bara i en "Developer"-prompt; vi kollar mjukt.
$cl = Get-Command cl.exe -ErrorAction SilentlyContinue
if ($cl) {
    $ok += "  [OK] C++-kompilator (cl.exe i PATH)"
} else {
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $vc = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
        if ($vc) {
            $ok += "  [OK] Visual Studio C++ Build Tools hittade ($vc)"
            $ok += "       Obs: bygg i 'x64 Native Tools Command Prompt' om cargo inte hittar cl.exe."
        } else {
            $missing += "  [SAKNAS] C++ Build Tools (VC.Tools.x86.x64)`n           -> Visual Studio Installer: lägg till 'Desktop development with C++'"
        }
    } else {
        $missing += "  [SAKNAS] Visual Studio C++ Build Tools`n           -> https://visualstudio.microsoft.com/visual-cpp-build-tools/  ('Desktop development with C++')"
    }
}

# Python (endast för KB-BERT-konvertering, build-pii-ner.ps1)
if (Get-Command python -ErrorAction SilentlyContinue) {
    $ok += "  [OK] Python  (behövs bara för build-pii-ner.ps1)"
} else {
    $ok += "  [INFO] Python saknas — behövs bara för build-pii-ner.ps1 (KB-BERT -> ONNX)."
}

Write-Host ($ok -join "`n") -ForegroundColor Green
if ($missing.Count -gt 0) {
    Write-Host "`nÅtgärda detta innan du går vidare:`n" -ForegroundColor Yellow
    Write-Host ($missing -join "`n`n") -ForegroundColor Yellow
    Write-Host "`nKör preflight igen när du installerat ovanstående.`n" -ForegroundColor Yellow
    exit 1
} else {
    Write-Host "`nAllt på plats! Nästa steg (se START.md):" -ForegroundColor Cyan
    Write-Host "  1) npm install ; npm run build"
    Write-Host "  2) cargo check --manifest-path src-tauri/Cargo.toml   <- fångar API-fel utan modeller`n"
    exit 0
}
