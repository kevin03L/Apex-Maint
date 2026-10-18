; =============================================================================
; Installateur Windows d'Apex Maint (Inno Setup 6.3 ou plus récent)
;
; Construction : packaging\windows\build-installer.ps1, qui compile les binaires en
; release puis appelle ISCC avec la version du workspace :
;   ISCC /DAppVersion=0.4.0-alpha.1 /DNumericVersion=0.4.0.1 /DSourceDir=<dépôt> apex-maint.iss
;
; Ce que fait l'installation :
;   - binaires et IHM web dans Program Files (remplacés à chaque mise à jour) ;
;   - service Windows « ApexMaint » (compte LocalService, démarrage automatique),
;     créé par « apex-maint service install », qui prépare aussi les répertoires de
;     %ProgramData%\Apex Maint et leurs permissions ;
;   - première installation : assistant de configuration à la fin ;
;   - mise à jour : service arrêté avant la copie, configuration mise au format courant
;     (valeurs conservées, sauvegarde), service redémarré s'il tournait.
; La configuration, les données et les journaux (%ProgramData%) ne sont jamais écrasés.
; La désinstallation les conserve, sauf réponse explicite à la question posée.
; =============================================================================

#ifndef AppVersion
  #error AppVersion non défini : construisez avec packaging\windows\build-installer.ps1
#endif
#ifndef NumericVersion
  #define NumericVersion "0.0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\.."
#endif
#ifndef BinDir
  #define BinDir SourceDir + "\target\release"
#endif
#ifndef OutputDir
  #define OutputDir SourceDir + "\dist"
#endif
#ifndef SignedUninstallerDir
  #define SignedUninstallerDir OutputDir
#endif

#define AppName "Apex Maint"
#define ServiceName "ApexMaint"
#define EngineExe "apex-maint.exe"
#define AdminExe "apex-maint-admin.exe"
#define ConfigFile "{commonappdata}\Apex Maint\config\apex-maint.toml"

[Setup]
; Identifiant stable : NE JAMAIS LE MODIFIER (il relie les mises à jour à l'installation existante)
AppId={{933C5782-ED1B-40BE-8358-A63557B047AA}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=kevin03L
AppPublisherURL=https://github.com/kevin03L/Apex-Maint
AppSupportURL=https://github.com/kevin03L/Apex-Maint/issues
VersionInfoVersion={#NumericVersion}
VersionInfoProductVersion={#NumericVersion}
VersionInfoProductTextVersion={#AppVersion}
VersionInfoDescription=Installateur {#AppName} {#AppVersion}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=apex-maint-{#AppVersion}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
LicenseFile={#SourceDir}\LICENSE
; Icône du produit (packaging\icons\generer-icones.ps1) ; les raccourcis et l'entrée
; « Applications installées » reprennent celle embarquée dans les exécutables.
SetupIconFile={#SourceDir}\packaging\icons\apex-maint.ico
UninstallDisplayIcon={app}\{#AdminExe}
UninstallDisplayName={#AppName}
CloseApplications=yes
RestartApplications=no
SetupLogging=yes
; Signature Authenticode (Smart App Control, Windows 11) :
; outil « apexsign » passé sur la ligne de commande ISCC via /Sapexsign=...
#ifdef Signer
SignTool=apexsign
SignedUninstaller=yes
SignedUninstallerDir={#SignedUninstallerDir}
#else
#ifdef SignedUninstaller
SignedUninstaller=yes
SignedUninstallerDir={#SignedUninstallerDir}
#endif
#endif

[Languages]
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BinDir}\{#EngineExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\{#AdminExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\static\*"; DestDir: "{app}\web"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#SourceDir}\docs\INSTALLATION.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "{#SourceDir}\docs\ADMINISTRATION.md"; DestDir: "{app}\docs"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName} — Administration"; Filename: "{app}\{#AdminExe}"
Name: "{group}\Assistant de configuration"; Filename: "{app}\{#AdminExe}"; Parameters: "--wizard"
Name: "{group}\Documentation"; Filename: "{app}\docs"
Name: "{autodesktop}\{#AppName} — Administration"; Filename: "{app}\{#AdminExe}"; Tasks: desktopicon

[InstallDelete]
; Mise à jour : l'IHM web, les exemples et la documentation sont remplacés en entier (aucun
; fichier retiré par la nouvelle version ne reste servi)
Type: filesandordirs; Name: "{app}\web"
Type: filesandordirs; Name: "{app}\exemples"
Type: filesandordirs; Name: "{app}\docs"

[Run]
; Première installation : assistant de configuration (droits administrateur conservés pour
; écrire la configuration et régler le service)
Filename: "{app}\{#AdminExe}"; Parameters: "--wizard"; Description: "Lancer l'assistant de configuration"; Flags: postinstall nowait skipifsilent runascurrentuser; Check: not ConfigExists
Filename: "{app}\{#AdminExe}"; Description: "Ouvrir l'utilitaire d'administration"; Flags: postinstall nowait skipifsilent unchecked runasoriginaluser; Check: ConfigExists

[UninstallRun]
; Arrêt propre et suppression du service (configuration, données et journaux conservés)
Filename: "{app}\{#EngineExe}"; Parameters: "service uninstall"; Flags: runhidden waituntilterminated; RunOnceId: "SupprimerService"

[Code]
var
  WasRunning: Boolean;
  ServiceExistedBefore: Boolean;
  InstallDone: Boolean;
  PortPage: TInputQueryWizardPage;
  NetworkPage: TInputOptionWizardPage;
  ServicePage: TInputOptionWizardPage;

procedure InitializeWizard();
begin
  PortPage := CreateInputQueryPage(wpSelectDir, 'Port web', 'Port de l''interface et de l''API',
    'Les réglages existants sont conservés lors d''une mise à jour.');
  PortPage.Add('Port TCP (1 à 65535) :', False);
  PortPage.Values[0] := '3100';
  NetworkPage := CreateInputOptionPage(PortPage.ID, 'Accès réseau', 'Adresse d''écoute',
    'Pour un accès réseau, configurez HTTPS ou un proxy inverse de confiance.', True, False);
  NetworkPage.Add('Ce poste uniquement (127.0.0.1)');
  NetworkPage.Add('Réseau (0.0.0.0)');
  NetworkPage.SelectedValueIndex := 0;
  ServicePage := CreateInputOptionPage(NetworkPage.ID, 'Service Windows', 'Démarrage du moteur',
    'Le service utilise LocalService. Un partage UNC exige un compte dédié, à configurer dans le terminal.', False, False);
  ServicePage.Add('Installer le service Windows');
  ServicePage.Values[0] := True;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var Port: Integer;
begin
  Result := True;
  if CurPageID = PortPage.ID then
  begin
    Port := StrToIntDef(PortPage.Values[0], 0);
    Result := (Port > 0) and (Port <= 65535);
    if not Result then MsgBox('Indiquez un port entre 1 et 65535.', mbError, MB_OK);
  end;
end;

function ServiceExists: Boolean;
begin
  Result := RegKeyExists(HKLM, 'SYSTEM\CurrentControlSet\Services\{#ServiceName}');
end;

function ConfigExists: Boolean;
begin
  Result := FileExists(ExpandConstant('{#ConfigFile}'));
end;

function ServiceWasRunning: Boolean;
begin
  Result := WasRunning;
end;

function ServiceExisted: Boolean;
begin
  Result := ServiceExistedBefore;
end;

procedure RunRequired(AppExe, Args: String);
var Code: Integer;
begin
  if not Exec(AppExe, Args, '', SW_HIDE, ewWaitUntilTerminated, Code) then
    RaiseException('Impossible de lancer : ' + Args);
  Log(Format('%s : code %d', [Args, Code]));
  if Code <> 0 then
    RaiseException('Échec de ' + Args + ' (code ' + IntToStr(Code) + ')');
end;

procedure CurStepChanged(CurStep: TSetupStep);
var AppExe, Bind: String;
begin
  if CurStep = ssPostInstall then
  begin
    AppExe := ExpandConstant('{app}\{#EngineExe}');
    if not ConfigExists then
    begin
      Bind := '127.0.0.1';
      if NetworkPage.SelectedValueIndex = 1 then Bind := '0.0.0.0';
      RunRequired(AppExe, ExpandConstant('config init --config "{#ConfigFile}" --port ') +
        PortPage.Values[0] + ' --bind ' + Bind);
    end;
    { Un service existant conserve son compte et ses autorisations spécifiques. }
    if ServicePage.Values[0] and (not ServiceExistedBefore) then
      RunRequired(AppExe, 'service install');
    if ConfigExists then
      RunRequired(AppExe, ExpandConstant('config migrate --config "{#ConfigFile}"'));
    if ServiceWasRunning or (ServicePage.Values[0] and (not ServiceExisted) and ConfigExists) then
      RunRequired(AppExe, 'service start');
    InstallDone := True;
  end;
end;

{ Mise à jour annulée ou en échec après l'arrêt du service : la supervision est relancée
  avec les programmes restés en place. }
procedure DeinitializeSetup();
var
  Code: Integer;
begin
  if WasRunning and (not InstallDone) then
  begin
    Log('Installation non terminée : redémarrage du service {#ServiceName}');
    Exec(ExpandConstant('{sys}\net.exe'), 'start {#ServiceName}', '', SW_HIDE, ewWaitUntilTerminated, Code);
  end;
end;

{ Mise à jour : le service est arrêté proprement avant le remplacement des binaires.
  « net stop » attend la fin de l'arrêt et renvoie 0 seulement si le service tournait.
  Les éventuelles instances de l'utilitaire desktop sont également fermées pour que son
  exécutable puisse être remplacé sans blocage ni annulation silencieuse. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  WasRunning := False;
  InstallDone := False;
  ServiceExistedBefore := ServiceExists;
  Result := '';
  if ServiceExists then
  begin
    Log('Service {#ServiceName} présent : arrêt avant mise à jour');
    if Exec(ExpandConstant('{sys}\net.exe'), 'stop {#ServiceName}', '', SW_HIDE, ewWaitUntilTerminated, Code) then
      WasRunning := (Code = 0);
    Log(Format('net stop : code %d', [Code]));
    if ConfigExists then RunRequired(ExpandConstant('{app}\{#EngineExe}'), ExpandConstant('backup --config "{#ConfigFile}"'));
  end;

  { tasklist | find renvoie 0 seulement si l'utilitaire est en cours d'exécution. }
  if not Exec(ExpandConstant('{cmd}'),
              '/C tasklist /NH /FI "IMAGENAME eq {#AdminExe}" | find /I "{#AdminExe}"',
              '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then
    Exit;

  if not WizardSilent then
  begin
    if MsgBox('L''utilitaire d''administration ({#AdminExe}) doit être fermé pour continuer la mise à jour.' + #13#10#13#10 +
              'Cliquez sur OK pour le fermer automatiquement, ou sur Annuler pour interrompre l''installation.',
              mbConfirmation, MB_OKCANCEL or MB_DEFBUTTON1) <> IDOK then
    begin
      Result := 'Installation annulée par l''utilisateur : {#AdminExe} est resté ouvert.';
      Exit;
    end;
  end;

  Log('Fermeture des instances de {#AdminExe}');
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM {#AdminExe} /F', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Log(Format('taskkill /IM {#AdminExe} /F : code %d', [Code]));
end;

{ Désinstallation : les données ne sont supprimées que sur demande explicite (jamais en
  mode silencieux). }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataRoot: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    DataRoot := ExpandConstant('{commonappdata}\{#AppName}');
    if DirExists(DataRoot) and (not UninstallSilent) then
    begin
      if MsgBox('Supprimer aussi la configuration, les données (base, historique, comptes) et les journaux ?' + #13#10#13#10 +
                DataRoot + #13#10#13#10 +
                'Cette suppression est définitive. Répondez « Non » pour les conserver (réinstallation ou mise à jour ultérieure).',
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      begin
        if DelTree(DataRoot, True, True, True) then
          Log('Données supprimées : ' + DataRoot)
        else
          MsgBox('Certains fichiers de « ' + DataRoot + ' » n''ont pas pu être supprimés.', mbError, MB_OK);
      end
      else
        Log('Données conservées : ' + DataRoot);
    end;
  end;
end;
