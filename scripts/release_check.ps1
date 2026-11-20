<#
.SYNOPSIS
  一键发布前检查：门禁 + 发布前全面检查（可选含治具自检）。

.DESCRIPTION
  只读操作，不改任何文件（--selftest 只在临时目录里造违规样本）。
  任一步失败即中止并返回非零退出码。

.EXAMPLE
  pwsh -File scripts/release_check.ps1 -Version v2.11
  pwsh -File scripts/release_check.ps1 -Version v2.11 -SelfTest
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Version,
  [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
$py = (Get-Command python -ErrorAction SilentlyContinue).Source
if (-not $py) { $py = (Get-Command py -ErrorAction SilentlyContinue).Source }
if (-not $py) { throw '找不到 python，请先安装 Python 3.10+ 并加入 PATH' }

Push-Location $root
try {
  Write-Host '[1/2] 门禁 guard_meta.py'
  & $py -X utf8 'scripts/guard_meta.py'
  if ($LASTEXITCODE -ne 0) { throw 'guard_meta 未通过' }

  Write-Host '[2/2] 发布前全面检查 prepublish_gate.py'
  if ($SelfTest) {
    & $py -X utf8 'scripts/prepublish_gate.py' --selftest --version $Version
  } else {
    & $py -X utf8 'scripts/prepublish_gate.py' --version $Version
  }
  if ($LASTEXITCODE -ne 0) { throw 'prepublish_gate 未通过' }

  Write-Host 'RELEASE_CHECK=PASS'
}
finally {
  Pop-Location
}
