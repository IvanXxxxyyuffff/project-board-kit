<#
  一条命令安装（新用户从这里开始）
  用法：
    pwsh -File install.ps1                       # 装到默认目录 <当前盘>:\project-board-kit-app
    pwsh -File install.ps1 -Root <TARGET_DIR>\app     # 指定安装目录
    pwsh -File install.ps1 -Root <TARGET_DIR>\app -SkipDoctor
  做四件事：1) 复制套件到安装目录  2) 初始化配置  3) 探测环境  4) 体检
#>
param(
  [string]$Root = '',
  [switch]$SkipDoctor,
  [switch]$NoAutostart
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
if (-not $Root) { $Root = Join-Path ([IO.Path]::GetPathRoot($here)) 'project-board-kit-app' }

Write-Host "== 1/4 安装到 $Root" -ForegroundColor Cyan
$setupArgs = @('-Root', $Root)
if ($NoAutostart) { $setupArgs += '-NoAutostart' }
& (Join-Path $here 'setup.ps1') @setupArgs
if ($LASTEXITCODE -ne 0) { throw "setup.ps1 失败（exit $LASTEXITCODE）" }

Write-Host "== 2/4 探测环境" -ForegroundColor Cyan
& (Join-Path $Root 'discover.ps1') -Root $Root
if ($LASTEXITCODE -ne 0) { Write-Host "  discover 有未完成项，继续（不阻断）" -ForegroundColor Yellow }

if (-not $SkipDoctor) {
  Write-Host "== 3/4 体检" -ForegroundColor Cyan
  & (Join-Path $Root 'doctor.ps1') -Root $Root
  $rc = $LASTEXITCODE
  if ($rc -ne 0) { Write-Host "  doctor 有 $rc 项 FAIL（多为未配置的账号/未登录的客户端，属环境项）" -ForegroundColor Yellow }
}

Write-Host "== 4/4 下一步" -ForegroundColor Cyan
Write-Host "  1) 首次抓取：pwsh -File `"$Root\每日项目简报.ps1`" -NoOpen"
Write-Host "  2) 看板：  start `"$Root\项目数据看板.html`""
Write-Host "  3) 定时：  pwsh -File `"$Root\research\register_daily_task.ps1`"（可选，默认 09:10）"
Write-Host "  4) 挂件：  后台常驻，位置/透明度双击托盘图标调整"
Write-Host "安装完成。" -ForegroundColor Green
