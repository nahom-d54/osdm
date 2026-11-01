; Never install an extension or set browser policies. Browser approval stays in
; the browser. The desktop's early CLI path manages only fixed HKCU host keys.
!macro NSIS_HOOK_POSTINSTALL
  IfSilent osdm_repair_helpers
  MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Set up OSDM's local browser helper for Chrome, Edge, Brave, Chromium and Firefox? Browser extensions still require your approval. Download interception stays off until you enable it in OSDM." IDNO osdm_repair_helpers
  ExecWait '"$INSTDIR\osdm-desktop.exe" --install-browser-integration' $0
  Goto osdm_helpers_done
  osdm_repair_helpers:
  ; Silent upgrades only refresh registrations already owned by OSDM.
  ExecWait '"$INSTDIR\osdm-desktop.exe" --repair-browser-integration' $0
  osdm_helpers_done:
  ${If} $0 != 0
    DetailPrint "Browser helper setup needs attention. Open Set up browser extension in OSDM."
    IfSilent +2
    MessageBox MB_OK|MB_ICONEXCLAMATION "Some browser helpers could not be configured. Open Set up browser extension in OSDM for details. Other registrations were preserved."
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ExecWait '"$INSTDIR\osdm-desktop.exe" --uninstall-browser-integration' $0
  ${If} $0 != 0
    DetailPrint "Some browser helper registrations were preserved because they could not be verified as OSDM-owned."
  ${EndIf}
!macroend
