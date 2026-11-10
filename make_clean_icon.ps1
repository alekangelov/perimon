Add-Type -AssemblyName System.Drawing

function Draw-MinimalistIcon([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)

    $scale = [float]$size / 256.0

    # 1. Black Rounded Square (Squircle)
    $sqPad = 14.0 * $scale
    $sqSize = 228.0 * $scale
    $sqRadius = 50.0 * $scale

    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $sqRadius * 2.0
    $r = New-Object System.Drawing.RectangleF $sqPad, $sqPad, $sqSize, $sqSize
    $path.AddArc($r.X, $r.Y, $d, $d, 180.0, 90.0)
    $path.AddArc(($r.Right - $d), $r.Y, $d, $d, 270.0, 90.0)
    $path.AddArc(($r.Right - $d), ($r.Bottom - $d), $d, $d, 0.0, 90.0)
    $path.AddArc($r.X, ($r.Bottom - $d), $d, $d, 90.0, 90.0)
    $path.CloseFigure()

    $bgBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 12, 14, 18))
    $borderPen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 45, 52, 68)), (2.5 * $scale)
    $g.FillPath($bgBrush, $path)
    $g.DrawPath($borderPen, $path)

    # 2. White Battery Shell
    $bX = 62.0 * $scale
    $bY = 96.0 * $scale
    $bW = 116.0 * $scale
    $bH = 64.0 * $scale
    $bRadius = 14.0 * $scale
    $bPenWidth = 8.0 * $scale

    $bPath = New-Object System.Drawing.Drawing2D.GraphicsPath
    $bd = $bRadius * 2.0
    $bR = New-Object System.Drawing.RectangleF $bX, $bY, $bW, $bH
    $bPath.AddArc($bR.X, $bR.Y, $bd, $bd, 180.0, 90.0)
    $bPath.AddArc(($bR.Right - $bd), $bR.Y, $bd, $bd, 270.0, 90.0)
    $bPath.AddArc(($bR.Right - $bd), ($bR.Bottom - $bd), $bd, $bd, 0.0, 90.0)
    $bPath.AddArc($bR.X, ($bR.Bottom - $bd), $bd, $bd, 90.0, 90.0)
    $bPath.CloseFigure()

    $whitePen = New-Object System.Drawing.Pen ([System.Drawing.Color]::White), $bPenWidth
    $g.DrawPath($whitePen, $bPath)

    # 3. Battery Terminal Cap
    $capX = ($bX + $bW + 2.0 * $scale)
    $capY = 114.0 * $scale
    $capW = 10.0 * $scale
    $capH = 28.0 * $scale
    $capRadius = 4.0 * $scale
    $capPath = New-Object System.Drawing.Drawing2D.GraphicsPath
    $cd = $capRadius * 2.0
    $capR = New-Object System.Drawing.RectangleF $capX, $capY, $capW, $capH
    $capPath.AddArc($capR.X, $capR.Y, $cd, $cd, 180.0, 90.0)
    $capPath.AddArc(($capR.Right - $cd), $capR.Y, $cd, $cd, 270.0, 90.0)
    $capPath.AddArc(($capR.Right - $cd), ($capR.Bottom - $cd), $cd, $cd, 0.0, 90.0)
    $capPath.AddArc($capR.X, ($capR.Bottom - $cd), $cd, $cd, 90.0, 90.0)
    $capPath.CloseFigure()

    $whiteBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::White)
    $g.FillPath($whiteBrush, $capPath)

    # 4. White Lightning Bolt
    $p1 = New-Object System.Drawing.PointF (125.0 * $scale), (106.0 * $scale)
    $p2 = New-Object System.Drawing.PointF (107.0 * $scale), (130.0 * $scale)
    $p3 = New-Object System.Drawing.PointF (120.0 * $scale), (130.0 * $scale)
    $p4 = New-Object System.Drawing.PointF (114.0 * $scale), (150.0 * $scale)
    $p5 = New-Object System.Drawing.PointF (137.0 * $scale), (124.0 * $scale)
    $p6 = New-Object System.Drawing.PointF (123.0 * $scale), (124.0 * $scale)
    $pts = [System.Drawing.PointF[]]@($p1, $p2, $p3, $p4, $p5, $p6)
    $g.FillPolygon($whiteBrush, $pts)

    $g.Dispose()
    return $bmp
}

$dir = "C:\Users\Alek\.gemini\antigravity\scratch\peripheral-battery-monitor-rs"

# Generate 256x256 PNG and ICO
$icon256 = Draw-MinimalistIcon 256
$icon256.Save((Join-Path $dir "app_icon.png"), [System.Drawing.Imaging.ImageFormat]::Png)

$hIcon = $icon256.GetHicon()
$iconObj = [System.Drawing.Icon]::FromHandle($hIcon)
$fs = [System.IO.File]::Create((Join-Path $dir "app_icon.ico"))
$iconObj.Save($fs)
$fs.Close()
$icon256.Dispose()

# Helper to dump raw RGBA
function Export-Rgba([int]$size, [string]$filename) {
    $bmp = Draw-MinimalistIcon $size
    $rect = New-Object System.Drawing.Rectangle 0, 0, $size, $size
    $bmpData = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $bytes = New-Object byte[] ($size * $size * 4)
    [System.Runtime.InteropServices.Marshal]::Copy($bmpData.Scan0, $bytes, 0, $bytes.Length)
    $bmp.UnlockBits($bmpData)
    $bmp.Dispose()

    # Convert GDI+ BGRA to RGBA
    for ($i = 0; $i -lt $bytes.Length; $i += 4) {
        $b = $bytes[$i]
        $r = $bytes[$i + 2]
        $bytes[$i] = $r
        $bytes[$i + 2] = $b
    }
    [System.IO.File]::WriteAllBytes($filename, $bytes)
}

Export-Rgba 32 (Join-Path $dir "tray_icon_32.rgba")
Export-Rgba 128 (Join-Path $dir "app_icon_128.rgba")

Write-Host "Created clean minimalist black & white icon assets successfully!" -ForegroundColor Green
