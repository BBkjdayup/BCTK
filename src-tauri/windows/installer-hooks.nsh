; The stock Tauri checkbox only removes AppData. Preserve its normal cleanup,
; but copy a restricted Rust cleanup helper and the current managed-root record
; before AppData is removed. The helper runs only after Tauri has stopped the
; application and removed the installed executable.

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    InitPluginsDir
    CopyFiles /SILENT "$INSTDIR\${MAINBINARYNAME}.exe" "$PLUGINSDIR"
    CopyFiles /SILENT "$LOCALAPPDATA\com.zhitiku.desktop\uninstall-data-root.json" "$PLUGINSDIR"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ${If} ${FileExists} "$PLUGINSDIR\${MAINBINARYNAME}.exe"
    ${AndIf} ${FileExists} "$PLUGINSDIR\uninstall-data-root.json"
      ExecWait '"$PLUGINSDIR\${MAINBINARYNAME}.exe" --tktiku-uninstall-delete-managed-data-v1 "$PLUGINSDIR\uninstall-data-root.json"' $0
      ${If} $0 <> 0
        MessageBox MB_ICONEXCLAMATION|MB_OK "程序已经卸载，但题库数据未能通过安全校验，因此没有删除。请保留该目录并联系技术支持。"
      ${EndIf}
    ${Else}
      MessageBox MB_ICONEXCLAMATION|MB_OK "程序已经卸载，但没有找到经过验证的题库目录记录，因此没有删除题库数据。"
    ${EndIf}
    Delete /REBOOTOK "$PLUGINSDIR\${MAINBINARYNAME}.exe"
    Delete /REBOOTOK "$PLUGINSDIR\uninstall-data-root.json"
  ${EndIf}
!macroend
