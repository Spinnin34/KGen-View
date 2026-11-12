param(
    [Parameter(Mandatory = $true)][string]$BuildId,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$ReleaseTag,
    [Parameter(Mandatory = $true)][ValidateSet('beta', 'stable')][string]$Channel
)

$ErrorActionPreference = 'Stop'
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$DistDirectory = Join-Path $ProjectRoot 'dist'
$InstallerPath = Join-Path $DistDirectory 'KGenView-Setup-x64.exe'
$PortablePath = Join-Path $DistDirectory 'KGenView.exe'
foreach ($path in @($InstallerPath, $PortablePath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "No se encontró el artefacto de release: $path"
    }
}
if ($BuildId -notmatch '^[A-Fa-f0-9]{40}$') {
    throw 'El identificador de compilación debe ser un SHA de Git de 40 caracteres.'
}
if ($ReleaseTag -notmatch '^[A-Za-z0-9._-]{1,128}$') {
    throw 'La etiqueta de release contiene caracteres no permitidos.'
}

$InstallerHash = (Get-FileHash -LiteralPath $InstallerPath -Algorithm SHA256).Hash.ToLowerInvariant()
$InstallerSize = (Get-Item -LiteralPath $InstallerPath).Length
$Manifest = [ordered]@{
    schema_version = 1
    version = $Version
    build_id = $BuildId.ToLowerInvariant()
    channel = $Channel
    installer_url = "https://github.com/Spinnin34/KGen-View/releases/download/$ReleaseTag/KGenView-Setup-x64.exe"
    installer_sha256 = $InstallerHash
    installer_size = $InstallerSize
}
$Utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$ManifestJson = $Manifest | ConvertTo-Json -Depth 4
[System.IO.File]::WriteAllText((Join-Path $DistDirectory 'update.json'), $ManifestJson + "`n", $Utf8NoBom)

$Checksums = foreach ($path in @($InstallerPath, $PortablePath, (Join-Path $DistDirectory 'update.json'))) {
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $([System.IO.Path]::GetFileName($path))"
}
[System.IO.File]::WriteAllLines((Join-Path $DistDirectory 'SHA256SUMS.txt'), $Checksums, $Utf8NoBom)
