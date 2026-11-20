<#
  项目看板「稳态浸泡（soak）」探针：盯住看板进程在长时间挂机后是否还活着、
  有没有偷偷重启、内存/句柄有没有持续爬升、窗口是否卡死，并把逐次采样写成
  soak_report.json 供 V4 看板/整改验收直接读取，不依赖人守着任务管理器。

  探测什么：
    - 10 分钟稳态（默认 $Minutes=10）：足够跨过看板定时刷新/GC/WS 重连几个周期，
      短跑只能看到冷启动、看不出缓慢泄漏。
    - WS 内存阈值 $LimitMB=60（峰值 WorkingSet64 不得越线）、10 分钟内 WS 净增
      >15 MB 判 FAIL、句柄净增 >40 判 FAIL：这三条分别卡「绝对值」「内存泄漏
      趋势」「GDI/USER 句柄泄漏」；单看峰值会放过缓慢爬升，单看趋势会放过一次性吃满。
    - 进程 PID 中途变化 / 窗口 Responding=$false / 进程根本不存在：判重启或假死。
    - 采样间隔 30s（$IntervalSec=30）：10 分钟正好 20 个点，既能描出爬升曲线又
      不会因为高频 Get-Process 本身扰动被测进程；再密收益很小、再疏会漏掉单峰。

  参数：
    $Minutes    浸泡总时长（分钟），默认 10。
    $IntervalSec 每次采样间隔（秒），默认 30。
    $LimitMB    WorkingSet 峰值上限（MB），默认 60。
    $Out        报告输出路径，默认本脚本同目录 soak_report.json。

  注意：只观测、不干预看板进程；判定全部基于采样序列首/尾/峰值，不改任何运行逻辑。
#>
param(
  [int]$Minutes = 10,
  [int]$IntervalSec = 30,
  [int]$LimitMB = 60,
  [string]$Out = (Join-Path $PSScriptRoot 'soak_report.json')
)
$ErrorActionPreference = 'Continue'
$samples = @()
$start = Get-Date
$pid0 = $null
while (((Get-Date) - $start).TotalMinutes -lt $Minutes) {
  $p = Get-Process -Name 项目看板 -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($p) {
    if (-not $pid0) { $pid0 = $p.Id }
    $samples += [pscustomobject]@{
      t       = (Get-Date).ToString('HH:mm:ss')
      pid     = $p.Id
      ws      = [math]::Round($p.WorkingSet64 / 1MB, 1)
      priv    = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
      handles = $p.HandleCount
      threads = $p.Threads.Count
      resp    = $p.Responding
    }
  } else {
    $samples += [pscustomobject]@{ t = (Get-Date).ToString('HH:mm:ss'); pid = 0; ws = 0; priv = 0; handles = 0; threads = 0; resp = $false }
  }
  Start-Sleep -Seconds $IntervalSec
}
$alive = @($samples | Where-Object { $_.pid -ne 0 })
$verdict = 'PASS'
$reasons = @()
if ($alive.Count -eq 0) { $verdict = 'FAIL'; $reasons += '进程中不存在' }
if ($alive.Count -gt 0) {
  if ($pid0 -and ($alive[-1].pid -ne $pid0)) { $verdict = 'FAIL'; $reasons += "进程重启过 $pid0 -> $($alive[-1].pid)" }
  $peakWs = ($alive | Measure-Object ws -Maximum).Maximum
  if ($peakWs -gt $LimitMB) { $verdict = 'FAIL'; $reasons += "峰值内存 $peakWs MB > $LimitMB MB" }
  $first = $alive[0].ws; $last = $alive[-1].ws
  if ($first -gt 0 -and ($last - $first) -gt 15) { $verdict = 'FAIL'; $reasons += "10 分钟内存增长 $([math]::Round($last-$first,1)) MB" }
  $h0 = $alive[0].handles; $h1 = $alive[-1].handles
  if ($h1 - $h0 -gt 40) { $verdict = 'FAIL'; $reasons += "句柄增长 $($h1-$h0)" }
  if (-not $alive[-1].resp) { $verdict = 'FAIL'; $reasons += '窗口无响应' }
}
$rep = [pscustomobject]@{
  at       = (Get-Date).ToString('s')
  minutes  = $Minutes
  verdict  = $verdict
  reasons  = $reasons
  samples  = $samples
  summary  = [pscustomobject]@{
    wsStart = if ($alive.Count) { $alive[0].ws } else { 0 }
    wsEnd   = if ($alive.Count) { $alive[-1].ws } else { 0 }
    wsPeak  = if ($alive.Count) { ($alive | Measure-Object ws -Maximum).Maximum } else { 0 }
    hStart  = if ($alive.Count) { $alive[0].handles } else { 0 }
    hEnd    = if ($alive.Count) { $alive[-1].handles } else { 0 }
  }
}
$rep | ConvertTo-Json -Depth 5 | Out-File $Out -Encoding utf8
"SOAK $verdict $($reasons -join '; ') ws=$($rep.summary.wsStart)->$($rep.summary.wsEnd) peak=$($rep.summary.wsPeak) handles=$($rep.summary.hStart)->$($rep.summary.hEnd)"
