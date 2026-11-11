param(
    [string]$Executable = (Join-Path $PSScriptRoot '..\target\release\kgen-view.exe'),
    [string]$FixtureDirectory = (Join-Path $PSScriptRoot '..\target\performance-fixtures'),
    [ValidateRange(1, 100)][int]$Iterations = 10,
    [switch]$Original,
    [string]$OutputPath = (Join-Path $PSScriptRoot '..\target\performance-results.json')
)
$ErrorActionPreference = 'Stop'
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$FixtureDirectory = (Resolve-Path -LiteralPath $FixtureDirectory).Path
$Images = Get-ChildItem -LiteralPath $FixtureDirectory -File |
    Where-Object { $_.Extension -in '.jpg', '.jpeg', '.png', '.gif', '.tif', '.tiff', '.svg', '.jxl', '.webp' }
if (-not $Images) { throw 'No hay imágenes de prueba en el directorio indicado.' }
$Results = foreach ($Image in $Images) {
    $Samples = for ($SampleIndex = 0; $SampleIndex -lt $Iterations; $SampleIndex++) {
        $StartInfo = [Diagnostics.ProcessStartInfo]::new()
        $StartInfo.FileName = $Executable
        $StartInfo.Arguments = '--benchmark-image "' + $Image.FullName + '"'
        if ($Original) { $StartInfo.Arguments += ' --original' }
        $StartInfo.UseShellExecute = $false
        $StartInfo.CreateNoWindow = $true
        $StartInfo.RedirectStandardOutput = $true
        $StartInfo.RedirectStandardError = $true
        $Process = [Diagnostics.Process]::new()
        $Process.StartInfo = $StartInfo
        try {
            [void]$Process.Start()
            $OutputRead = $Process.StandardOutput.ReadToEndAsync()
            $ErrorRead = $Process.StandardError.ReadToEndAsync()
            if (-not $Process.WaitForExit(60000)) { $Process.Kill(); throw 'La medición superó 60 segundos.' }
            $OutputText = $OutputRead.GetAwaiter().GetResult()
            $ErrorText = $ErrorRead.GetAwaiter().GetResult()
            if ($Process.ExitCode -ne 0) { throw "Error midiendo $($Image.Name): $ErrorText" }
            $OutputText | ConvertFrom-Json
        } finally { $Process.Dispose() }
    }
    $Times = @($Samples.elapsed_ms | Sort-Object)
    [pscustomobject]@{
        image = $Image.Name
        iterations = $Iterations
        quality = $(if ($Original) { 'original' } else { 'preview-1920x1080' })
        median_ms = $Times[[Math]::Floor($Times.Count / 2)]
        p95_ms = $Times[[Math]::Max(0, [Math]::Ceiling($Times.Count * 0.95) - 1)]
        peak_working_set_bytes = ($Samples.peak_working_set_bytes | Measure-Object -Maximum).Maximum
        managed_peak_bytes = ($Samples.managed_peak_bytes | Measure-Object -Maximum).Maximum
        samples = @($Samples)
    }
}
$Report = [pscustomobject]@{
    generated_utc = [DateTime]::UtcNow.ToString('o')
    executable = $Executable
    logical_processors = [Environment]::ProcessorCount
    note = 'Fresh process per sample; the OS file cache is not purged. Decoder measurements do not include the window or GPU.'
    results = @($Results)
}
$Report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $OutputPath -Encoding UTF8
$Results | Select-Object image, quality, median_ms, p95_ms, peak_working_set_bytes, managed_peak_bytes | Format-Table -AutoSize
Write-Output "Informe: $OutputPath"
