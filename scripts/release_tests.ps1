# Prueba el comando completo con Git/Cargo/gh simulados: nunca publica nada.
$ErrorActionPreference = 'Stop'
$global:CanvasReleaseTestState = @{
    root = Split-Path $PSScriptRoot -Parent
    calls = [System.Collections.Generic.List[string]]::new()
    branch = 'main'
    dirty = $false
    remoteSha = 'abc'
    tagExists = $false
}
function git {
    $global:CanvasReleaseTestState.calls.Add("git $($args -join ' ')")
    $global:LASTEXITCODE = 0
    switch ($args -join ' ') {
        'rev-parse --show-toplevel' { return $global:CanvasReleaseTestState.root }
        'branch --show-current' { return $global:CanvasReleaseTestState.branch }
        'status --porcelain --untracked-files=no' { if ($global:CanvasReleaseTestState.dirty) { return ' M Cargo.toml' }; return }
        'rev-parse HEAD' { return 'abc' }
        'rev-parse origin/main' { return $global:CanvasReleaseTestState.remoteSha }
        'show-ref --verify --quiet refs/tags/v0.7.0' {
            $global:LASTEXITCODE = [int](-not $global:CanvasReleaseTestState.tagExists)
        }
    }
}
function cargo {
    $global:LASTEXITCODE = 0
    return '{"packages":[{"name":"canvas-app","version":"0.7.0"}]}'
}
function gh { $global:LASTEXITCODE = 0 }
function Start-Process {
    param($FilePath, [string[]]$ArgumentList, [switch]$PassThru, $WindowStyle)
    $global:CanvasReleaseTestState.installerArgs = $ArgumentList
    $process = [pscustomobject]@{ ExitCode = 0 }
    $process | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($Timeout) return $true }
    return $process
}
function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Assert-Rejected([string]$Expected) {
    $global:CanvasReleaseTestState.calls.Clear()
    try {
        & "$PSScriptRoot/release.ps1"
        throw 'El release no fue rechazado.'
    } catch {
        Assert ($_.Exception.Message -like "*$Expected*") "Error inesperado: $_"
    }
    Assert (-not ($global:CanvasReleaseTestState.calls | Where-Object { $_ -like 'git tag *' -or $_ -like 'git push *' })) 'Publicó pese al rechazo.'
}

try {
    # Solo carga la función de ejecución, nunca la instalación/registro real.
    $parseErrors = $null; $tokens = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile(
        "$PSScriptRoot/test_installer.ps1", [ref]$tokens, [ref]$parseErrors)
    Assert ($parseErrors.Count -eq 0) 'Sintaxis del smoke test incorrecta.'
    $runner = $ast.Find({ param($Node) $Node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $Node.Name -eq 'Run-Checked' }, $true)
    . ([scriptblock]::Create($runner.Extent.Text))
    Run-Checked -File 'fake-installer.exe' -Arguments @('/S', '/NS', '/D=temp with spaces')
    Assert ($global:CanvasReleaseTestState.installerArgs.Count -eq 3) 'Se perdieron argumentos NSIS.'
    Assert ($global:CanvasReleaseTestState.installerArgs[2] -eq '/D=temp with spaces') 'Destino NSIS incorrecto.'
    & "$PSScriptRoot/release.ps1" -CheckOnly
    Assert (-not ($global:CanvasReleaseTestState.calls | Where-Object { $_ -like 'git tag *' -or $_ -like 'git push *' })) 'CheckOnly publicó un tag.'
    Assert ($global:CanvasReleaseTestState.calls.Contains('git fetch origin main --tags')) 'No comprobó el remoto.'
    $global:CanvasReleaseTestState.calls.Clear()
    & "$PSScriptRoot/release.ps1"
    Assert ($global:CanvasReleaseTestState.calls.Contains('git tag -a v0.7.0 -m Release 0.7.0')) 'No creó el tag anotado correcto.'
    Assert ($global:CanvasReleaseTestState.calls.Contains('git push origin refs/tags/v0.7.0')) 'No envió el ref exacto.'
    $global:CanvasReleaseTestState.branch = 'feature'
    Assert-Rejected 'Haz merge a main'
    $global:CanvasReleaseTestState.branch = 'main'; $global:CanvasReleaseTestState.dirty = $true
    Assert-Rejected 'Hay cambios'
    $global:CanvasReleaseTestState.dirty = $false; $global:CanvasReleaseTestState.remoteSha = 'different'
    Assert-Rejected 'no coincide con origin/main'
    $global:CanvasReleaseTestState.remoteSha = 'abc'; $global:CanvasReleaseTestState.tagExists = $true
    Assert-Rejected 'ya existe'
    Write-Output 'OK: CheckOnly, publicación y cuatro rechazos verificados sin tocar GitHub.'

} finally {
    Remove-Variable -Name CanvasReleaseTestState -Scope Global
}
