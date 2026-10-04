Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "StrFunc.nsh"
${StrStr}
Name "MonoCode"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\MonoCode"
InstallDirRegKey HKCU "Software\monocode\MonoCode" ""
RequestExecutionLevel user
SetCompressor /SOLID lzma
Icon "${ICON}"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
Var Restart
Var LaunchArgs
Var Update
Function .onInit
  ${GetParameters} $0
  ${GetOptions} $0 "/P" $1
  ${IfNot} ${Errors}
    SetSilent silent
  ${EndIf}
  ClearErrors
  ${GetOptions} $0 "/R" $Restart
  ${IfNot} ${Errors}
    StrCpy $Restart "yes"
  ${EndIf}
  ClearErrors
  ${GetOptions} $0 "/UPDATE" $Update
  ${IfNot} ${Errors}
    StrCpy $Update "yes"
  ${EndIf}
  ClearErrors
  ; Keep the complete argument tail, including slash-prefixed paths.
  ${StrStr} $LaunchArgs $0 "/ARGS "
  ${If} $LaunchArgs != ""
    StrCpy $LaunchArgs $LaunchArgs "" 6
  ${EndIf}
FunctionEnd
Section "MonoCode"
  SetOutPath "$INSTDIR"
  ; The updater starts this installer before it exits. Allow the old app
  ; to flush its data before retrying a locked executable.
  StrCpy $2 0
  retry:
    ClearErrors
    File "${BIN_DIR}\monocode-app.exe"
    ${If} ${Errors}
      IntOp $2 $2 + 1
      ${If} $2 < 30
        Sleep 1000
        Goto retry
      ${EndIf}
      Abort "Close MonoCode and run the installer again."
    ${EndIf}
  File "${BIN_DIR}\monocode-host.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\monocode\MonoCode" "" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonoCode" "DisplayName" "MonoCode"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonoCode" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonoCode" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "Software\Classes\monocode" "" "URL:MonoCode pairing"
  WriteRegStr HKCU "Software\Classes\monocode" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\monocode\DefaultIcon" "" '"$INSTDIR\monocode-app.exe",0'
  WriteRegStr HKCU "Software\Classes\monocode\shell\open\command" "" '"$INSTDIR\monocode-app.exe" "%1"'
  ${If} $Update != "yes"
    CreateDirectory "$SMPROGRAMS\MonoCode"
    CreateShortcut "$SMPROGRAMS\MonoCode\MonoCode.lnk" "$INSTDIR\monocode-app.exe"
  ${EndIf}
  ${If} $Restart == "yes"
    Exec '"$INSTDIR\monocode-app.exe" $LaunchArgs'
  ${EndIf}
SectionEnd
Section "Uninstall"
  Delete "$INSTDIR\monocode-app.exe"
  Delete "$INSTDIR\monocode-host.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\MonoCode\MonoCode.lnk"
  RMDir "$SMPROGRAMS\MonoCode"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonoCode"
  DeleteRegKey HKCU "Software\monocode\MonoCode"
  ReadRegStr $0 HKCU "Software\Classes\monocode\shell\open\command" ""
  ${If} $0 == '"$INSTDIR\monocode-app.exe" "%1"'
    DeleteRegKey HKCU "Software\Classes\monocode"
  ${EndIf}
SectionEnd
