#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef SourceDir
  #error SourceDir is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif

[Setup]
AppId={{A1C4F2B8-6D31-4E52-9A0C-3E7F5B2D91AA}
AppName=PPA EQuart
AppVersion={#AppVersion}
AppPublisher=Phosphorus Audio
DefaultDirName={autopf}\PPA EQuart
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename=ppa-equart-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName=equart

[Files]
Source: "{#SourceDir}\equart.clap"; DestDir: "{commoncf64}\CLAP"; Flags: ignoreversion
Source: "{#SourceDir}\equart.vst3\*"; DestDir: "{commoncf64}\VST3\equart.vst3"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#SourceDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion

[UninstallDelete]
Type: filesandordirs; Name: "{commoncf64}\VST3\equart.vst3"
