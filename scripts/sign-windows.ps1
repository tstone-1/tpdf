# Signs one Windows file with ssign, for sign-windows.cmd.
#
# Two things ssign alone does not do for a release build:
#
# It says why. Tauri shows nothing of a sign command that failed:
# v26.10.11-rc2 ended with "failed to run ssign" and no reason. Everything
# ssign prints goes to a log the workflow prints afterwards.
#
# It writes the file in place. ssign replaces the file by renaming a signed
# copy over it, and v26.10.11-rc3 failed there on tpdf.exe, a fifth of a
# second after Tauri had written to it: "atomically replacing ...: Access is
# denied. (os error 5)". So ssign signs into a folder of its own, and the
# signed bytes are copied over the file here, again for a few seconds while
# something else still has it open. The remote signature is made once.
param([Parameter(Mandatory)][string]$Path)

$log = Join-Path $env:RUNNER_TEMP 'ssign.log'
$name = Split-Path $Path -Leaf
Add-Content $log "---- $name"

$signed = Join-Path $env:RUNNER_TEMP ("signed-" + [guid]::NewGuid().ToString('N'))
ssign --verbose --output-dir $signed $Path *>> $log
if ($LASTEXITCODE -ne 0) { Add-Content $log "ssign exit code $LASTEXITCODE"; exit 1 }

$copy = Join-Path $signed $name
foreach ($attempt in 1..15) {
    try {
        Copy-Item -LiteralPath $copy -Destination $Path -Force -ErrorAction Stop
        Add-Content $log "written to $name at attempt $attempt"
        Remove-Item -Recurse -Force $signed -ErrorAction SilentlyContinue
        exit 0
    } catch {
        Add-Content $log "attempt ${attempt}: $($_.Exception.Message)"
        Start-Sleep -Seconds 2
    }
}
Add-Content $log "gave up writing $name"
exit 1
