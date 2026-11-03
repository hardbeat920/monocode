Unicode True
!include "MUI2.nsh"
!include "FileFunc.nsh"
Name "Monocode Native"
OutFile "@OUTFILE@"
InstallDir "$LOCALAPPDATA\Programs\Monocode Native"
InstallDirRegKey HKCU "Software\Monocode Native" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
VIProductVersion "@WINDOWS_VERSION@"
VIAddVersionKey "ProductName" "Monocode Native"
VIAddVersionKey "FileDescription" "Monocode Native preview installer"
VIAddVersionKey "FileVersion" "@VERSION@"
!define MUI_ICON "@ICON@"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
Var UpdatePID
Var HadPrevious
Function .onInit
  ${GetParameters} $R0
  ${GetOptions} $R0 "/UPDATEPID=" $UpdatePID
  StrCmp $UpdatePID "" ready
  System::Call 'kernel32::OpenProcess(i 0x00100000, i 0, i $UpdatePID) p .r1'
  StrCmp $1 0 ready
  System::Call 'kernel32::WaitForSingleObject(p r1, i 120000) i .r2'
  System::Call 'kernel32::CloseHandle(p r1)'
  StrCmp $2 0 ready
  Abort "Monocode Native did not close. Please try updating again."
  ready:
FunctionEnd
Section "Monocode Native"
  SetOutPath "$INSTDIR"
  ClearErrors
  File /oname=monocode-gpui.next.exe "@STAGE@\monocode-gpui.exe"
  IfErrors install_failed
  StrCpy $HadPrevious 0
  IfFileExists "$INSTDIR\monocode-gpui.exe" 0 replace_binary
  Delete "$INSTDIR\monocode-gpui.previous.exe"
  ClearErrors
  Rename "$INSTDIR\monocode-gpui.exe" "$INSTDIR\monocode-gpui.previous.exe"
  IfErrors install_failed
  StrCpy $HadPrevious 1
  replace_binary:
  ClearErrors
  Rename "$INSTDIR\monocode-gpui.next.exe" "$INSTDIR\monocode-gpui.exe"
  IfErrors rollback_binary
  File "@STAGE@\native-install.json"
  File /r "@STAGE@\licenses"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Monocode Native" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "DisplayName" "Monocode Native"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "DisplayVersion" "@VERSION@"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  CreateShortcut "$SMPROGRAMS\Monocode Native.lnk" "$INSTDIR\monocode-gpui.exe"
  CreateShortcut "$DESKTOP\Monocode Native.lnk" "$INSTDIR\monocode-gpui.exe"
  Delete "$INSTDIR\monocode-gpui.previous.exe"
  Goto install_done
  rollback_binary:
  StrCmp $HadPrevious 1 0 install_failed
  Rename "$INSTDIR\monocode-gpui.previous.exe" "$INSTDIR\monocode-gpui.exe"
  install_failed:
  Delete "$INSTDIR\monocode-gpui.next.exe"
  MessageBox MB_ICONSTOP "Monocode Native could not be updated. The previous executable was retained. Close other Monocode Native processes and try again."
  Abort
  install_done:
SectionEnd
Function .onInstFailed
  StrCmp $UpdatePID "" failed_done
  IfFileExists "$INSTDIR\monocode-gpui.exe" 0 failed_done
  Exec '"$INSTDIR\monocode-gpui.exe"'
  failed_done:
FunctionEnd
Function .onInstSuccess
  StrCmp $UpdatePID "" done
  Exec '"$INSTDIR\monocode-gpui.exe"'
  done:
FunctionEnd
Section "Uninstall"
  Delete "$SMPROGRAMS\Monocode Native.lnk"
  Delete "$DESKTOP\Monocode Native.lnk"
  Delete "$INSTDIR\monocode-gpui.exe"
  Delete "$INSTDIR\native-install.json"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir /r "$INSTDIR\licenses"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Monocode Native"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MonocodeNative"
SectionEnd
