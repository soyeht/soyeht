; The installer of the Soyeht Windows app (ADR 0123), compiled by `scripts/package_windows.py`.
;
; Every value that is not written here comes from the script through `ISCC /DName=value`, which
; reads them from `contracts/release.json` and `contracts/profiles.json`: AppName, AppId, InstallFolder,
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
AppName={#AppName}
AppVersion={#Version}
AppVerName={#AppName} {#Version}
AppPublisher={#Publisher}
AppPublisherURL={#Url}
AppSupportURL={#Url}
VersionInfoVersion={#VersionInfo}
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppName} installer
DefaultDirName={localappdata}\Programs\{#InstallFolder}
DisableDirPage=yes
DisableProgramGroupPage=yes
DefaultGroupName={#AppName}
PrivilegesRequired=lowest
ArchitecturesAllowed={#Allowed}
ArchitecturesInstallIn64BitMode={#Allowed}
OutputDir={#OutputDir}
OutputBaseFilename={#OutputName}
SetupIconFile={#Icon}
UninstallDisplayIcon={app}\soyeht-desktop.exe
UninstallDisplayName={#AppName}
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
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\soyeht-desktop.exe"

[Tasks]
; Registering the login tasks needs the person's consent (contract 13, section 3), so the box
; starts unchecked. A silent install opts in with /TASKS=loginstart.
Name: "loginstart"; Description: "Start {#AppName} when I log in"; Flags: unchecked

[Run]
Filename: "{app}\soyeht-desktop.exe"; Description: "Open {#AppName}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Only what this installer put in the folder: the programs moved aside by an upgrade.
Type: files; Name: "{app}\soyeht*.exe.*.old"
Type: dirifempty; Name: "{app}"

[Code]
const
  // The command line of the installed program, by its profile, so that a SOYEHT_PROFILE in the
  // person's environment cannot point it at another one.
  Profile = '{#Profile}';
  // How long a command of the installed program, or a probe of it, may run.
  CliSeconds = 120;
  ProbeSeconds = 15;

function SetEnvironmentVariable(Name, Value: String): Boolean;
  external 'SetEnvironmentVariableW@kernel32.dll stdcall';

var
  Originals, MovedAside: TStringList;
  InstallCompleted: Boolean;
  DeleteData: Boolean;

// ---- helpers ------------------------------------------------------------------------------

// Written to the log always, and shown only when a person is there to read it: a message box
// in a silent run would wait for ever.
procedure Tell(Text: String; Uninstalling: Boolean);
begin
  Log(Text);
  if Uninstalling then
  begin
    if not UninstallSilent() then
      MsgBox(Text, mbInformation, MB_OK);
  end
  else if not WizardSilent() then
    MsgBox(Text, mbInformation, MB_OK);
end;

function Cli(): String;
begin
  Result := ExpandConstant('{app}\soyeht.exe');
end;

function PowerShell(): String;
begin
  // The native one: Setup is a 32-bit program, and the 32-bit PowerShell sees a different view.
  Result := ExpandConstant('{sysnative}\WindowsPowerShell\v1.0\powershell.exe');
end;

// Runs a PowerShell script that reads its inputs from the environment (a path never goes into
// the script text, so a quote in a user name cannot break it). Returns the exit code, -1 when
// PowerShell did not start.
function RunScript(Script: String): Integer;
var
  Code: Integer;
begin
  if not Exec(PowerShell(), '-NoProfile -NonInteractive -Command "' + Script + '"', '', SW_HIDE,
    ewWaitUntilTerminated, Code) then
    Code := -1;
  Log('powershell exit code ' + IntToStr(Code));
  Result := Code;
end;

// Runs a program with a deadline and returns its exit code: 124 when the deadline passed (the
// program is ended), -1 when it did not start.
function RunWithin(Exe, Arguments: String; Seconds: Integer): Integer;
begin
  SetEnvironmentVariable('SOYEHT_RUN_EXE', Exe);
  SetEnvironmentVariable('SOYEHT_RUN_ARGUMENTS', Arguments);
  SetEnvironmentVariable('SOYEHT_RUN_MS', IntToStr(Seconds * 1000));
  Result := RunScript(
    '$p = Start-Process -FilePath $env:SOYEHT_RUN_EXE -ArgumentList $env:SOYEHT_RUN_ARGUMENTS ' +
    '-WindowStyle Hidden -PassThru; $null = $p.Handle; ' +
    'if (-not $p.WaitForExit([int]$env:SOYEHT_RUN_MS)) { Stop-Process -Id $p.Id -Force; exit 124 }; ' +
    'exit $p.ExitCode');
  Log('"' + Exe + '" ' + Arguments + ' -> ' + IntToStr(Result));
end;

// Runs the installed command line and returns its exit code.
function RunCli(Arguments: String): Integer;
begin
  Result := RunWithin(Cli(), '--profile ' + Profile + ' ' + Arguments, CliSeconds);
end;

// The first line a program prints for an argument, or '' when it printed none or failed.
function Probe(Exe, Argument: String): String;
var
  Output: String;
  Lines: TArrayOfString;
  Code: Integer;
begin
  Result := '';
  Output := ExpandConstant('{tmp}\probe.txt');
  DeleteFile(Output);
  if Exec(ExpandConstant('{cmd}'), '/c ""' + Exe + '" ' + Argument + ' > "' + Output + '""', '', SW_HIDE,
    ewWaitUntilTerminated, Code) and (Code = 0) then
    if LoadStringsFromFile(Output, Lines) and (GetArrayLength(Lines) > 0) then
      Result := Trim(Lines[0]);
end;

// ---- where the programs may go ------------------------------------------------------------

function ExpectedFolder(): String;
begin
  Result := ExpandConstant('{localappdata}\Programs\{#InstallFolder}');
end;

// The install folder is the one this installer is built for and no other: it moves, renames and
// deletes programs there, and with any other folder it would touch someone else's. A missing
// directory page does not stop /DIR= or a remembered folder, so the folder is checked twice: at
// the start for /DIR= (a silent run then ends with exit code 1 and no message box), and before the
// first file for the one Setup resolved, whatever way it came.
function FolderIsRefused(Folder: String): Boolean;
begin
  Result := CompareText(RemoveBackslash(Folder), ExpectedFolder()) <> 0;
  if Result then
    Log('{#AppName} installs only in ' + ExpectedFolder() + ', not in ' + Folder + '.');
end;

function InitializeSetup(): Boolean;
var
  Given: String;
begin
  Result := True;
  Given := ExpandConstant('{param:DIR|}');
  if (Given <> '') and FolderIsRefused(ExpandFileName(Given)) then
  begin
    if not WizardSilent() then
      MsgBox('{#AppName} installs only in ' + ExpectedFolder() + '.', mbError, MB_OK);
    Result := False;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if FolderIsRefused(ExpandConstant('{app}')) then
    Result := '{#AppName} installs only in ' + ExpectedFolder() + '.';
end;

// ---- upgrade ------------------------------------------------------------------------------

// Windows lets a running program's file be renamed but not overwritten, so each program of an
// earlier install (soyeht*.exe, nothing else in the folder) is renamed to NAME.exe.STAMP.old
// before the new ones are written. A running engine or supervisor keeps going from its renamed
// file, and so do the terminals the supervisor holds.
procedure MoveProgramsAside();
var
  Found: TFindRec;
  Stamp, Folder: String;
begin
  Originals := TStringList.Create;
  MovedAside := TStringList.Create;
  Stamp := GetDateTimeString('yyyymmddhhnnss', #0, #0);
  Folder := ExpandConstant('{app}\');
  if FindFirst(Folder + 'soyeht*.exe', Found) then
  begin
    try
      repeat
        if (Found.Attributes and FILE_ATTRIBUTE_DIRECTORY) = 0 then
          if RenameFile(Folder + Found.Name, Folder + Found.Name + '.' + Stamp + '.old') then
          begin
            Originals.Add(Folder + Found.Name);
            MovedAside.Add(Folder + Found.Name + '.' + Stamp + '.old');
          end
          else
            Log('could not move aside ' + Found.Name);
      until not FindNext(Found);
    finally
      FindClose(Found);
    end;
  end;
end;

// Puts the moved programs back when the install did not complete.
procedure RestoreMovedAside();
var
  Index: Integer;
begin
  for Index := 0 to MovedAside.Count - 1 do
  begin
    DeleteFile(Originals[Index]);
    if RenameFile(MovedAside[Index], Originals[Index]) then
      Log('restored ' + Originals[Index])
    else
      Log('could not restore ' + Originals[Index]);
  end;
end;

// The moved-aside copy of a program of this install, or '' when it was not there.
function MovedCopyOf(Name: String): String;
var
  Index: Integer;
begin
  Result := '';
  for Index := 0 to MovedAside.Count - 1 do
    if CompareText(Originals[Index], ExpandConstant('{app}\') + Name) = 0 then
      Result := MovedAside[Index];
end;

// Deletes every program moved aside, now and by earlier upgrades; one that still runs stays
// and is removed by the next install or the uninstall. Returns whether any stayed.
function RemoveMovedAside(): Boolean;
var
  Found: TFindRec;
  Folder: String;
begin
  Result := False;
  Folder := ExpandConstant('{app}\');
  if FindFirst(Folder + 'soyeht*.exe.*.old', Found) then
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

// Ends what runs from the install folder, only the programs this installer puts there (soyeht*)
// and only below the folder, by process identifier: first the engine, which would start a lost
// supervisor again (ADR 0108), then the rest, twice in case something restarted in between.
// `Everything` False ends the engine alone, which is safe for the terminals (contract 07).
procedure EndProgramsOfTheFolder(Everything: Boolean);
begin
  SetEnvironmentVariable('SOYEHT_END_FOLDER', ExpandConstant('{app}'));
  if Everything then
    SetEnvironmentVariable('SOYEHT_END_REST', '1')
  else
    SetEnvironmentVariable('SOYEHT_END_REST', '');
  RunScript(
    '$d = $env:SOYEHT_END_FOLDER.TrimEnd([char]92) + [char]92; ' +
    'function Live($pattern) { Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and ' +
    '$_.ExecutablePath.StartsWith($d, [StringComparison]::OrdinalIgnoreCase) -and $_.Name -like $pattern } }; ' +
    'foreach ($round in 1..2) { ' +
    'Live ''soyeht-engine*'' | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }; ' +
    'if ($env:SOYEHT_END_REST) { Start-Sleep -Milliseconds 500; ' +
    'Live ''soyeht*'' | ForEach-Object { Stop-Process -Id $_.ProcessId -Force } } }');
end;

function WindowRuns(): Boolean;
begin
  SetEnvironmentVariable('SOYEHT_END_FOLDER', ExpandConstant('{app}'));
  Result := RunScript(
    '$d = $env:SOYEHT_END_FOLDER.TrimEnd([char]92) + [char]92; ' +
    'if (Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and ' +
    '$_.ExecutablePath.StartsWith($d, [StringComparison]::OrdinalIgnoreCase) -and $_.Name -like ''soyeht-desktop*'' }) ' +
    '{ exit 1 } else { exit 0 }') = 1;
end;

// The protocol the moved-aside supervisor speaks, '' when there was none or it cannot say.
function OldSupervisorProtocol(): String;
var
  Old: String;
begin
  Result := '';
  Old := MovedCopyOf('soyeht-ptyd.exe');
  if Old <> '' then
  begin
    // Run a copy, under a name Windows will start: the moved file ends in .old.
    FileCopy(Old, ExpandConstant('{tmp}\ptyd-old.exe'), False);
    Result := Probe(ExpandConstant('{tmp}\ptyd-old.exe'), '--protocol-version');
  end;
end;

// Deletes the programs moved aside and says what the upgrade means for what was running.
//
// The terminals live in the supervisor, so its build matters. When the new supervisor speaks the
// same protocol as the one that runs, the old engine is ended (the terminals stay, and the next
// window or the login task starts the new engine); when it differs, or a window is still open,
// nothing is ended and the person is told what happens.
procedure SettleUpgrade();
var
  OldProtocol, NewProtocol, Supervisor: String;
begin
  Supervisor := MovedCopyOf('soyeht-ptyd.exe');
  OldProtocol := OldSupervisorProtocol();
  NewProtocol := Probe(ExpandConstant('{app}\soyeht-ptyd.exe'), '--protocol-version');
  Log('supervisor protocol: old "' + OldProtocol + '", new "' + NewProtocol + '"');
  if not RemoveMovedAside() then
    exit;
  // Something that was running still runs from a moved file.
  if (Supervisor <> '') and FileExists(Supervisor) and ((OldProtocol = '') or (OldProtocol <> NewProtocol)) then
    Tell('{#AppName} was updated, and the new terminal supervisor speaks a different protocol from the ' +
      'one that is running. The running engine and supervisor keep the previous build until they restart. ' +
      'To use the new build, quit {#AppName}, run "soyeht service stop" if you started it at login, and ' +
      'open it again. Restarting the supervisor ends your terminals.', False)
  else if WindowRuns() then
    Tell('{#AppName} was running while it was updated. The running programs keep the previous build, and ' +
      'your terminals keep running, until they restart. Close the window and open it again; the engine ' +
      'starts the new build once it is restarted.', False)
  else
  begin
    EndProgramsOfTheFolder(False);
    RemoveMovedAside();
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
    MoveProgramsAside()
  else if CurStep = ssPostInstall then
  begin
    InstallCompleted := True;
    SettleUpgrade();
    if WizardIsTaskSelected('loginstart') then
      if RunCli('service install --programs "' + ExpandConstant('{app}') + '"') <> 0 then
        Tell('{#AppName} is installed, but it could not register itself to start at login. ' +
          'You can try again with: soyeht service install', False);
  end;
end;

procedure DeinitializeSetup();
begin
  if (not InstallCompleted) and (MovedAside <> nil) then
    RestoreMovedAside();
end;

// ---- uninstall ----------------------------------------------------------------------------

function SwitchGiven(Name: String): Boolean;
var
  Index: Integer;
begin
  Result := False;
  for Index := 1 to ParamCount do
    if CompareText(ParamStr(Index), Name) = 0 then
      Result := True;
end;

// A silent removal keeps the data unless /DELETEDATA is given. Otherwise the person is asked once,
// after Inno's own "are you sure", with No as the default.
procedure AskAboutData();
begin
  DeleteData := SwitchGiven('/DELETEDATA');
  if not UninstallSilent() then
    DeleteData := SuppressibleMsgBox('{#AppName} will stop your terminals and agents.' + #13#10 + #13#10 +
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
  // Unregister the login tasks and stop them; this keeps the data.
  if FileExists(Cli()) then
  begin
    Code := RunCli('uninstall');
    if Code <> 0 then
      Tell('{#AppName} could not unregister its login tasks (exit code ' + IntToStr(Code) + '). ' +
        'Install {#AppName} again and remove it, or delete its tasks in Task Scheduler.', True);
  end;
  // What the window started is in no task, and a running program cannot be deleted.
  EndProgramsOfTheFolder(True);
  if DeleteData and FileExists(Cli()) then
  begin
    Code := RunCli('uninstall --delete-data --yes');
    if Code <> 0 then
      Tell('{#AppName} was removed, but its data could not be deleted. It is in the ' +
        'data folder of {#AppName} in your local application data.', True);
  end;
end;
