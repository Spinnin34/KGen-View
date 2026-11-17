param(
    [string]$FixtureDirectory = (Join-Path $PSScriptRoot '..\target\performance-fixtures'),
    [ValidateRange(100, 20000)][int]$PageCount = 1000,
    [switch]$IncludeLargePng
)
$ErrorActionPreference = 'Stop'
$FixtureDirectory = [IO.Path]::GetFullPath($FixtureDirectory)
if (-not (Test-Path -LiteralPath $FixtureDirectory -PathType Container)) {
    throw 'Genera primero las imágenes con scripts/new-benchmark-fixtures.ps1.'
}
Add-Type -AssemblyName System.IO.Compression
$OutputDirectory = Join-Path $FixtureDirectory 'archives'
[void][IO.Directory]::CreateDirectory($OutputDirectory)
$Seed = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\resources\kgen-view-logo.png'))
$ManyPagesPath = Join-Path $OutputDirectory "cbz-many-$PageCount-pages.cbz"

function New-ZipFixture {
    param(
        [string]$Path,
        [scriptblock]$Populate
    )
    if (Test-Path -LiteralPath $Path) {
        Write-Output "Se conserva el archivo existente: $Path"
        return
    }
    $TemporaryPath = "$Path.partial"
    try {
        $Stream = [IO.File]::Open($TemporaryPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        try {
            $Archive = [IO.Compression.ZipArchive]::new($Stream, [IO.Compression.ZipArchiveMode]::Create, $false)
            try { & $Populate $Archive }
            finally { $Archive.Dispose() }
        } finally { $Stream.Dispose() }
        [IO.File]::Move($TemporaryPath, $Path)
    } catch {
        Remove-Item -LiteralPath $TemporaryPath -Force -ErrorAction SilentlyContinue
        throw
    }
}

New-ZipFixture $ManyPagesPath {
    param($Archive)
    $InputStream = [IO.File]::OpenRead($Seed)
    try {
        for ($Index = 1; $Index -le $PageCount; $Index++) {
            $Entry = $Archive.CreateEntry(('pages/page{0:D5}.png' -f $Index), [IO.Compression.CompressionLevel]::Fastest)
            $OutputStream = $Entry.Open()
            try {
                $InputStream.Position = 0
                $InputStream.CopyTo($OutputStream, 128 * 1024)
            } finally { $OutputStream.Dispose() }
        }
    } finally { $InputStream.Dispose() }
}

$HeavySources = @(
    [pscustomobject]@{ Name = 'photo-24mp.jpg'; Path = (Join-Path $FixtureDirectory 'photo-24mp.jpg') },
    [pscustomobject]@{ Name = 'photo-60mp.jpg'; Path = (Join-Path $FixtureDirectory 'photo-60mp.jpg') }
)
if ($IncludeLargePng) {
    foreach ($Name in @('photo-24mp.png', 'photo-60mp.png')) {
        $Path = Join-Path $FixtureDirectory $Name
        if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
            throw "Falta $Name. Genera las imágenes con scripts/new-benchmark-fixtures.ps1 -LargePng."
        }
        $HeavySources += [pscustomobject]@{ Name = $Name; Path = $Path }
    }
}
foreach ($Source in $HeavySources) {
    if (-not (Test-Path -LiteralPath $Source.Path -PathType Leaf)) {
        throw "Falta $($Source.Name). Genera primero las imágenes con scripts/new-benchmark-fixtures.ps1."
    }
}
$HeavyZipPath = Join-Path $OutputDirectory $(if ($IncludeLargePng) { 'zip-heavy-jpeg-png.zip' } else { 'zip-heavy-jpeg.zip' })
New-ZipFixture $HeavyZipPath {
    param($Archive)
    foreach ($Source in $HeavySources) {
        $Entry = $Archive.CreateEntry("pages/$($Source.Name)", [IO.Compression.CompressionLevel]::Fastest)
        $OutputStream = $Entry.Open()
        $InputStream = [IO.File]::OpenRead($Source.Path)
        try { $InputStream.CopyTo($OutputStream, 128 * 1024) }
        finally { $InputStream.Dispose(); $OutputStream.Dispose() }
    }
}

Get-ChildItem -LiteralPath $OutputDirectory -File -Include '*.cbz', '*.zip' |
    Select-Object Name, Length, LastWriteTime
