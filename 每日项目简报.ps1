# 每日项目简报 · 一键管道（宁可多抓，不可错过）
# 用法: pwsh -File ".\每日项目简报.ps1" [-NoOpen] [-Fresh] [-Quiet] [-NoWidgetRestart]
#   -NoOpen 不自动打开 HTML   -Fresh 强制 E-Mobile 全量重建   -Quiet 少打印
#   -NoWidgetRestart 只刷新数据，不拉起/重启「项目看板」进程（看板按 mtime 每 30s 自行重载）
# 依次: 微信表格全页签(动态发现) → E-Mobile 增量(时间游标+冗余翻页) → 重建聊天 → 结构化汇总(超时兜底)
#       → 生成 HTML/MD 看板 → 完整性自检写入 research\数据完整性.json
param([switch]$NoOpen, [switch]$Fresh, [switch]$Quiet, [switch]$NoWidgetRestart, [switch]$ForceEmobile, [switch]$PullOnly, [switch]$WithPull)

# B24：中文路径 / Node stdout 统一 UTF-8，修 PowerShell 按错误代码页解码落盘乱码
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$ErrorActionPreference = 'Continue'
# 根路径自动探测：优先脚本所在目录，回退当前目录（计划任务 / 相对 -File 调用都可用）
$ROOT  = $PSScriptRoot

# 全局互斥：挂件 30 分钟轮询、手工运行、定时任务可能同时在跑，抢不到就退出
# （以前并发写 wxwork_chat.json / 数据完整性.json，半写文件没人校验）
$pipeMutex = New-Object System.Threading.Mutex($false, 'Global\FRD-PIPELINE')
try { $haveMutex = $pipeMutex.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $haveMutex = $true }
if (-not $haveMutex) {
  if (-not $Quiet) { Write-Host "PIPELINE busy：另一轮抓取正在跑，本次退出" }
  exit 9
}

# 日志轮转：单个日志超过 2 MB 滚一份 .1，长跑不会无限长
foreach ($lg in @('research\unread_live.log', 'research\widget_pull.log', 'research\cookie_renew.log')) {
  $lp = Join-Path $ROOT $lg
  if ((Test-Path $lp) -and ((Get-Item -LiteralPath $lp).Length -gt 2MB)) {
    Move-Item -LiteralPath $lp -Destination ($lp + '.1') -Force
  }
}
if (-not $ROOT) { $ROOT = (Get-Location).Path }
# 运行时探测：优先 PATH，环境变量可覆盖，最后回退本机路径（去本机化但保留老机器可用）
$PWSH  = (Get-Command pwsh -ErrorAction SilentlyContinue).Source
if (-not $PWSH) { $PWSH = (Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\pwsh.exe') }
# node 解析顺序：config.paths.node（安装即用）→ PATH → NODE_BIN → ProgramFiles\nodejs → 字面 'node'
# 不写死任何用户目录下的运行时路径：DSH 运行时目录随时会被删，写死只会在运行到那一行才炸
$NODE = $null
try { $NODE = (Get-Content -Raw (Join-Path $ROOT 'research\config.json') -ErrorAction Stop | ConvertFrom-Json).paths.node } catch {}
if (-not ($NODE -and (Test-Path -LiteralPath $NODE))) { $NODE = (Get-Command node -ErrorAction SilentlyContinue).Source }
if (-not $NODE -and $env:NODE_BIN) { $NODE = $env:NODE_BIN }
if (-not $NODE) {
  foreach ($c in @((Join-Path $env:ProgramFiles 'nodejs\node.exe'),
                   (Join-Path ${env:ProgramFiles(x86)} 'nodejs\node.exe'),
                   (Join-Path $env:LOCALAPPDATA 'Programs\nodejs\node.exe'))) {
    if ($c -and (Test-Path -LiteralPath $c)) { $NODE = $c; break }
  }
}
if (-not $NODE) { $NODE = 'node' }
$WXDIR = Join-Path $ROOT 'research\wxdata'
$EMDIR = Join-Path $ROOT 'research\raw\emobile'

$T0 = Get-Date
$steps = New-Object System.Collections.ArrayList
$alerts = New-Object System.Collections.ArrayList

function Add-Step {
  # $Ok：$true=成功 / $false=降级 / 'skip'=按设计跳过（不算成功、也不算失败）
  param($Name, $Start, $Ok, $Detail)
  $sec = [math]::Round(((Get-Date) - $Start).TotalSeconds, 1)
  $state = if ($Ok -is [string] -and $Ok -eq 'skip') { 'skip' } elseif ($Ok) { 'ok' } else { 'fail' }
  # state 是三态真值（ok/skip/fail）；ok 保留给下游 build_widget 兼容，skip/failed 让状态灯能区分
  # 「设计跳过」与「真实失败」，避免 skip 被当成 ok 掩盖故障（审核 2026-11-14）。
  [void]$steps.Add([ordered]@{ name = $Name; sec = $sec; state = $state; ok = ($state -ne 'fail'); skip = ($state -eq 'skip'); failed = ($state -eq 'fail'); detail = ([string]$Detail).Trim() })
  if (-not $Quiet) {
    $tag = switch ($state) { 'ok' { 'OK' } 'skip' { '跳过' } default { '降级' } }
    Write-Host ("  {0,-30} {1,6}s  {2}" -f $Name, $sec, $tag)
  }
  if ($state -eq 'fail') { [void]$alerts.Add("$Name 未成功") }
}
function Tail($o, $n = 3) {
  ($o -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -Last $n) -join ' | '
}
function Invoke-Retry {
  param([int]$Times = 2, [scriptblock]$Body)
  $last = ''
  for ($i = 1; $i -le $Times; $i++) {
    try { $last = & $Body 2>&1 | Out-String; if ($LASTEXITCODE -eq 0) { return $last } }
    catch { $last = "$last`n$_" }
    if ($i -lt $Times -and -not $Quiet) { Write-Host "    retry $i ..." }
    Start-Sleep -Seconds 2
  }
  return $last
}

if (-not $Quiet) { Write-Host "==== 每日项目简报 $($T0.ToString('yyyy-MM-dd HH:mm:ss')) ====" }

# ---- 抓取 / 渲染 解耦 ----
#   $doPull   : 是否抓取数据（PullOnly / WithPull / 数据超 150 分钟自动补抓）
#   $doRender : 渲染恒为真 —— 抓取轮与纯渲染轮都生成看板并刷新桌面挂件，且只执行一次
#   -PullOnly : 抓取 + 渲染（旧版只抓不渲染，按 m11077 修复）
#   -WithPull : 强制先抓，再渲染
$doPull = [bool]$PullOnly -or [bool]$WithPull
$doRender = $true
if (-not $doPull) {
  $ia = Join-Path $ROOT 'research\数据完整性.json'
  $ageMinD = 1e9
  if (Test-Path $ia) { try { $atStr = (Get-Content $ia -Raw | ConvertFrom-Json).at; if ($atStr) { $ageMinD = ((Get-Date) - [datetime]$atStr).TotalMinutes } } catch {} }
  if ($ageMinD -gt 150) { $doPull = $true }
}
if ($doPull) {
# 0) 每周一次全量重建：距上次 fresh 超过 7 天，本次自动 --fresh（宁可多抓）
$emStat = Join-Path $EMDIR 'em_full_pull_stats.json'
$needFresh = $Fresh
if (-not $needFresh -and (Test-Path $emStat)) {
  try {
    $st = Get-Content $emStat -Raw | ConvertFrom-Json
    $lastFresh = $null
    if ($st.fresh -eq $true -and $st.at) { $lastFresh = [datetime]$st.at }
    if (-not $lastFresh -or ((Get-Date) - $lastFresh.ToLocalTime()).TotalDays -ge 7) { $needFresh = $true }
  } catch { $needFresh = $true }
}
if ($needFresh -and -not $Quiet) { Write-Host "  (本次走 E-Mobile 全量重建)" }

# 0b) cookie 自动续期（无人值守）：探测 → 失效则拉起企业微信客户端让它自己续期 → 重探
#     成功/失败都写 research\cookie_renew.log 与 research\cookie_expiry_log.csv
$t = Get-Date
$rot = Join-Path $ROOT 'research\wxdoc_autorenew.ps1'
$oRenew = ''
$okRenew = $false
if (Test-Path $rot) {
  $oRenew = & $PWSH -NoProfile -ExecutionPolicy Bypass -File $rot -Quiet 2>&1 | Out-String
  $okRenew = ($LASTEXITCODE -eq 0)
  # -Quiet 下脚本只往 cookie_renew.log 写，stdout 是空的：失败时把日志尾行带进 step detail，
  # 否则历史上 86 轮 fail 的 detail 全是空白，看不出是 node 缺失、探活失败还是客户端没起来。
  if (-not $okRenew -and [string]::IsNullOrWhiteSpace($oRenew)) {
    $lgPath = Join-Path $ROOT 'research\cookie_renew.log'
    if (Test-Path $lgPath) { $oRenew = ((Get-Content $lgPath -Tail 3 -Encoding utf8) -join ' / ') }
  }
} else { $oRenew = 'autorenew script missing' }
Add-Step 'cookie 自动续期' $t $okRenew (Tail $oRenew 2)

# 1) 微信表格：全部页签
#    主通道 = 企业微信 cookie 直连（抓取脚本自带「重抓 cookie + 动态发现页签」）
#    回退链：主通道失败 → wxdoc_pull_all.mjs（复用磁盘 cookie）。EGO 浏览器通道已删除。
$t = Get-Date
$httpFile = Join-Path $WXDIR 'ALL_HTTP_TEXT.txt'
$wxPull = Join-Path $EMDIR 'wxwork_sheet_pull.js'
$wxText = Join-Path $WXDIR 'ALL_HTTP_TEXT_WXWORK.txt'
$o = ''
if (Test-Path $wxPull) {
  $o = Invoke-Retry -Times 2 -Body { & $NODE $wxPull 2>&1 | Out-String }
  if ((Test-Path $wxText) -and ((Get-Item $wxText).LastWriteTime -ge $t)) { Copy-Item $wxText $httpFile -Force }
}
# 成功判据必须认「收尾行」OK sheets=N/N（N 与 N 相等），不能只认 sheets=数字：
# 采集脚本在页签发现阶段就会打印 [discover] ... sheets=19，全页签失败（ok=0/20）时那行照样出现，
# 旧判据 $o -match 'sheets=\d+' 因此把整轮失败记成 ok，回退通道也不会触发（2026-11-19 09:17、11-20 09:55 各复现一次）。
$m1 = [regex]::Match($o, 'OK sheets=(\d+)/(\d+) bytes=(\d+)')
$ok1 = (Test-Path $httpFile) -and ((Get-Item $httpFile).LastWriteTime -ge $t) -and $m1.Success -and ($m1.Groups[1].Value -eq $m1.Groups[2].Value) -and ([int]$m1.Groups[3].Value -gt 0)
if (-not $ok1) {
  Write-Host '  (cookie 直连失败，回退 wxdoc_pull_all)'
  $o2 = Invoke-Retry -Times 1 -Body { & $NODE (Join-Path $ROOT 'research\wxdoc_pull_all.mjs') 2>&1 | Out-String }
  # 回退通道同样不能只看「出现 ok=」：全失败时也会打印 ok=0/20
  $m2 = [regex]::Match($o2, 'DONE \d+ms \| ok=(\d+)/(\d+) \|')
  $ok1 = (Test-Path $httpFile) -and ((Get-Item $httpFile).LastWriteTime -ge $t) -and $m2.Success -and ($m2.Groups[1].Value -eq $m2.Groups[2].Value) -and ([int]$m2.Groups[1].Value -gt 0)
  $o = $o + "`n--fallback2--`n" + $o2
}
if (-not $ok1) {
  $o = $o + "`n--fallback3 removed: 已废弃 EGO 通道，只保留 cookie 直连 + wxdoc_pull_all--"
}
Add-Step '微信表格·全页签(cookie直连)' $t $ok1 (Tail $o)

# 1b) 页签变动检测：新增/删除/改名页签 → research\sheets_diff.json（每轮把当轮清单变成新基线）
$t = Get-Date
$oDiff = Invoke-Retry -Times 1 -Body { & $NODE (Join-Path $ROOT 'research\sheet_diff.js') 2>&1 | Out-String }
Add-Step '页签变动检测' $t ($oDiff -match 'DIFF ') (Tail $oDiff 3)

# 1c) 机型对齐检测：表格里在跑的机型 vs 看板/台账收录（新增未收录机型 → 告警）
$t = Get-Date
$oProj = Invoke-Retry -Times 1 -Body { & $NODE (Join-Path $ROOT 'research\project_diff.js') 2>&1 | Out-String }
Add-Step '机型对齐检测' $t ($oProj -match 'PROJDIFF ') (Tail $oProj 3)

# 2) E-Mobile 增量（时间游标续抓 + 连续 2 页无新消息才收敛）
#    先看桌面客户端是否在线：E-Mobile 服务端同一账号只保留一个消息长连接，
#    我们一旦绑定就会把用户的桌面端顶下线。客户端在线 → 本轮跳过（表格照常抓）。
$t = Get-Date
$emOnline = $false
try {
  $sessJs = Join-Path $ROOT 'research\_em_session_state.cjs'
  if (Test-Path $sessJs) {
    $oSess = & $NODE $sessJs 2>&1 | Out-String
    if ($oSess -match '"isLogout":\s*false') { $emOnline = $true }
  }
} catch { }
# 说明（2026-11-11 实测结论）：
#   - 只做 XMPP 绑定 + 历史拉取，不会踢掉桌面客户端；
#   - 真正踢人的是 /emp/passport/login 与 /emp/api/msg/gettoken（服务端会重建消息会话，
#     调用后约 1 分钟内在线客户端断连）。所以本管道**永不调用这两步**，只用已存 msgconn.json 的令牌。
if ($false) {
} else {
  Push-Location $EMDIR
  $emArgs = @('em_full_pull.js'); if ($needFresh) { $emArgs += '--fresh' }
  try { $o = Invoke-Retry -Times 2 -Body { & $NODE $emArgs } } finally { Pop-Location }
  $ok2 = (Test-Path $emStat) -and ((Get-Item $emStat).LastWriteTime -ge $t) -and ($o -match 'END ALL')
  Add-Step 'E-Mobile·增量抓取' $t $ok2 (Tail $o 2)
}

# 3) 发言人姓名反查（用消息命名空间 simplelist 把「用户<id>」变成真名，先于重建聊天）
$t = Get-Date
Push-Location $EMDIR
try { $o = & $NODE 'em_names_by_id.js' 2>&1 | Out-String } finally { Pop-Location }
$nb = Join-Path $EMDIR 'names_by_id.json'
$okNames = ((Test-Path $nb) -and ((Get-Item $nb).LastWriteTime -ge $t)) -or ($o -match 'cached ok')
Add-Step 'E-Mobile·发言人姓名反查' $t $okNames (Tail $o 1)

# 3b) 重建聊天记录
$t = Get-Date
$cl = Join-Path $EMDIR 'project_chatlog.json'
Push-Location $EMDIR
try { $o = Invoke-Retry -Times 2 -Body { & $NODE 'build_chatlog.js' } } finally { Pop-Location }
Add-Step 'E-Mobile·重建聊天记录' $t ((Test-Path $cl) -and ((Get-Item $cl).LastWriteTime -ge $t)) (Tail $o 2)

# 4) 结构化汇总（已废弃：EGO 通道删除，cookie 直连已覆盖全部 19 页签文本，看板走台账）
$t = Get-Date
Add-Step '微信表格·结构化汇总' $t 'skip' '已废弃：EGO 通道已删除'

# 4b) 先落一版完整性快照（看板在第 5 步读它做「数据时间」，最终第 6 步会再覆盖一次带告警的版本）
try {
  $sheetN = 0; $msgN = 0; $grpN = 0
  try { $ws = Get-Content (Join-Path $WXDIR 'wxwork_pull_stats.json') -Raw -Encoding UTF8 | ConvertFrom-Json; $sheetN = [int]$ws.sheets } catch {}
  try { $es = Get-Content $emStat -Raw -Encoding UTF8 | ConvertFrom-Json; $msgN = [int]$es.totalMessages; $grpN = [int]$es.groups } catch {}
  $lines = 0; $txt = Join-Path $WXDIR 'ALL_HTTP_TEXT.txt'
  if (Test-Path $txt) { $lines = (Get-Content $txt -Encoding UTF8 | Measure-Object).Count }
  [ordered]@{ at = (Get-Date).ToString('o'); sheets = $sheetN; httpLines = $lines; groups = $grpN; messages = $msgN } |
    ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $ROOT 'research\数据完整性.json')
} catch { }

# 4c) 文档节点 × 群聊口径 交叉校验：由 build_daily_brief.mjs 渲染时基于「当轮 ALL」现算，
#     结果写 research\progress_conflicts.json。此处不再单独跑 progress_crosscheck.mjs——
#     那个脚本读的是上一轮 项目数据看板.html，在渲染前跑会滞后一轮，且 -PullOnly 时没有看板可纠偏。
$t = Get-Date
Add-Step '进度交叉校验(随看板现算)' $t 'skip' '由 build_daily_brief.mjs 基于当轮文档节点现算，结果写 research\progress_conflicts.json'

# 4d) 抓取完刷新挂件数据（exe 常驻读 widget_data.json，不必等 09:05 汇总）
$t = Get-Date
$o = Invoke-Retry -Times 1 -Body { & $NODE (Join-Path $ROOT 'research\build_widget.mjs') }
Add-Step '刷新挂件数据' $t (Test-Path (Join-Path $ROOT 'research\widget_data.json')) (Tail $o 1)

}  # end doPull

# ---- 渲染（恒执行，仅一次）----

# 5) 生成看板
$t = Get-Date
$o = Invoke-Retry -Times 2 -Body { & $NODE (Join-Path $ROOT 'research\build_daily_brief.mjs') }
$html = Join-Path $ROOT '最新项目简报.html'
$board = Join-Path $ROOT '项目数据看板_生成.html'

Add-Step '生成 HTML/MD 看板' $t ((Test-Path $html) -and (Get-Item $html).LastWriteTime -ge $t) (Tail $o 2)
# 旧的「老 UI」中间页如仍存在，顺手清掉（2026-11-16 起不再产出）
$oldGen = Join-Path $ROOT '项目数据看板_生成.html'
if (Test-Path $oldGen) { Remove-Item $oldGen -Force -ErrorAction SilentlyContinue }

# 5a) 重建 V4 看板（品牌分布 / 已归档 / 里程碑真值）：表格 → 解析 → 归一 → 注入 → 圆环
#     四个脚本必须逐个查 $LASTEXITCODE；旧版只看 $v4 的 mtime，任一脚本报错但
#     上一轮成品还在，5a 仍报 ok（审核 2026-11-14 坐实的假绿）。
$t = Get-Date
# python 解析顺序：config.paths.python → 本机 venv → 随项目分发的便携版 → PATH
$PY = $null
$cfgPy = $null
try { $cfgPy = (Get-Content -Raw (Join-Path $ROOT 'research\config.json') -ErrorAction Stop | ConvertFrom-Json).paths.python } catch {}
foreach ($cand in @($cfgPy, '<TOOLS>\ppt-master-venv\Scripts\python.exe', (Join-Path $ROOT 'tools\python\python.exe'))) {
  if ($cand -and (Test-Path -LiteralPath $cand)) { $PY = $cand; break }
}
if (-not $PY) { $PY = (Get-Command python -ErrorAction SilentlyContinue).Source }
if (-not $PY) { $PY = 'python' }
$v4 = Join-Path $ROOT '项目数据看板.html'
$oV4 = ''
$v4Ok = $true
foreach ($s4 in @('sheet_parse.py','sheet_canon.py','_v4_rebuild.py','_kpi_pop.py')) {
  $a4 = @('-X','utf8',(Join-Path $ROOT ('research\' + $s4))); if ($s4 -eq '_v4_rebuild.py') { $a4 += '--inplace' }
  $one = (& $PY @a4 2>&1 | Out-String)
  $oV4 = $oV4 + $one
  if ($LASTEXITCODE -ne 0) {
    $v4Ok = $false
    $oV4 = $oV4 + "`n[$s4 EXIT=$LASTEXITCODE]"
  }
}
Add-Step '重建 V4 看板(品牌/归档/里程碑)' $t ($v4Ok -and (Test-Path $v4) -and ((Get-Item $v4).LastWriteTime -ge $t)) (Tail $oV4 2)

# 5a2) 看板自检：日期戳 / 结构 / 柱数字与节点数口径；不合格则用模板重置并重建
$t = Get-Date
$gOut = (& $PY -X utf8 (Join-Path $ROOT 'research\_page_guard.py') 2>&1 | Out-String)
$gOk = ($LASTEXITCODE -eq 0)
Add-Step '看板自检(日期/结构/口径)' $t $gOk (Tail $gOut 2)
# 5a3) 节点覆盖交叉验证：表(sheet_canon) × CLIENT_A 报告 × 看板 __DRAW × 群聊；hard>0 记为失败但继续跑
$t = Get-Date
$cOut = (& $PY -X utf8 (Join-Path $ROOT 'research\node_coverage.py') 2>&1 | Out-String)
$cOk = ($LASTEXITCODE -eq 0)
Add-Step '节点覆盖交叉验证(表/报告/看板)' $t $cOk (Tail $cOut 2)
# 5a4) 会议安排数据（Ecology 日历，内存取 Cookie）
$mtFetch = Join-Path $PSScriptRoot 'research\ec_meeting_fetch.py'
if (Test-Path $mtFetch) {
    $t = Get-Date
    $mtOut = & $PY -X utf8 $mtFetch 2>&1
    $mtOk = ($LASTEXITCODE -eq 0) -and (($mtOut -join "`n") -match "MEETING OK")
    Add-Step '会议安排数据(当日/当周/当月)' $t $mtOk (Tail $mtOut 2)
# 5a5) 企业微信未读（只读：复制库副本+离线解密）+ 三源交叉验证（文档 × EM 群聊 × 企微群聊）
$t = Get-Date
$wOk = $false; $oW = ''
try {
  $wxB = Join-Path $ROOT 'research\wxwork_unread_build.py'
  if ((Test-Path $wxB) -and $PY) {
    $oW = (& $PY -X utf8 $wxB --hours 24 --json (Join-Path $ROOT 'research\wxwork_unread.json')) 2>&1 | Out-String
    $wOk = ($LASTEXITCODE -eq 0)
    $cx = Join-Path $ROOT 'research\wx_crosscheck.py'
    if ($wOk -and (Test-Path $cx)) { $oW += (& $PY -X utf8 $cx) 2>&1 | Out-String }
  } else { $oW = 'wxwork_unread_build.py 或 python 缺失，跳过' }
} catch { $oW = $_.Exception.Message }
Add-Step '企微未读+三源交叉验证' $t $wOk (Tail $oW 2)
} else {
    Write-Host '  [SKIP] 缺 research\ec_meeting_fetch.py'
}

# 5a6) 企微日历（只读解密本地 calendar_r7.db）→ 并入会议数据
$t = Get-Date
$cOk = $false; $oC = ''
try {
    $calD = Join-Path $ROOT 'research\wxwork_calendar.py'
    $calM = Join-Path $ROOT 'research\wxwork_calendar_merge.py'
    if ((Test-Path $calD) -and (Test-Path $calM) -and $PY) {
        $oC = (& $PY -X utf8 $calD) 2>&1 | Out-String
        $oC += (& $PY -X utf8 $calM) 2>&1 | Out-String
        $cOk = ($LASTEXITCODE -eq 0)
    } else { $oC = 'wxwork_calendar*.py 或 python 缺失，跳过' }
} catch { $oC = $_.Exception.Message }
# CAL_EMPTY = 缓存里没有未来日程，属正常空结果（脚本自己返回 0）；
# 旧版仍按 batch 收尾码判 ok，偶发被记成 fail，制造假告警。
if ($oC -match 'CAL_EMPTY') { $cOk = $true }
Add-Step '企微日历并入会议' $t $cOk (Tail $oC 2)

# 5a7) 企微待办并入部门进度（只在取证文件存在时执行，幂等）
$mtTodo = Join-Path $ROOT 'research\wxwork_todo.json'
# 先从本地库导出企微待办（只读，无需登录态）
$mtLocal = Join-Path $ROOT 'research\wxwork_todo_local.py'
if ((Test-Path $mtLocal) -and $PY) {
    try { (& $PY -X utf8 $mtLocal --days 180) 2>&1 | Out-Null } catch { }
}
$mtMerge = Join-Path $ROOT 'research\wxwork_todo_merge.py'
if ((Test-Path $mtTodo) -and (Test-Path $mtMerge) -and $PY) {
    $t = Get-Date
    $oM = ''
    try { $oM = (& $PY -X utf8 $mtMerge) 2>&1 | Out-String } catch { $oM = $_.Exception.Message }
    Add-Step '企微待办并入部门进度' $t ($LASTEXITCODE -eq 0) (Tail $oM 1)
} else {
    Write-Host '  [SKIP] 无企微待办取证文件（未登录或尚未捕获）'
}

# 5b) 刷新桌面看板：重建 widget_data.json，并按需拉起「项目看板」(Rust 版)
#     看板进程按文件 mtime 每 30s 自动重载，所以数据刷新后不必重启；只在没在跑时拉起。
#     -NoWidgetRestart 时完全不动进程（给 t7 等自动化用）。WPF 旧流程已作废，保留 widget_show.ps1 作 fallback。
$t = Get-Date
$o = Invoke-Retry -Times 1 -Body { & $NODE (Join-Path $ROOT 'research\build_widget.mjs') }
$wHtml = Join-Path $ROOT '桌面挂件.html'
$widgetData = Join-Path $ROOT 'research\widget_data.json'
# 挂件 exe：环境变量可覆盖，其次探测项目内 rust-widget 产物，本机保留 <TOOLS> 回退
$boardExe = $env:BOARD_EXE
if (-not $boardExe -or -not (Test-Path $boardExe)) {
  # 解析顺序：<ROOT>\tools（套件安装位）→ 构建产物 → 本机部署位
  $probe = Join-Path $ROOT 'tools\项目看板.exe'
  if (-not (Test-Path $probe)) { $probe = Join-Path $ROOT 'rust-widget\target\release\项目看板.exe' }
  if (-not (Test-Path $probe)) { $probe = '<TOOLS>\项目看板\项目看板.exe' }
  $boardExe = $probe
}
$wpfFallback = Join-Path $ROOT 'research\widget_show.ps1'
if (-not (Test-Path $wpfFallback)) { $wpfFallback = '' }
if ($NoWidgetRestart) {
  $o = "$o | widget restart skipped (-NoWidgetRestart)"
} elseif (Test-Path $boardExe) {
  # 与自启「项目看板.lnk」一致：--mode float --data/--state/--log（state/log 落 exe 同目录）
  $exeDir = Split-Path -Parent $boardExe
  $widgetState = Join-Path $exeDir 'widget_state.json'
  $widgetLog   = Join-Path $exeDir 'rust-widget.log'
  $running = @(Get-Process -Name '项目看板' -ErrorAction SilentlyContinue)
  if ($running.Count -gt 0) {
    # 已有实例：先停掉再拉起。数据文件已更新，进程内 30s 轮询也能重载，
    # 但跨会话/跨身份启动可能漏检（实测见过两个窗口），统一按幂等处理。
    $running | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 700
    $o = "$o | board restarted (killed $($running.Count))"
  } else {
    $o = "$o | board cold start"
  }
  $boardArgs = @('--mode','normal','--data',('"' + $widgetData + '"'),'--state',('"' + $widgetState + '"'),'--log',('"' + $widgetLog + '"'))
  Start-Process -FilePath $boardExe -ArgumentList $boardArgs -WindowStyle Hidden
  $o = "$o | board up"
} elseif (Test-Path $wpfFallback) {
  Get-CimInstance Win32_Process -Filter "Name='pwsh.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -like '*widget_show.ps1*' } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 300
  Start-Process $PWSH -ArgumentList '-NoProfile','-WindowStyle','Hidden','-File',$wpfFallback,'-X','24','-Y','816' -WindowStyle Hidden
  $o = "$o | WPF fallback used"
}
Add-Step '刷新桌面看板' $t ((Test-Path $wHtml) -and (Get-Item $wHtml).LastWriteTime -ge $t) (Tail $o 2)

# 6) 完整性自检：各源条数 + 新鲜度（抓取侧写盘；只渲染时回读上次结果）
if ($doPull) {
$sheets = 0; $httpLines = 0; $httpBytes = 0
if (Test-Path $httpFile) {
  $txt = Get-Content $httpFile -Raw
  $sheets = ([regex]::Matches($txt, '(?m)^===== ')).Count
  $httpLines = ($txt -split "`n").Count
  # 行数是双峰口径（同一份数据按单元格/按行合并会差 3 倍以上，字节数不变），
  # 当判据会既误红又形同虚设；主判据用字节数，行数只写进 alert 文案便于对照。
  $httpBytes = (Get-Item $httpFile).Length
}
$groups = 0; $msgs = 0; $newMsgs = 0; $fullAt = ''
try {
  $st = Get-Content $emStat -Raw | ConvertFrom-Json
  $groups = $st.groups; $msgs = $st.totalMessages; $newMsgs = $st.newMessages
} catch { [void]$alerts.Add('E-Mobile 统计缺失') }
try {
  $cj = Get-Content $cl -Raw | ConvertFrom-Json
  $msgs2 = ($cj | Measure-Object -Property count -Sum).Sum
  if ($msgs2) { $msgs = $msgs2 }
} catch { [void]$alerts.Add('聊天记录无法解析') }

function Age($p) { if (Test-Path $p) { return [math]::Round(((Get-Date) - (Get-Item $p).LastWriteTime).TotalHours, 1) } else { return 999 } }
$ageWx = Age $httpFile; $ageEm = Age $cl
if ($ageWx -gt 48) { [void]$alerts.Add("微信表格快照 $ageWx h 未更新") }
if ($ageEm -gt 48) { [void]$alerts.Add("聊天记录 $ageEm h 未更新") }
if ($sheets -lt 18) { [void]$alerts.Add("页签数 $sheets < 18") }
$httpKB = [math]::Round($httpBytes / 1024, 0)
if ($httpBytes -lt 120000) { [void]$alerts.Add("表格文本仅 $httpKB KB / $httpLines 行（疑似解析不完整；正常约 145 KB）") }

# 页签变动：新增/删除/改名都要显式报出来（页签变动 = 项目/表结构变动）
$sdFile = Join-Path $ROOT 'research\sheets_diff.json'
$sdAdded = 0; $sdRemoved = 0; $sdRenamed = 0; $sdFirst = $true; $sdBaseAt = ''
if (Test-Path $sdFile) {
  try {
    $sd = Get-Content $sdFile -Raw | ConvertFrom-Json
    $sdFirst = [bool]$sd.firstRun
    $sdAdded = @($sd.added).Count
    $sdRemoved = @($sd.removed).Count
    $sdRenamed = @($sd.renamed).Count
    $sdBaseAt = [string]$sd.baselineAt
  } catch { [void]$alerts.Add('sheets_diff.json 无法解析') }
}
if (-not $sdFirst) {
  if ($sdAdded -gt 0) {
    $nm = (@($sd.added) | ForEach-Object { $_.name }) -join '、'
    [void]$alerts.Add("页签新增 $sdAdded 个：$nm（已自动抓取，需确认是否补进台账）")
  }
  if ($sdRemoved -gt 0) {
    $nm = (@($sd.removed) | ForEach-Object { $_.name }) -join '、'
    [void]$alerts.Add("页签减少 $sdRemoved 个：$nm（项目被删或表结构变更）")
  }
  if ($sdRenamed -gt 0) { [void]$alerts.Add("页签改名 $sdRenamed 个") }
}

# 机型对齐：表格新出现但看板没收录 → 可能漏了新项目
$pdFile = Join-Path $ROOT 'research\projects_diff.json'
$pdMissing = 0; $pdNew = @(); $pdNames = @()
if (Test-Path $pdFile) {
  try {
    $pd = Get-Content $pdFile -Raw | ConvertFrom-Json
    $pdMissing = @($pd.missingOnBoard).Count
    $pdNames = @($pd.missingOnBoard | ForEach-Object { $_.model })
    $pdNew = @($pd.newlyAppeared)
  } catch { [void]$alerts.Add('projects_diff.json 无法解析') }
}
# 只要表格在跑、看板/台账没有，就一直报（直到补进台账）——按 newlyAppeared 只报一次，补录前就静默了：
# 2026-11-20 那轮 ITEM_X 就是这样消失的。最多列 5 个，其余给个数。
if ($pdMissing -gt 0) {
  $shown = @($pdNames | Select-Object -First 5) -join '、'
  $more = ''
  if ($pdMissing -gt 5) { $more = " 等 $pdMissing 个" }
  $tag = ''
  if ($pdNew.Count -gt 0) { $tag = "（本期新增：$($pdNew -join '、')）" }
  [void]$alerts.Add("表格在跑但看板/台账未收录：$shown$more$tag —— 请确认是否新项目 / 补进台账")
}
$cookieAlert = Join-Path $ROOT 'research\cookie_alert.txt'
if (Test-Path $cookieAlert) {
  $ca = (Get-Content $cookieAlert -Raw).Trim()
  if ($ca) { [void]$alerts.Add("cookie 失效未自愈：$ca") }
}

# cookie 剩余天数：<3 天提醒、<1 天按硬故障；数值同时写进数据完整性，便于回溯
$cookieDays = $null
try {
  $cexp = Join-Path $ROOT 'research\cookie_expiry_log.csv'
  if (Test-Path $cexp) {
    $lastRow = Import-Csv -LiteralPath $cexp | Where-Object { $_.days_left } | Select-Object -Last 1
    if ($lastRow) { $cookieDays = [math]::Round([double]$lastRow.days_left, 1) }
  }
} catch { }
if ($null -ne $cookieDays) {
  if ($cookieDays -lt 1) { [void]$alerts.Add("cookie 剩余不足 1 天（$cookieDays 天），立即手工续期") }
  elseif ($cookieDays -lt 3) { [void]$alerts.Add("cookie 剩余 $cookieDays 天，续期可能没生效") }
}

# 挂件版本：本机 exe 与最近一次发布上传的 exe 不一致时提示，避免长期跑旧构建
$wExp = Join-Path $ROOT 'research\widget_expected.json'
if ((Test-Path $probe) -and (Test-Path $wExp)) {
  try {
    $exp = Get-Content -LiteralPath $wExp -Raw -Encoding UTF8 | ConvertFrom-Json
    $hNow = (Get-FileHash -LiteralPath $probe -Algorithm SHA256).Hash.ToLower()
    if ($exp.sha256 -and $hNow -ne $exp.sha256) {
      $mb = [math]::Round((Get-Item -LiteralPath $probe).Length / 1KB)
      $em = [math]::Round($exp.size / 1KB)
      [void]$alerts.Add("挂件本机构建与 $($exp.tag) 发布版不同（$mb KB vs $em KB），重装可按发布版对齐")
    }
  } catch { }
}

$integrity = [ordered]@{
  at        = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
  cookieDaysLeft = $cookieDays
  sheets    = $sheets
  httpLines = $httpLines
  groups    = $groups
  messages  = $msgs
  newMsgs   = $newMsgs
  ageWxH    = $ageWx
  ageEmH    = $ageEm
  fresh     = [bool]$needFresh
  sheetsAdded   = $sdAdded
  sheetsRemoved = $sdRemoved
  sheetsRenamed = $sdRenamed
  sheetsBaseAt  = $sdBaseAt
  projMissing   = $pdMissing
  projNew       = @($pdNew)
  alerts    = @($alerts)
}
$integrity | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $ROOT 'research\数据完整性.json')
} else {
  try {
    $integrity = Get-Content (Join-Path $ROOT 'research\数据完整性.json') -Raw | ConvertFrom-Json
    $sheets = [int]$integrity.sheets; $groups = [int]$integrity.groups; $msgs = [int]$integrity.messages; $newMsgs = [int]$integrity.newMsgs
    $alerts = @($integrity.alerts)
  } catch { }
}

$totalSec = [math]::Round(((Get-Date) - $T0).TotalSeconds, 1)
$runRecord = [ordered]@{ started = $T0.ToString('yyyy-MM-dd HH:mm:ss'); totalSec = $totalSec; fresh = [bool]$needFresh; pullOnly = [bool]$PullOnly
  integrity = $integrity; steps = @($steps); alerts = @($alerts) }
$runRecord | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $ROOT 'research\brief_run_status.json')
# 逐步耗时可复现：每轮追加一行压缩 JSON（按日累积），纯记录、不覆盖、不碰 brief_focus.json 业务口径
try {
  $histLine = $runRecord | ConvertTo-Json -Depth 8 -Compress
  Add-Content -Encoding UTF8 -LiteralPath (Join-Path $ROOT 'research\brief_run_history.jsonl') -Value $histLine
} catch { }

if (-not $Quiet) {
  Write-Host ("==== DONE {0}s | 页签 {1} · 群 {2} · 消息 {3} · 新增 {4} · 告警 {5} ====" -f $totalSec, $sheets, $groups, $msgs, $newMsgs, $alerts.Count)
  if ($alerts.Count) { Write-Host ("  !! " + ($alerts -join ' / ')) }
}
if ($doRender -and -not $NoOpen) { Start-Process $board }
