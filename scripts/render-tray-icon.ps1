# Export the tray's simple vector artwork at native Windows icon sizes.
# Uses only System.Drawing; PNGs are checked in, so ordinary builds need no renderer.
[CmdletBinding()]
param(
    [string]$Source,
    [string]$OutputDirectory,
    [string]$PreviewPath
)
$ErrorActionPreference = 'Stop'
if (!$Source) { $Source = Join-Path $PSScriptRoot '../native/CodexLidGuard/Assets/Tray/lid-guard.svg' }
if (!$OutputDirectory) { $OutputDirectory = Join-Path $PSScriptRoot '../native/CodexLidGuard/Assets/Tray' }
Add-Type -AssemblyName System.Drawing
[xml]$artwork = Get-Content -LiteralPath $Source -Raw
$culture = [Globalization.CultureInfo]::InvariantCulture
function Number([string]$value) { [float]::Parse($value, $culture) }
function Attribute($element, [string]$name, [string]$fallback) {
    $value = $element.GetAttribute($name)
    if ($value) { $value } else { $fallback }
}
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$sizes = @(16, 20, 24, 28, 32, 40, 48, 64)
foreach ($size in $sizes) {
    $bitmap = New-Object Drawing.Bitmap $size, $size, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.Clear([Drawing.Color]::Transparent)
        $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $graphics.ScaleTransform(($size / 16.0), ($size / 16.0))
        foreach ($element in $artwork.DocumentElement.ChildNodes) {
            if ($element.NodeType -ne [Xml.XmlNodeType]::Element) { continue }
            $shape = New-Object Drawing.Drawing2D.GraphicsPath
            try {
                switch ($element.LocalName) {
                    'polygon' {
                        [Drawing.PointF[]]$points = @($element.points.Trim() -split '\s+' | ForEach-Object {
                            $pair = $_ -split ','
                            New-Object Drawing.PointF (Number $pair[0]), (Number $pair[1])
                        })
                        $shape.AddPolygon($points)
                    }
                    'circle' {
                        $radius = Number $element.r
                        $shape.AddEllipse((Number $element.cx) - $radius, (Number $element.cy) - $radius, 2 * $radius, 2 * $radius)
                    }
                    'line' {
                        $shape.AddLine((Number $element.x1), (Number $element.y1), (Number $element.x2), (Number $element.y2))
                    }
                    default { throw "Unsupported tray artwork element: $($element.LocalName)" }
                }
                if ($element.HasAttribute('fill')) {
                    $brush = New-Object Drawing.SolidBrush ([Drawing.ColorTranslator]::FromHtml($element.fill))
                    try { $graphics.FillPath($brush, $shape) } finally { $brush.Dispose() }
                }
                if ($element.HasAttribute('stroke')) {
                    $pen = New-Object Drawing.Pen ([Drawing.ColorTranslator]::FromHtml($element.stroke)), (Number (Attribute $element 'stroke-width' '1'))
                    try {
                        $pen.LineJoin = [Drawing.Drawing2D.LineJoin]::Round
                        $pen.StartCap = $pen.EndCap = [Drawing.Drawing2D.LineCap]::Round
                        $graphics.DrawPath($pen, $shape)
                    } finally { $pen.Dispose() }
                }
            } finally { $shape.Dispose() }
        }
        $bitmap.Save((Join-Path $OutputDirectory "lid-guard-$size.png"), [Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}
if ($PreviewPath) {
    $previewWidth = $sizes.Length * 105 + 30
    $preview = New-Object Drawing.Bitmap $previewWidth, 220
    $graphics = [Drawing.Graphics]::FromImage($preview)
    $font = New-Object Drawing.Font 'Segoe UI', 10
    try {
        foreach ($row in 0..1) {
            $background = if ($row -eq 0) { '#292929' } else { '#F2F2F2' }
            $foreground = if ($row -eq 0) { '#FFFFFF' } else { '#202020' }
            $brush = New-Object Drawing.SolidBrush ([Drawing.ColorTranslator]::FromHtml($background))
            try { $graphics.FillRectangle($brush, 0, $row * 110, $previewWidth, 110) } finally { $brush.Dispose() }
            $textBrush = New-Object Drawing.SolidBrush ([Drawing.ColorTranslator]::FromHtml($foreground))
            try {
                for ($index = 0; $index -lt $sizes.Length; $index++) {
                    $size = $sizes[$index]
                    $icon = [Drawing.Image]::FromFile((Join-Path $OutputDirectory "lid-guard-$size.png"))
                    try { $graphics.DrawImageUnscaled($icon, 50 + $index * 105 - [int]($size / 2), $row * 110 + 18) }
                    finally { $icon.Dispose() }
                    $graphics.DrawString("$size px", $font, $textBrush, 32 + $index * 105, $row * 110 + 77)
                }
            } finally { $textBrush.Dispose() }
        }
        $preview.Save($PreviewPath, [Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $font.Dispose(); $preview.Dispose() }
}
Write-Output "Rendered tray icon sizes: $($sizes -join ', ')"
