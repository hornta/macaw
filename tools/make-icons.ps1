# Renders assets/macaw.ico (active) and assets/macaw-paused.ico: a white "M" on a rounded
# square, in every size Windows asks for (PNG-compressed icon entries).
param([string]$OutDir = (Join-Path $PSScriptRoot '..\assets'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

function New-IconPng([int]$size, [System.Drawing.Color]$top, [System.Drawing.Color]$bottom, [int]$glyphAlpha) {
  $bmp = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = 'AntiAlias'
  $g.PixelOffsetMode = 'HighQuality'

  $radius = [Math]::Max(2, [int]($size * 0.22))
  $d = 2 * $radius
  $w = $size - 1
  $shape = New-Object System.Drawing.Drawing2D.GraphicsPath
  $shape.AddArc(0, 0, $d, $d, 180, 90)
  $shape.AddArc($w - $d, 0, $d, $d, 270, 90)
  $shape.AddArc($w - $d, $w - $d, $d, $d, 0, 90)
  $shape.AddArc(0, $w - $d, $d, $d, 90, 90)
  $shape.CloseFigure()
  $fill = New-Object System.Drawing.Drawing2D.LinearGradientBrush (New-Object System.Drawing.Point 0, 0), (New-Object System.Drawing.Point 0, $size), $top, $bottom
  $g.FillPath($fill, $shape)

  $glyph = New-Object System.Drawing.Drawing2D.GraphicsPath
  $family = New-Object System.Drawing.FontFamily 'Segoe UI'
  $glyph.AddString('M', $family, [int][System.Drawing.FontStyle]::Bold, [single]100, (New-Object System.Drawing.PointF 0, 0), [System.Drawing.StringFormat]::GenericTypographic)
  $b = $glyph.GetBounds()
  $scale = [Math]::Min(($size * 0.64) / $b.Width, ($size * 0.54) / $b.Height)
  $m = New-Object System.Drawing.Drawing2D.Matrix
  $m.Translate(-($b.X + $b.Width / 2), -($b.Y + $b.Height / 2), [System.Drawing.Drawing2D.MatrixOrder]::Append)
  $m.Scale($scale, $scale, [System.Drawing.Drawing2D.MatrixOrder]::Append)
  $m.Translate($size / 2, $size / 2 + $size * 0.01, [System.Drawing.Drawing2D.MatrixOrder]::Append)
  $glyph.Transform($m)
  $g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb($glyphAlpha, 255, 255, 255))), $glyph)
  $g.Dispose()

  $ms = New-Object System.IO.MemoryStream
  $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  , $ms.ToArray()
}

function Write-Ico([string]$path, $pngs, [int[]]$sizes) {
  $stream = [System.IO.File]::Create($path)
  $out = New-Object System.IO.BinaryWriter $stream
  $out.Write([uint16]0); $out.Write([uint16]1); $out.Write([uint16]$pngs.Count)
  $offset = 6 + 16 * $pngs.Count
  for ($i = 0; $i -lt $pngs.Count; $i++) {
    $dim = if ($sizes[$i] -ge 256) { 0 } else { $sizes[$i] }
    $out.Write([byte]$dim); $out.Write([byte]$dim); $out.Write([byte]0); $out.Write([byte]0)
    $out.Write([uint16]1); $out.Write([uint16]32)
    $out.Write([uint32]$pngs[$i].Length); $out.Write([uint32]$offset)
    $offset += $pngs[$i].Length
  }
  foreach ($png in $pngs) { $out.Write([byte[]]$png) }
  $out.Close()
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$red = foreach ($s in $sizes) { , (New-IconPng $s ([System.Drawing.Color]::FromArgb(255, 240, 72, 56)) ([System.Drawing.Color]::FromArgb(255, 190, 24, 52)) 255) }
$grey = foreach ($s in $sizes) { , (New-IconPng $s ([System.Drawing.Color]::FromArgb(255, 152, 157, 165)) ([System.Drawing.Color]::FromArgb(255, 104, 109, 118)) 230) }
Write-Ico (Join-Path $OutDir 'macaw.ico') $red $sizes
Write-Ico (Join-Path $OutDir 'macaw-paused.ico') $grey $sizes
[System.IO.File]::WriteAllBytes((Join-Path $OutDir 'macaw-256.png'), $red[-1])
'icons written to ' + (Resolve-Path $OutDir)
