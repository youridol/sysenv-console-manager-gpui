# SECM v2.0.0 发布脚本（纯 Rust + GPUI + LHM sidecar）
#
# 作用：一次构建完整发布目录：
#   1. cargo build --release -p secm-app        → secm-app.exe
#   2. dotnet publish sidecar-lhm（self-contained win-x64）→ LhmSidecar.exe + 运行时
#   3. 组装发布目录 <repo>/dist/secm-v2.0.0/：
#        secm-app.exe
#        lhm/publish/  （sidecar 全量产物）
#        lhm/source/   （sidecar 源码 + 许可，MPL-2.0/LGPL 源码义务）
#        third_party/  （驱动 + 许可：WinRing0x64.sys / PawnIO / OpenLibSys）
#
# 用法：pwsh -ExecutionPolicy Bypass -File scripts/publish.ps1

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$version = "2.0.0"
$distDir = Join-Path $repoRoot "dist"
$outDir = Join-Path $distDir "secm-v$version"

Write-Host "[publish] 1/3 cargo build --release -p secm-app" -ForegroundColor Cyan
Push-Location $repoRoot
try {
    cargo build --release -p secm-app
    if ($LASTEXITCODE -ne 0) { throw "cargo build 失败 (exit $LASTEXITCODE)" }
} finally {
    Pop-Location
}

Write-Host "[publish] 2/3 dotnet publish sidecar-lhm (self-contained win-x64)" -ForegroundColor Cyan
$sidecarPublish = Join-Path $repoRoot "sidecar-lhm\bin\Release\net8.0\win-x64\publish"
if (-not (Test-Path (Join-Path $sidecarPublish "LhmSidecar.exe"))) {
    Push-Location (Join-Path $repoRoot "sidecar-lhm")
    try {
        dotnet publish -c Release -r win-x64 --self-contained true
        if ($LASTEXITCODE -ne 0) { throw "dotnet publish 失败 (exit $LASTEXITCODE)" }
    } finally {
        Pop-Location
    }
}

Write-Host "[publish] 3/3 组装发布目录 $outDir" -ForegroundColor Cyan
if (Test-Path $outDir) { Remove-Item $outDir -Recurse -Force }
New-Item $outDir -ItemType Directory -Force | Out-Null

# 1) 主程序
Copy-Item (Join-Path $repoRoot "target\release\secm-app.exe") $outDir -Force

# 2) LHM sidecar 产物 + 源码 + 许可
Copy-Item $sidecarPublish (Join-Path $outDir "lhm\publish") -Recurse -Force
New-Item (Join-Path $outDir "lhm\source") -ItemType Directory -Force | Out-Null
Copy-Item (Join-Path $repoRoot "sidecar-lhm\Program.cs") (Join-Path $outDir "lhm\source") -Force
Copy-Item (Join-Path $repoRoot "sidecar-lhm\sidecar-lhm.csproj") (Join-Path $outDir "lhm\source") -Force
if (Test-Path (Join-Path $repoRoot "sidecar-lhm\licenses")) {
    Copy-Item (Join-Path $repoRoot "sidecar-lhm\licenses") (Join-Path $outDir "lhm") -Recurse -Force
}

# 3) 第三方驱动 + 许可
Copy-Item (Join-Path $repoRoot "third_party") $outDir -Recurse -Force

# 4) 校验核心产物
$required = @(
    (Join-Path $outDir "secm-app.exe"),
    (Join-Path $outDir "lhm\publish\LhmSidecar.exe"),
    (Join-Path $outDir "lhm\publish\LibreHardwareMonitorLib.dll")
)
$missing = $required | Where-Object { -not (Test-Path $_) }
if ($missing.Count -gt 0) {
    Write-Host "[publish] 错误：产物缺失：$($missing -join ', ')" -ForegroundColor Red
    exit 1
}

$sizeMB = [math]::Round(((Get-ChildItem $outDir -Recurse -File | Measure-Object Length -Sum).Sum / 1MB), 1)
Write-Host "[publish] 完成：$outDir（$sizeMB MB）" -ForegroundColor Green
Write-Host "  便携运行：$outDir\secm-app.exe（sidecar 自动从 lhm/publish 启动）"
