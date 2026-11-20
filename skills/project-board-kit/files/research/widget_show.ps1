# 桌面挂件 v5：WPF 分层窗口 + 纯 alpha 深色玻璃（壁纸实时透出，不做假模糊）+ 干净圆角
param([switch]$BottomRight, [int]$X = -1, [int]$Y = -1)

$ErrorActionPreference = 'Stop'
$ROOT = $PSScriptRoot
while ($ROOT -and -not (Test-Path (Join-Path $ROOT '每日项目简报.ps1'))) {
  $parent = Split-Path $ROOT -Parent
  if (-not $parent -or $parent -eq $ROOT) { break }
  $ROOT = $parent
}
$DATA = Join-Path $ROOT 'research\widget_data.json'
$BG   = Join-Path $env:TEMP 'frd_widget_bg.png'

Add-Type -AssemblyName PresentationFramework, PresentationCore, WindowsBase, System.Xaml, System.Drawing

$d = Get-Content $DATA -Raw -Encoding UTF8 | ConvertFrom-Json
function E($s) { [System.Security.SecurityElement]::Escape([string]$s) }

$WinW = 980; $WinH = 566
if ($BottomRight) {
  $wa = [System.Windows.SystemParameters]::WorkArea
  $X = [int]($wa.Right - $WinW - 24); $Y = [int]($wa.Bottom - $WinH - 16)
}
if ($X -lt 0) { $X = 40 }; if ($Y -lt 0) { $Y = 40 }

function Grab-Background([int]$x, [int]$y, [int]$w, [int]$h, [string]$path) {
  $bmp = New-Object System.Drawing.Bitmap($w, $h)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($x, $y, 0, 0, (New-Object System.Drawing.Size($w, $h)))
  $g.Dispose()
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
}
Grab-Background $X $Y $WinW $WinH $BG
$bgSrc = $BG.Replace('\', '/')

$prioColor = @{ 'P0' = '#FF6B61'; 'P1' = '#FFC247'; 'P2' = '#5AA9FF' }

$sb = New-Object System.Text.StringBuilder
foreach ($n in $d.todo) {
  $pc = $prioColor[$n.prio]; if (-not $pc) { $pc = '#5AA9FF' }
  $badge = if ($n.dd -eq 0) { '今天' } elseif ($n.dd -eq 1) { '明天' } else { "还有 $($n.dd) 天" }
  $urgent = ($n.dd -le 1)
  $badgeBg = if ($urgent) { '#FF9F43' } else { '#33FFFFFF' }
  $badgeFg = if ($urgent) { '#1A1208' } else { '#EDF1F8' }
  [void]$sb.Append(@"
<Grid Margin="0,4,0,4">
  <Grid.ColumnDefinitions><ColumnDefinition Width="12"/><ColumnDefinition Width="Auto" MinWidth="78"/><ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/></Grid.ColumnDefinitions>
  <Ellipse Width="7" Height="7" Fill="$pc" VerticalAlignment="Center"/>
  <TextBlock Grid.Column="1" Text="$(E $n.model)" Foreground="#FFFFFF" FontFamily="Cascadia Mono,Consolas" FontSize="13" VerticalAlignment="Center"/>
  <TextBlock Grid.Column="2" Text="$(E $n.name)" Foreground="#E9EEF7" FontSize="13" VerticalAlignment="Center" TextTrimming="CharacterEllipsis" Margin="10,0"/>
  <Border Grid.Column="3" Background="$badgeBg" CornerRadius="6" Padding="8,3">
    <TextBlock Text="$(E $badge)" FontSize="11" Foreground="$badgeFg"/>
  </Border>
</Grid>
"@)
}

$p0x = New-Object System.Text.StringBuilder
foreach ($p in $d.p0) {
  $bl = [string]$p.blocker; if ($bl.Length -gt 58) { $bl = $bl.Substring(0, 58) + '…' }
  [void]$p0x.Append(@"
<Grid Margin="0,5,0,5">
  <Grid.ColumnDefinitions><ColumnDefinition Width="30"/><ColumnDefinition Width="Auto" MinWidth="82"/><ColumnDefinition Width="*"/></Grid.ColumnDefinitions>
  <Border Background="#FF6B61" CornerRadius="5" Padding="0,2" VerticalAlignment="Top"><TextBlock Text="P0" FontSize="10" FontWeight="Bold" Foreground="#1A1208" HorizontalAlignment="Center"/></Border>
  <TextBlock Grid.Column="1" Text="$(E $p.model)" Foreground="#FFFFFF" FontFamily="Cascadia Mono,Consolas" FontSize="12.5" VerticalAlignment="Top" Margin="6,0,10,0"/>
  <TextBlock Grid.Column="2" Text="$(E $bl)" Foreground="#E9EEF7" FontSize="11.5" TextWrapping="Wrap" LineHeight="16" LineStackingStrategy="BlockLineHeight" MaxHeight="50"/>
</Grid>
"@)
}

$cal = New-Object System.Text.StringBuilder
foreach ($wk in @('一','二','三','四','五','六','日')) {
  [void]$cal.Append("<Border Height=""22""><TextBlock Text=""$wk"" FontSize=""11"" Foreground=""#B3BCCA"" HorizontalAlignment=""Center"" VerticalAlignment=""Center""/></Border>")
}
$startPad = ([int]([datetime]::new([int]$d.cal.y, [int]$d.cal.mo, 1).DayOfWeek) + 6) % 7
for ($i = 0; $i -lt $startPad; $i++) { [void]$cal.Append('<Border Height="36"/>') }
for ($day = 1; $day -le [int]$d.cal.daysIn; $day++) {
  $key = '{0}-{1:D2}-{2:D2}' -f [int]$d.cal.y, [int]$d.cal.mo, $day
  $isT = ($day -eq [int]$d.todayDay)
  $pr = @()
  if ($d.cal.perDay.PSObject.Properties.Name -contains $key) { $pr = @($d.cal.perDay.$key) }
  $has = $pr.Count -gt 0
  $bg = '#00000000'; $fg = '#D8DFEA'
  if ($has) { $bg = '#26FFFFFF'; $fg = '#FFFFFF' }
  if ($isT) { $bg = '#FF9F43'; $fg = '#1A1208' }
  $dots = ''
  if ($has) {
    $dcol = if ($isT) { '#1A1208' } elseif ($pr -contains 'P0') { '#FF6B61' } else { '#7CC0FF' }
    $dots = '<StackPanel Orientation="Horizontal" HorizontalAlignment="Center" Margin="0,2,0,0">'
    foreach ($k in 1..([Math]::Min(3, $pr.Count))) { $dots += "<Ellipse Width=""3.5"" Height=""3.5"" Fill=""$dcol"" Margin=""1,0""/>" }
    $dots += '</StackPanel>'
  }
  [void]$cal.Append(@"
<Border Margin="1" CornerRadius="8" Height="36" Background="$bg">
  <StackPanel VerticalAlignment="Center">
    <TextBlock Text="$day" FontSize="12.5" Foreground="$fg" HorizontalAlignment="Center"/>
    $dots
  </StackPanel>
</Border>
"@)
}
$tailCount = (7 - (($startPad + [int]$d.cal.daysIn) % 7)) % 7
for ($i = 0; $i -lt $tailCount; $i++) { [void]$cal.Append('<Border Height="36"/>') }

$xaml = @"
<Window xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
        xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
        Title="项目挂件" Width="$WinW" Height="$WinH" WindowStyle="None" AllowsTransparency="True"
        Background="Transparent" ShowInTaskbar="False" ResizeMode="NoResize" Topmost="False"
        FontFamily="Microsoft YaHei UI" UseLayoutRounding="True" SnapsToDevicePixels="True" Left="$X" Top="$Y">
  <Window.Resources>
    <Style TargetType="TextBlock">
      <Setter Property="Effect"><Setter.Value><DropShadowEffect BlurRadius="3" ShadowDepth="0" Opacity="0.55" Color="#000000"/></Setter.Value></Setter>
    </Style>
  </Window.Resources>
  <Grid x:Name="root">
    <Grid.Clip><RectangleGeometry RadiusX="18" RadiusY="18" Rect="0,0,$WinW,$WinH"/></Grid.Clip>
    <Rectangle RadiusX="18" RadiusY="18">
      <Rectangle.Fill>
        <LinearGradientBrush StartPoint="0,0" EndPoint="1,1">
          <GradientStop Color="#C8121420" Offset="0"/>
          <GradientStop Color="#BC161A24" Offset="0.45"/>
          <GradientStop Color="#CE0E1016" Offset="1"/>
        </LinearGradientBrush>
      </Rectangle.Fill>
    </Rectangle>
    <Border CornerRadius="18" BorderThickness="1" BorderBrush="#3AFFFFFF" Padding="22,16,22,12">
      <Grid>
        <Grid.RowDefinitions><RowDefinition Height="Auto"/><RowDefinition Height="*"/><RowDefinition Height="Auto"/></Grid.RowDefinitions>
        <Grid.ColumnDefinitions><ColumnDefinition Width="1.06*"/><ColumnDefinition Width="0.94*"/></Grid.ColumnDefinitions>

        <StackPanel Grid.Row="0" Grid.ColumnSpan="2" Orientation="Horizontal" Margin="0,0,0,12">
          <TextBlock Text="$($d.todayDay)" FontSize="44" FontWeight="Bold" Foreground="#FFFFFF" VerticalAlignment="Center"/>
          <TextBlock Text="$(E $d.week) · $(E $d.ym)" FontSize="13" Foreground="#DCE3EF" VerticalAlignment="Bottom" Margin="12,0,0,8"/>
          <StackPanel Orientation="Horizontal" HorizontalAlignment="Right" VerticalAlignment="Center">
            <Border Background="#26FFFFFF" CornerRadius="12" Padding="11,5" Margin="6,0,0,0"><TextBlock Text="在管 $($d.counts.projects)" FontSize="11" Foreground="#EDF1F8"/></Border>
            <Border Background="#FF9F43" CornerRadius="12" Padding="11,5" Margin="6,0,0,0"><TextBlock Text="P0 $($d.counts.p0)" FontSize="11" FontWeight="Bold" Foreground="#1A1208"/></Border>
            <Border Background="#26FFFFFF" CornerRadius="12" Padding="11,5" Margin="6,0,0,0"><TextBlock Text="近 7 天节点 $($d.counts.nodes7)" FontSize="11" Foreground="#EDF1F8"/></Border>
          </StackPanel>
        </StackPanel>

        <Border Grid.Row="1" Grid.Column="0" Background="#14FFFFFF" CornerRadius="14" Padding="14,12" Margin="0,0,9,0" BorderThickness="1" BorderBrush="#2EFFFFFF">
          <Grid>
            <Grid.RowDefinitions><RowDefinition Height="Auto"/><RowDefinition Height="*"/><RowDefinition Height="Auto"/></Grid.RowDefinitions>
            <StackPanel Orientation="Horizontal" Margin="0,0,0,8">
              <Ellipse Width="6" Height="6" Fill="#FF9F43" VerticalAlignment="Center"/>
              <TextBlock Text="TODO · 未来 10 天" FontSize="11" Foreground="#B3BCCA" Margin="8,0,0,0"/>
            </StackPanel>
            <StackPanel Grid.Row="1">$($sb.ToString())</StackPanel>
            <StackPanel Grid.Row="2" Margin="0,10,0,0">
              <StackPanel Orientation="Horizontal" Margin="0,0,0,5">
                <Ellipse Width="6" Height="6" Fill="#FF6B61" VerticalAlignment="Center"/>
                <TextBlock Text="P0 卡点" FontSize="11" Foreground="#B3BCCA" Margin="8,0,0,0"/>
              </StackPanel>
              $($p0x.ToString())
            </StackPanel>
          </Grid>
        </Border>

        <Border Grid.Row="1" Grid.Column="1" Background="#14FFFFFF" CornerRadius="14" Padding="14,12" Margin="9,0,0,0" BorderThickness="1" BorderBrush="#2EFFFFFF">
          <Grid>
            <Grid.RowDefinitions><RowDefinition Height="Auto"/><RowDefinition Height="*"/></Grid.RowDefinitions>
            <StackPanel Orientation="Horizontal" Margin="0,0,0,8">
              <Ellipse Width="6" Height="6" Fill="#FF9F43" VerticalAlignment="Center"/>
              <TextBlock Text="$($d.cal.mo) 月节点" FontSize="11" Foreground="#B3BCCA" Margin="8,0,0,0"/>
            </StackPanel>
            <UniformGrid Grid.Row="1" Columns="7" VerticalAlignment="Top">$($cal.ToString())</UniformGrid>
          </Grid>
        </Border>

        <TextBlock Grid.Row="2" Grid.ColumnSpan="2" Margin="0,10,0,0" FontSize="10.5" Foreground="#C2CAD7"
                   Text="$(E $d.stamp)   ·   双击打开完整看板   ·   背景每 60 秒重新采样"/>
      </Grid>
    </Border>
  </Grid>
</Window>
"@

$win = [System.Windows.Markup.XamlReader]::Parse($xaml)
$win.add_MouseLeftButtonDown({ try { $win.DragMove() } catch {} })
$win.add_MouseDoubleClick({ try { Start-Process (Join-Path $ROOT '项目数据看板.html') } catch {} })
$win.WindowStartupLocation = 'Manual'
$win.Left = $X; $win.Top = $Y
$win.Show()

$bgImg = $win.FindName('bg')
$counter = 0
$reBg = New-Object System.Windows.Threading.DispatcherTimer
$reBg.Interval = [TimeSpan]::FromSeconds(60)
$reBg.add_Tick({
  try {
    $script:counter++
    $tmp = Join-Path $env:TEMP ("frd_widget_bg_{0}.png" -f $script:counter)
    Grab-Background $X $Y $WinW $WinH $tmp
    $bmp = New-Object System.Windows.Media.Imaging.BitmapImage
    $bmp.BeginInit(); $bmp.UriSource = [uri]$tmp; $bmp.CacheOption = 'OnLoad'; $bmp.EndInit()
    $bgImg.Source = $bmp
  } catch {}
})
$reBg.Start()

$sig = @'
using System;
using System.Runtime.InteropServices;
public class WW2 {
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern int SetWindowLong(IntPtr h, int i, int v);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint f);
}
'@
if (-not ('WW2' -as [type])) { Add-Type -TypeDefinition $sig }
$hwnd = (New-Object System.Windows.Interop.WindowInteropHelper $win).Handle
$ex = [WW2]::GetWindowLong($hwnd, -20)
[void][WW2]::SetWindowLong($hwnd, -20, ($ex -bor 0x80 -bor 0x08000000))
[void][WW2]::SetWindowPos($hwnd, [IntPtr]0, 0, 0, 0, 0, 0x0013)

Start-Sleep -Milliseconds 400
Write-Host ("WIDGET4_OK at={0},{1} size={2}x{3}" -f $X, $Y, $WinW, $WinH)
[System.Windows.Threading.Dispatcher]::Run()
