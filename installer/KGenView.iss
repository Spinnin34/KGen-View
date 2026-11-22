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
Source: "{#KGenExecutable}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\filetype-icons\*.ico"; DestDir: "{app}\icons"; Flags: ignoreversion
Source: "{#VideoRuntime}\*"; DestDir: "{app}\vlc"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{group}\KGen View"; Filename: "{app}\{#MyAppExeName}"; AppUserModelID: "KGen.View"
Name: "{autodesktop}\KGen View"; Filename: "{app}\{#MyAppExeName}"; AppUserModelID: "KGen.View"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\KGenView.exe"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "KGen View"; ValueData: "Software\KGenView\Capabilities"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\KGenView\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\KGenView\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "Visor de imágenes y vídeo para Windows"

#include "..\target\filetype-icons\associations.iss"

; Keep defaults selected with previous versions working after an update.
Root: HKCU; Subkey: "Software\Classes\KGenView.Archive"; ValueType: string; ValueName: ""; ValueData: "Archivo ZIP de imágenes"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.Archive\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\zip.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.Archive\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\KGenView.Video"; ValueType: string; ValueName: ""; ValueData: "Vídeo de KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KGenView.Video\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\video.ico"
Root: HKCU; Subkey: "Software\Classes\KGenView.Video\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""

Root: HKCU; Subkey: "Software\Classes\Applications\KGenView.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "KGen View"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Applications\KGenView.exe\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"
Root: HKCU; Subkey: "Software\Classes\Applications\KGenView.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\Applications\KGenView.exe\SupportedTypes"; ValueType: string; ValueName: ".zip"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\Applications\KGenView.exe\SupportedTypes"; ValueType: string; ValueName: ".cbz"; ValueData: ""; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,KGen View}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\{#MyAppExeName}"; Parameters: "--capture-stop"; Flags: runhidden waituntilterminated; RunOnceId: "StopCaptureHost"

[Code]
var
  ReopenViewer: Boolean;
  ReopenParameters: String;

function OpenViewerProcess(Access: Cardinal; Inherit: Boolean; ProcessId: Cardinal): THandle;
  external 'OpenProcess@kernel32.dll stdcall setuponly';
function WaitForViewer(Process: THandle; Milliseconds: Cardinal): Cardinal;
  external 'WaitForSingleObject@kernel32.dll stdcall setuponly';
function CloseViewerProcessHandle(Process: THandle): Boolean;
  external 'CloseHandle@kernel32.dll stdcall setuponly';

function UpdateParameter(Prefix: String): String;
var
  I: Integer;
  Arg: String;
begin
  Result := '';
  for I := 1 to ParamCount do
  begin
    Arg := ParamStr(I);
    if Copy(Arg, 1, Length(Prefix)) = Prefix then
    begin
      Result := Copy(Arg, Length(Prefix) + 1, Length(Arg));
      Exit;
    end;
  end;
end;

function DecodeReopenParameters(Encoded: String): Boolean;
var
  I, ByteValue: Integer;
  Bytes: AnsiString;
begin
  Result := False;
  if (Length(Encoded) > 28000) or ((Length(Encoded) mod 2) <> 0) then Exit;
  SetLength(Bytes, Length(Encoded) div 2);
  for I := 1 to Length(Bytes) do
  begin
    ByteValue := StrToIntDef('$' + Copy(Encoded, I * 2 - 1, 2), -1);
    if (ByteValue < 1) or (ByteValue > 255) then Exit;
    Bytes[I] := Chr(ByteValue);
  end;
  ReopenParameters := Utf8Decode(Bytes);
  Result := True;
end;

function InitializeSetup: Boolean;
var
  ParentId: Integer;
  Process: THandle;
begin
  Result := True;
  ParentId := StrToIntDef(UpdateParameter('/KGENPARENTPID='), 0);
  if ParentId <= 0 then Exit;
  ReopenViewer := DecodeReopenParameters(UpdateParameter('/KGENREOPEN='));
  if not ReopenViewer then
  begin
    Result := False;
    Exit;
  end;

  Process := OpenViewerProcess($00100000, False, ParentId);
  if Process = 0 then
    Result := DLLGetLastError = 87
  else
  begin
    try
      Result := WaitForViewer(Process, 30000) = 0;
    finally
      CloseViewerProcessHandle(Process);
    end;
  end;
  if not Result then
  begin
    if ActiveLanguage = 'spanish' then
      MsgBox('Cierra KGen View y vuelve a iniciar la actualización.', mbError, MB_OK)
    else
      MsgBox('Close KGen View and start the update again.', mbError, MB_OK);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Extensions: array[0..13] of String;
  I, ExitCode: Integer;
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
  if (CurStep = ssDone) and ReopenViewer then
    if not Exec(ExpandConstant('{app}\{#MyAppExeName}'), ReopenParameters,
      ExpandConstant('{app}'), SW_SHOWNORMAL, ewNoWait, ExitCode) then
      Log('Could not reopen KGen View. Win32 error: ' + IntToStr(ExitCode));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'KGenViewCapture');
end;
