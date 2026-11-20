# discover.ps1 —— 零人工探测：把本机能自动拿到的信息写进 research\config.json
# 用法: pwsh -File discover.ps1 [-Root <安装根目录>] [-Force]
# 只做「发现 + 写配置」，不联网登录、不改系统；幂等，已存在的值默认保留。
param(
  [string]$Root = "",
  [switch]$Force
)
$ErrorActionPreference = 'Stop'
function Say($s) { Write-Host $s }
function P($a, $b) { if ($a) { Join-Path $a $b } else { $null } }

# ---------- 0. 定位安装根目录 ----------
if (-not $Root) {
  $here = $PSScriptRoot
  if (Test-Path (P $here 'research')) { $Root = $here }
  elseif (Test-Path (P (Split-Path $here -Parent) 'research')) { $Root = Split-Path $here -Parent }
  else { Write-Host "未检测到已安装的目录（research/ 不存在）。" -ForegroundColor Yellow
  Write-Host "  请先运行：setup.ps1 -Root <安装根目录>，再跑本脚本。" -ForegroundColor Yellow
  exit 2 }
}
$cfgPath = Join-Path $Root 'research\config.json'
if (Test-Path $cfgPath) { $cfgBackup = "$cfgPath.bak-$(Get-Date -Format yyyyMMdd-HHmmss)"; Copy-Item $cfgPath $cfgBackup -Force; Say "已备份原配置 -> $(Split-Path -Leaf $cfgBackup)" }
$secPath = Join-Path $Root 'research\secrets.json'
Say "[根目录] $Root"

$cfg = $null
if ((Test-Path $cfgPath) -and -not $Force) { $cfg = Get-Content $cfgPath -Raw -Encoding UTF8 | ConvertFrom-Json }
if (-not $cfg) {
  $cfg = [pscustomobject]@{
    root = $Root
    emobile = [pscustomobject]@{ host = ''; httpPort = 8999; wsUrl = ''; loginId = ''; myUserId = ''; authcid = '' }
    wxwork  = [pscustomobject]@{ exe = ''; commandExe = ''; userDataRoot = ''; docT1 = ''; docT2 = ''; docCandidates = @() }
    schedule = [pscustomobject]@{ dailyPull = '09:05'; dailyReport = '09:10'; weeklyCheckPatch = '09:10' }
    paths = [pscustomobject]@{ node = ''; pwsh = '' }
  }
}
$found = @()

# 探测可执行文件：PATH -> where.exe -> 常见安装目录（任一命中即返回）
function Find-Exe([string]$Name, [string[]]$Fallbacks) {
  $c = Get-Command $Name -ErrorAction SilentlyContinue
  if ($c) { return $c.Source }
  $w = (where.exe $Name 2>$null | Select-Object -First 1)
  if ($w) { return $w.Trim() }
  foreach ($f in $Fallbacks) { if ($f -and (Test-Path $f)) { return $f } }
  return ''
}

# 确保 paths 段存在（首次运行时 config 里可能没有）
if (-not $cfg.PSObject.Properties['paths']) {
  $cfg | Add-Member -NotePropertyName paths -NotePropertyValue ([pscustomobject]@{ node=''; pwsh='' }) -Force
}
# ---------- 1. node / pwsh ----------
$n = Find-Exe 'node' @('<ROOT>\nodejs\node.exe', "$env:LOCALAPPDATA\Programs\nodejs\node.exe")
if ($n) { if (-not $cfg.paths.node) { $cfg.paths.node = $n }; $found += "node  $n" } else { $found += "node  未找到（需安装 Node >=18）" }
$p = Find-Exe 'pwsh' @('<ROOT>\PowerShell\7\pwsh.exe')
if ($p) { if (-not $cfg.paths.pwsh) { $cfg.paths.pwsh = $p }; $found += "pwsh  $p" } else { $found += "pwsh  未找到（PowerShell 5.1 亦可运行，建议装 7）" }

# ---------- 2. E-Mobile 客户端：目录名自带 服务器:端口 ----------
$emRoots = @()
try { $pp = (Get-Process E-Mobile -ErrorAction SilentlyContinue | Select-Object -First 1).Path; if ($pp) { $emRoots += (Split-Path $pp -Parent) } } catch {}
foreach ($c in @("$env:ProgramFiles\E-Mobile", "${env:ProgramFiles(x86)}\E-Mobile", "$env:SystemDrive\E-Mobile", "${env:ProgramFiles(x86)}\E-Mobile\..")) { if ((Test-Path $c) -and ($emRoots -notcontains $c)) { $emRoots += $c } }
Say "[E-Mobile] 客户端目录: $($emRoots -join ' | ')"

# Electron userData：%APPDATA%\<产品名> 里含 "User Files\http-<host>-<port>"
$emData = @()
foreach ($d in (Get-ChildItem $env:APPDATA -Directory -ErrorAction SilentlyContinue)) {
  if (Test-Path (Join-Path $d.FullName 'User Files')) { $emData += $d.FullName }
}
Say "[E-Mobile] userData: $($emData -join ' | ')"
foreach ($u in $emData) {
  foreach ($hd in (Get-ChildItem (Join-Path $u 'User Files') -Directory -ErrorAction SilentlyContinue)) {
    if ($hd.Name -match '^http-(.+)-(\d+)$') {
      $h = $Matches[1]; $port = [int]$Matches[2]
      if (-not $cfg.emobile.host -or $Force) {
        $cfg.emobile.host = $h
        $cfg.emobile.httpPort = $port
        $cfg.emobile.wsUrl = "ws://$h`:7070/ws/"
      }
      $found += "emobile.host  $h : $port  (来自 $(Split-Path $u -Leaf)\User Files\$($hd.Name))"
      # uid：该目录下的数字子目录，或 login.json 里的 baseUserId
      $uid = ''
      $numDirs = Get-ChildItem $hd.FullName -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^\d{2,}$' }
      if ($numDirs) { $uid = $numDirs[0].Name }
      if ($uid -and (-not $cfg.emobile.myUserId -or $Force)) {
        $cfg.emobile.myUserId = $uid
        if (-not $cfg.emobile.loginId) { $cfg.emobile.loginId = $uid }
        $cfg.emobile.authcid = "$uid|c"
        $found += "emobile.myUserId  $uid  (来自缓存子目录)"
      }
    }
  }
}

# ---------- 3. 企业微信：exe / commandExe / userDataRoot / 账号目录 ----------
$wxExe = ''
$regDirs = @()
try {
  $regDirs = Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -match '企业微信|WXWork' -and $_.InstallLocation } |
    ForEach-Object { $_.InstallLocation }
} catch {}
foreach ($c in (@($regDirs) + @('<ROOT> (x86)\WXWork', '<ROOT>\WXWork', '<ROOT>'))) {
  if ($c -and (Test-Path $c)) { $wxExe = $c; break }
}
try { $pp = (Get-Process WXWork -ErrorAction SilentlyContinue | Select-Object -First 1).Path; if ($pp) { $wxExe = Split-Path $pp -Parent } } catch {}
if ($wxExe) {
  $exe = Join-Path $wxExe 'WXWork.exe'
  $cmd = Get-ChildItem $wxExe -Recurse -Filter 'WXWorkCommand.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $cfg.wxwork.exe -or $Force) { $cfg.wxwork.exe = $exe }
  if ($cmd -and ((-not $cfg.wxwork.commandExe) -or $Force)) { $cfg.wxwork.commandExe = $cmd.FullName }
  $found += "wxwork.exe  $exe"
  $found += "wxwork.commandExe  $($cmd.FullName)"
}
$wxData = Join-Path $env:USERPROFILE 'Documents\WXWork'
if (Test-Path $wxData) {
  if (-not $cfg.wxwork.userDataRoot -or $Force) { $cfg.wxwork.userDataRoot = $wxData }
  $acc = Get-ChildItem $wxData -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^\d{10,}$' } | Select-Object -First 1
  $found += "wxwork.userDataRoot  $wxData" + $(if ($acc) { "  (账号目录 $($acc.Name))" } else { '' })
}

# ---------- 4. 自动挖腾讯文档候选 URL（企业微信缓存里的 doc.weixin.qq.com/sheet/e3_*） ----------
$cands = @()
if ($cfg.wxwork.userDataRoot -and (Test-Path $cfg.wxwork.userDataRoot)) {
  Say "[文档] 扫描企业微信数据目录，找 doc.weixin.qq.com/sheet/e3_* …"
  $roots = Get-ChildItem $cfg.wxwork.userDataRoot -Directory -ErrorAction SilentlyContinue |
           Where-Object { $_.Name -match '^\d{10,}$' }
  $scanned = 0
  $cFull = @(); $cBare = @()
  foreach ($r in $roots) {
    $files = Get-ChildItem $r.FullName -Recurse -File -ErrorAction SilentlyContinue |
             Where-Object { $_.Length -gt 200 -and $_.Length -lt 6MB }
    foreach ($f in $files) {
      if ($scanned -ge 6000 -or $cands.Count -ge 60) { break }
      $scanned++
      try {
        $fs = [IO.File]::Open($f.FullName, 'Open', 'Read', 'ReadWrite')
        $len = [Math]::Min($fs.Length, 6MB); $buf = New-Object byte[] $len
        [void]$fs.Read($buf, 0, $len); $fs.Close()
      } catch { continue }
      $t = [Text.Encoding]::Latin1.GetString($buf)   # Latin1 不会因非法字节抛错
      foreach ($m in [regex]::Matches($t, 'doc\.weixin\.qq\.com/sheet/(e3_[A-Za-z0-9]{20,})')) {
        $v = $m.Groups[1].Value
        if ($v.Length -gt 32) { $v = $v.Substring(0, 32) }
        if ($v -match '^e3_[A-Za-z0-9]{29}$') { $cFull += $v }
      }
      foreach ($m in [regex]::Matches($t, 'e3_[A-Za-z0-9]{29}')) { $cBare += $m.Value }
    }
  }
  Say "[文档] 扫描文件 $scanned 个（完整 URL 命中 $($cFull.Count) / 裸 padId $($cBare.Count)）"
  $cands = if ($cFull.Count) { $cFull } else { $cBare }   # 优先完整 URL；缓存被切片时退回裸 padId
}
# 按出现次数排序：常用文档（就是你每天在看的那两张表）会明显靠前
$rank = $cands | Group-Object | Sort-Object Count -Descending | Select-Object -First 12
$cands = @($rank | ForEach-Object { $_.Name })
if ($cands) {
  $found += "文档候选 padId  $($cands.Count) 个（按出现次数）"
  foreach ($g in $rank) { Say ("    次数 {0,4}  {1}" -f $g.Count, $g.Name) }
}

# 4b. 自动选定两张项目表：缓存里访问次数最高的两个 padId（实测就是「已立项 / 预立项」两张）
if ($cands.Count -ge 2) {
  if ((-not $cfg.wxwork.docT1) -or $Force) { $cfg.wxwork.docT1 = "https://doc.weixin.qq.com/sheet/$($cands[0])" }
  if ((-not $cfg.wxwork.docT2) -or $Force) { $cfg.wxwork.docT2 = "https://doc.weixin.qq.com/sheet/$($cands[1])" }
  $found += "wxwork.docT1  取次数第 1 的文档 $($cands[0])"
  $found += "wxwork.docT2  取次数第 2 的文档 $($cands[1])"
  $found += "  → 若不对，用 wxwork_sheet_pull.js --padid1 <id> --check 逐个验证后改 config"
}

# 4c. 小黄条（YYNote）：数据库 + SQLCipher 原生库（导入部门进度用；没装就留空）
$yyDb = @("$env:LOCALAPPDATA\YYNoteVip\yynote.db", "$env:USERPROFILE\Documents\YYNote\yynote.db") |
        Where-Object { Test-Path $_ } | Select-Object -First 1
$yyNat = @('<ROOT>\YYNote\runtimes\win-x64\native', '<ROOT> (x86)\YYNote\runtimes\win-x64\native') |
         Where-Object { Test-Path (Join-Path $_ 'e_sqlcipher.dll') } | Select-Object -First 1
if (-not $cfg.PSObject.Properties['yynote']) {
  $cfg | Add-Member -NotePropertyName yynote -NotePropertyValue ([pscustomobject]@{ db = ''; nativeDir = '' }) -Force
}
if ($yyDb)  { $cfg.yynote.db = $yyDb;   $found += "yynote.db  $yyDb" } else { $found += "yynote.db  未发现（没装小黄条 → 部门待办只能自建）" }
if ($yyNat) { $cfg.yynote.nativeDir = $yyNat; $found += "yynote 原生库  $yyNat" }
# ---------- 5. 写回 config.json（不覆盖已有非空值，除非 -Force） ----------
if ($true) {
  if (-not $cfg.PSObject.Properties['paths']) { $cfg | Add-Member -NotePropertyName paths -NotePropertyValue ([pscustomobject]@{ node=''; pwsh='' }) -Force }
  if ($cands) { $cfg.wxwork | Add-Member -NotePropertyName docCandidates -NotePropertyValue @($cands) -Force }
  if ($cfg.wxwork.PSObject.Properties['docCandidates'] -and $cands -and $Force) { $cfg.wxwork.docCandidates = @($cands) }
  $cfg.root = $Root
  $cfg | ConvertTo-Json -Depth 8 | Set-Content $cfgPath -Encoding UTF8
  Say "[写出] $cfgPath"
}
if (-not (Test-Path $secPath)) {
  '{"emobile":{"password":""}}' | Set-Content $secPath -Encoding UTF8
  Say "[写出] $secrets.json（口令留空，见 SKILL.md 第 2.3 节三种取法）"
}

Say ""
Say "=== 发现结果 ==="
$found | ForEach-Object { Say "  $_" }
Say ""
Say "=== 还缺什么 ==="
$missing = @()
if (-not $cfg.emobile.host) { $missing += "emobile.host（客户端没装/没登录过 → 见 SKILL 2.1）" }
if (-not $cfg.emobile.myUserId) { $missing += "emobile.myUserId（同上）" }
if (-not $cfg.wxwork.exe) { $missing += "wxwork.exe（企业微信没装 → 只能跑 E-Mobile 通道）" }
if (-not $cfg.wxwork.docT1) { $missing += "wxwork.docT1/docT2（从 docCandidates 里挑，见 SKILL 2.2）" }
$sec = Get-Content $secPath -Raw -Encoding UTF8 | ConvertFrom-Json
if (-not $sec.emobile.password) { $missing += "emobile.password（唯一可能要人给的东西，见 SKILL 2.3）" }
if ($missing) { $missing | ForEach-Object { Say "  - $_" } } else { Say "  无 —— 全部就绪，可直接跑 每日项目简报.ps1" }
