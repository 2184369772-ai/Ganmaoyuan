$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$root = Join-Path $repoRoot "var\acceptance\checkpoint-a\source 中文 空格 长路径\第一层目录\第二层目录"
$null = New-Item -ItemType Directory -Path $root -Force

[System.IO.File]::WriteAllText(
    (Join-Path $root "01-UTF8 项目说明.txt"),
    "项目目标：验证感冒院中文长路径和 UTF-8 资料解析。`r`n主要角色：项目负责人、业务用户。",
    [System.Text.UTF8Encoding]::new($false)
)
[System.IO.File]::WriteAllText(
    (Join-Path $root "02-GBK 需求说明.txt"),
    "项目目标：验证 GBK 中文资料解析。`r`n下一步：综合全部资料形成项目理解。",
    [System.Text.Encoding]::GetEncoding(936)
)
[System.IO.File]::WriteAllText(
    (Join-Path $root "03-UTF16 决策记录.txt"),
    "重要决定：原文件不移动、不覆盖，所有受管副本保留来源路径。",
    [System.Text.Encoding]::Unicode
)
[System.IO.File]::WriteAllText(
    (Join-Path $root "04-资料清单.csv"),
    "名称,用途,负责人`r`n需求文档,定义范围,张三`r`n数据表格,""包含,逗号的说明"",李四",
    [System.Text.UTF8Encoding]::new($true)
)
[System.IO.File]::WriteAllText(
    (Join-Path $root "05-任务清单.tsv"),
    "任务`t状态`t下一步`r`n导入资料`t进行中`t检查摘要",
    [System.Text.UTF8Encoding]::new($false)
)
[System.IO.File]::WriteAllText(
    (Join-Path $root "06-重复资料.txt"),
    "项目目标：验证感冒院中文长路径和 UTF-8 资料解析。`r`n主要角色：项目负责人、业务用户。",
    [System.Text.UTF8Encoding]::new($false)
)
[System.IO.File]::WriteAllText(
    (Join-Path $root "01-UTF8 项目说明-v2.txt"),
    "项目目标：验证感冒院中文长路径、版本关系和恢复点。`r`n新增要求：接入项目级工作历史。",
    [System.Text.UTF8Encoding]::new($false)
)
[System.IO.File]::WriteAllText(
    (Join-Path $root "损坏资料.pdf"),
    "%PDF-1.4`nthis is intentionally damaged",
    [System.Text.Encoding]::ASCII
)

Add-Type -AssemblyName System.Drawing
$bitmapPath = Join-Path $root "09-扫描需求图片.png"
$bitmap = [System.Drawing.Bitmap]::new(1800, 1000)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$font = $null
try {
    $graphics.Clear([System.Drawing.Color]::White)
    $font = [System.Drawing.Font]::new("Microsoft YaHei", 48, [System.Drawing.FontStyle]::Regular)
    $graphics.DrawString("感冒院扫描资料验收", $font, [System.Drawing.Brushes]::Black, 120, 180)
    $graphics.DrawString("项目目标：自动识别图片中的中文需求", $font, [System.Drawing.Brushes]::Black, 120, 320)
    $graphics.DrawString("下一步：形成项目理解并列出待确认问题", $font, [System.Drawing.Brushes]::Black, 120, 460)
    $bitmap.Save($bitmapPath, [System.Drawing.Imaging.ImageFormat]::Png)
}
finally {
    if ($font) { $font.Dispose() }
    $graphics.Dispose()
    $bitmap.Dispose()
}

$existingRoot = $env:GANMAOYUAN_ACCEPTANCE_FIXTURE_ROOT
if ([string]::IsNullOrWhiteSpace($existingRoot)) {
    throw "请设置 GANMAOYUAN_ACCEPTANCE_FIXTURE_ROOT 指向已授权的受管资料目录。"
}
$existingFiles = @{
    "06-结构化需求.docx" = Join-Path $existingRoot "02_requirements\supOS-Free_KEBA_OPCUA_修正版.docx"
    "07-文本需求.pdf" = Join-Path $existingRoot "02_requirements\QR-M215.13-2025电子设备管理规定（发布版）- 20250325.pdf"
    "07-旧版数据.xls" = Join-Path $existingRoot "03_data\202601.xls"
    "08-复杂数据.xlsx" = Join-Path $existingRoot "03_data\RIC - 复制零件 - 项目 260723-162832.xlsx"
}
foreach ($entry in $existingFiles.GetEnumerator()) {
    if (-not (Test-Path -LiteralPath $entry.Value -PathType Leaf)) {
        throw "缺少现有真实验收文件：$($entry.Value)"
    }
    Copy-Item -LiteralPath $entry.Value -Destination (Join-Path $root $entry.Key) -Force
}

Add-Type -AssemblyName System.IO.Compression
function New-ZipDocument {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][hashtable]$Entries
    )
    if (Test-Path -LiteralPath $Path) {
        Remove-Item -LiteralPath $Path -Force
    }
    $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::CreateNew)
    $archive = [System.IO.Compression.ZipArchive]::new(
        $stream,
        [System.IO.Compression.ZipArchiveMode]::Create,
        $false
    )
    try {
        foreach ($entry in $Entries.GetEnumerator()) {
            $zipEntry = $archive.CreateEntry($entry.Key)
            $entryStream = $zipEntry.Open()
            $writer = [System.IO.StreamWriter]::new(
                $entryStream,
                [System.Text.UTF8Encoding]::new($false)
            )
            try {
                $writer.Write([string]$entry.Value)
            }
            finally {
                $writer.Dispose()
            }
        }
    }
    finally {
        $archive.Dispose()
        $stream.Dispose()
    }
}

$pptxPath = Join-Path $root "09-项目方案.pptx"
New-ZipDocument -Path $pptxPath -Entries @{
    "[Content_Types].xml" = '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>'
    "_rels/.rels" = '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>'
    "ppt/presentation.xml" = '<?xml version="1.0" encoding="UTF-8"?><p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>'
    "ppt/_rels/presentation.xml.rels" = '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>'
    "ppt/slides/slide1.xml" = '<?xml version="1.0" encoding="UTF-8"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>感冒院项目方案</a:t></a:r></a:p><a:p><a:r><a:t>目标：统一管理文件位置</a:t></a:r></a:p><a:p><a:r><a:t>下一步：理解整个项目</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>'
}
[System.IO.File]::WriteAllText(
    (Join-Path $root "10-旧版方案.ppt"),
    "Legacy PowerPoint fixture: expected to enter review.",
    [System.Text.Encoding]::ASCII
)

function Add-AsciiBytes {
    param(
        [System.Collections.Generic.List[byte]]$Target,
        [string]$Text
    )
    $Target.AddRange([System.Text.Encoding]::ASCII.GetBytes($Text))
}

$jpegPath = Join-Path $root ".scan-source.jpg"
$sourceBitmap = [System.Drawing.Bitmap]::FromFile($bitmapPath)
try {
    $sourceBitmap.Save($jpegPath, [System.Drawing.Imaging.ImageFormat]::Jpeg)
}
finally {
    $sourceBitmap.Dispose()
}
$jpegBytes = [System.IO.File]::ReadAllBytes($jpegPath)
$content = "q 900 0 0 500 0 0 cm /Im0 Do Q"
$contentBytes = [System.Text.Encoding]::ASCII.GetBytes($content)
$objects = [System.Collections.Generic.List[byte[]]]::new()
$objects.Add([System.Text.Encoding]::ASCII.GetBytes("<< /Type /Catalog /Pages 2 0 R >>"))
$objects.Add([System.Text.Encoding]::ASCII.GetBytes("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"))
$objects.Add([System.Text.Encoding]::ASCII.GetBytes("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 900 500] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"))
$objects.Add([System.Text.Encoding]::ASCII.GetBytes("<< /Length $($contentBytes.Length) >>`nstream`n$content`nendstream"))
$imageObject = [System.Collections.Generic.List[byte]]::new()
Add-AsciiBytes -Target $imageObject -Text "<< /Type /XObject /Subtype /Image /Width 1800 /Height 1000 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length $($jpegBytes.Length) >>`nstream`n"
$imageObject.AddRange($jpegBytes)
Add-AsciiBytes -Target $imageObject -Text "`nendstream"
$objects.Add($imageObject.ToArray())

$pdf = [System.Collections.Generic.List[byte]]::new()
Add-AsciiBytes -Target $pdf -Text "%PDF-1.4`n"
$offsets = [System.Collections.Generic.List[int]]::new()
for ($index = 0; $index -lt $objects.Count; $index++) {
    $offsets.Add($pdf.Count)
    Add-AsciiBytes -Target $pdf -Text "$($index + 1) 0 obj`n"
    $pdf.AddRange($objects[$index])
    Add-AsciiBytes -Target $pdf -Text "`nendobj`n"
}
$xrefOffset = $pdf.Count
Add-AsciiBytes -Target $pdf -Text "xref`n0 6`n0000000000 65535 f `n"
foreach ($offset in $offsets) {
    Add-AsciiBytes -Target $pdf -Text ("{0:D10} 00000 n `n" -f $offset)
}
Add-AsciiBytes -Target $pdf -Text "trailer << /Root 1 0 R /Size 6 >>`nstartxref`n$xrefOffset`n%%EOF"
[System.IO.File]::WriteAllBytes((Join-Path $root "10-扫描需求.pdf"), $pdf.ToArray())
Remove-Item -LiteralPath $jpegPath -Force

Get-ChildItem -LiteralPath $root -File |
    Sort-Object Name |
    Select-Object Name, Length, FullName
