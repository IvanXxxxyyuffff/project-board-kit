# wxwork_enable.ps1
#
# 一键启用「企业微信本地数据只读采集」。
#
# 设计约束（用户硬要求）：不注入、不写目标进程内存、不重启客户端、不调登录接口。
# 本脚本只做三件事：①只读扫描内存里的 16 字节库密钥；②把库文件复制成副本后离线解密；
# ③写密钥与指纹文件供后续采集使用。全程不会导致企微掉线或重启。
#
# 用法：
#   pwsh -File research\wxwork_enable.ps1                 # 自动：扫密钥 -> 解密 -> 校验
#   pwsh -File research\wxwork_enable.ps1 -Key <32位hex>  # 已有密钥时跳过扫描
#   pwsh -File research\wxwork_enable.ps1 -CheckOnly      # 只检查环境与既有密钥是否可用
#
param(
  [string]$Key = '',
  [string]$DataDir = '',
  [switch]$CheckOnly
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
$out  = Join-Path $here '_wxdec'
$py   = $env:FRD_PYTHON
if (-not $py) { $py = Join-Path $root 'tools\python\python.exe' }
if (-not (Test-Path $py)) { $py = 'python' }
if (-not (Test-Path $py)) { $py = (Get-Command python -ErrorAction SilentlyContinue).Source }
if (-not $py) { $py = 'python' }
$scanner = Join-Path $here 'wxwork_key_readonly.exe'
$scannerCs = Join-Path $here 'wxwork_key_readonly.cs'
$decryptor = Join-Path $here 'wxwork_db_decrypt.py'

function Log([string]$m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $m) }

# ---------- 1. 定位账号数据目录 ----------
if (-not $DataDir) {
  $bases = @(
    (Join-Path $env:USERPROFILE 'Documents\WXWork'),
    (Join-Path $env:USERPROFILE 'WXWork')
  )
  foreach ($b in $bases) {
    if (-not (Test-Path $b)) { continue }
    $hit = Get-ChildItem $b -Directory -ErrorAction SilentlyContinue |
             Where-Object { Test-Path (Join-Path $_.FullName 'Data\message.db') } |
             Select-Object -First 1
    if ($hit) { $DataDir = Join-Path $hit.FullName 'Data'; break }
  }
}
if (-not $DataDir -or -not (Test-Path (Join-Path $DataDir 'message.db'))) {
  Log "WXWORK_DB_MISSING 未找到企业微信本地库（请确认客户端已登录过）"
  exit 2
}
Log "db_dir=$DataDir"

# ---------- 2. 必要组件 ----------
if (-not (Test-Path $scanner)) {
  if (-not (Test-Path $scannerCs)) { Log "SCANNER_MISSING wxwork_key_readonly.cs"; exit 2 }
  $csc = "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
  if (-not (Test-Path $csc)) { $csc = "$env:WINDIR\Microsoft.NET\Framework\v4.0.30319\csc.exe" }
  & $csc /nologo /platform:x86 /optimize+ /out:$scanner $scannerCs | Out-Null
  if (-not (Test-Path $scanner)) { Log "SCANNER_COMPILE_FAIL"; exit 2 }
  Log "scanner_compiled"
}
if (-not (Test-Path (Join-Path $here 'libcrypto32.dll'))) {
  $lib = Get-ChildItem $env:WXWORK_DIR -Recurse -Filter 'libcrypto-1_1.dll' -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($lib) { Copy-Item $lib.FullName (Join-Path $here 'libcrypto32.dll') -Force; Log "libcrypto_copied" }
  else { Log "LIBCRYPTO_MISSING 需要从企微安装目录复制 32 位 libcrypto-1_1.dll 为 research\libcrypto32.dll"; exit 2 }
}

# ---------- 3. 取密钥 ----------
$keyFile = Join-Path $out '_key.txt'
if (-not $Key -and (Test-Path $keyFile)) {
  $cached = (Get-Content $keyFile -Raw).Trim()
  if ($cached -match '^[0-9a-fA-F]{32}$') {
    & $scanner verify (Join-Path $DataDir 'message.db') $cached | Out-Host
    if ($LASTEXITCODE -eq 0) { $Key = $cached; Log "key_reused_from_cache" }
    else { Log "cached_key_stale 需要重新扫描" }
  }
}

if (-not $Key) {
  if ($CheckOnly) { Log "NO_KEY CheckOnly 模式下不扫描"; exit 3 }
  if (-not (Get-Process -Name 'WXWork' -ErrorAction SilentlyContinue)) { Log "WXWORK_NOT_RUNNING 请先登录企业微信"; exit 3 }
  Log "scanning_process_memory (只读, 不修改客户端)"
  $scanOut = & $scanner scan (Join-Path $DataDir 'message.db') 'WXWork' 240
  $scanOut | Out-Host
  $m = [regex]::Match(($scanOut -join "`n"), 'FOUND key=([0-9a-fA-F]{32})')
  if (-not $m.Success) { Log "KEY_NOT_FOUND 客户端空闲时密钥可能不在内存，请在企微里点开任意会话后重试"; exit 3 }
  $Key = $m.Groups[1].Value
}

if ($Key -notmatch '^[0-9a-fA-F]{32}$') { Log "BAD_KEY 需要 32 位十六进制"; exit 2 }
Log ("key=" + $Key.Substring(0, 6) + "..." + $Key.Substring(26))

# ---------- 4. 解密三个库（复制副本后离线解） ----------
New-Item -ItemType Directory -Path $out -Force | Out-Null
$targets = @(
  @{ db = 'session.db'; name = 'session_plain.db' },
  @{ db = 'message.db'; name = 'msg_plain.db' },
  @{ db = 'user.db';    name = 'user_plain.db' }
)
$ok = 0
foreach ($t in $targets) {
  $src = Join-Path $DataDir $t.db
  if (-not (Test-Path $src)) { Log ("SKIP {0} 不存在" -f $t.db); continue }
  $tmp = Join-Path $out $t.db
  Copy-Item $src $tmp -Force
  $dst = Join-Path (Join-Path $out $t.name) $t.db
  $res = & $py -X utf8 $decryptor $tmp $Key (Join-Path $out $t.name) 2>&1
  if ($LASTEXITCODE -eq 0 -and (Test-Path $dst)) { $ok++; Log ("decrypt {0} OK -> {1}" -f $t.db, $t.name) }
  else { Log ("decrypt {0} FAIL: {1}" -f $t.db, ($res -join ' ')); }
}
if ($ok -eq 0) { Log "DECRYPT_FAILED"; exit 4 }

Set-Content -Path $keyFile -Value $Key -NoNewline -Encoding ascii
Log "key_saved=$keyFile"

# ---------- 5. 校验：解出的库能读出表与行 ----------
$probe = & $py -X utf8 (Join-Path $here 'wxwork_chat_export.py') 2>&1
Log ("export_probe: " + (($probe | Select-Object -Last 1) -join ' '))
Log "WXWORK_ENABLE_OK decrypted=$ok"
exit 0
