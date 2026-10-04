; Macaw installer (Inno Setup). Built by .github/workflows/release.yml:
;   ISCC /DAppVersion=0.1.0 /DDist=<folder with x64\macaw.exe and arm64\macaw.exe> installer\macaw.iss
; One installer for both x64 and ARM64 PCs; it installs the matching program.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Dist
  #define Dist "..\dist"
#endif

[Setup]
; Never change AppId: Windows uses it to recognise upgrades and the uninstaller.
AppId={{6F3B1C2E-8A4D-4E57-9B21-3C5D7E9A1F40}
AppName=Macaw
AppVersion={#AppVersion}
AppVerName=Macaw {#AppVersion}
AppPublisher=Macaw contributors
AppPublisherURL=https://github.com/hornta/macaw
AppSupportURL=https://github.com/hornta/macaw/issues
AppUpdatesURL=https://github.com/hornta/macaw/releases
DefaultDirName={autopf}\Macaw
DisableProgramGroupPage=yes
OutputDir={#Dist}
OutputBaseFilename=Macaw-{#AppVersion}-setup
SetupIconFile=..\assets\macaw.ico
UninstallDisplayIcon={app}\macaw.exe
UninstallDisplayName=Macaw
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Program Files and the start-at-sign-in task (which gives Macaw admin rights) need admin rights.
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible or arm64
ArchitecturesInstallIn64BitMode=x64compatible or arm64
; Windows 10 1809 or later.
MinVersion=10.0.17763
; Macaw is stopped by [Code] below instead.
CloseApplications=no

[Tasks]
Name: "autostart"; Description: "Start Macaw when I sign in, with admin rights (so it also works in admin apps)"

[Files]
Source: "{#Dist}\x64\macaw.exe"; DestDir: "{app}"; Check: not IsArm64; Flags: ignoreversion
Source: "{#Dist}\arm64\macaw.exe"; DestDir: "{app}"; Check: IsArm64; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Macaw"; Filename: "{app}\macaw.exe"

[Run]
Filename: "{app}\macaw.exe"; Parameters: "--install-autostart"; Tasks: autostart; Flags: runhidden waituntilterminated; StatusMsg: "Setting up start at sign-in..."
; Started as the signed-in user; with start at sign-in set up, it hands over to the elevated task.
Filename: "{app}\macaw.exe"; Description: "Start Macaw now"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{app}\macaw.exe"; Parameters: "--uninstall-autostart"; Flags: runhidden waituntilterminated; RunOnceId: "RemoveAutostart"

[Code]
// Asks a running Macaw to quit (it releases any keys it holds), then insists.
procedure StopMacaw();
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM macaw.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Sleep(1500);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM macaw.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopMacaw();
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    StopMacaw();
end;
