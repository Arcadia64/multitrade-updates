!macro NSIS_HOOK_POSTINSTALL
  ; Remove the desktop shortcut created by the installer
  Delete "$DESKTOP\${PRODUCTNAME}.lnk"
!macroend
