# SECM 发布脚本（纯 Rust + GPUI；v3.0.0 起无 sidecar/HTTP 链路）
#
# 作用：一次构建完整发布目录：
#   1. cargo build --release -p secm-app        → secm-app.exe
#   2. 组装发布目录 <repo>/dist/secm-v<version>/：
#        secm-app.exe（单文件，硬件采集全部进程内原生，无外部依赖）
#        LICENSE / README.md / CHANGELOG.md（随包许可与说明）
#
# 版本号单点来源：根 Cargo.toml [workspace.package] version
# （P1-20 修复：历史为脚本内硬编码，与 Rust 侧多处字面量脱节）
#
# v3.0.0 变更：硬件采集纯原生（NVML/DXGI/PDH/Win32/IOCTL/WMI 进程内直调），
# 删除 LHM sidecar 的 dotnet publish 与 lhm/ 产物组装段；third_party 驱动资产
# 无消费者，不再随包分发（仓库内保留为预留资产）。
#
# 用法：pwsh -ExecutionPolicy Bypass -File scripts/publish.ps1

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

# —— 版本：从根 Cargo.toml 解析（单点维护）——
$cargoToml = Join-Path $repoRoot "Cargo.toml"
$versionLine = Select-String -Path $cargoToml -Pattern '^\s*version\s*=\s*"([^"]+)"' |
    Select-Object -First 1
if (-not $versionLine) { throw "无法从根 Cargo.toml 解析 [workspace.package] version" }
$version = $versionLine.Matches[0].Groups[1].Value
$distDir = Join-Path $repoRoot "dist"
$outDir = Join-Path $distDir "secm-v$version"
Write-Host "[publish] 版本 v$version（来源：Cargo.toml）" -ForegroundColor Cyan

Write-Host "[publish] 1/2 cargo build --release -p secm-app" -ForegroundColor Cyan
Push-Location $repoRoot
try {
    cargo build --release -p secm-app
    if ($LASTEXITCODE -ne 0) { throw "cargo build 失败 (exit $LASTEXITCODE)" }
} finally {
    Pop-Location
}

Write-Host "[publish] 2/2 组装发布目录 $outDir" -ForegroundColor Cyan
if (Test-Path $outDir) { Remove-Item $outDir -Recurse -Force }
New-Item $outDir -ItemType Directory -Force | Out-Null

# 1) 主程序（单文件；NVML 运行时从系统 NVIDIA 驱动加载，无随包运行时依赖）
Copy-Item (Join-Path $repoRoot "target\release\secm-app.exe") $outDir -Force

# 2) 随包许可与说明（P1-20：历史缺失主项目 LICENSE）
foreach ($doc in @("LICENSE", "README.md", "CHANGELOG.md")) {
    $src = Join-Path $repoRoot $doc
    if (Test-Path $src) { Copy-Item $src $outDir -Force }
}

# 3) 校验核心产物（防半成品发行包）
$required = @(
    (Join-Path $outDir "secm-app.exe"),
    (Join-Path $outDir "LICENSE"),
    (Join-Path $outDir "CHANGELOG.md")
)
$missing = $required | Where-Object { -not (Test-Path $_) }
if ($missing.Count -gt 0) {
    Write-Host "[publish] 错误：产物缺失：$($missing -join ', ')" -ForegroundColor Red
    exit 1
}

$sizeMB = [math]::Round(((Get-ChildItem $outDir -Recurse -File | Measure-Object Length -Sum).Sum / 1MB), 1)
Write-Host "[publish] 完成：$outDir（$sizeMB MB）" -ForegroundColor Green
Write-Host "  便携运行：$outDir\secm-app.exe（普通用户可直接运行；无 sidecar/HTTP/驱动依赖）"
