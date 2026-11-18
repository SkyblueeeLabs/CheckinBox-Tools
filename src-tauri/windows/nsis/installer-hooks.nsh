; 升级时复用旧版安装目录，避免 NSIS/MSI 或自定义目录装出第二份。
; ${UNINSTKEY} / SHCTX 由 Tauri 的 NSIS 模板提供。
!macro NSIS_HOOK_PREINSTALL
  Push $0
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "InstallLocation"
  ${If} $0 != ""
    ${If} ${FileExists} "$0\CheckinBox.exe"
      StrCpy $INSTDIR $0
    ${EndIf}
  ${EndIf}
  Pop $0
!macroend
