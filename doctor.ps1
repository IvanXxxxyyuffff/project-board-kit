# doctor.ps1 — 环境自检（装完先跑这个；有 FAIL 就别继续，先按 SKILL.md 第 9 章排错）
# 用法：pwsh -NoProfile -ExecutionPolicy Bypass -File doctor.ps1
$ErrorActionPreference = 'Continue'
$rows = @()
function T($name, $ok, $detail, [switch]$Optional) {
  $script:rows += [pscustomobject]@{
    检查 = $name
    结果 = if ($ok) { 'OK' } elseif ($Optional) { 'WARN' } else { 'FAIL' }
    详情 = $detail
  }
}
function Port($host_, $port) {
  try {
    $c = New-Object Net.Sockets.TcpClient
    $ok = $c.ConnectAsync($host_, $port).Wait(2500)
    $c.Close(); return [bool]$ok
  } catch { return $false }
}

# 0. 定位安装根目录（本脚本在 <root>\ 下）
$root = $PSScriptRoot
if (-not (Test-Path (Join-Path $root 'research\config.json'))) {
  $up = Split-Path $root -Parent
  if (Test-Path (Join-Path $up 'research\config.json')) { $root = $up }
}
Write-Host "安装根目录：$root`n"

# 1. 配置
$cfgP = Join-Path $root 'research\config.json'
$em = $null
if (Test-Path $cfgP) {
  try { $cfg = Get-Content $cfgP -Raw -Encoding UTF8 | ConvertFrom-Json; $em = $cfg.emobile; T 'config.json' $true $cfgP }
  catch { T 'config.json' $false "解析失败：$_" }
} else { T 'config.json' $false "缺 $cfgP（先跑 setup.ps1）" }

# 1b. 脚本里写死的根目录是否 = 当前安装目录（整个目录被搬动/改名过会不一致）
$probe = Join-Path $root 'research\build_daily_brief.mjs'
if (Test-Path $probe) {
  $txt = [IO.File]::ReadAllText($probe)
  # 代码里路径多写成双反斜杠/正斜杠，统一成单反斜杠再比
  $norm = $txt.Replace('/','\').Replace('\\','\')
  # 现役脚本用 __dirname / PSScriptRoot 相对定位，不含绝对安装路径；
  # 因此「含 $root」不再是唯一正确信号，检测到相对定位也算一致（审核 2026-11-14）。
  $relOk = ($txt -match '__dirname|PSScriptRoot|Get-Location|dirname\(') -or $norm.Contains($root)
  T '脚本根目录一致' $relOk $(if ($relOk) { '相对定位 / ' + $root } else { "脚本仍指向旧目录 —— 跑：pwsh -File setup.ps1 -RepairPath -Config .\config.json" })
}

# 2. 运行时
$node = (Get-Command node -ErrorAction SilentlyContinue).Source
$nv = if ($node) { (& node -v) 2>$null } else { '' }
T 'node(>=18)' ([bool]$node) $(if ($node) { "$node $nv" } else { '没找到 node —— 装 Node LTS 并保证在 PATH' })
T 'pwsh 7' ($PSVersionTable.PSVersion.Major -ge 7) "$($PSVersionTable.PSVersion)"

# 3. E-Mobile 连通性（7070 是消息服务，独立于 8999）
if ($em) {
  $h = $em.host; $p1 = if ($em.httpPort) { [int]$em.httpPort } else { 8999 }
  T "E-Mobile $p1(HTTP)" (Port $h $p1) "${h}:$p1（登录/通讯录/simplelist 走这里）"
  T 'E-Mobile 7070(WS)' (Port $h 7070) "${h}:7070（SCRAM + 群/私聊历史走这里）"
  if (-not $em.myUserId) { T 'config.myUserId' $false '没填 —— 先跑 em_contacts_refresh.js 看 [login] ok uid=?' }
}

# 4. 企业微信客户端（cookie 通道的唯一凭据来源）
$wx = Get-Process WXWork -ErrorAction SilentlyContinue
T '企业微信进程' ([bool]$wx) $(if ($wx) { "pid=$($wx.Id -join ',')" } else { '未运行 —— 启动并登录一次；之后 cookie 由管道自动续期' })

# 5. 微信文档登录态（真探活：跑 --check，看退出码，不看 HTTP 200）
$pull = Join-Path $root 'research\raw\emobile\wxwork_sheet_pull.js'
if ((Test-Path $pull) -and $node) {
  $o = (& node $pull --check 2>&1 | Out-String)
  $ok = ($LASTEXITCODE -eq 0)
  T '微信文档 cookie' $ok (($o -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -Last 2) -join ' | ')
} else { T '微信文档 cookie' $false "缺 $pull" }

# 6. 可选：Rust（只有要做桌面 exe 才需要）
$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
T 'cargo(可选)' ([bool]$cargo) $(if ($cargo) { (& cargo --version) 2>$null } else { '没装 —— 只要网页看板可忽略；要 exe 需 rustup + x86_64-pc-windows-gnu' }) -Optional

# 7. 可选：待办同步源（小黄条 / YYNote，没装也能跑）
$yy = Test-Path (Join-Path $env:USERPROFILE 'Documents\YYNote\yynote.db')
T '待办源(可选)' $yy $(if ($yy) { '发现 yynote.db，可跑 sync_yynote_todos.mjs' } else { '无待办软件，部门待办只能自建' }) -Optional

# 8. 看板挂件（exe 与进程，可选但装了就该跑起来）
$bin = Join-Path $root 'tools\项目看板.exe'
T '看板 exe' (Test-Path $bin) $(if (Test-Path $bin) { "$((Get-Item $bin).Length) B  $bin" } else { '缺 tools\项目看板.exe（挂件二进制不随仓库分发：先在 rust-widget 里 cargo build --release，再把产物复制到 tools\）' }) -Optional
$wp = Get-Process '项目看板' -ErrorAction SilentlyContinue
T '看板进程' ([bool]$wp) $(if ($wp) { "pid=$($wp.Id -join ',')  ws=$([math]::Round(($wp | Select-Object -First 1).WorkingSet64 / 1MB, 1))MB" } else { '未运行 —— 双击 tools\项目看板.exe（或重新登录）' }) -Optional

# 9. 小黄条导出链路（数据库 + e_sqlcipher.dll；缺一个就只能自建待办）
$yys = @("$env:LOCALAPPDATA\YYNoteVip\yynote.db", "$env:USERPROFILE\Documents\YYNote\yynote.db") | Where-Object { Test-Path $_ } | Select-Object -First 1
T '小黄条数据库' ([bool]$yys) $(if ($yys) { $yys } else { '未发现（可选；装了小黄条才有）' }) -Optional
$yyd = @('<ROOT>\YYNote\runtimes\win-x64\native', '<ROOT> (x86)\YYNote\runtimes\win-x64\native') | Where-Object { Test-Path (Join-Path $_ 'e_sqlcipher.dll') } | Select-Object -First 1
T '小黄条导出库' ([bool]$yyd) $(if ($yyd) { $yyd } else { '缺 e_sqlcipher.dll（随小黄条安装）' }) -Optional
# 10. 发消息能力（文本/图片，可选）
$sendJs = Join-Path $root 'research\raw\emobile\em_send_brief.js'
$nodeExe = (Get-Command node -ErrorAction SilentlyContinue).Source
$wsMod = Test-Path (Join-Path $root 'research\node_modules\ws\index.js')
$tokFile = Join-Path $root 'research\raw\emobile\api\access_live.json'
$tokOk = $false
if (Test-Path $tokFile) { try { $j = Get-Content $tokFile -Raw | ConvertFrom-Json; $tokOk = [bool]$j.access_token } catch { $tokOk = $false } }
T '发消息脚本' (Test-Path $sendJs) $(if (Test-Path $sendJs) { 'research\raw\emobile\em_send_brief.js' } else { '缺 em_send_brief.js（发消息脚本按需自建，见 SKILL「发消息」一节；不需要推送可不装）' }) -Optional
T '发消息依赖(可选)' ([bool]($nodeExe -and $wsMod)) ("node=$(if($nodeExe){'有'}else{'缺'}) / ws=$(if($wsMod){'有'}else{'缺'})") -Optional
T '发消息凭证(可选)' $tokOk $(if ($tokOk) { '已有 access_live.json（6 小时内有效）' } else { '没有；首次发消息会自动从 E-Mobile 客户端内存取' }) -Optional
$rows | Format-Table -AutoSize -Wrap
# 9. 企业微信只读采集（不注入、不改内存；未启用只告警）
$wxKey = Join-Path $ROOT 'research\_wxdec\_key.txt'
$wxScanner = Join-Path $ROOT 'research\wxwork_key_readonly.exe'
if ((Test-Path $wxKey) -and (Test-Path $wxScanner)) {
  $kk = (Get-Content $wxKey -Raw).Trim()
  if ($kk -match '^[0-9a-fA-F]{32}$') { T '企微库密钥' $true ("已启用 " + $kk.Substring(0,6)) }
  else { T '企微库密钥' $false '格式异常，请重跑 research\wxwork_enable.ps1' -Optional }
} else {
  T '企微库密钥' $false '未启用；需企微群聊数据时运行 research\wxwork_enable.ps1' -Optional
}
# 9b. 企微只读链路的三份产物：日历、待办取证脚本、待办数据（缺失只告警）
$calJson = Join-Path $ROOT 'research\wxwork_calendar.json'
if (Test-Path $calJson) {
  $calAge = [int]((Get-Date) - (Get-Item $calJson).LastWriteTime).TotalHours
  T '企微日历数据' ($calAge -lt 48) ("$calAge 小时前更新（超过 48h 说明客户端未同步）") -Optional
} else {
  T '企微日历数据' $false '尚未导出；运行 research\wxwork_calendar.py' -Optional
}
$capScript = Join-Path $ROOT 'research\wxwork_todo_capture.py'
T '企微待办取证脚本' (Test-Path $capScript) $(if (Test-Path $capScript) { '已就位（只读内存，零输入事件）' } else { '缺 research\wxwork_todo_capture.py' })
$todoJson = Join-Path $ROOT 'research\wxwork_todo.json'
if (Test-Path $todoJson) {
  $tdAge = [int]((Get-Date) - (Get-Item $todoJson).LastWriteTime).TotalHours
  T '企微待办数据' ($tdAge -lt 72) ("$tdAge 小时前捕获") -Optional
} else {
  $wxn = (Get-Process -Name WXWorkWeb -ErrorAction SilentlyContinue | Measure-Object).Count
  T '企微待办数据' $false $(if ($wxn -gt 0) { '未捕获；客户端已登录，等它拉一次待办后跑 wxwork_todo_capture.py' } else { '未捕获；企微当前不在登录态' }) -Optional
}
$fail = ($rows | Where-Object { $_.结果 -eq 'FAIL' }).Count
Write-Host ""
if ($fail -eq 0) { Write-Host "全部通过，可以开始抓取。" -ForegroundColor Green }
else { Write-Host "$fail 项未通过 —— 对照 SKILL.md 第 9 章排错表逐项修。" -ForegroundColor Yellow }
# ---------- 脚本一致性：与套件自带 manifest.json 比对 ----------
try {
  # setup.ps1 把 files\* 复制到 <root>，安装后清单在 <root>\manifest.json；
  # 未安装（直接跑套件内 doctor）时才在 $PSScriptRoot\files\manifest.json。
  $mani = Join-Path $root 'manifest.json'
  if (-not (Test-Path $mani)) { $mani = Join-Path $PSScriptRoot 'files\manifest.json' }
  if (Test-Path $mani) {
    $m = Get-Content $mani -Raw -Encoding UTF8 | ConvertFrom-Json
    $diff = @()
    $keys = @($m.files.PSObject.Properties.Name)
    foreach ($k in $keys) {
      $rel2 = $k -replace '^files/', ''
      # 优先比对套件源（manifest 描述的是它），再退回安装根
      $cands = @(
        (Join-Path $ROOT ('skills\project-board-kit\' + ($k -replace '/', '\'))),
        (Join-Path $PSScriptRoot ($k -replace '/', '\')),
        (Join-Path $ROOT $rel2)
      )
      $f = $cands | Where-Object { Test-Path $_ } | Select-Object -First 1
      if (-not $f) { $diff += "$k (缺失)"; continue }
      if (!(Test-Path $f)) { $diff += "$k (缺失)"; continue }
      $bytes = [IO.File]::ReadAllBytes($f)
$txt = [Text.Encoding]::UTF8.GetString($bytes)
if ($txt.Length -gt 0 -and [int][char]$txt[0] -eq 0xFEFF) { $txt = $txt.Substring(1) }
$txt = $txt -replace "`r`n", "`n"
# 反向还原安装路径：setup.ps1 会把占位符改写成本机安装路径，
# 清单哈希按占位符算，所以比对前先把三种写法还原。
# 占位符用拼接生成，避免将来被外部批量改写吃掉。
$PH = '<' + 'ROOT>'
if ($ROOT) {
  foreach ($form in @($ROOT.Replace('\', '\\'), $ROOT, $ROOT.Replace('\', '/'))) {
    if ($form -and $form.Length -gt 3) { $txt = [regex]::Replace($txt, [regex]::Escape($form), $PH, 'IgnoreCase') }
  }
}
$norm = [Text.Encoding]::UTF8.GetBytes($txt)
$sha = [Security.Cryptography.SHA256]::Create()
$h = ([BitConverter]::ToString($sha.ComputeHash($norm)) -replace '-','').ToLower()
      if ($h -ne $m.files.$k.sha256) { $diff += "$k (被改过)" }
    }
    if ($diff.Count -eq 0) { Write-Host ('  [OK]   脚本与套件清单一致 ({0} 个文件)' -f $keys.Count) -ForegroundColor Green }
    else { Write-Host ('  [WARN] 脚本与套件清单不一致：' + ($diff -join '，') + '（可能是本地改过；不影响抓取）') -ForegroundColor Yellow }
  }
} catch { Write-Host "  [WARN] 清单比对跳过：$($_.Exception.Message)" -ForegroundColor Yellow }

# 会议数据链路（可选能力：需要 em_cookie_scan.exe 与已登录的 E-Mobile 客户端）
$ckExe  = Join-Path $PSScriptRoot 'research\em_cookie_scan.exe'
$ckData = Join-Path $PSScriptRoot 'meeting_data.json'
T '会议Cookie采集器' (Test-Path $ckExe) '缺 research\em_cookie_scan.exe（重跑 setup.ps1 或 csc /out:… research\em_cookie_scan.cs）' -Optional
if (Test-Path $ckData) {
  $mAge = [int]((Get-Date) - (Get-Item $ckData).LastWriteTime).TotalMinutes
  T '会议数据(meeting_data.json)' ($mAge -lt 180) "已 $mAge 分钟未更新（挂件每 10 分钟自刷新；先确认客户端已登录）" -Optional
} else {
  T '会议数据(meeting_data.json)' $false '尚未生成（首次自刷新需要客户端登录）' -Optional
}

exit $fail
