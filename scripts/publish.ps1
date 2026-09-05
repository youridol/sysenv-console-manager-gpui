# SECM 发布脚本（纯 Rust + GPUI + LHM sidecar）
#
# 作用：一次构建完整发布目录：
#   1. cargo build --release -p secm-app        → secm-app.exe
#   2. dotnet publish sidecar-lhm（self-contained win-x64）→ LhmSidecar.exe + 运行时
#   3. 组装发布目录 <repo>/dist/secm-v<version>/：
#        secm-app.exe
#        lhm/publish/  （sidecar 全量产物）
#        lhm/source/   （sidecar 源码 + 许可，MPL-2.0 源码义务）
#        third_party/  （驱动 + 许可：WinRing0x64.sys / PawnIO / OpenLibSys）
#        LICENSE / README.md / CHANGELOG.md（随包许可与说明）
#
# 版本号单点来源：根 Cargo.toml [workspace.package] version
# （P1-20 修复：历史为脚本内硬编码，与 Rust 侧多处字面量脱节）
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

Write-Host "[publish] 1/3 cargo build --release -p secm-app" -ForegroundColor Cyan
Push-Location $repoRoot
try {
    cargo build --release -p secm-app
    if ($LASTEXITCODE -ne 0) { throw "cargo build 失败 (exit $LASTEXITCODE)" }
} finally {
    Pop-Location
}

Write-Host "[publish] 2/3 dotnet publish sidecar-lhm (self-contained win-x64)" -ForegroundColor Cyan
# dotnet CLI 检查：缺失时明确失败，不再静默跳过（P1-20）
if (-not (Get-Command dotnet -ErrorAction SilentlyContinue)) {
    throw "未找到 dotnet CLI（.NET 8 SDK）：无法发布 LHM sidecar。请安装 .NET 8 SDK 后重试，或从已有发布产物手动组装。"
}
$sidecarPublish = Join-Path $repoRoot "sidecar-lhm\bin\Release\net8.0\win-x64\publish"
# 始终重新发布（dotnet publish 为增量构建；P1-20 修复：历史实现"产物存在即整段跳过"，
# 可能打包陈旧 sidecar 残留）
Push-Location (Join-Path $repoRoot "sidecar-lhm")
try {
    dotnet publish -c Release -r win-x64 --self-contained true
    if ($LASTEXITCODE -ne 0) { throw "dotnet publish 失败 (exit $LASTEXITCODE)" }
} finally {
    Pop-Location
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

# 3) 第三方驱动 + 许可（含 PawnIO GPL-2.0 源码义务：PawnIO-src 整目录随包）
Copy-Item (Join-Path $repoRoot "third_party") $outDir -Recurse -Force

# 4) 随包许可与说明（P1-20：历史缺失主项目 LICENSE）
foreach ($doc in @("LICENSE", "README.md", "CHANGELOG.md")) {
    $src = Join-Path $repoRoot $doc
    if (Test-Path $src) { Copy-Item $src $outDir -Force }
}

# 5) 校验核心产物（P1-20：驱动与许可一并校验，防半成品发行包）
$required = @(
    (Join-Path $outDir "secm-app.exe"),
    (Join-Path $outDir "lhm\publish\LhmSidecar.exe"),
    (Join-Path $outDir "lhm\publish\LibreHardwareMonitorLib.dll"),
    (Join-Path $outDir "lhm\source\Program.cs"),
    (Join-Path $outDir "lhm\licenses"),
    (Join-Path $outDir "third_party\WinRing0x64.sys"),
    (Join-Path $outDir "third_party\OpenLibSys-LICENSE.txt"),
    (Join-Path $outDir "third_party\PawnIO\PawnIO.sys"),
    (Join-Path $outDir "third_party\PawnIO\PawnIO-src"),
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
Write-Host "  便携运行：$outDir\secm-app.exe（sidecar 自动从 lhm/publish 启动）"
