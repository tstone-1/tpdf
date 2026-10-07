# Reads the Authenticode signature of each file back and fails unless every one
# is valid, timestamped and made by the named signer.
#
#   scripts/verify_signature.ps1 -Signer 'Open Source Developer Timo Stein' -Path a.exe, b.msi
#
# Two readers are asked: signtool for the verdict Windows gives, PowerShell for
# the signer's name and the timestamp. A signature without a timestamp is
# refused: it stops being valid on the day the certificate expires.
param(
    [Parameter(Mandatory)][string]$Signer,
    [Parameter(Mandatory)][string[]]$Path
)

$signtool = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
    Where-Object FullName -like '*\x64\*' | Sort-Object FullName | Select-Object -Last 1 -ExpandProperty FullName
if (-not $signtool) { Write-Output '[FAIL] signtool.exe was not found'; exit 2 }

$failed = 0
foreach ($file in $Path) {
    $name = Split-Path $file -Leaf
    if (-not (Test-Path $file)) { Write-Output "[FAIL] ${name}: no such file"; $failed++; continue }
    & $signtool verify /pa /all $file | Out-Null
    $verify = $LASTEXITCODE
    $signature = Get-AuthenticodeSignature $file
    $subject = "$($signature.SignerCertificate.Subject)"
    $stamped = [bool]$signature.TimeStamperCertificate
    $good = $verify -eq 0 -and $signature.Status -eq 'Valid' -and $stamped -and $subject -like "*CN=$Signer,*"
    $mark = if ($good) { '[OK]  ' } else { '[FAIL]' }
    Write-Output "$mark ${name}: signtool $verify, status $($signature.Status), timestamp $stamped, signer $subject"
    if (-not $good) { $failed++ }
}
Write-Output "$($Path.Count - $failed) of $($Path.Count) file(s) validly signed by $Signer"
if ($failed) { exit 1 }
