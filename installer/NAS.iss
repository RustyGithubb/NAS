; Installeur Windows de la console NAS, en français et en anglais (Inno Setup 7 ou 6 — https://jrsoftware.org/isinfo.php).
; Construit par installer\build.ps1, qui compile la console puis appelle ISCC avec la version de Cargo.toml.
; Repris de celui de WorkTable (identifiant AppId différent : les deux apps s'installent côte à côte).
;
; - Installation par utilisateur par défaut (pas besoin d'être administrateur) dans
;   %LOCALAPPDATA%\Programs\NAS ; « pour tous les utilisateurs » reste possible (Program Files).
; - Les réglages ne sont JAMAIS dans le dossier du programme : ils vont dans %APPDATA%\NAS ; la
;   clé d'accès de l'agent est dans le coffre de Windows. Ils survivent aux mises à jour ; à la
;   désinstallation, l'utilisateur choisit de garder les réglages ou non.
; - Le nom de l'exe (nas-console.exe) ne doit plus changer : la mise à jour automatique en dépend.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#define AppName "NAS"
#define AppExe "nas-console.exe"

[Setup]
AppId={{D996DDC0-EAA6-4B87-8966-D5E57D3147C9}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=RustyGithubb
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=Output
OutputBaseFilename=NAS-{#AppVersion}-setup
SetupIconFile=..\crates\console\assets\logo.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
; Langue de l'installeur : celle de Windows (français ou anglais), sans question.
ShowLanguageDialog=auto

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
french.DesktopIcon=Créer un raccourci sur le Bureau
english.DesktopIcon=Create a desktop shortcut
french.Shortcuts=Raccourcis :
english.Shortcuts=Shortcuts:
french.StartupIcon=Lancer NAS à l'ouverture de session
english.StartupIcon=Start NAS when I sign in
french.Startup=Démarrage :
english.Startup=Startup:
french.DeleteData=Supprimer aussi les réglages de NAS (thème, adresse de l'agent, onglets) ?
english.DeleteData=Also delete the NAS settings (theme, agent address, tabs)?

[Tasks]
Name: "desktopicon"; Description: "{cm:DesktopIcon}"; GroupDescription: "{cm:Shortcuts}"
Name: "startup"; Description: "{cm:StartupIcon}"; GroupDescription: "{cm:Startup}"; Flags: unchecked

[Files]
Source: "..\target\dist\nas-console.exe"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon
Name: "{userstartup}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: startup

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
// Désinstallation : proposer de supprimer aussi les réglages (non par défaut). Les fichiers
// téléchargés (Téléchargements\NAS) ne sont jamais touchés.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    if MsgBox(CustomMessage('DeleteData') + #13#10 +
              ExpandConstant('{userappdata}\NAS'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
    begin
      DelTree(ExpandConstant('{userappdata}\NAS'), True, True, True);
    end;
  end;
end;
