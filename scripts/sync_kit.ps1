# 同步真源 → 套件副本，然后跑门禁
#
# 规则（见 AGENTS.md「修改代码的硬规矩」）：
#   research/ 与 rust-widget/ 是唯一真源，skills/project-board-kit/files/ 下是同名副本。
#   改完真源必须同步副本，否则 guard_meta.py 的 PARITY 会报「副本滞后」。
#
# 用法：
#   pwsh -File scripts/sync_kit.ps1                 # 同步 + 跑门禁
#   pwsh -File scripts/sync_kit.ps1 -CheckOnly      # 只比对不同步，适合 CI
#   pwsh -File scripts/sync_kit.ps1 -DryRun         # 只打印将要同步的文件

[CmdletBinding()]
param(
  [switch]$CheckOnly,
  [switch]$DryRun,
  [string]$Root = '',
  [switch]$SkipGuard
)

$ErrorActionPreference = 'Stop'
$root = if ($Root) { (Resolve-Path $Root).Path } else { Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path) }
$kit = Join-Path $root 'skills\project-board-kit\files'

# 真源 → 副本 的目录对（与 guard_meta.py 的 PARITY_GROUPS 保持一致）
$pairs = @(
  @{ Src = 'rust-widget\src'; Dst = 'rust-widget\src'; Filter = '*.rs' },
  @{ Src = 'rust-widget'; Dst = 'rust-widget'; Filter = 'Cargo.toml' },
  @{ Src = 'rust-widget'; Dst = 'rust-widget'; Filter = 'Cargo.lock' },
  @{ Src = 'rust-widget'; Dst = 'rust-widget'; Filter = 'build.rs' },
  @{ Src = 'rust-widget'; Dst = 'rust-widget'; Filter = 'README.md' },
  @{ Src = 'research'; Dst = 'research'; Filter = '*' }
)

# 按设计不分发的真源（数据、缓存、客户专用、临时脚本）
$skipFile = '(?i)^(_|client_|fc_|client_)|\.(json|jsonl|csv|xlsx|txt|html|log|png|db)$|^README-'
$skipDir = @('raw', 'wxdata', 'api', '_raw', '_legacy', '_rv', '_t', '__pycache__', 'node_modules')

function Get-RelFiles([string]$dir) {
  if (-not (Test-Path $dir)) { return @() }
  Get-ChildItem $dir -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName }
}

$toCopy = New-Object System.Collections.Generic.List[object]
foreach ($p in $pairs) {
  $srcDir = Join-Path $root $p.Src
  $dstDir = Join-Path $kit $p.Dst
  foreach ($f in Get-RelFiles $srcDir) {
    $rel = $f.Substring($srcDir.Length).TrimStart('\')
    if ($rel -match $skipFile) { continue }
    if (($rel -split '\\')[0] -in $skipDir) { continue }
    if ($p.Filter -ne '*' -and (-not (Split-Path $f -Leaf).EndsWith($p.Filter.TrimStart('*')))) { continue }
    $dst = Join-Path $dstDir $rel
    $srcHash = if (Test-Path $dst) { (Get-FileHash $f -Algorithm SHA256).Hash } else { '' }
    $dstHash = if (Test-Path $dst) { (Get-FileHash $dst -Algorithm SHA256).Hash } else { '' }
    if ($srcHash -ne $dstHash) {
      $toCopy.Add([pscustomobject]@{ Src = $f; Dst = $dst })
    }
  }
}

if ($toCopy.Count -eq 0) {
  Write-Host 'KIT_ALREADY_IN_SYNC（真源与副本一致）'
} else {
  Write-Host ("待同步 {0} 个文件：" -f $toCopy.Count)
  foreach ($c in $toCopy) { Write-Host ("  {0}" -f $c.Src.Substring($root.Length).TrimStart('\')) }
  if ($CheckOnly -or $DryRun) {
    if ($CheckOnly) { Write-Host 'SYNC_CHECK=FAIL（副本滞后，请同步）'; exit 1 }
    Write-Host 'SYNC_DRYRUN=OK'; exit 0
  }
  foreach ($c in $toCopy) {
    $d = Split-Path $c.Dst -Parent
    if (-not (Test-Path $d)) { New-Item -ItemType Directory -Path $d -Force | Out-Null }
    Copy-Item $c.Src $c.Dst -Force
  }
  Write-Host ("KIT_SYNCED files={0}" -f $toCopy.Count)
}

if ($CheckOnly) { exit 0 }

# 源树（含 _business/ 与真实数据）不跑脱敏门禁：门禁只针对导出仓/纯净仓
$isSource = Test-Path (Join-Path $root '_business')
if ($SkipGuard -or $isSource) { Write-Host 'SYNC_OK（跳过门禁）'; exit 0 }

$guard = Join-Path $root 'scripts\guard_meta.py'
if (-not (Test-Path $guard)) { $guard = Join-Path $root '_business\repo_overlay\scripts\guard_meta.py' }
$py = (Get-Command python -ErrorAction SilentlyContinue).Source
if ($py -and (Test-Path $guard)) {
  Push-Location $root
  try {
    & $py -X utf8 $guard
    if ($LASTEXITCODE -ne 0) { throw 'guard_meta 未通过（PARITY 或脱敏仍有问题）' }
  } finally { Pop-Location }
}
Write-Host 'SYNC_OK'
