param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('beta', 'stable')]
    [string]$Channel,

    [Parameter(Mandatory = $true)]
    [string]$BuildSha,

    [Parameter(Mandatory = $true)]
    [string]$ReleaseTag,

    [Parameter(Mandatory = $true)]
    [string]$Version,

    [Parameter(Mandatory = $true)]
    [string]$OutputPath
)

$ErrorActionPreference = 'Stop'

function Remove-Emoji([string]$Text) {
    return ($Text -replace '[\uD800-\uDBFF][\uDC00-\uDFFF]|[\u2600-\u27BF]|\u00A9|\u00AE|\u203C|\u2049|\u2122|\u2139|\u3030|\u303D|\u3297|\u3299|\uFE0E|\uFE0F|\u200D', '').Trim()
}

function Add-Unique([System.Collections.Generic.List[string]]$List, [string]$Value) {
    $clean = Remove-Emoji $Value
    if ($clean -and -not ($List | Where-Object { $_.Equals($clean, [StringComparison]::OrdinalIgnoreCase) })) {
        $List.Add($clean)
    }
}

$sections = [ordered]@{
    'Resumen funcional' = [System.Collections.Generic.List[string]]::new()
    'Funciones nuevas' = [System.Collections.Generic.List[string]]::new()
    'Formatos y compatibilidad' = [System.Collections.Generic.List[string]]::new()
    'Interfaz y rendimiento' = [System.Collections.Generic.List[string]]::new()
    'Correcciones y robustez' = [System.Collections.Generic.List[string]]::new()
    'Cambios técnicos' = [System.Collections.Generic.List[string]]::new()
}
$commitSummaries = [System.Collections.Generic.List[string]]::new()

$tagPattern = if ($Channel -eq 'stable') { '^v\d+\.\d+\.\d+$' } else { '^beta-' }
$head = (& git rev-parse --verify "$BuildSha^{commit}").Trim()
$base = $null
$releaseRefs = @(& git for-each-ref --sort=-creatordate --format='%(refname:short)%09%(objectname)' refs/tags)
foreach ($line in $releaseRefs) {
    $parts = $line -split "`t", 2
    if ($parts.Count -lt 2 -or $parts[0] -notmatch $tagPattern) { continue }
    if ($parts[0] -eq $env:GITHUB_REF_NAME) { continue }
    $tagCommit = (& git rev-list -n 1 $parts[0]).Trim()
    if ($tagCommit -and $tagCommit -ne $head) {
        $base = $tagCommit
        break
    }
}

$commits = if ($base) {
    @(& git rev-list --reverse --no-merges "$base..$head")
} else {
    @(& git rev-list --reverse --no-merges -n 1 $head)
}

foreach ($commit in $commits) {
    $message = @(& git show -s --format=%B $commit)
    if ($message.Count -eq 0) { continue }
    $subject = Remove-Emoji $message[0]
    Add-Unique $commitSummaries $subject
    $section = $null
    $categorized = $false

    foreach ($line in $message | Select-Object -Skip 1) {
        $cleanLine = Remove-Emoji $line
        $heading = $cleanLine.Trim()
        if ($heading -match '^#{1,4}\s*(?:Resumen|Summary)(?: funcional| técnico)?\s*$') {
            $section = 'Resumen funcional'
            $categorized = $true
        } elseif ($heading -match '^#{1,4}\s*(?:Novedades|Nuevas funciones|Features?)\s*$') {
            $section = 'Funciones nuevas'
            $categorized = $true
        } elseif ($heading -match '^#{1,4}\s*(?:Formatos(?: compatibles)?|Compatibilidad|Supported formats)\s*$') {
            $section = 'Formatos y compatibilidad'
            $categorized = $true
        } elseif ($heading -match '^#{1,4}\s*(?:Mejoras?|Optimización|Performance|Interfaz y rendimiento|User experience)\s*$') {
            $section = 'Interfaz y rendimiento'
            $categorized = $true
        } elseif ($heading -match '^#{1,4}\s*(?:Correcciones|Arreglos|Fixes?|Bugs?|Robustez)\s*$') {
            $section = 'Correcciones y robustez'
            $categorized = $true
        } elseif ($heading -match '^#{1,4}\s*(?:Detalles técnicos|Cambios técnicos|Implementación|Engineering|Technical changes)\s*$') {
            $section = 'Cambios técnicos'
            $categorized = $true
        } elseif ($heading -match '^#{1,4}\s+') {
            $section = $null
        } elseif ($section -and $cleanLine -match '^\s*[-*]\s+(.+?)\s*$') {
            Add-Unique $sections[$section] $Matches[1]
        }
    }

    if (-not $categorized) {
        $category = if ($subject -match '^(feat|feature|nueva|añade)(\(.+\))?:.*(?:formato|codec|imagen|abrir|open|format)' -or $subject -match '^(?:format|codec|image)(\(.+\))?:') {
            'Formatos y compatibilidad'
        } elseif ($subject -match '^(feat|feature|nueva|añade)(\(.+\))?:') {
            'Funciones nuevas'
        } elseif ($subject -match '^(fix|bugfix|corrige|arregla)(\(.+\))?:') {
            'Correcciones y robustez'
        } else {
            'Interfaz y rendimiento'
        }
        $summary = $subject -replace '^(feat|feature|nueva|añade|fix|bugfix|corrige|arregla|perf|refactor)(\(.+\))?:\s*', ''
        Add-Unique $sections[$category] $summary
    }
}

$range = if ($base) { "$( $base.Substring(0, 12) )..$( $head.Substring(0, 12) )" } else { $head.Substring(0, 12) }
$paths = if ($base) {
    @(& git diff --name-only "$base..$head")
} else {
    @(& git show --pretty=format: --name-only $head | Where-Object { $_ })
}
$shortStat = if ($base) {
    (& git diff --shortstat "$base..$head" | Out-String).Trim()
} else {
    (& git show --shortstat --oneline $head | Out-String).Trim()
}
$components = [System.Collections.Generic.List[string]]::new()
if ($paths -match '(^|/)imaging(/|\.rs)') { $components.Add('Pipeline de imagen: decodificación, transformación de píxeles y exportación') }
if ($paths -match '(^|/)platform/conversion_dialog') { $components.Add('Diálogo nativo de conversión: cálculo de espacio disponible y presentación de formatos') }
if ($paths -match '(^|/)controller/') { $components.Add('Servicios del controlador: apertura, guardado y operaciones de archivo') }
if ($paths -match '(^|/)formats\.rs|Cargo\.toml|Cargo\.lock') { $components.Add('Inventario de formatos y dependencias de códecs') }
if ($paths -match 'README\.md') { $components.Add('Documentación de compatibilidad, límites y flujo de conversión') }
if ($paths -match 'scripts/|\.github/workflows/') { $components.Add('Automatización de compilación, validación y publicación') }

$channelLabel = if ($Channel -eq 'stable') { 'Estable' } else { 'Beta' }
$notes = [System.Collections.Generic.List[string]]::new()
$notes.Add('## Resumen de la compilación')
$notes.Add("- Canal: $channelLabel")
$notes.Add("- Versión de la aplicación: ``$Version``")
$notes.Add("- Etiqueta de GitHub Release: ``$ReleaseTag``")
$notes.Add("- Commit publicado: ``$head``")
$notes.Add("- Rango desde la publicación anterior: ``$range``")
$notes.Add("- Commits incluidos: $($commitSummaries.Count)")
$notes.Add("- Archivos modificados: $($paths.Count)")
if ($shortStat) { $notes.Add("- Alcance del diff: $shortStat") }
$notes.Add('')

foreach ($entry in $sections.GetEnumerator()) {
    if ($entry.Value.Count -eq 0) { continue }
    $notes.Add("## $($entry.Key)")
    foreach ($item in $entry.Value) { $notes.Add("- $item") }
    $notes.Add('')
}

if ($components.Count -gt 0) {
    $notes.Add('## Componentes afectados')
    foreach ($component in $components) { $notes.Add("- $component") }
    $notes.Add('')
}

$notes.Add('## Validación de calidad')
$notes.Add('- Formato del código: ``cargo fmt -- --check``.')
$notes.Add('- Análisis estático: ``cargo clippy --locked --all-targets -- -D warnings -A clippy::too_many_arguments``.')
$notes.Add('- Pruebas automatizadas: ``cargo test --locked -- --test-threads=1``.')
$notes.Add('- Compilación de distribución: ``cargo build --locked --release``.')
$notes.Add('- GitHub Actions publica los artefactos solo después de que el trabajo de calidad haya terminado correctamente.')
$notes.Add('')
$notes.Add('## Archivos publicados')
$notes.Add('- ``KGenView-Setup-x64.exe``: instalador de Windows x64 para actualizar la copia instalada.')
$notes.Add('- ``KGenView-Portable-x64.zip`` y ``KGenView.exe``: paquete portable y ejecutable independiente.')
$notes.Add('- ``update.json`` y ``SHA256SUMS.txt``: manifiesto del actualizador y hashes SHA-256 de los artefactos.')
$notes.Add('- Las copias portables se actualizan descargando y extrayendo el nuevo ZIP.')
$notes.Add('')
$notes.Add('## Commits incluidos')
foreach ($subject in $commitSummaries) { $notes.Add("- $subject") }

$cleanNotes = [System.Collections.Generic.List[string]]::new()
foreach ($line in $notes) { $cleanNotes.Add((Remove-Emoji $line)) }
$notes = $cleanNotes
$directory = Split-Path -Parent $OutputPath
if ($directory) { New-Item -ItemType Directory -Force -Path $directory | Out-Null }
Set-Content -LiteralPath $OutputPath -Value ($notes -join "`n") -Encoding utf8NoBOM
