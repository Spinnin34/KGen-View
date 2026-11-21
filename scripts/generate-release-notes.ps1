param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('beta', 'stable')]
    [string]$Channel,

    [Parameter(Mandatory = $true)]
    [string]$BuildSha,

    [Parameter(Mandatory = $true)]
    [string]$OutputPath
)

$ErrorActionPreference = 'Stop'
$sections = [ordered]@{
    'Novedades' = [System.Collections.Generic.List[string]]::new()
    'Mejoras' = [System.Collections.Generic.List[string]]::new()
    'Correcciones' = [System.Collections.Generic.List[string]]::new()
}

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
    $subject = $message[0].Trim()
    $section = $null
    $categorized = $false
    foreach ($line in $message | Select-Object -Skip 1) {
        $heading = $line.Trim()
        if ($heading -match '^#{1,4}\s*(?:✨\s*)?(?:Novedades|Nuevas funciones|Features?)\s*$') {
            $section = 'Novedades'
            $categorized = $true
        } elseif ($heading -match '^#{1,4}\s*(?:⚡\s*)?(?:Mejoras?|Optimización|Performance)\s*$') {
            $section = 'Mejoras'
            $categorized = $true
        } elseif ($heading -match '^#{1,4}\s*(?:🐛\s*)?(?:Correcciones|Arreglos|Fixes?|Bugs?)\s*$') {
            $section = 'Correcciones'
            $categorized = $true
        } elseif ($heading -match '^#{1,4}\s+') {
            $section = $null
        } elseif ($section -and $line -match '^\s*[-*]\s+(.+?)\s*$') {
            $sections[$section].Add($Matches[1].Trim())
        }
    }

    if (-not $categorized) {
        $category = if ($subject -match '^(feat|feature|nueva|añade)(\(.+\))?:') {
            'Novedades'
        } elseif ($subject -match '^(fix|bugfix|corrige|arregla)(\(.+\))?:') {
            'Correcciones'
        } else {
            'Mejoras'
        }
        $summary = $subject -replace '^(feat|feature|nueva|añade|fix|bugfix|corrige|arregla|perf|refactor)(\(.+\))?:\s*', ''
        $sections[$category].Add($summary)
    }
}

$channelLabel = if ($Channel -eq 'stable') { 'Estable' } else { 'Beta' }
$notes = [System.Collections.Generic.List[string]]::new()
$notes.Add("Canal: $channelLabel")
$notes.Add("Build: $head")
$notes.Add('')
foreach ($entry in $sections.GetEnumerator()) {
    $notes.Add("## $($entry.Key)")
    if ($entry.Value.Count -eq 0) {
        $notes.Add('- Sin cambios en esta categoría.')
    } else {
        foreach ($item in $entry.Value) {
            $notes.Add("- $item")
        }
    }
    $notes.Add('')
}
$notes.Add('Descarga el instalador para actualizar la copia instalada. El ZIP portátil incluye el motor de vídeo.')

$directory = Split-Path -Parent $OutputPath
if ($directory) { New-Item -ItemType Directory -Force -Path $directory | Out-Null }
Set-Content -LiteralPath $OutputPath -Value ($notes -join "`n") -Encoding utf8NoBOM
