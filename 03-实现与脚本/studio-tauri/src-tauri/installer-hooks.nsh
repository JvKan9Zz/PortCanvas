!macro NSIS_HOOK_POSTINSTALL
  ${If} ${FileExists} "$DESKTOP\PortCanvas.lnk"
    CreateShortCut "$DESKTOP\PortCanvas.lnk" "$INSTDIR\PortCanvas.exe" "" "$INSTDIR\portcanvas-0.5.2.ico" 0
    !insertmacro SetLnkAppUserModelId "$DESKTOP\PortCanvas.lnk"
  ${EndIf}
  !if "${STARTMENUFOLDER}" != ""
    ${If} ${FileExists} "$SMPROGRAMS\$AppStartMenuFolder\PortCanvas.lnk"
      CreateShortCut "$SMPROGRAMS\$AppStartMenuFolder\PortCanvas.lnk" "$INSTDIR\PortCanvas.exe" "" "$INSTDIR\portcanvas-0.5.2.ico" 0
      !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\$AppStartMenuFolder\PortCanvas.lnk"
    ${EndIf}
  !else
  ${If} ${FileExists} "$SMPROGRAMS\PortCanvas.lnk"
    CreateShortCut "$SMPROGRAMS\PortCanvas.lnk" "$INSTDIR\PortCanvas.exe" "" "$INSTDIR\portcanvas-0.5.2.ico" 0
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\PortCanvas.lnk"
  ${EndIf}
  !endif
!macroend
