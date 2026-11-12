#define MyAppName "KGen View"
#define MyAppExeName "KGenView.exe"

[Setup]
AppId={{E48C455C-0EB6-4D02-9FB8-4F09563CA3AD}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher=KGen View
LicenseFile=..\LICENSE
DefaultDirName={localappdata}\Programs\KGenView
DefaultGroupName=KGen View
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
CloseApplications=yes
RestartApplications=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=KGenView-Setup-x64
SetupIconFile=..\target\kgen-view.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
UninstallDisplayIcon={app}\{#MyAppExeName}
VersionInfoVersion={#MyAppVersion}.0
VersionInfoDescription=Instalador de KGen View
VersionInfoCompany=KGen View
VersionInfoCopyright=Copyright (C) 2026 KGen View contributors
VersionInfoProductName=KGen View
VersionInfoProductVersion={#MyAppVersion}.0
VersionInfoOriginalFileName=KGenView-Setup-x64.exe

[Languages]
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\dist\KGenView.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\filetype-icons\*.ico"; DestDir: "{app}\icons"; Flags: ignoreversion

[Icons]
Name: "{group}\KGen View"; Filename: "{app}\{#MyAppExeName}"; AppUserModelID: "KGen.View"
Name: "{autodesktop}\KGen View"; Filename: "{app}\{#MyAppExeName}"; AppUserModelID: "KGen.View"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\KGenView.exe"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "KGen View"; ValueData: "Software\KGenView\Capabilities"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\KGenView\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\KGenView\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "Visor de imágenes nativo, rápido y compacto"

Root: HKCU; Subkey: "Software\Classes\KGenView.bmp"; ValueType: string; ValueName: ""; ValueData: "Imagen BMP de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.bmp\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\bmp.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.bmp\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.jpg"; ValueType: string; ValueName: ""; ValueData: "Imagen JPEG de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.jpg\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\jpg.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.jpg\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.jpeg"; ValueType: string; ValueName: ""; ValueData: "Imagen JPEG de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.jpeg\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\jpeg.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.jpeg\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.png"; ValueType: string; ValueName: ""; ValueData: "Imagen PNG de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.png\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\png.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.png\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.gif"; ValueType: string; ValueName: ""; ValueData: "Imagen GIF de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.gif\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\gif.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.gif\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.webp"; ValueType: string; ValueName: ""; ValueData: "Imagen WebP de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.webp\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\webp.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.webp\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.tiff"; ValueType: string; ValueName: ""; ValueData: "Imagen TIFF de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.tiff\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\tiff.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.tiff\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.ico"; ValueType: string; ValueName: ""; ValueData: "Icono de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.ico\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\ico.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.ico\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.heic"; ValueType: string; ValueName: ""; ValueData: "Imagen HEIC de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.heic\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\heic.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.heic\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.jxl"; ValueType: string; ValueName: ""; ValueData: "Imagen JPEG XL de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.jxl\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"
Root: HKCU; Subkey: "Software\Classes\KGenView.jxl\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.svg"; ValueType: string; ValueName: ""; ValueData: "Imagen Vectorial SVG de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.svg\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\svg.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.svg\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.Archive"; ValueType: string; ValueName: ""; ValueData: "Archivo ZIP de imágenes"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.Archive\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"
Root: HKCU; Subkey: "Software\Classes\KGenView.Archive\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""

Root: HKCU; Subkey: "Software\Classes\Applications\KGenView.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Applications\KGenView.exe\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"
Root: HKCU; Subkey: "Software\Classes\Applications\KGenView.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\Applications\KGenView.exe\SupportedTypes"; ValueType: string; ValueName: ".zip"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\Applications\KGenView.exe\SupportedTypes"; ValueType: string; ValueName: ".cbz"; ValueData: ""; Flags: uninsdeletevalue

Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".bmp"; ValueData: "KGenView.bmp"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".dib"; ValueData: "KGenView.bmp"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jpg"; ValueData: "KGenView.jpg"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jpeg"; ValueData: "KGenView.jpeg"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jpe"; ValueData: "KGenView.jpg"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".png"; ValueData: "KGenView.png"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".gif"; ValueData: "KGenView.gif"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".tif"; ValueData: "KGenView.tiff"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".tiff"; ValueData: "KGenView.tiff"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".ico"; ValueData: "KGenView.ico"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".webp"; ValueData: "KGenView.webp"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".heic"; ValueData: "KGenView.heic"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".heif"; ValueData: "KGenView.heic"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".jxl"; ValueData: "KGenView.jxl"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".svg"; ValueData: "KGenView.svg"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".zip"; ValueData: "KGenView.Archive"
Root: HKCU; Subkey: "Software\KGenView\Capabilities\FileAssociations"; ValueType: string; ValueName: ".cbz"; ValueData: "KGenView.Archive"

Root: HKCU; Subkey: "Software\Classes\.bmp\OpenWithProgids"; ValueType: none; ValueName: "KGenView.bmp"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.dib\OpenWithProgids"; ValueType: none; ValueName: "KGenView.bmp"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.jpg\OpenWithProgids"; ValueType: none; ValueName: "KGenView.jpg"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.jpeg\OpenWithProgids"; ValueType: none; ValueName: "KGenView.jpeg"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.jpe\OpenWithProgids"; ValueType: none; ValueName: "KGenView.jpg"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.png\OpenWithProgids"; ValueType: none; ValueName: "KGenView.png"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.gif\OpenWithProgids"; ValueType: none; ValueName: "KGenView.gif"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.tif\OpenWithProgids"; ValueType: none; ValueName: "KGenView.tiff"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.tiff\OpenWithProgids"; ValueType: none; ValueName: "KGenView.tiff"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.ico\OpenWithProgids"; ValueType: none; ValueName: "KGenView.ico"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.webp\OpenWithProgids"; ValueType: none; ValueName: "KGenView.webp"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.heic\OpenWithProgids"; ValueType: none; ValueName: "KGenView.heic"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.heif\OpenWithProgids"; ValueType: none; ValueName: "KGenView.heic"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.jxl\OpenWithProgids"; ValueType: none; ValueName: "KGenView.jxl"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.svg\OpenWithProgids"; ValueType: none; ValueName: "KGenView.svg"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.zip\OpenWithProgids"; ValueType: none; ValueName: "KGenView.Archive"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.cbz\OpenWithProgids"; ValueType: none; ValueName: "KGenView.Archive"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,KGen View}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\{#MyAppExeName}"; Parameters: "--capture-stop"; Flags: runhidden waituntilterminated; RunOnceId: "StopCaptureHost"

[Code]
procedure CurStepChanged(CurStep: TSetupStep);
var
  Extensions: array[0..13] of String;
  I: Integer;
begin
  if CurStep = ssPostInstall then
  begin
    Extensions[0] := '.bmp'; Extensions[1] := '.dib'; Extensions[2] := '.jpg';
    Extensions[3] := '.jpeg'; Extensions[4] := '.jpe'; Extensions[5] := '.png';
    Extensions[6] := '.gif'; Extensions[7] := '.tif'; Extensions[8] := '.tiff';
    Extensions[9] := '.ico'; Extensions[10] := '.webp'; Extensions[11] := '.heic';
    Extensions[12] := '.heif'; Extensions[13] := '.jxl';
    for I := 0 to 13 do
      RegDeleteValue(HKCU, 'Software\Classes\' + Extensions[I] + '\OpenWithProgids', 'KGenView.Image');
    RegDeleteValue(HKCU, 'Software\Classes\.jpeg\OpenWithProgids', 'KGenView.jpg');
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Classes\KGenView.Image');
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'KGenViewCapture');
end;
