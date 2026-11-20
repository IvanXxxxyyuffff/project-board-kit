# Cookie auto-renew for WeCom cloud docs (wxwork -> doc.weixin.qq.com)
# Goal: keep the pull channel alive with zero user action.
#   1) probe with wxwork_sheet_pull.js --check  (0 = ok, 2 = auth dead)
#   2) if dead: make sure WXWork client is running, then ask the client itself to
#      open one of the docs (WXWorkCommand.exe routes the URL into the client, the
#      client logs in with its own session and refreshes doc cookies on disk)
#   3) re-probe with backoff, log everything, append one CSV row per day
# Exit: 0 = channel usable, 2 = still dead (needs a human touch), 1 = script error

param(
  [int]$Retries = 3,
  [int]$WaitSec = 12,
  [switch]$Quiet
)

$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
while ($Root -and -not (Test-Path (Join-Path $Root '每日项目简报.ps1'))) {
  $parent = Split-Path $Root -Parent
  if (-not $parent -or $parent -eq $Root) { break }
  $Root = $parent
}
$Pull = Join-Path $Root 'research\raw\emobile\wxwork_sheet_pull.js'
# node 解析顺序（与 每日项目简报.ps1 一致）：config.paths.node → PATH → NODE_BIN → 常见本机路径
# 旧版写死 DSH 运行时 node 路径；该运行时已不存在，导致每轮续期在 catch 里报「术语不被识别」整步降级
$Node = $null
try {
  $cfgNode = (Get-Content -Raw (Join-Path $Root 'research\config.json') -ErrorAction Stop | ConvertFrom-Json).paths.node
  if ($cfgNode -and (Test-Path -LiteralPath $cfgNode)) { $Node = $cfgNode }
} catch {}
if (-not $Node) { $Node = (Get-Command node -ErrorAction SilentlyContinue).Source }
if (-not $Node -and $env:NODE_BIN -and (Test-Path -LiteralPath $env:NODE_BIN)) { $Node = $env:NODE_BIN }
if (-not $Node) {
  foreach ($c in @(
      (Join-Path $env:ProgramFiles 'nodejs\node.exe'),
      (Join-Path ${env:ProgramFiles(x86)} 'nodejs\node.exe'),
      (Join-Path $env:LOCALAPPDATA 'Programs\nodejs\node.exe'))) {
    if ($c -and (Test-Path -LiteralPath $c)) { $Node = $c; break }
  }
}
if (-not $Node) { Write-Host 'NODE_NOT_FOUND 未找到 node，跳过续期' -ForegroundColor Yellow; exit 1 }
$Log  = Join-Path $Root 'research\cookie_renew.log'
$Csv  = Join-Path $Root 'research\cookie_expiry_log.csv'
$Alert = Join-Path $Root 'research\cookie_alert.txt'
# 企业微信主程序与命令行走查（不写死盘符：环境变量 → 运行中进程 → 常见安装目录）
$WxExe = $env:WXWORK_EXE
if (-not $WxExe) {
  $p = Get-Process WXWork -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($p -and $p.Path) { $WxExe = $p.Path }
}
if (-not $WxExe) {
  foreach ($cand in @(
      (Join-Path $env:ProgramFiles 'WXWork\WXWork.exe'),
      (Join-Path ${env:ProgramFiles(x86)} 'WXWork\WXWork.exe'),
      (Join-Path $env:LOCALAPPDATA 'Programs\WXWork\WXWork.exe'))) {
    if ($cand -and (Test-Path $cand)) { $WxExe = $cand; break }
  }
}
$WxCmd = $null
if ($WxExe -and (Test-Path $WxExe)) {
  $wxRoot = Split-Path $WxExe -Parent
  $direct = Join-Path $wxRoot 'WXWorkCommand.exe'
  if (Test-Path $direct) { $WxCmd = $direct }
  else {
    $hit = Get-ChildItem $wxRoot -Recurse -Filter 'WXWorkCommand.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($hit) { $WxCmd = $hit.FullName }
  }
}
if (-not $WxExe -or -not $WxCmd) { Write-Host 'WXWORK_NOT_FOUND 未找到企业微信安装目录，跳过续期' -ForegroundColor Yellow; exit 2 }
$DocUrl = 'https://doc.weixin.qq.com/sheet/<PAD_ID>?scode=ACUABAfEAAYl8zjwgl'

function Say([string]$m) {
  $line = '[' + (Get-Date -Format 'yyyy-MM-dd HH:mm:ss') + '] ' + $m
  Add-Content -Path $Log -Value $line -Encoding utf8
  if (-not $Quiet) { Write-Output $line }
}

function Probe {
  # returns 'ok' | 'dead' | 'error'
  $out = & $Node $Pull --check 2>&1 | Out-String
  if ($LASTEXITCODE -eq 0) { return 'ok' }
  if ($LASTEXITCODE -eq 2) { return 'dead' }
  Say ("probe unexpected exit=" + $LASTEXITCODE + " out=" + ($out -replace '\s+', ' ').Trim())
  return 'error'
}

function EarliestExpiry {
  # earliest future expiry among the six auth cookies, as UTC ISO; '' if unknown
  # 取数前先把磁盘 cookie 快照刷成本轮实况（--cookies 只读客户端库、不发网络请求）。
  # 旧版直接读 research\wxwork_cookies.json，该文件只有手工跑 --cookies 才更新，
  # 导致 earliest_expiry / days_left 长期是旧快照（2026-11-10 起一成不变），
  # 2026-11-11 的 state=dead 也因此被误判成 probe_dead。
  try { & $Node $Pull --cookies 2>$null | Out-Null } catch {}
  $js = @'
const fs=require('fs');
try{
  const a=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));
  const names=['wedoc_sid','wedoc_sids','wedoc_skey','wedoc_ticket','wedoc_openid','tdoc_uid'];
  let min=null;
  for(const c of a){ if(names.includes(c.name) && c.expires && c.expires>0){ if(min===null||c.expires<min) min=c.expires; } }
  if(min===null){ console.log(''); } else { console.log(new Date(min*1000).toISOString()); }
}catch(e){ console.log(''); }
'@
  $tmp = Join-Path $env:TEMP 'exp_probe.js'
  Set-Content -Path $tmp -Value $js -Encoding utf8
  $r = & $Node $tmp (Join-Path $Root 'research\wxwork_cookies.json') 2>$null
  return ($r | Out-String).Trim()
}

function LogExpiry([string]$state) {
  $iso = EarliestExpiry
  $days = ''
  if ($iso) {
    try { $days = [math]::Round((( [datetime]$iso ) - (Get-Date).ToUniversalTime()).TotalDays, 1) } catch { $days = '' }
  }
  # 失败态分级（仅影响写入 csv 的 state 与日志文案，不动探活/重试逻辑）：days_left 仍 >5 即 earliest_expiry 未前移、cookie 并未真到期 -> probe_dead（check 探活失败）；days_left <=5 或无法判定 -> expired（真接近/超过到期）
  $effState = $state
  if ($state -eq 'dead') {
    $daysNum = $null
    if ($days -ne '') { try { $daysNum = [double]$days } catch { $daysNum = $null } }
    if ($null -ne $daysNum -and $daysNum -gt 5) { $effState = 'probe_dead' } else { $effState = 'expired' }
    $script:lastDeadState = $effState
  }
  $today = Get-Date -Format 'yyyy-MM-dd'
  if (-not (Test-Path $Csv)) { Set-Content -Path $Csv -Value 'date,state,earliest_expiry,days_left' -Encoding utf8 }
  $rows = @(Get-Content $Csv -Encoding utf8)
  $hit = $rows | Where-Object { $_ -like ($today + ',*') }
  $line = "$today,$effState,$iso,$days"
  if ($hit) {
    $rows = $rows | ForEach-Object { if ($_ -like ($today + ',*')) { $line } else { $_ } }
    Set-Content -Path $Csv -Value $rows -Encoding utf8
  } else {
    Add-Content -Path $Csv -Value $line -Encoding utf8
  }
  $note = ''
  if ($effState -ne $state) { $note = ' (classified from ' + $state + ': probe failure vs real expiry)' }
  Say ("expiry state=$effState earliest=$iso days_left=$days" + $note)
  return $days
}

function EnsureWxRunning {
  $p = Get-Process WXWork -ErrorAction SilentlyContinue
  if ($p) { Say ("wxwork running pid=" + ($p | Select-Object -First 1).Id); return }
  if (-not (Test-Path $WxExe)) { Say 'wxwork exe not found, cannot start'; return }
  Say 'wxwork not running, starting minimized'
  Start-Process -FilePath $WxExe -ArgumentList '-min','-autorun' -WindowStyle Minimized
  Start-Sleep -Seconds 20
}

function AskClientToRenew {
  if (-not (Test-Path $WxCmd)) { Say 'WXWorkCommand.exe missing, skip client renew'; return }
  Say 'asking wxwork client to open a doc (refreshes doc cookies on disk)'
  try {
    Start-Process -FilePath $WxCmd -ArgumentList ('"' + $DocUrl + '"') -WindowStyle Minimized
  } catch {
    Say ('client renew launch failed: ' + $_.Exception.Message)
  }
}

try {
  Say '=== autorenew start ==='
  $st = Probe
  if ($st -eq 'ok') {
    LogExpiry 'ok' | Out-Null
    if (Test-Path $Alert) { Remove-Item $Alert -Force }
    Say '=== autorenew done (already ok) ==='
    exit 0
  }

  Say ("cookie dead (" + $st + ") -> trying to renew without user action")
  EnsureWxRunning
  for ($i = 1; $i -le $Retries; $i++) {
    AskClientToRenew
    Start-Sleep -Seconds $WaitSec
    $st = Probe
    Say ("retry " + $i + "/" + $Retries + " -> " + $st)
    if ($st -eq 'ok') {
      LogExpiry 'renewed' | Out-Null
      if (Test-Path $Alert) { Remove-Item $Alert -Force }
      Say '=== autorenew done (renewed automatically) ==='
      exit 0
    }
  }

  $d = LogExpiry 'dead'
  $msg = 'cookie dead after ' + $Retries + ' auto renew attempts (' + (Get-Date -Format 'yyyy-MM-dd HH:mm') + '), days_left=' + $d
  Set-Content -Path $Alert -Value $msg -Encoding utf8
  $cls = 'dead'
  if ($script:lastDeadState) { $cls = $script:lastDeadState }
  Say ('dead state classified as ' + $cls + ' (probe_dead=check 探活失败非真过期 / expired=真接近或超过到期), days_left=' + $d)
  Say ('=== autorenew FAILED: ' + $msg + ' ===')
  exit 2
}
catch {
  Say ('=== autorenew ERROR: ' + $_.Exception.Message + ' ===')
  exit 1
}
