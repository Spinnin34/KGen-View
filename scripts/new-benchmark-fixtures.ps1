param(
    [string]$Directory = (Join-Path $PSScriptRoot '..\target\performance-fixtures'),
    [switch]$LargePng,
    [switch]$LargeFolders
)
$ErrorActionPreference = 'Stop'
$Directory = [IO.Path]::GetFullPath($Directory)
[void][IO.Directory]::CreateDirectory($Directory)
Add-Type -AssemblyName System.Drawing
if (-not ('KGenFixtureGenerator' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
public static class KGenFixtureGenerator {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern bool CreateHardLink(string name, string existing, IntPtr reserved);
    public static void Noise(IntPtr pointer, int stride, int width, int height) {
        var row=new byte[width*3];uint state=123456789;
        for (int y=0;y<height;y++) {
            for (int x=0;x<width;x++) {
                state=unchecked(state*1664525+1013904223);byte noise=(byte)(state>>24);int offset=x*3;
                row[offset]=(byte)((x*255/width+noise)/2);
                row[offset+1]=(byte)((y*255/height+noise)/2);row[offset+2]=noise;
            }
            Marshal.Copy(row,0,IntPtr.Add(pointer,y*stride),row.Length);
        }
    }
    public static void Folder(string directory, string image, int count) {
        Directory.CreateDirectory(directory);
        for (int index=0; index<count; index++) {
            string destination = Path.Combine(directory, "image"+index.ToString("D6")+".png");
            string seed=Path.Combine(directory,"_seed"+(index/1000).ToString("D3")+".dat");
            if (!File.Exists(seed)) File.Copy(image,seed);
            if (!File.Exists(destination) && !CreateHardLink(destination, seed, IntPtr.Zero))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        }
    }
}
'@
}
function New-FixtureImage {
    param([string]$Path,[int]$Width,[int]$Height,[bool]$Png)
    if (Test-Path -LiteralPath $Path) { return }
    $Image=[Drawing.Bitmap]::new($Width,$Height,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
    try {
        $Rectangle=[Drawing.Rectangle]::new(0,0,$Width,$Height)
        $Data=$Image.LockBits($Rectangle,[Drawing.Imaging.ImageLockMode]::WriteOnly,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
        try { [KGenFixtureGenerator]::Noise($Data.Scan0,$Data.Stride,$Width,$Height) }
        finally { $Image.UnlockBits($Data) }
        $Format=if ($Png) { [Drawing.Imaging.ImageFormat]::Png } else { [Drawing.Imaging.ImageFormat]::Jpeg }
        $Image.Save($Path,$Format)
    } finally { $Image.Dispose() }
}
foreach ($Size in @(@('2mp',1920,1080), @('24mp',6000,4000), @('60mp',10000,6000))) {
    New-FixtureImage (Join-Path $Directory "photo-$($Size[0]).jpg") $Size[1] $Size[2] $false
    if ($Size[0] -eq '2mp' -or $LargePng) {
        New-FixtureImage (Join-Path $Directory "photo-$($Size[0]).png") $Size[1] $Size[2] $true
    }
}
if ($LargeFolders) {
    $TinyImage = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\resources\kgen-view-logo.png'))
    foreach ($Count in @(10000,100000)) {
        [KGenFixtureGenerator]::Folder((Join-Path $Directory "folder-$Count"), $TinyImage, $Count)
    }
}
Write-Output "Imágenes reproducibles: $Directory"
Write-Output 'Los archivos existentes se conservan. Las carpetas grandes usan enlaces físicos agrupados en un volumen NTFS.'
