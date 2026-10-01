; NSIS hooks for the Windows installer.
;
; Two things live here: the 26.8.8 repair described next, and, at the end,
; putting the command-line tool's folder on the user's PATH.
;
; Tauri inserts NSIS_HOOK_PREINSTALL immediately after `SetOutPath $INSTDIR`
; and before the resource copies -- see the generated `installer.nsi`, where
; the next lines are `CreateDirectory "$INSTDIR\pdfium"` and
; `File /a "/oname=pdfium\pdfium.dll"`.
;
; ---------------------------------------------------------------------------
; Why this exists: 26.8.8 left a FILE where 26.8.9 needs a DIRECTORY.
; ---------------------------------------------------------------------------
;
; 26.8.8's resource map read `"../vendor/pdfium/bin/pdfium.dll": "pdfium/"`.
; A trailing slash means "into this directory" on macOS and is a RENAME on
; Windows, so that build installed a 7 MB file named `pdfium` -- no extension,
; and nothing loads it, which is the defect 26.8.9 was cut to fix. 26.8.9
; names the destination file outright and needs `$INSTDIR\pdfium` to be a
; directory.
;
; Upgrading over 26.8.8 therefore walks into the stray file. `CreateDirectory`
; fails silently against it, `File` then reports "Error opening file for
; writing: ...\pdfium\pdfium.dll", and the reader gets Abort/Retry/Ignore.
; Retry cannot help -- it re-attempts the file write, not the directory
; creation that already failed -- and Ignore is worse than Abort: the install
; reports success while `pdfium_library_dir` finds no library under either
; bundled candidate, which is an application that opens no document at all.
; That is 26.8.9's own defect, reintroduced by the upgrade to 26.8.9.
;
; ---------------------------------------------------------------------------
; When this can be deleted.
; ---------------------------------------------------------------------------
;
; It is dead code on every machine that has not run 26.8.8, and dead code on
; every machine that has run this once. It can go when no supported upgrade
; path starts at 26.8.8 -- i.e. when it is safe to assume nobody upgrades from
; a build older than 26.8.10. Delete the NSIS_HOOK_PREINSTALL macro then; the
; PATH hooks below stay.
;
; `Delete` cannot be blocked by our own running application: the stray file is
; named `pdfium`, and every candidate in `pdfium_library_dir` looks for
; `pdfium.dll`, so no tpdf build ever mapped it. If it fails anyway the
; install proceeds and fails exactly where it does today, which is no worse
; than the current behaviour and is why there is no handling for it.

!macro NSIS_HOOK_PREINSTALL
  ; Already a directory -- every ordinary upgrade, and every re-run of this.
  IfFileExists "$INSTDIR\pdfium\*.*" tpdf_pdfium_ready 0
  ; Exists and is not a directory: 26.8.8's stray file.
  IfFileExists "$INSTDIR\pdfium" 0 tpdf_pdfium_ready
    DetailPrint "Removing a file named pdfium left behind by 26.8.8"
    Delete "$INSTDIR\pdfium"
  tpdf_pdfium_ready:
!macroend

; ---------------------------------------------------------------------------
; The command-line tool's folder on the user's PATH.
; ---------------------------------------------------------------------------
;
; `tpdf-cli.exe` is installed beside `tpdf.exe`, and a script calls it by
; name. The tool edits PATH itself (`tpdf-cli path --add`, `userpath.rs`),
; because an NSIS string is cut at a fixed length: a PATH read here, cut, and
; written back would lose the reader's own entries. Nothing is read or written
; in this file.
;
; It is the user's PATH (HKCU), whatever mode the installer runs in: the tool
; runs as whoever runs the installer. Adding is idempotent, so every update
; repeats it harmlessly. Removing is skipped during an update, where the old
; version's uninstaller runs with /UPDATE and the folder is about to be
; installed into again.
;
; A failure is logged and not fatal: an install whose PATH could not be
; changed is still an install, and *Install command-line tool...* in the
; application does the same thing later.

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog '"$INSTDIR\tpdf-cli.exe" path --add'
  Pop $0
  DetailPrint "tpdf-cli path --add: exit $0"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    nsExec::ExecToLog '"$INSTDIR\tpdf-cli.exe" path --remove'
    Pop $0
    DetailPrint "tpdf-cli path --remove: exit $0"
  ${EndIf}
!macroend
