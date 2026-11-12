param(
    [switch]$SkipValidation,
    [string]$BuildId = $env:KGEN_BUILD_ID
)

$ErrorActionPreference = 'Stop'
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$DistDirectory = Join-Path $ProjectRoot 'dist'
$CargoManifest = Join-Path $ProjectRoot 'Cargo.toml'
$CargoVersionLine = Select-String -LiteralPath $CargoManifest -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $CargoVersionLine) {
    throw 'No se pudo leer la versión de Cargo.toml.'
}
$PackageVersion = $CargoVersionLine.Matches[0].Groups[1].Value
if (-not $BuildId) {
    $BuildId = (git -C $ProjectRoot rev-parse HEAD).Trim()
}
if (-not $BuildId) {
    $BuildId = 'local'
}
$env:KGEN_BUILD_ID = $BuildId
$SigningCertificate = $env:KGEN_SIGN_CERT
$SigningPassword = $env:KGEN_SIGN_PASSWORD
$SignToolPath = $null

if ($SigningCertificate) {
    if (-not (Test-Path -LiteralPath $SigningCertificate -PathType Leaf)) {
        throw "No se encontró el certificado de firma: $SigningCertificate"
    }
    $SignToolCommand = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($SignToolCommand) {
        $SignToolPath = $SignToolCommand.Source
    } elseif (${env:ProgramFiles(x86)}) {
        $WindowsKitBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
        $SignToolPath = Get-ChildItem -LiteralPath $WindowsKitBin -Filter signtool.exe -File -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
            Sort-Object FullName -Descending |
            Select-Object -First 1 -ExpandProperty FullName
    }
    if (-not $SignToolPath) {
        throw 'No se encontró signtool.exe. Instala Windows SDK o añádelo al PATH.'
    }
}

function Sign-ReleaseFile {
    param([Parameter(Mandatory = $true)][string]$FilePath)

    if (-not $script:SigningCertificate) {
        return
    }
    $SignArguments = @(
        'sign', '/fd', 'SHA256', '/f', $script:SigningCertificate,
        '/tr', 'http://timestamp.digicert.com', '/td', 'SHA256'
    )
    if ($script:SigningPassword) {
        $SignArguments += @('/p', $script:SigningPassword)
    }
    $SignArguments += $FilePath
    & $script:SignToolPath @SignArguments
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo firmar: $FilePath"
    }
}

Push-Location $ProjectRoot
try {
    if (-not $SkipValidation) {
    cargo fmt -- --check
    if ($LASTEXITCODE -ne 0) { throw 'cargo fmt falló' }
    # Existing Win32 dialog helpers intentionally keep their compact call
    # signatures; keep the rest of the lint gate strict for release builds.
    cargo clippy --locked --all-targets -- -D warnings -A clippy::too_many_arguments
    if ($LASTEXITCODE -ne 0) { throw 'cargo clippy falló' }
    cargo test --locked
    if ($LASTEXITCODE -ne 0) { throw 'cargo test falló' }
    }
    cargo build --locked --release
    if ($LASTEXITCODE -ne 0) { throw 'cargo build --release falló' }
    New-Item -ItemType Directory -Force -Path $DistDirectory | Out-Null
    $ExecutablePath = Join-Path $DistDirectory 'KGenView.exe'
    Copy-Item -LiteralPath (Join-Path $ProjectRoot 'target\release\kgen-view.exe') -Destination $ExecutablePath -Force
    Sign-ReleaseFile $ExecutablePath

    $InnoCandidates = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
    )
    $InnoCompiler = $InnoCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $InnoCompiler) {
        throw 'Inno Setup 6 no está instalado. Instálalo con: winget install --id JRSoftware.InnoSetup -e'
    }
    & $InnoCompiler "/DMyAppVersion=$PackageVersion" (Join-Path $ProjectRoot 'installer\KGenView.iss')
    if ($LASTEXITCODE -ne 0) { throw 'La compilación del instalador falló' }
    Sign-ReleaseFile (Join-Path $DistDirectory 'KGenView-Setup-x64.exe')
}
finally {
    Pop-Location
}
