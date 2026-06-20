# 栗＋クリップボードのアイコンを生成して app.ico を作成する
Add-Type -AssemblyName System.Drawing

function New-RoundRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $p.AddArc($x, $y, $r, $r, 180, 90)
    $p.AddArc($x + $w - $r, $y, $r, $r, 270, 90)
    $p.AddArc($x + $w - $r, $y + $h - $r, $r, $r, 0, 90)
    $p.AddArc($x, $y + $h - $r, $r, $r, 90, 90)
    $p.CloseFigure()
    return $p
}

function C([int]$a, [int]$r, [int]$g, [int]$b) { [System.Drawing.Color]::FromArgb($a, $r, $g, $b) }

function Draw-Icon([System.Drawing.Graphics]$g, [int]$px) {
    $s = $px / 256.0
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.ScaleTransform($s, $s)

    # ===== 背景: 青の角丸スクエア =====
    $bg = New-RoundRect 6 6 244 244 58
    $bgBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.Point(0, 0)), (New-Object System.Drawing.Point(0, 256)),
        (C 255 104 196 226), (C 255 50 128 186))
    $g.FillPath($bgBrush, $bg)

    # ===== クリップボード: 台座 =====
    $board = New-RoundRect 58 38 140 188 16
    $g.FillPath((New-Object System.Drawing.SolidBrush(C 255 207 196 180)), $board)
    $penBoard = New-Object System.Drawing.Pen((C 255 138 124 104), 5)
    $g.DrawPath($penBoard, $board)

    # ===== 用紙 =====
    $paper = New-RoundRect 72 62 112 150 8
    $g.FillPath((New-Object System.Drawing.SolidBrush(C 255 250 250 250)), $paper)
    $penPaper = New-Object System.Drawing.Pen((C 255 215 215 215), 3)
    $g.DrawPath($penPaper, $paper)

    # 用紙の行 (栗の下に見える部分)
    $penLine = New-Object System.Drawing.Pen((C 255 185 192 202), 7)
    $penLine.StartCap = 'Round'; $penLine.EndCap = 'Round'
    $g.DrawLine($penLine, 90, 196, 166, 196)
    $g.DrawLine($penLine, 90, 207, 142, 207)

    # ===== クリップ金具 =====
    $clip = New-RoundRect 100 22 56 36 14
    $g.FillPath((New-Object System.Drawing.SolidBrush(C 255 201 206 214)), $clip)
    $penClip = New-Object System.Drawing.Pen((C 255 128 137 150), 4)
    $g.DrawPath($penClip, $clip)
    $g.FillEllipse((New-Object System.Drawing.SolidBrush(C 255 88 160 205)), 120, 31, 16, 14)
    $g.DrawEllipse($penClip, 120, 31, 16, 14)

    # ===== 栗 (くりのキャラクター) =====
    $kuri = New-Object System.Drawing.Drawing2D.GraphicsPath
    $pts = @(
        (New-Object System.Drawing.PointF(128, 66)),
        (New-Object System.Drawing.PointF(174, 92)),
        (New-Object System.Drawing.PointF(196, 142)),
        (New-Object System.Drawing.PointF(176, 180)),
        (New-Object System.Drawing.PointF(128, 192)),
        (New-Object System.Drawing.PointF(80, 180)),
        (New-Object System.Drawing.PointF(60, 142)),
        (New-Object System.Drawing.PointF(82, 92))
    )
    $kuri.AddClosedCurve($pts, 0.55)
    $kuriBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.Point(0, 60)), (New-Object System.Drawing.Point(0, 200)),
        (C 255 187 116 66), (C 255 145 82 42))
    $g.FillPath($kuriBrush, $kuri)
    $penKuri = New-Object System.Drawing.Pen((C 255 100 56 26), 7)
    $penKuri.LineJoin = 'Round'
    $g.DrawPath($penKuri, $kuri)

    # ===== 顔 =====
    # 眉
    $penBrow = [System.Drawing.Pen]::new((C 255 40 24 12), [single]9)
    $penBrow.StartCap = 'Round'; $penBrow.EndCap = 'Round'
    $g.DrawArc($penBrow, 82, 98, 40, 26, 195, 130)
    $g.DrawArc($penBrow, 134, 98, 40, 26, 215, 130)
    # 目
    $eyeBrush = New-Object System.Drawing.SolidBrush(C 255 35 21 11)
    $g.FillEllipse($eyeBrush, 93, 120, 19, 21)
    $g.FillEllipse($eyeBrush, 144, 120, 19, 21)
    $hlBrush = New-Object System.Drawing.SolidBrush(C 255 255 255 255)
    $g.FillEllipse($hlBrush, 98, 124, 6, 7)
    $g.FillEllipse($hlBrush, 149, 124, 6, 7)
    # 口 (にっこり)
    $penMouth = New-Object System.Drawing.Pen((C 255 60 34 16), 6)
    $penMouth.StartCap = 'Round'; $penMouth.EndCap = 'Round'
    $g.DrawArc($penMouth, 113, 136, 30, 22, 25, 130)
    # ほっぺ
    $blush = New-Object System.Drawing.SolidBrush(C 150 226 142 100)
    $g.FillEllipse($blush, 66, 136, 24, 14)
    $g.FillEllipse($blush, 166, 136, 24, 14)
}

function Render-Size([int]$px) {
    $bmp = New-Object System.Drawing.Bitmap($px, $px, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    Draw-Icon $g $px
    $g.Dispose()
    return $bmp
}

# ===== ICO ファイル組み立て (256=PNG, 48/32/16=BMP) =====
$sizes = @(256, 48, 32, 16)
$images = @{}
foreach ($px in $sizes) { $images[$px] = Render-Size $px }

# プレビュー保存
$images[256].Save("$PSScriptRoot\icon_preview.png", [System.Drawing.Imaging.ImageFormat]::Png)

$entries = @()
foreach ($px in $sizes) {
    $bmp = $images[$px]
    if ($px -eq 256) {
        $ms = New-Object System.IO.MemoryStream
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $entries += ,@($px, $ms.ToArray())
    } else {
        $ms = New-Object System.IO.MemoryStream
        $bw = New-Object System.IO.BinaryWriter($ms)
        $bw.Write([int32]40); $bw.Write([int32]$px); $bw.Write([int32]($px * 2)); $bw.Write([int16]1)
        $bw.Write([int16]32); $bw.Write([int32]0); $bw.Write([int32]($px * $px * 4))
        $bw.Write([int32]0); $bw.Write([int32]0); $bw.Write([int32]0); $bw.Write([int32]0)
        for ($y = $px - 1; $y -ge 0; $y--) {
            for ($x = 0; $x -lt $px; $x++) {
                $c = $bmp.GetPixel($x, $y)
                $bw.Write([byte]$c.B); $bw.Write([byte]$c.G); $bw.Write([byte]$c.R); $bw.Write([byte]$c.A)
            }
        }
        $maskRow = [int]([Math]::Ceiling($px / 32.0) * 4)
        $bw.Write((New-Object byte[] ($maskRow * $px)))
        $entries += ,@($px, $ms.ToArray())
    }
}

$out = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter($out)
$w.Write([int16]0); $w.Write([int16]1); $w.Write([int16]$entries.Count)
$offset = 6 + 16 * $entries.Count
foreach ($e in $entries) {
    $px = $e[0]; $bytes = $e[1]
    $dim = if ($px -ge 256) { 0 } else { $px }
    $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([int16]1); $w.Write([int16]32)
    $w.Write([int32]$bytes.Length); $w.Write([int32]$offset)
    $offset += $bytes.Length
}
foreach ($e in $entries) { $w.Write($e[1]) }
[System.IO.File]::WriteAllBytes("$PSScriptRoot\app.ico", $out.ToArray())
"app.ico written: $($out.Length) bytes"
