; ============================================================================
; SpeakIn NSIS uninstall hooks — minimal, scoped, safe, upgrade-aware.
; ============================================================================
;
; Goal: when the user uninstalls SpeakIn AND explicitly checks "delete app
; data", also remove its OS keyring credentials and its app data folders —
; WITHOUT touching anything that belongs to any other application, AND
; WITHOUT wiping user data during in-place upgrades.
;
; ─── Safety contract ────────────────────────────────────────────────────────
;
; 1. UPGRADE + USER-CONSENT GUARD: destructive cleanup is wrapped in
;    `${If} $UpdateMode <> 1 ${AndIf} $DeleteAppDataCheckboxState = 1`.
;    Tauri's NSIS flow runs the OLD uninstaller with `/UPDATE` before
;    installing a new version; in that mode we must NOT wipe the user's
;    credentials or data (they'd have to re-enter every API key and lose
;    all settings on every upgrade). Tauri's own default template uses the
;    identical guard for its built-in cleanup (see installer.nsi lines
;    795 and 802 in a fresh build).
;    Real uninstall cleanup also requires the user to explicitly check
;    Tauri's "delete app data" checkbox.
;
; 2. PRE-UNINSTALL (real uninstall + delete-app-data checked only): runs the installed exe with
;    `--uninstall-cleanup` to remove OS keyring credentials. The Rust
;    cleanup routine uses the SAME
;      `keyring::Entry::new(service="com.magiccodelab.speakin", key=<known>)`
;    API that wrote each credential. It only touches:
;      • A hardcoded list of ASR provider keys
;      • `ai_provider_<id>` keys with <id> read from our own
;        `ai_providers.json` inside our own APPDATA folder
;    It is mathematically impossible for this step to affect credentials
;    belonging to any other application.
;
; 3. POST-UNINSTALL (real uninstall + delete-app-data checked only): `RMDir /r` both APPDATA and
;    LOCALAPPDATA subfolders.
;      • `$APPDATA\com.magiccodelab.speakin` — settings.json, stats.json,
;        ai_providers.json, recent_transcripts.json, etc.
;      • `$LOCALAPPDATA\com.magiccodelab.speakin` — WebView2 cache.
;    Both paths use the fully qualified reverse-DNS identifier from
;    tauri.conf.json; it is unique worldwide and cannot collide with any
;    other application. We mirror Tauri's delete-app-data checkbox so a
;    plain Next/Next/Finish uninstall preserves user settings and credentials.
;
; 4. AUTOSTART REGISTRY VALUE is INTENTIONALLY NOT DELETED HERE.
;    Tauri's default NSIS template already deletes it at
;      `DeleteRegValue HKCU "Software\Microsoft\...\Run" "${PRODUCTNAME}"`
;    using the correct value name. `${PRODUCTNAME}` resolves to
;    tauri.conf.json `productName`, which is ALSO the string that
;    `tauri-plugin-autostart` writes (via `PackageInfo.name`, populated
;    from `product_name` by tauri-codegen). Our previous attempt to
;    duplicate this with hardcoded "speakin" (lowercase) targeted the
;    wrong value name and was a no-op — worse, it created the impression
;    of belt-and-suspenders coverage that did not actually exist.
;
; 5. We do NOT use:
;    • PowerShell wildcards
;    • Registry tree deletions (`DeleteRegKey /ifempty`)
;    • Enumeration of unrelated namespaces
;    • Any path that isn't fully hardcoded
;
; Both hooks are best-effort. `ExecWait` failures and missing paths are
; silently ignored so an uninstall never aborts because of cleanup.
; ============================================================================

!define SPEAKIN_DISPLAY_NAME "SpeakIn声入"

!macro NSIS_HOOK_POSTINSTALL
  ; Keep PRODUCTNAME as ASCII-only for the install directory, but expose the
  ; bilingual display name in Windows search and Apps / Programs & Features.
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${SPEAKIN_DISPLAY_NAME}"

  ; Rename the Start Menu shortcut from "SpeakIn" to "SpeakIn声入"
  ; so Windows search finds the app when user types "声入".
  ; Tauri creates the shortcut as "$SMPROGRAMS\${PRODUCTNAME}.lnk".
  ${If} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    Rename "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$SMPROGRAMS\${SPEAKIN_DISPLAY_NAME}.lnk"
  ${EndIf}
  ; Migrate the previous bilingual shortcut name used by older installers.
  ${If} ${FileExists} "$SMPROGRAMS\SpeakIn 声入.lnk"
    ${If} ${FileExists} "$SMPROGRAMS\${SPEAKIN_DISPLAY_NAME}.lnk"
      Delete "$SMPROGRAMS\SpeakIn 声入.lnk"
    ${Else}
      Rename "$SMPROGRAMS\SpeakIn 声入.lnk" "$SMPROGRAMS\${SPEAKIN_DISPLAY_NAME}.lnk"
    ${EndIf}
  ${EndIf}
  ; Also rename the desktop shortcut if Tauri created one
  ${If} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
    Rename "$DESKTOP\${PRODUCTNAME}.lnk" "$DESKTOP\${SPEAKIN_DISPLAY_NAME}.lnk"
  ${EndIf}
  ${If} ${FileExists} "$DESKTOP\SpeakIn 声入.lnk"
    ${If} ${FileExists} "$DESKTOP\${SPEAKIN_DISPLAY_NAME}.lnk"
      Delete "$DESKTOP\SpeakIn 声入.lnk"
    ${Else}
      Rename "$DESKTOP\SpeakIn 声入.lnk" "$DESKTOP\${SPEAKIN_DISPLAY_NAME}.lnk"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Clean up renamed shortcuts (both old and new names, in case of upgrade)
  Delete "$SMPROGRAMS\${SPEAKIN_DISPLAY_NAME}.lnk"
  Delete "$DESKTOP\${SPEAKIN_DISPLAY_NAME}.lnk"
  Delete "$SMPROGRAMS\SpeakIn 声入.lnk"
  Delete "$DESKTOP\SpeakIn 声入.lnk"
  ; Skip during in-place upgrades and unless the user explicitly requested
  ; app data deletion — credentials are part of app data.
  ${If} $UpdateMode <> 1
  ${AndIf} $DeleteAppDataCheckboxState = 1
    ${If} ${FileExists} "$INSTDIR\speakin.exe"
      DetailPrint "${SPEAKIN_DISPLAY_NAME}: 正在清除已保存的 API 凭据…"
      ExecWait '"$INSTDIR\speakin.exe" --uninstall-cleanup' $0
      DetailPrint "${SPEAKIN_DISPLAY_NAME}: 凭据清理完成 (exit=$0)"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Skip during in-place upgrades and unless the user explicitly requested
  ; app data deletion.
  ${If} $UpdateMode <> 1
  ${AndIf} $DeleteAppDataCheckboxState = 1
    DetailPrint "${SPEAKIN_DISPLAY_NAME}: 移除应用数据目录…"
    RMDir /r "$APPDATA\com.magiccodelab.speakin"
    RMDir /r "$LOCALAPPDATA\com.magiccodelab.speakin"
  ${EndIf}
!macroend
