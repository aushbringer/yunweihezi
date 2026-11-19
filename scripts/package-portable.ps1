# OpenTerm 免安装版打包脚本
# 用法: .\scripts\package-portable.ps1
# 前置: 已执行 npm run tauri build,存在 src-tauri\target\release\openterm.exe
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "src-tauri\target\release\openterm.exe"
if (-not (Test-Path $exe)) {
    throw "未找到 $exe,请先执行 npm run tauri build"
}

$conf = Get-Content (Join-Path $root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
$ver = $conf.version
$outDir = Join-Path $root "src-tauri\target\release\bundle\portable"
$stage = Join-Path $outDir "openterm_${ver}_x64-portable"

if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path $stage -Force | Out-Null

Copy-Item $exe $stage
foreach ($doc in @("README.md", "docs\USER_GUIDE.md", "docs\FAQ.md")) {
    $src = Join-Path $root $doc
    if (Test-Path $src) { Copy-Item $src $stage }
}

$quick = @"
OpenTerm v$ver 免安装版
============================
1. 双击 openterm.exe 即可运行,无需安装、不写注册表。
2. 首次启动无需设置密码,直接使用;需要保护凭证时点工具栏「锁定」设置主密码。
3. 配置与会话数据保存在: %APPDATA%\com.administrator.openterm\
4. 详细用法见 USER_GUIDE.md,常见问题见 FAQ.md。
5. 系统要求: Windows 10 1809 及以上,需 WebView2 运行时(Win11 自带)。
"@
Set-Content -Path (Join-Path $stage "快速开始.txt") -Value $quick -Encoding UTF8

$zip = Join-Path $outDir "openterm_${ver}_x64-portable.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal

Write-Host "完成: $zip"
