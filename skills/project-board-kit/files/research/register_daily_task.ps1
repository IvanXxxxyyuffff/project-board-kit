# 注册 Windows 计划任务：每个工作日（周一至周五）09:05 跑「每日项目简报.ps1」
# 用法：pwsh -NoProfile -File <项目根>\research\register_daily_task.ps1
$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
while ($Root -and -not (Test-Path (Join-Path $Root '每日项目简报.ps1'))) {
  $parent = Split-Path $Root -Parent
  if (-not $parent -or $parent -eq $Root) { break }
  $Root = $parent
}
$script = Join-Path $Root '每日项目简报.ps1'
$pwsh   = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\pwsh.exe'

$action = New-ScheduledTaskAction -Execute $pwsh `
  -Argument ('-NoProfile -ExecutionPolicy Bypass -File "' + $script + '" -NoOpen') `
  -WorkingDirectory $Root

# 每个工作日 09:05（周末不跑）
$trigger = New-ScheduledTaskTrigger -Weekly -WeeksInterval 1 `
  -DaysOfWeek Monday,Tuesday,Wednesday,Thursday,Friday -At '09:05'

$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 10) `
  -DontStopIfGoingOnBatteries -AllowStartIfOnBatteries
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited

$name = 'SELF_CO-Daily-Brief'
try { Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue } catch {}
Register-ScheduledTask -TaskName $name -Action $action -Trigger $trigger `
  -Settings $settings -Principal $principal `
  -Description 'CLIENT_A 项目部：每个工作日 09:05 抓微信表格 + E-Mobile 全量消息，生成项目看板 .html/.md' | Out-Null

$t = Get-ScheduledTask -TaskName $name
$i = Get-ScheduledTaskInfo -TaskName $name
Write-Host ("REGISTERED {0} | state={1} | next={2} | triggers={3}" -f $name, $t.State, $i.NextRunTime, $t.Triggers.Count)
