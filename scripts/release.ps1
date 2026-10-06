# Entrada reutilizable: crea un tag anotado solo desde main limpio y sincronizado.
param([switch]$CheckOnly)
$ErrorActionPreference = 'Stop'
function Git-Checked([string[]]$Arguments) {
    $result = & git @Arguments
    if ($LASTEXITCODE -ne 0) { throw "git $Arguments falló" }
    return $result
}

$repoRoot = Git-Checked -Arguments @('rev-parse', '--show-toplevel')
Push-Location $repoRoot
try {
    if ((Git-Checked -Arguments @('branch', '--show-current')) -ne 'main') {
        throw 'Haz merge a main y cambia a main antes de crear el release.'
    }
    if (Git-Checked -Arguments @('status', '--porcelain', '--untracked-files=no')) {
        throw 'Hay cambios en archivos del proyecto. Haz commit antes del release.'
    }
    Git-Checked -Arguments @('fetch', 'origin', 'main', '--tags') > $null
    $sha = Git-Checked -Arguments @('rev-parse', 'HEAD')
    if ($sha -ne (Git-Checked -Arguments @('rev-parse', 'origin/main'))) {
        throw 'main local no coincide con origin/main. Sincroniza antes de continuar.'
    }
    $metadata = & cargo metadata --no-deps --format-version 1 --locked
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata falló' }
    $version = ($metadata | ConvertFrom-Json).packages |
        Where-Object name -eq 'canvas-app' | Select-Object -ExpandProperty version
    $tag = "v$version"
    if ($tag -notmatch '^v\d+\.\d+\.\d+$') { throw 'Solo se publican versiones estables.' }
    & git show-ref --verify --quiet "refs/tags/$tag"
    if ($LASTEXITCODE -eq 0) { throw "$tag ya existe. Incrementa la versión en Cargo.toml." }
    & gh auth status
    if ($LASTEXITCODE -ne 0) { throw 'Inicia sesión con gh auth login.' }
    Write-Output "Release: $tag ($sha). CI y el instalador se validarán en Actions."
    if ($CheckOnly) { return }
    Git-Checked -Arguments @('tag', '-a', $tag, '-m', "Release $version") > $null
    Git-Checked -Arguments @('push', 'origin', "refs/tags/$tag")
    Write-Output 'Tag enviado. Consulta Actions > Release; Windows se publica primero.'
} finally {
    Pop-Location
}
