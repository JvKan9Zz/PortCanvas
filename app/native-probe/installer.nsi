Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!cd "${__FILEDIR__}"
!ifndef PRODUCT
  !define PRODUCT "PortCanvas Beta"
!endif
!ifndef REGKEY
  !define REGKEY "PortCanvasBeta"
!endif
!ifndef VERSION
  !define VERSION "0.6.0-beta.1"
!endif
!ifndef VERSION_QUAD
  !define VERSION_QUAD "0.6.0.1"
!endif
!ifndef BINARY
  !error "BINARY must name the verified native release executable"
!endif
!ifndef OUTPUT
  !error "OUTPUT must name the preview installer"
!endif
Name "${PRODUCT}"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT}"
InstallDirRegKey HKCU "Software\${REGKEY}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
VIProductVersion "${VERSION_QUAD}"
VIAddVersionKey "ProductName" "PortCanvas Beta"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "PortCanvas native preview installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "PortCanvas contributors"
!define MUI_ABORTWARNING
!define MUI_ICON "assets\portcanvas.ico"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${GetParameters} $0
  ${GetOptions} $0 "/PID=" $1
  ${If} $1 != ""
    System::Call 'kernel32::OpenProcess(i 0x100000, i 0, i r1) p.r2'
    ${If} $2 P<> 0
      System::Call 'kernel32::WaitForSingleObject(p r2, i 60000) i.r3'
      System::Call 'kernel32::CloseHandle(p r2)'
      ${If} $3 != 0
        SetErrorLevel 1
        Abort "PortCanvas Beta is still closing. Try again after it closes."
      ${EndIf}
    ${EndIf}
  ${EndIf}
FunctionEnd

Function .onInstSuccess
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/RESTART" $1
  IfErrors restart_done
  Exec '"$INSTDIR\PortCanvasBeta.exe"'
  restart_done:
FunctionEnd

Section "PortCanvas Beta" Main
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  SetOverwrite on
  ClearErrors
  Delete "$INSTDIR\PortCanvasBeta.previous.exe"
  ${If} ${Errors}
    SetErrorLevel 1
    Abort "Close the previous preview and try again."
  ${EndIf}
  File /oname=PortCanvasBeta.new.exe "${BINARY}"
  ; Preserve the previous executable if replacement fails or it is still running.
  ClearErrors
  IfFileExists "$INSTDIR\PortCanvasBeta.exe" 0 commit
  Rename "$INSTDIR\PortCanvasBeta.exe" "$INSTDIR\PortCanvasBeta.previous.exe"
  ${If} ${Errors}
    Delete "$INSTDIR\PortCanvasBeta.new.exe"
    SetErrorLevel 1
    Abort "Close PortCanvas Beta and try again."
  ${EndIf}
  commit:
  ClearErrors
  Rename "$INSTDIR\PortCanvasBeta.new.exe" "$INSTDIR\PortCanvasBeta.exe"
  ${If} ${Errors}
    Rename "$INSTDIR\PortCanvasBeta.previous.exe" "$INSTDIR\PortCanvasBeta.exe"
    SetErrorLevel 1
    Abort "The previous preview was preserved."
  ${EndIf}
  Delete "$INSTDIR\PortCanvasBeta.previous.exe"
  File "assets\portcanvas.ico"
  File /oname=FONT-NOTICES.txt "assets\NOTICE.txt"
  File "assets\LICENSE-Macintosh-Skeletal.txt"
  File /oname=LICENSE.txt "..\..\LICENSE"
  File "LICENSE-Slint.md"
  File "THIRD-PARTY-NOTICES.txt"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\${REGKEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${REGKEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${REGKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${REGKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${REGKEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${REGKEY}" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${REGKEY}" "NoRepair" 1
  CreateDirectory "$SMPROGRAMS\${PRODUCT}"
  CreateShortCut "$SMPROGRAMS\${PRODUCT}\${PRODUCT}.lnk" "$INSTDIR\PortCanvasBeta.exe"
  CreateShortCut "$DESKTOP\${PRODUCT}.lnk" "$INSTDIR\PortCanvasBeta.exe" "" "$INSTDIR\portcanvas.ico"
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  ClearErrors
  Delete "$INSTDIR\PortCanvasBeta.exe"
  ${If} ${Errors}
    SetErrorLevel 1
    Abort "Close PortCanvas Beta and try uninstalling again."
  ${EndIf}
  Delete "$INSTDIR\PortCanvasBeta.new.exe"
  Delete "$INSTDIR\PortCanvasBeta.previous.exe"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$INSTDIR\portcanvas.ico"
  Delete "$INSTDIR\FONT-NOTICES.txt"
  Delete "$INSTDIR\LICENSE-Macintosh-Skeletal.txt"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\LICENSE-Slint.md"
  Delete "$INSTDIR\THIRD-PARTY-NOTICES.txt"
  Delete "$SMPROGRAMS\${PRODUCT}\${PRODUCT}.lnk"
  RMDir "$SMPROGRAMS\${PRODUCT}"
  Delete "$DESKTOP\${PRODUCT}.lnk"
  DeleteRegKey HKCU "Software\${REGKEY}"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${REGKEY}"
  ; Non-recursive: user-created files and independent preview data are retained.
  RMDir "$INSTDIR"
SectionEnd
