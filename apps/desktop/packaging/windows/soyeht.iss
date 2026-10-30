; The installer of the Soyeht Windows app (ADR 0123), compiled by `scripts/package_windows.py`.
;
; Every value that is not written here comes from the script through `ISCC /DName=value`, which
; reads them from `contracts/release.json` and `contracts/profiles.json`: AppId, InstallFolder,
; Version, VersionInfo, Allowed, Stage, OutputDir, OutputName, Profile, Publisher, Url, Icon and,
; for a signed build, Signed.
;
; A per-user install: no administrator, nothing outside the person's own account. The programs
; go to %LOCALAPPDATA%\Programs\<InstallFolder>, the shortcut to the person's Start menu, the
; entry of Add or remove programs to HKCU. The data directory of the profile is outside the install
; folder and is never touched by an install or an upgrade.

#ifndef Version
  #error Run this script through scripts/package_windows.py: it defines the values below.
#endif

[Setup]
AppId={#AppId}
AppName=Soyeht
AppVersion={#Version}
AppVerName=Soyeht {#Version}
AppPublisher={#Publisher}
AppPublisherURL={#Url}
AppSupportURL={#Url}
VersionInfoVersion={#VersionInfo}
VersionInfoProductName=Soyeht
VersionInfoDescription=Soyeht installer
DefaultDirName={localappdata}\Programs\{#InstallFolder}
DisableDirPage=yes
DisableProgramGroupPage=yes
DefaultGroupName=Soyeht
PrivilegesRequired=lowest
ArchitecturesAllowed={#Allowed}
ArchitecturesInstallIn64BitMode={#Allowed}
OutputDir={#OutputDir}
OutputBaseFilename={#OutputName}
SetupIconFile={#Icon}
UninstallDisplayIcon={app}\soyeht-desktop.exe
UninstallDisplayName=Soyeht
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Only the window is closed for an upgrade. The engine and the supervisor are never closed by
; the installer: the supervisor holds the person's terminals (contract 07). Their programs are
; moved aside below, so replacing them does not need them to stop.
CloseApplications=yes
CloseApplicationsFilter=soyeht-desktop.exe
RestartApplications=no
#ifdef Signed
SignTool=soyehtsign
SignedUninstaller=yes
#endif

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Soyeht"; Filename: "{app}\soyeht-desktop.exe"

[Tasks]
; Registering the login tasks needs the person's consent (contract 13, section 3), so the box
; starts unchecked. A silent install opts in with /TASKS=loginstart.
Name: "loginstart"; Description: "Start Soyeht when I log in"; Flags: unchecked

[Run]
Filename: "{app}\soyeht-desktop.exe"; Description: "Open Soyeht"; Flags: nowait postinstall skipifsilent

[InstallDelete]
Type: files; Name: "{app}\*.old"

[UninstallDelete]
Type: files; Name: "{app}\*.old"
Type: dirifempty; Name: "{app}"

[Code]
const
  // The command line of the installed program, by its profile, so that a SOYEHT_PROFILE in the
  // person's environment cannot point it at another one.
  Profile = '{#Profile}';

var
  KeptRunning: Boolean;

function Cli(): String;
begin
  Result := ExpandConstant('{app}\soyeht.exe');
end;

// Runs the installed command line and returns its exit code, or -1 when it did not start.
function RunCli(Arguments: String): Integer;
var
  Code: Integer;
begin
  if not Exec(Cli(), '--profile ' + Profile + ' ' + Arguments, '', SW_HIDE, ewWaitUntilTerminated, Code) then
    Code := -1;
  Result := Code;
end;

// Windows lets a running program's file be renamed but not overwritten, so the programs of an
// earlier install are renamed to NAME.exe.TICK.old before the new ones are written. A running
// engine or supervisor keeps going from its renamed file, and so do the terminals the
// supervisor holds.
procedure MoveProgramsAside();
var
  Found: TFindRec;
  Stamp, Folder: String;
begin
  Stamp := GetDateTimeString('yyyymmddhhnnss', #0, #0);
  Folder := ExpandConstant('{app}\');
  if FindFirst(Folder + '*.exe', Found) then
  begin
    try
      repeat
        if (Found.Attributes and FILE_ATTRIBUTE_DIRECTORY) = 0 then
          if Pos('unins', Found.Name) <> 1 then
            RenameFile(Folder + Found.Name, Folder + Found.Name + '.' + Stamp + '.old');
      until not FindNext(Found);
    finally
      FindClose(Found);
    end;
  end;
end;

// Deletes the files moved aside; one that is still running stays and is removed by the next
// install or by the uninstall. Returns whether any stayed.
function RemoveMovedAside(): Boolean;
var
  Found: TFindRec;
  Folder: String;
begin
  Result := False;
  Folder := ExpandConstant('{app}\');
  if FindFirst(Folder + '*.old', Found) then
  begin
    try
      repeat
        if not DeleteFile(Folder + Found.Name) then
          Result := True;
      until not FindNext(Found);
    finally
      FindClose(Found);
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
    MoveProgramsAside()
  else if CurStep = ssPostInstall then
  begin
    KeptRunning := RemoveMovedAside();
    if WizardIsTaskSelected('loginstart') then
      if RunCli('service install --programs "' + ExpandConstant('{app}') + '"') <> 0 then
        SuppressibleMsgBox('Soyeht is installed, but it could not register itself to start at login. ' +
          'You can try again with: soyeht service install', mbError, MB_OK, IDOK);
    if KeptRunning then
      SuppressibleMsgBox('Soyeht was running while it was updated. The programs that were running keep ' +
        'the previous build, and your terminals keep running, until they restart. Close and open the ' +
        'window to use the new build; the engine and the supervisor start it the next time they start.',
        mbInformation, MB_OK, IDOK);
  end;
end;

// ---- uninstall ----------------------------------------------------------------------------

var
  DeleteData: Boolean;

function SwitchGiven(Name: String): Boolean;
var
  Index: Integer;
begin
  Result := False;
  for Index := 1 to ParamCount do
    if CompareText(ParamStr(Index), Name) = 0 then
      Result := True;
end;

// Ends what still runs from the install folder after the tasks are gone: an engine and a
// supervisor that the window started are in no task, and a running program cannot be deleted.
// Only a process whose executable is below the folder is ended, by its identifier.
procedure EndProgramsOfTheFolder();
var
  Code: Integer;
  Script: String;
begin
  Script := '$d = ''' + ExpandConstant('{app}') + '\''; ' +
    'Get-Process | Where-Object { $_.Path -and $_.Path.StartsWith($d, [StringComparison]::OrdinalIgnoreCase) } | ' +
    'ForEach-Object { Stop-Process -Id $_.Id -Force }';
  Exec('powershell.exe', '-NoProfile -NonInteractive -Command "' + Script + '"', '', SW_HIDE,
    ewWaitUntilTerminated, Code);
end;

// A silent removal keeps the data unless /DELETEDATA is given. Otherwise the person is asked once,
// after Inno's own "are you sure", with No as the default.
procedure AskAboutData();
begin
  DeleteData := SwitchGiven('/DELETEDATA');
  if not UninstallSilent() then
    DeleteData := SuppressibleMsgBox('Soyeht will stop your terminals and agents.' + #13#10 + #13#10 +
      'Also delete your data (workspaces, transcripts, settings and logs)? Choose No to keep it on this computer.',
      mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Code: Integer;
begin
  if CurUninstallStep <> usUninstall then
    exit;
  AskAboutData();
  if not FileExists(Cli()) then
    exit;
  // Unregister the login tasks and stop them; this keeps the data.
  Code := RunCli('uninstall');
  if Code <> 0 then
    SuppressibleMsgBox('Soyeht could not unregister its login tasks (exit code ' + IntToStr(Code) +
      '). Install Soyeht again and remove it, or delete its tasks in Task Scheduler.',
      mbError, MB_OK, IDOK);
  EndProgramsOfTheFolder();
  if DeleteData then
  begin
    Code := RunCli('uninstall --delete-data --yes');
    if Code <> 0 then
      SuppressibleMsgBox('Soyeht was removed, but its data could not be deleted. It is in the ' +
        'data folder of Soyeht in your local application data.', mbError, MB_OK, IDOK);
  end;
end;
