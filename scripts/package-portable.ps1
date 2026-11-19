# OpenTerm 免安装版打包脚本
# 用法: .\scripts\package-portable.ps1 [-ExePath <已编译的 openterm.exe>] [-OutDir <输出目录>]
# 前置: 已执行 npm run tauri build,存在 src-tauri\target\release\openterm.exe
param(
    [string]$ExePath = "",
    [string]$OutDir = ""
)
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
if (-not $ExePath) { $ExePath = Join-Path $root "src-tauri\target\release\openterm.exe" }
if (-not (Test-Path $ExePath)) {
    throw "未找到 $ExePath,请先执行 npm run tauri build"
}

$conf = Get-Content (Join-Path $root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
$ver = $conf.version
if (-not $OutDir) { $OutDir = Join-Path $root "src-tauri\target\release\bundle\portable" }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$stage = Join-Path $OutDir "openterm_${ver}_x64-portable"

if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path $stage -Force | Out-Null

Copy-Item $ExePath $stage
foreach ($doc in @("README.md", "docs\USER_GUIDE.md", "docs\FAQ.md")) {
    $src = Join-Path $root $doc
    if (Test-Path $src) { Copy-Item $src $stage }
}

$quick = @(
    "OpenTerm v$ver 免安装版"
    "============================"
    "1. 双击 openterm.exe 即可运行,无需安装、不写注册表。"
    "2. 首次启动无需设置密码,直接使用;需要保护凭证时点工具栏「锁定」设置主密码。"
    "3. 配置与会话数据保存在: %APPDATA%\com.administrator.openterm\"
    "4. 详细用法见 USER_GUIDE.md,常见问题见 FAQ.md。"
    "5. 系统要求: Windows 10 1809 及以上,需 WebView2 运行时(Win11 自带)。"
) -join "`r`n"
Set-Content -Path (Join-Path $stage "快速开始.txt") -Value $quick -Encoding UTF8

$zip = Join-Path $outDir "openterm_${ver}_x64-portable.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal

Write-Host "完成: $zip"
