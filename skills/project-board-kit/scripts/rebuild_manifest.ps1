<#
  重建 files/manifest.json（本机化后使用）
  用法：pwsh -File scripts\rebuild_manifest.ps1 -Root <安装根>
#>
param([Parameter(Mandatory = $true)][string]$Root)
$ErrorActionPreference = 'Stop'
$kit = Join-Path $Root 'skills\project-board-kit'
$files = Join-Path $kit 'files'
if (-not (Test-Path $files)) { throw "找不到 $files" }

function Get-NormBytes([string]$p) {
  # BIN：二进制不做 BOM/换行归一（否则 hash 与实体不符）
  $ext = [IO.Path]::GetExtension($p).ToLower()
  if (@('.whl', '.dll', '.exe', '.zip', '.png', '.jpg', '.jpeg', '.ico', '.pdf', '.pyd', '.so') -contains $ext) {
    return [IO.File]::ReadAllBytes($p)
  }
  $txt = [IO.File]::ReadAllText($p, [Text.UTF8Encoding]::new($false))
  if ($txt.Length -gt 0 -and $txt[0] -eq [char]0xFEFF) { $txt = $txt.Substring(1) }
  $txt = $txt.Replace("`r`n", "`n")
  return [Text.Encoding]::UTF8.GetBytes($txt)
}

$map = [ordered]@{}
Get-ChildItem $files -Recurse -File | ForEach-Object {
  $rel = $_.FullName.Substring($files.Length + 1).Replace('\', '/')
  $len = $_.Length
  if ($len -gt 4194304) { return }
  $b = Get-NormBytes $_.FullName
  $h = [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($b)).Replace('-', '').ToLower()
  $map[$rel] = [ordered]@{ sha256 = $h; bytes = $b.Length }
}
$out = [ordered]@{ at = (Get-Date).ToString('yyyy-MM-dd HH:mm'); files = $map }
$json = $out | ConvertTo-Json -Depth 5
[IO.File]::WriteAllText((Join-Path $files 'manifest.json'), $json, [Text.UTF8Encoding]::new($false))
Write-Host ("MANIFEST_REBUILT files=" + $map.Count)
