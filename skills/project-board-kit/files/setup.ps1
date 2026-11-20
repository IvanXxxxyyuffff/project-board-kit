# setup.ps1 — 把 project-board-kit 安装到本机（保持原目录结构，脚本零改动即可跑）
# 用法：pwsh -NoProfile -ExecutionPolicy Bypass -File setup.ps1 -Config .\config.json [-Force]
param(
  [string]$Config = (Join-Path $PSScriptRoot 'config.json'),
  [string]$Root = '',      # 安装根目录（优先于 config.installRoot）
  [switch]$Force,
  [switch]$RepairPath,
  [switch]$NoAutostart   # 演练/多实例安装时跳过开机自启与自动启动
)
$ErrorActionPreference = 'Stop'
$Kit = $PSScriptRoot
if (-not (Test-Path $Config)) {
  $ex = Join-Path $Kit 'config.example.json'
  if (Test-Path $ex) {
    Copy-Item $ex $Config -Force
    Write-Host "      未找到 config.json，已用 config.example.json 初始化" -ForegroundColor Yellow
  } else { throw "找不到配置文件：$Config" }
}
$c = Get-Content $Config -Raw -Encoding UTF8 | ConvertFrom-Json

function Req($v, $name) { if (-not $v -or "$v" -like '<*') { throw "config.json 里 $name 没填" }; $v }
if ($Root) { $root = $Root } else { $root = $c.installRoot }
if (-not $root -or "$root" -like '<*') { throw "缺安装根目录：用 -Root <目录> 指定，或先填 config.json 的 installRoot" }
# 规范化：分隔符统一为单反斜杠、去重复与结尾斜杠（否则会拼出 D://a//b）
# 用 split/join 归一（-replace 的替换串里写反斜杠容易被 .NET 转义吃掉）
$root = ($root -split '[\\/]+' | Where-Object { $_ }) -join '\'
$em = $c.emobile; $wx = $c.wxwork
foreach ($k in @('host','loginId')) {
  if (-not $em.$k -or "$($em.$k)" -like '<*') { Write-Host "      emobile.$k 尚未填写：安装继续，稍后用 discover.ps1 探测或手工补全" -ForegroundColor Yellow }
}
$softKeys = @(
  @{ obj = $em; key = 'password'; label = 'emobile.password' },
  @{ obj = $wx; key = 'docT1';    label = 'wxwork.docT1' },
  @{ obj = $wx; key = 'docT2';    label = 'wxwork.docT2' }
)
foreach ($it in $softKeys) {
  $v = $it.obj.($it.key)
  if (-not $v -or "$v" -like '<*') {
    Write-Host "      $($it.label) 尚未填写：安装继续（可后补）" -ForegroundColor Yellow
  }
}

# ---- RepairPath：安装目录被移动/改名后，把脚本里的旧根目录改写成新根目录（不动业务文件）
# 统一排除名单：配置/密钥由各步骤单独处理；doctor/discover 只用 $PSScriptRoot，
# 不能把它们的 <ROOT> 占位符改写成绝对路径（否则清单比对会误报，见 v2.5 修复）。
$skipNames = @('config.json','secrets.json','doctor.ps1','discover.ps1')

if ($RepairPath) {
  $cfgPath = Join-Path $root 'research\config.json'
  if (-not (Test-Path $cfgPath)) { throw "RepairPath 要求目标目录已安装过（缺少 $cfgPath）" }
  $old = Get-Content $cfgPath -Raw -Encoding UTF8 | ConvertFrom-Json
  $oldRoot = "$($old.root)"
  if (-not $oldRoot -or $oldRoot -eq $root) { Write-Host "根目录未变化（$root），无需修复" -ForegroundColor Green; exit 0 }
  $fix = Get-ChildItem $root -Recurse -File -Include *.js,*.mjs,*.ps1,*.json,*.rs,*.toml,*.md |
         Where-Object { $_.FullName -notlike "*\skills\project-board-kit\files\*" -and $_.FullName -notlike "*\research\wxdata\*" -and $_.Name -notin $skipNames }
  $k = 0
  foreach ($f in $fix) {
    $t = [IO.File]::ReadAllText($f.FullName); $o = $t
    $isCode = $f.Extension -in @('.js','.mjs')   # Rust 用的是 r"..." 原始字符串，反斜杠不能再翻倍
    $bs = if ($isCode) { $root.Replace('\','\\') } else { $root }
    $t = $t.Replace($oldRoot.Replace('\','\\'), $bs)
    $t = $t.Replace($oldRoot, $bs)
    $t = $t.Replace($oldRoot.Replace('\','/'), $root.Replace('\','/'))
    if ($t -ne $o) {
      if ($f.Extension -eq '.ps1') { [IO.File]::WriteAllText($f.FullName, $t, (New-Object Text.UTF8Encoding($true))) }
      else { [IO.File]::WriteAllText($f.FullName, $t, (New-Object Text.UTF8Encoding($false))) }
      $k++
    }
  }
  $old.root = $root
  $old | ConvertTo-Json -Depth 6 | Set-Content $cfgPath -Encoding UTF8
  Write-Host "[RepairPath] $oldRoot  ->  $root   改了 $k 个文件" -ForegroundColor Green
  exit 0
}

Write-Host "[1/5] 目标目录 $root"
foreach ($d in @('', '\research', '\research\raw\emobile', '\research\wxdata', '\rust-widget', '\tools', '\backup')) {
  New-Item -ItemType Directory -Force -Path ($root + $d) | Out-Null
}

if ((Test-Path (Join-Path $root 'research\config.json')) -and -not $Force) {
  throw "目标目录已安装过（$root\research\config.json 已存在）。确认覆盖请加 -Force"
}

Write-Host "[2/5] 复制文件（保持 research\ 与 research\raw\emobile\ 结构）"
Copy-Item (Join-Path $Kit 'files\*') $root -Recurse -Force
Copy-Item (Join-Path $Kit 'doctor.ps1') $root -Force -ErrorAction SilentlyContinue
Copy-Item (Join-Path $Kit 'discover.ps1') $root -Force -ErrorAction SilentlyContinue
Copy-Item (Join-Path $Kit 'SKILL.md')   $root -Force -ErrorAction SilentlyContinue

Write-Host "[3/5] 生成 research\config.json（脚本运行时读它）"
$rt = [ordered]@{
  root     = $root
  emobile = [ordered]@{
    host     = $em.host
    httpPort = if ($em.httpPort) { [int]$em.httpPort } else { 8999 }
    wsUrl    = if ($em.wsUrl) { $em.wsUrl } else { "ws://$($em.host):7070/ws/" }
    loginId  = $em.loginId
    myUserId = if ($em.myUserId) { "$($em.myUserId)" } else { '' }
    authcid  = "$($em.myUserId)|c"
  }
  wxwork   = [ordered]@{
    exe          = $wx.exe
    commandExe   = $wx.commandExe
    userDataRoot = $wx.userDataRoot
    docT1        = $wx.docT1
    docT2        = $wx.docT2
  }
  schedule = $c.schedule
}
$rt | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $root 'research\config.json') -Encoding UTF8

Write-Host "[4/5] 替换脚本里的旧根目录 / 服务器 IP / 账号 / uid"
# 关键：不同文件语法不同，反斜杠转义规则也不同（JS/Rust/TOML 字符串里 \r 会被当成回车）
$rootPs1 = $root                                   # PowerShell / Markdown：单反斜杠
$rootJs  = $root.Replace('\', '\\')                # JS / Rust / TOML 字符串：双反斜杠
$rootFwd = $root.Replace('\', '/')                 # 正斜杠形式（Node 也能吃）
$rootFwdJs = $root.Replace('\', '/')   # 必须从单反斜杠的 $root 派生，否则 // 会翻倍
$files = Get-ChildItem $root -Recurse -File -Include *.js,*.mjs,*.ps1,*.json,*.rs,*.toml,*.md |
         Where-Object { $_.FullName -notlike '*\skills\project-board-kit\files\*' -and $_.FullName -notlike '*\research\wxdata\*' -and $_.Name -notin $skipNames }
$n = 0
foreach ($f in $files) {
  $t = [IO.File]::ReadAllText($f.FullName)
  $o = $t
  $isCode = $f.Extension -in @('.js','.mjs')   # Rust 用的是 r"..." 原始字符串，反斜杠不能再翻倍
  $bs = if ($isCode) { $rootJs } else { $rootPs1 }
  $fw = if ($isCode) { $rootFwdJs } else { $rootFwd }
  # 先换加倍的旧根目录，再换单反斜杠，最后换正斜杠
  if ($t -ne $o) {
    if ($f.Extension -eq '.ps1') { [IO.File]::WriteAllText($f.FullName, $t, (New-Object Text.UTF8Encoding($true))) }
    else { [IO.File]::WriteAllText($f.FullName, $t, (New-Object Text.UTF8Encoding($false))) }
    $n++
  }
}
Write-Host "      改了 $n 个文件"

Write-Host "[5/5] 写口令（独立文件，脚本优先读它）"
if ($em.password -and "$($em.password)" -notlike '<*') {
  Set-Content (Join-Path $root 'research\secrets.json') `
    (@{ emobile = @{ loginId = $em.loginId; password = $em.password } } | ConvertTo-Json) -Encoding UTF8
} else { Write-Host "      跳过口令文件（口令为空）" -ForegroundColor Yellow }

# 5a2) 分发采集脚本（套件自带；目标目录已存在同名文件时不覆盖）
$emSrc = Join-Path $PSScriptRoot 'files\research\emobile'
if (-not (Test-Path $emSrc)) { $emSrc = Join-Path $root 'skills\project-board-kit\files\research\emobile' }
$emDst = Join-Path $root 'research\raw\emobile'
if (Test-Path $emSrc) {
  New-Item -ItemType Directory -Force -Path $emDst | Out-Null
  $copiedEm = 0
  Get-ChildItem $emSrc -File | ForEach-Object {
    $d = Join-Path $emDst $_.Name
    if ((-not (Test-Path $d)) -or $Force) { Copy-Item $_.FullName $d -Force; $copiedEm++ }
  }
  Write-Host "      采集脚本已就位：$copiedEm 个 -> research\raw\emobile"
} else {
  Write-Host "      未找到套件内采集脚本目录，跳过（可另行获取）" -ForegroundColor Yellow
}

# 5b) 顺手编译 cookie 读取工具（需要 .NET Framework 自带的 csc；失败不阻断，doctor 会提示）
$csc  = "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
$csCands = @((Join-Path $root 'research\raw\emobile\wxwork_handle.cs'),
  (Join-Path $root 'research\wxwork_handle.cs'),(Join-Path $root 'research\emobile\wxwork_handle.cs'))
$csSrc = ($csCands | Where-Object { Test-Path $_ } | Select-Object -First 1)
$exeOut = Join-Path $root 'research\wxwork_handle.exe'
if ((Test-Path $csc) -and (Test-Path $csSrc)) {
  if ((Test-Path $exeOut) -and -not $Force) { Write-Host "      wxwork_handle.exe 已存在，跳过编译" }
  else {
    & $csc /nologo /out:"$exeOut" "$csSrc" 2>&1 | Out-Null
    if (Test-Path $exeOut) { Write-Host "      已编译 $exeOut" } else { Write-Host "      编译失败，稍后手动跑：csc /out:`"$exeOut`" `"$csSrc`"" -ForegroundColor Yellow }
  }
} else {
  # csc 缺失：用 PowerShell 自带的 Add-Type 编译（不依赖 VS Build Tools）
  if (Test-Path $csSrc) {
    try {
      Add-Type -Path $csSrc -OutputAssembly $exeOut -OutputType ConsoleApplication -ErrorAction Stop
      if (Test-Path $exeOut) { Write-Host "      已用 Add-Type 编译 $exeOut" }
      else { Write-Host "      Add-Type 未产出 exe，请手动编译 $csSrc" -ForegroundColor Yellow }
    } catch {
      Write-Host "      Add-Type 兜底失败：$($_.Exception.Message)" -ForegroundColor Yellow
      Write-Host "      可手动编译：csc /out:`"$exeOut`" `"$csSrc`"" -ForegroundColor Yellow
    }
  } else { Write-Host "      未找到 wxwork_handle.cs，跳过 cookie 通道" -ForegroundColor Yellow }
}

# 5d) 编译会议 Cookie 扫描器（ec_meeting_fetch.py 依赖它；失败不阻断，doctor 会提示）
$ckCsCands = @((Join-Path $root 'research\em_cookie_scan.cs'),(Join-Path $root 'research\raw\emobile\em_cookie_scan.cs'))
$ckCs  = ($ckCsCands | Where-Object { Test-Path $_ } | Select-Object -First 1)
$ckExe = Join-Path $root 'research\em_cookie_scan.exe'
if ((Test-Path $csc) -and (Test-Path $ckCs)) {
  if ((Test-Path $ckExe) -and -not $Force) { Write-Host "      em_cookie_scan.exe 已存在，跳过编译" }
  else {
    & $csc /nologo /optimize+ /out:"$ckExe" "$ckCs" 2>&1 | Out-Null
    if (Test-Path $ckExe) { Write-Host "      已编译 $ckExe" } else { Write-Host "      em_cookie_scan 编译失败（会议链路会缺 Cookie；可手动 csc 编译）" -ForegroundColor Yellow }
  }
} elseif (Test-Path $ckCs) {
  try {
    Add-Type -Path $ckCs -OutputAssembly $ckExe -OutputType ConsoleApplication -ErrorAction Stop
    if (Test-Path $ckExe) { Write-Host "      已用 Add-Type 编译 $ckExe" } else { Write-Host "      Add-Type 未产出 em_cookie_scan.exe" -ForegroundColor Yellow }
  } catch { Write-Host "      em_cookie_scan 编译跳过：$($_.Exception.Message)" -ForegroundColor Yellow }
} else { Write-Host "      未找到 em_cookie_scan.cs，会议链路跳过" -ForegroundColor Yellow }

# 5c) 编译 E-Mobile 内存令牌扫描器（静默获取登录态的第 3 级；失败不阻断）
$emCands = @((Join-Path $root 'research\em_memscan.cs'),(Join-Path $root 'research\raw\emobile\em_memscan.cs'))
$emCs  = ($emCands | Where-Object { Test-Path $_ } | Select-Object -First 1)
$emExe = Join-Path $root 'research\raw\emobile\em_memscan.exe'
if ((Test-Path $csc) -and (Test-Path $emCs)) {
  if ((Test-Path $emExe) -and -not $Force) { Write-Host "      em_memscan.exe 已存在，跳过编译" }
  else {
    & $csc /nologo /optimize+ /out:"$emExe" "$emCs" 2>&1 | Out-Null
    if (Test-Path $emExe) { Write-Host "      已编译 $emExe" } else { Write-Host "      em_memscan 编译失败（不影响前两级静默获取）" -ForegroundColor Yellow }
  }
} else { Write-Host "      跳过 em_memscan（无 csc 或源文件缺失）" -ForegroundColor Yellow }


# 5d) 部署看板挂件 exe：套件内置 → 本机 cargo 自建 → 从 Release 下载预编译
Write-Host "[5d] 部署看板挂件 exe"
$binSrc = Join-Path $Kit 'files\bin\项目看板.exe'
$binDst = Join-Path $root 'tools\项目看板.exe'
New-Item -ItemType Directory -Force (Split-Path $binDst -Parent) | Out-Null
$binOk = $false
if (Test-Path $binSrc) {
  Copy-Item $binSrc $binDst -Force
  $binOk = Test-Path $binDst
  if ($binOk) { Write-Host "      套件内置：$binDst ($((Get-Item $binDst).Length) B)" }
}
if (-not $binOk) {
  $cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
  $crateDir = Join-Path $root 'rust-widget'
  if ($cargoCmd -and (Test-Path (Join-Path $crateDir 'Cargo.toml'))) {
    Write-Host "      本机有 cargo，尝试从源码构建（约 1–2 分钟）"
    Push-Location $crateDir
    try { & $cargoCmd.Source build --release --locked 2>&1 | Out-Null } finally { Pop-Location }
    $built = Join-Path $crateDir 'target\release\rust-widget.exe'
    if (Test-Path $built) { Copy-Item $built $binDst -Force; $binOk = Test-Path $binDst }
    if ($binOk) { Write-Host "      源码构建完成：$binDst ($((Get-Item $binDst).Length) B)" }
  }
}
if (-not $binOk) {
  $ghOwner = $env:FRD_GH_OWNER
  if (-not $ghOwner) { try { $r = (git -C $PSScriptRoot config --get remote.origin.url); if ($r -match 'github\.com[:/]([^/]+)/') { $ghOwner = $Matches[1] } } catch {} }
  if (-not $ghOwner -or $ghOwner -eq 'project-board-kit') { $ghOwner = '<GITHUB_USER>' }
  $url = "https://github.com/$ghOwner/project-board-kit/releases/latest/download/project-board-widget.exe"
  try {
    Write-Host "      从 Release 下载预编译挂件：" + $url
    $old = $env:HTTP_PROXY
    if (-not $env:HTTP_PROXY -and (Test-NetConnection -ComputerName 127.0.0.1 -Port 7897 -InformationLevel Quiet -WarningAction SilentlyContinue)) { $env:HTTP_PROXY = 'http://127.0.0.1:7897'; $env:HTTPS_PROXY = 'http://127.0.0.1:7897' }
    Invoke-WebRequest -Uri $url -OutFile $binDst -UseBasicParsing -TimeoutSec 120
    $env:HTTP_PROXY = $old
    $binOk = (Test-Path $binDst) -and ((Get-Item $binDst).Length -gt 200KB)
  } catch { Write-Host ("      下载失败：" + $_.Exception.Message) -ForegroundColor Yellow }
}
if (-not $binOk) {
  Write-Host "      未能取得挂件 exe：请装 Rust 后执行 cargo build --release，或手动从 Release 下载 project-board-widget.exe 放到 tools\项目看板.exe" -ForegroundColor Yellow
} else {
  & $binDst --version | ForEach-Object { Write-Host "      " + $_ }
}

# 5e) 开机自启：启动文件夹快捷方式（参数显式给 state/data/log，避免 exe 内默认路径指向别的机器）
Write-Host "[5e] 建开机自启快捷方式"
if ((Test-Path $binDst) -and -not $NoAutostart) {
  try {
    $lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\项目看板.lnk'
    $ws = New-Object -ComObject WScript.Shell
    $s = $ws.CreateShortcut($lnk)
    $s.TargetPath = $binDst
    $s.Arguments = "--mode normal --state `"$root\research\widget_state.json`" --data `"$root\research\widget_data.json`" --log `"$root\research\rust-widget.log`""
    $s.WorkingDirectory = (Split-Path $binDst -Parent)
    $s.WindowStyle = 7
    $s.Save()
    Write-Host "      $lnk"
  } catch { Write-Host "      快捷方式创建失败：$_" -ForegroundColor Yellow }
} else { Write-Host "      跳过（-NoAutostart 或二进制缺失）" }

# 5f) 小黄条待办 → 部门进度（没装 YYNote 会打印 skip）
Write-Host "[5f] 导入小黄条待办到部门进度"
$imp = Join-Path $root 'research\yynote_import.ps1'
if (Test-Path $imp) { & pwsh -NoProfile -ExecutionPolicy Bypass -File $imp 2>&1 | ForEach-Object { Write-Host "      $_" } }

# 5g) 启动挂件（首次装完立即可见）
Write-Host "[5g] 启动看板挂件"
if ((Test-Path $binDst) -and -not $NoAutostart -and -not (Get-Process '项目看板' -ErrorAction SilentlyContinue)) {
  Start-Process $binDst -ArgumentList '--mode','normal','--state',"`"$root\research\widget_state.json`"",'--data',"`"$root\research\widget_data.json`"",'--log',"`"$root\research\rust-widget.log`"" -WindowStyle Hidden
  Write-Host "      已启动（贴屏幕底部；托盘图标在任务栏右侧）"
} else { Write-Host "      已在运行 / -NoAutostart / 二进制缺失，跳过" }

Write-Host ""
Write-Host "完成。按顺序跑：" -ForegroundColor Green
Write-Host "  1) pwsh -File `"$root\discover.ps1`"   # 探测环境、补全 config（必须先于 doctor）"
Write-Host "  2) pwsh -File `"$root\doctor.ps1`"     # 体检"
Write-Host "  3) node  `"$root\research\raw\emobile\em_login_state.js`" --auto --probe  # 静默拿 E-Mobile 登录态（零输入）"
Write-Host "     node  `"$root\research\raw\emobile\em_contacts_refresh.js`"           # 通讯录快照（可选）"
Write-Host "  4) node  `"$root\research\raw\emobile\em_full_pull.js`" --fresh    # 首轮全量群/私聊"
Write-Host "  5) node  `"$root\research\raw\emobile\em_names_by_id.js`"          # 未识别发言人 → 真名"
Write-Host "  6) node  `"$root\research\raw\emobile\build_chatlog.js`""
Write-Host "  7) node  `"$root\research\raw\emobile\wxwork_sheet_pull.js`" --check  # 企业微信文档登录态"
Write-Host "  8) node  `"$root\research\raw\emobile\wxwork_sheet_pull.js`""
Write-Host "  9) pwsh -File `"$root\每日项目简报.ps1`" -NoOpen                   # 端到端"
Write-Host "  10) pwsh -File `"$root\research\register_daily_task.ps1`"           # 工作日 09:05 定时"
Write-Host " 11) 挂件：tools\项目看板.exe 已部署 + 开机自启（见 5d–5g）；小黄条待办已导入部门进度（5f）"

# 5h) 企业微信只读采集组件（选装）：编译只读密钥扫描器 + 复制 32 位 libcrypto
try {
  $wxCs = Join-Path $root 'research\wxwork_key_readonly.cs'
  $wxExe = Join-Path $root 'research\wxwork_key_readonly.exe'
  if ((Test-Path $wxCs) -and -not (Test-Path $wxExe)) {
    $csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    if (-not (Test-Path $csc)) { $csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework\v4.0.30319\csc.exe' }
    if (Test-Path $csc) { & $csc /nologo /platform:x86 /optimize+ /out:$wxExe $wxCs | Out-Null }
    if (Test-Path $wxExe) { Write-Host "  已编译 企微只读密钥扫描器" -ForegroundColor Green }
    else { Write-Host "  企微扫描器未编译（可稍后运行 research\wxwork_enable.ps1 自动编译）" -ForegroundColor Yellow }
  }
  $wxLib = Join-Path $root 'research\libcrypto32.dll'
  if (-not (Test-Path $wxLib)) {
    $wxDirs = @()
    if ($env:WXWORK_DIR) { $wxDirs += $env:WXWORK_DIR }
    $wxProc = Get-Process WXWork -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($wxProc -and $wxProc.Path) { $wxDirs += (Split-Path $wxProc.Path -Parent) }
    $wxDirs += @(
      (Join-Path $env:ProgramFiles 'WXWork'),
      (Join-Path ${env:ProgramFiles(x86)} 'WXWork'),
      (Join-Path $env:LOCALAPPDATA 'WXWork'),
      ($env:SystemDrive + '\WXWork')
    )
    $libSrc = $null
    foreach ($d in ($wxDirs | Where-Object { $_ -and (Test-Path $_) })) {
      $libSrc = Get-ChildItem $d -Recurse -Filter 'libcrypto-1_1.dll' -ErrorAction SilentlyContinue | Select-Object -First 1
      if ($libSrc) { break }
    }
    if ($libSrc) { Copy-Item $libSrc.FullName $wxLib -Force; Write-Host "  已复制 32 位 libcrypto（来自 $($libSrc.DirectoryName)）" -ForegroundColor Green }
    else { Write-Host "  未找到企微 libcrypto（需企微群聊数据时手动复制 libcrypto-1_1.dll 到 research\libcrypto32.dll）" -ForegroundColor Yellow }
  }
} catch { Write-Host ("  企微组件准备跳过：" + $_.Exception.Message) -ForegroundColor Yellow }
