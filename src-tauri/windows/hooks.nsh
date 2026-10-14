; Perora's additions to the installer Tauri makes (tauri.conf.json, bundle.windows.nsis.installerHooks).

; Uninstalling, not updating: Perora gives back what it took from the system (the shell's
; Win+Shift+S, when the user had it capture with Perora). Perora does it itself, as it knows whether
; it had it; before its files are removed. (It does nothing else when started so.)
;
; Perora is asked to close first, as the uninstaller does next anyway: should the user say no, the
; uninstall ends there, with nothing given back.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --uninstalling'
  ${EndIf}
!macroend
