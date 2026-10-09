; Let the application manage its SendTo shortcut so it can verify ownership
; before updating or removing an existing link.

!macro NSIS_HOOK_POSTINSTALL
  Push $0
  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --register-sendto' $0
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Push $0
  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --unregister-sendto' $0
  Pop $0
!macroend
