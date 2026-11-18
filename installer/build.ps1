# Construit l'installeur Windows de la console NAS (repris de WorkTable).
#   1. compile la console en version finale optimisée (cargo build --profile dist -p nas-console) ;
#   2. lit la version dans Cargo.toml ;
#   3. appelle Inno Setup (ISCC.exe) sur installer\NAS.iss.
# Résultat : installer\Output\NAS-<version>-setup.exe
# Prérequis : Rust (cargo) et Inno Setup 7 ou 6 (https://jrsoftware.org/isdl.php).

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    cargo build --profile dist -p nas-console
    if ($LASTEXITCODE -ne 0) { throw "La compilation a échoué." }

    $version = (Select-String -Path "Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value

    # ISCC.exe : Inno Setup 7 (64 bits, Program Files) en priorité, sinon 6, sinon celui du PATH.
    $iscc = @(
        "$env:ProgramFiles\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 7\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $iscc) { $iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source }
    if (-not $iscc) { throw "Inno Setup introuvable : installez Inno Setup 7 depuis https://jrsoftware.org/isdl.php" }

    & $iscc "/DAppVersion=$version" "installer\NAS.iss"
    if ($LASTEXITCODE -ne 0) { throw "La création de l'installeur a échoué." }
    Write-Host "Installeur prêt : installer\Output\NAS-$version-setup.exe"
}
finally {
    Pop-Location
}
