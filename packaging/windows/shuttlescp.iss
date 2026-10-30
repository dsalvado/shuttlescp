; Inno Setup script for the ShuttleSCP installer:
; target\windows\ShuttleSCP-<version>-windows-x64-setup.exe
;
; It installs the folder build-bundle.sh makes, for the current user only (in
; %LOCALAPPDATA%\Programs\ShuttleSCP, no administrator rights needed), with a Start menu
; entry and an uninstaller (Settings > Apps). The app's settings, logs and saved
; passwords are the user's and stay when it is uninstalled, as WinSCP's do.
;
; The app updates itself by downloading the next installer and running it with /SILENT
; once it has closed (see update_dialog.rs and app.rs): the installer replaces the files,
; then the app starts its new version.
;
; Built by build-installer.sh, which passes the version and folders:
;   ISCC /DAppVersion=0.11.0 /DBundle=<folder> /DOutputDir=<folder> shuttlescp.iss

#ifndef AppVersion
  #error Pass the version: /DAppVersion=x.y.z
#endif
#ifndef Bundle
  #error Pass the folder build-bundle.sh made: /DBundle=<folder>
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
; Identifies ShuttleSCP to Windows across versions: never change it.
AppId={{0D767BC0-39E9-4695-99F8-34417E237D4F}
AppName=ShuttleSCP
AppVersion={#AppVersion}
AppVerName=ShuttleSCP {#AppVersion}
AppPublisher=David Salvador
AppPublisherURL=https://shuttlescp.dsalvado.ca/
AppSupportURL=https://github.com/dsalvado/shuttlescp/issues
AppUpdatesURL=https://github.com/dsalvado/shuttlescp/releases
VersionInfoVersion={#AppVersion}
; For this user only: no administrator rights, and updates run without asking for them.
PrivilegesRequired=lowest
DefaultDirName={autopf}\ShuttleSCP
DisableProgramGroupPage=yes
DisableDirPage=auto
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
SetupIconFile=shuttlescp.ico
; The privacy policy, shown before installing, since the update check contacts GitHub
; (code-signing programs ask for this too). Plain text, so it is kept to ASCII. A silent
; install (the app's own update) skips the page.
InfoBeforeFile=..\..\PRIVACY.md
UninstallDisplayIcon={app}\bin\shuttlescp.exe
UninstallDisplayName=ShuttleSCP
; Close a running ShuttleSCP before replacing its files.
CloseApplications=yes
RestartApplications=no
Compression=lzma2/ultra64
SolidCompression=yes
OutputDir={#OutputDir}
OutputBaseFilename=ShuttleSCP-{#AppVersion}-windows-x64-setup

[Tasks]
Name: desktopicon; Description: "Create a &desktop shortcut"; Flags: unchecked

[InstallDelete]
; An update replaces the whole program, so no file of an older version stays behind.
Type: filesandordirs; Name: "{app}\bin"
Type: filesandordirs; Name: "{app}\lib"
Type: filesandordirs; Name: "{app}\share"

[Files]
Source: "{#Bundle}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\ShuttleSCP"; Filename: "{app}\bin\shuttlescp.exe"
Name: "{autodesktop}\ShuttleSCP"; Filename: "{app}\bin\shuttlescp.exe"; Tasks: desktopicon

[Run]
; Offered at the end of an install. A silent install (the app's own update) skips it: the
; app starts its new version itself once the installer is done.
Filename: "{app}\bin\shuttlescp.exe"; Description: "Start ShuttleSCP"; Flags: nowait postinstall skipifsilent
