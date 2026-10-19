; MultiClaude used to ship as cc-switch.exe (the upstream binary name). An
; update installs MultiClaude.exe beside it, so the old binary is removed here;
; shortcuts and the uninstaller already point at the new one.
!macro NSIS_HOOK_POSTINSTALL
  Delete "$INSTDIR\cc-switch.exe"
!macroend
