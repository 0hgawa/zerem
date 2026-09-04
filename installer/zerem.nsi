; Zerem — per-user installer.
;
; No administrator, no UAC. Everything lands under %LOCALAPPDATA%, which is
; writable by the person running it, and every registry write is HKCU. The
; consequence worth stating: an update can replace the binary without a prompt,
; which is what makes an in-app updater possible at all.
;
; File associations are NOT written here. The app registers them itself on first
; run, and refuses to unless it is running from this install location — so the
; two cannot disagree about where the handler points.

Unicode true
ManifestDPIAware true

!define APP     "Zerem"
!define EXE     "zerem.exe"
!define PUBLISHER "Ohgawa"

!ifndef VERSION
  !define VERSION "0.1.0"
!endif

Name "${APP}"
OutFile "${APP}-Setup.exe"
InstallDir "$LOCALAPPDATA\Programs\${APP}"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails hide
ShowUninstDetails hide

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName"     "${APP}"
VIAddVersionKey "FileDescription" "${APP} installer"
VIAddVersionKey "FileVersion"     "${VERSION}"
VIAddVersionKey "ProductVersion"  "${VERSION}"
VIAddVersionKey "CompanyName"     "${PUBLISHER}"
VIAddVersionKey "LegalCopyright"  "Copyright (C) 2026 ${PUBLISHER}"

!include "MUI2.nsh"

!define MUI_ICON   "..\target\release\build\zerem.ico"
!define MUI_UNICON "..\target\release\build\zerem.ico"
!define MUI_ABORTWARNING

; No component page and no directory page: there is one component, and a
; per-user install has one correct location. A choice that has one right answer
; is a question that should not be asked.
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP}"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Refuses to install over a running copy, which would leave a half-written
; executable and a tray icon pointing at nothing.
!macro EnsureNotRunning UN
Function ${UN}EnsureNotRunning
  StrCpy $0 0
  loop:
    nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq ${EXE}" /NH | find /I "${EXE}"'
    Pop $1
    ${If} $1 != 0
      Return
    ${EndIf}
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION \
      "${APP} is running. Quit it from the tray, then press Retry." \
      IDRETRY loop
    Abort
FunctionEnd
!macroend
!include LogicLib.nsh
!insertmacro EnsureNotRunning ""
!insertmacro EnsureNotRunning "un."

Section "Install"
  Call EnsureNotRunning

  SetOutPath "$INSTDIR"
  File "..\target\release\${EXE}"

  CreateShortCut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\${EXE}"
  CreateShortCut "$DESKTOP\${APP}.lnk" "$INSTDIR\${EXE}"

  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; HKCU, so it shows in this user's Add/Remove Programs without admin.
  !define UNINST "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP}"
  WriteRegStr HKCU "${UNINST}" "DisplayName"     "${APP}"
  WriteRegStr HKCU "${UNINST}" "DisplayVersion"  "${VERSION}"
  WriteRegStr HKCU "${UNINST}" "Publisher"       "${PUBLISHER}"
  WriteRegStr HKCU "${UNINST}" "DisplayIcon"     "$INSTDIR\${EXE}"
  WriteRegStr HKCU "${UNINST}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST}" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegDWORD HKCU "${UNINST}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Call un.EnsureNotRunning

  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APP}.lnk"
  Delete "$DESKTOP\${APP}.lnk"

  ; Everything the app claimed goes back. Leaving a dead `magnet:` handler
  ; behind would send every magnet link in the browser to a file that is gone.
  DeleteRegKey HKCU "Software\Classes\magnet"
  DeleteRegKey HKCU "Software\Classes\${APP}.torrent"
  DeleteRegValue HKCU "Software\Classes\.torrent\OpenWithProgids" "${APP}.torrent"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP}"

  ; The download folder and the session are deliberately NOT touched. Someone
  ; uninstalling a torrent client is not asking for their downloads to be
  ; deleted, and %LOCALAPPDATA%\Zerem holds the resume data for them.
SectionEnd
