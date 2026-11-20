# 企微只读数据每日刷新（不动客户端、不调登录接口）
# 用法: pwsh -NoProfile -File research\wxwork_daily_refresh.ps1 [-Hours 3]
param([int]$Hours = 3)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$py = if ($env:FRD_PYTHON) { $env:FRD_PYTHON } else { 'python' }
if (-not (Get-Command $py -ErrorAction SilentlyContinue) -and -not (Test-Path $py)) {
    $cand = Join-Path $root 'tools\python\python.exe'
    if (Test-Path $cand) { $py = $cand } else { Write-Host "PYTHON_MISSING"; exit 2 }
}

$steps = @(
    @{ name = '企微日历'; args = @('wxwork_calendar.py', '--days', '120'); cwd = $PSScriptRoot },
    @{ name = '企微未读'; args = @('wxwork_unread_build.py', '--hours', "$Hours", '--json', (Join-Path $PSScriptRoot 'wxwork_unread.json')); cwd = $PSScriptRoot },
    @{ name = '日历并入会议'; args = @('wxwork_calendar_merge.py'); cwd = $PSScriptRoot }
    @{ name = '三源交叉验证'; args = @((Join-Path $PSScriptRoot 'wx_crosscheck.py')); cwd = $root }
)

$fail = 0
foreach ($s in $steps) {
    $t0 = Get-Date
    Push-Location $s.cwd
    try {
        $out = & $py -X utf8 @($s.args) 2>&1
        $code = $LASTEXITCODE
    } finally { Pop-Location }
    $secs = [int]((Get-Date) - $t0).TotalSeconds
    $tail = if ($out) { ($out | Select-Object -Last 1) } else { '' }
    if ($code -eq 0) { Write-Host ("[OK]   {0} {1}s {2}" -f $s.name, $secs, $tail) -ForegroundColor Green }
    else { Write-Host ("[FAIL] {0} {1}s {2}" -f $s.name, $secs, $tail) -ForegroundColor Red; $fail++ }
}

Write-Host ("企微刷新结束 fail={0}" -f $fail)
exit $fail
