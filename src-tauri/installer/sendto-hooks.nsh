; Keep the SendTo integration without replacing Tauri's generated installer.
; Tauri supplies $INSTDIR and ${MAINBINARYNAME} at bundle time.

!macro NSIS_HOOK_POSTINSTALL
  Delete "$SENDTO\Ganmaoyuan.lnk"
  CreateShortcut "$SENDTO\感冒院.lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "--shell-source windowsSendTo"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete "$SENDTO\感冒院.lnk"
  Delete "$SENDTO\Ganmaoyuan.lnk"
!macroend
