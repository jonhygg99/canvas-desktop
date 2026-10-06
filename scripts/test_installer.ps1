# Ejecutar solo en el runner desechable de CI: registra/desregistra asociaciones.
param([Parameter(Mandatory)][string]$Installer,
      [Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:CI -ne 'true' -or -not $env:RUNNER_TEMP) {
    throw 'Esta prueba modifica HKCU; solo se permite en CI con RUNNER_TEMP.'
}
$installRoot = Join-Path $env:RUNNER_TEMP 'canvas-installer-smoke'
if (Test-Path -LiteralPath $installRoot) { throw 'El destino de prueba ya existe.' }

function Run-Checked([string]$File, [string[]]$Arguments) {
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -PassThru -WindowStyle Hidden
    if (-not $process.WaitForExit(120000)) {
        $process.Kill()
        throw "Timeout ejecutando $File"
    }
    if ($process.ExitCode -ne 0) { throw "$File terminó con $($process.ExitCode)" }
}

try {
    # /D debe ser el último argumento y NSIS lo interpreta sin comillas.
    Run-Checked -File (Resolve-Path -LiteralPath $Installer).Path -Arguments @('/S', '/NS', "/D=$installRoot")
    $binary = Join-Path $installRoot 'canvas-desktop.exe'
    $uninstaller = Join-Path $installRoot 'uninstall.exe'
    if (-not (Test-Path -LiteralPath $binary) -or -not (Test-Path -LiteralPath $uninstaller)) {
        throw 'El instalador no creó el ejecutable y el desinstalador.'
    }
    $actual = (Get-Item -LiteralPath $binary).VersionInfo.ProductVersion
    if ($actual -ne $Version) { throw "Versión instalada $actual; esperada $Version" }
    $association = Get-Item -LiteralPath 'HKCU:\Software\Classes\CanvasDesktop.Image\shell\open\command'
    if (-not $association.GetValue('').Contains($binary)) { throw 'Asociación Abrir con incorrecta.' }
    Run-Checked -File $uninstaller -Arguments @('/S', "_?=$installRoot")
    if (Test-Path -LiteralPath $binary) { throw 'La desinstalación dejó el ejecutable instalado.' }
    if (Test-Path -LiteralPath 'HKCU:\Software\Classes\CanvasDesktop.Image') {
        throw 'La desinstalación dejó el ProgID registrado.'
    }
    Write-Output 'Instalación, versión, asociación y desinstalación verificadas.'
} finally {
    # No borra directorios: si falla, conserva la evidencia en el runner.
    $uninstaller = Join-Path $installRoot 'uninstall.exe'
    if (Test-Path -LiteralPath $uninstaller) {
        Run-Checked -File $uninstaller -Arguments @('/S', "_?=$installRoot")
    }
}
