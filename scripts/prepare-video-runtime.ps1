$ErrorActionPreference = 'Stop'
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Version = '3.0.24'
$ArchiveHash = 'FCF30850371AD10C9373CC4F0F4501E7DEE49E3E9AE9F20C72FB2661A1CA6323'
$Cache = Join-Path $ProjectRoot 'target\video-deps'
$ArchiveName = "vlc-$Version-win64.zip"
$DownloadUrl = "https://downloads.videolan.org/videolan/vlc/$Version/win64/$ArchiveName"
$SourceName = "vlc-$Version.tar.xz"
$SourceUrl = "https://downloads.videolan.org/videolan/vlc/$Version/$SourceName"
$SourceHash = 'E7CAB503D1D7D5849B89D2CF0E1EE60D0EF6D012407791B644B9CFC0CC225FDF'
New-Item -ItemType Directory -Path $Cache -Force | Out-Null

function Get-VerifiedArchive($Name, $Url, $Hash) {
    $Path = Join-Path $Cache $Name
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        $Partial = Join-Path $Cache "$Name.$([Guid]::NewGuid()).download"
        try {
            & curl.exe --fail --location --proto '=https' --proto-redir '=https' --retry 3 --max-time 240 --silent --show-error --output $Partial $Url
            if ($LASTEXITCODE -ne 0) { throw "No se pudo descargar $Name." }
            if ((Get-FileHash -LiteralPath $Partial -Algorithm SHA256).Hash -ne $Hash) {
                throw "El SHA-256 de $Name no coincide con el publicado por VideoLAN."
            }
            Move-Item -LiteralPath $Partial -Destination $Path
        } finally {
            if (Test-Path -LiteralPath $Partial) { Remove-Item -LiteralPath $Partial }
        }
    }
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $Hash) {
        throw "El SHA-256 de $Path no coincide con el publicado por VideoLAN."
    }
    return $Path
}

$ArchivePath = Get-VerifiedArchive $ArchiveName $DownloadUrl $ArchiveHash
$SourcePath = Get-VerifiedArchive $SourceName $SourceUrl $SourceHash

Add-Type -AssemblyName System.IO.Compression.FileSystem
$Stage = Join-Path $Cache "staging-$([Guid]::NewGuid())"
$Runtime = Join-Path $Stage 'vlc'
New-Item -ItemType Directory -Path $Runtime | Out-Null

$Archive = [IO.Compression.ZipFile]::OpenRead($ArchivePath)
try {
    $Files = @('libvlc.dll', 'libvlccore.dll', 'AUTHORS.txt', 'COPYING.txt', 'THANKS.txt')
    $Groups = @(
        'audio_filter', 'audio_mixer', 'audio_output', 'codec', 'd3d11', 'd3d9',
        'demux', 'packetizer', 'text_renderer', 'video_chroma', 'video_filter', 'video_output'
    )
    $Plugins = @(
        'access/libfilesystem_plugin.dll', 'access/libattachment_plugin.dll',
        'keystore/libmemory_keystore_plugin.dll',
        'logger/libconsole_logger_plugin.dll', 'misc/libxml_plugin.dll',
        'spu/liblogo_plugin.dll', 'spu/libmarq_plugin.dll',
        'stream_filter/libcache_block_plugin.dll', 'stream_filter/libcache_read_plugin.dll',
        'stream_filter/libprefetch_plugin.dll', 'stream_filter/libskiptags_plugin.dll'
    )
    foreach ($Entry in $Archive.Entries) {
        $Parts = $Entry.FullName -split '/'
        if ($Parts[0] -ne "vlc-$Version") { continue }
        if ($Parts.Count -eq 2 -and $Parts[1] -in $Files) {
            $Destination = Join-Path $Runtime $Parts[1]
        } elseif ($Parts.Count -eq 4 -and $Parts[1] -eq 'plugins' -and
            $Parts[3] -match '^lib[a-zA-Z0-9_-]+_plugin\.dll$' -and
            ($Parts[2] -in $Groups -or "$($Parts[2])/$($Parts[3])" -in $Plugins) -and
            $Parts[3] -notin @('libplaylist_plugin.dll', 'libadaptive_plugin.dll')) {
            $Destination = Join-Path $Runtime "plugins\$($Parts[2])\$($Parts[3])"
        } else {
            continue
        }
        [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Destination)) | Out-Null
        [IO.Compression.ZipFileExtensions]::ExtractToFile($Entry, $Destination)
    }
    $CacheBuilder = Join-Path $Runtime 'vlc-cache-gen.exe'
    $CacheEntry = $Archive.GetEntry("vlc-$Version/vlc-cache-gen.exe")
    [IO.Compression.ZipFileExtensions]::ExtractToFile($CacheEntry, $CacheBuilder)
} finally {
    $Archive.Dispose()
}

try {
    $CacheProcess = Start-Process -FilePath $CacheBuilder -ArgumentList ('"' + (Join-Path $Runtime 'plugins') + '"') -WindowStyle Hidden -Wait -PassThru
    if ($CacheProcess.ExitCode -ne 0 -or -not (Test-Path -LiteralPath (Join-Path $Runtime 'plugins\plugins.dat'))) {
        throw 'No se pudo generar la caché de módulos de LibVLC.'
    }
} finally {
    Remove-Item -LiteralPath $CacheBuilder
}
& tar.exe -xf $SourcePath -C $Runtime --strip-components 1 "vlc-$Version/COPYING.LIB"
if ($LASTEXITCODE -ne 0) { throw 'No se pudo extraer la licencia de LibVLC.' }
@"
LibVLC $Version for Windows x64
Copyright the VideoLAN project and contributors.
Binary archive: $DownloadUrl
SHA-256: $ArchiveHash
Corresponding VLC source: $SourceUrl
Contrib sources: https://download.videolan.org/contrib/
The VLC source archive includes the contrib recipes, versions, checksums and patches.
LibVLC is licensed under LGPL-2.1-or-later. The included VLC modules and their
dependencies have their own licenses, as described in the corresponding sources.
These DLLs are the unmodified VideoLAN binaries and can be replaced independently.
"@ | Set-Content -LiteralPath (Join-Path $Runtime 'THIRD-PARTY.txt') -Encoding UTF8

Write-Output $Runtime
