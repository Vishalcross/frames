; Script generated for Inno Setup 6
; Builds Windows standalone installer for Frames desktop application

#define MyAppName "Frames"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "Frames"
#define MyAppURL "https://github.com/vishal/frames"
#define MyAppExeName "Frames.exe"

[Setup]
; NOTE: The value of AppId uniquely identifies this application. Do not use the same AppId value in installers for other applications.
AppId={{D158E62C-09B6-4A9B-9F7C-821B27357A42}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DisableProgramGroupPage=yes
; PrivilegesRequired=lowest allows installation without admin rights if user installs for current user
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=..\target\release\bundle\inno
OutputBaseFilename=Frames_{#MyAppVersion}_x64-setup
SetupIconFile=..\frames-shell\src-tauri\icons\icon.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64
ArchitecturesAllowed=x64

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; Main application executable
Source: "..\target\release\frames-shell.exe"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion
; Bundled ffmpeg and ffprobe sidecars for Windows (both standard and target-triple names for sidecar compatibility)
Source: "..\frames-shell\src-tauri\binaries\ffmpeg-x86_64-pc-windows-msvc.exe"; DestDir: "{app}"; DestName: "ffmpeg.exe"; Flags: ignoreversion
Source: "..\frames-shell\src-tauri\binaries\ffmpeg-x86_64-pc-windows-msvc.exe"; DestDir: "{app}"; DestName: "ffmpeg-x86_64-pc-windows-msvc.exe"; Flags: ignoreversion
Source: "..\frames-shell\src-tauri\binaries\ffprobe-x86_64-pc-windows-msvc.exe"; DestDir: "{app}"; DestName: "ffprobe.exe"; Flags: ignoreversion
Source: "..\frames-shell\src-tauri\binaries\ffprobe-x86_64-pc-windows-msvc.exe"; DestDir: "{app}"; DestName: "ffprobe-x86_64-pc-windows-msvc.exe"; Flags: ignoreversion
; Application icon
Source: "..\frames-shell\src-tauri\icons\icon.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\icon.ico"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\icon.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
