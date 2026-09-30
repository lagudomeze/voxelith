<#
.SYNOPSIS
    Voxelith 架构守卫（R90–R95）。

.DESCRIPTION
    提交前运行： pwsh ./scripts/arch-guard.ps1
    退出码 0 = 全部通过；非 0 = 有失败项。

    检查项：
      1. R90  cargo check --workspace
      2. R91  cargo tree -p voxelith-axiom 不含 bevy_render/bevy_ui/bevy_sprite/bevy_pbr
      3. R92  cargo tree -p voxelith-axiom 不含 voxelith-prime
      4. R93  tokei：无源码文件 > 500 行（R26）
      5. R5   voxelith-axiom 依赖白名单：bevy_ecs/bevy_app/bevy_reflect/bevy_time
      6. R95  每个 crate 入口文件顶部含规则注释
      7. R21/R96 源码中不出现 mod base/common/utils/helpers
      8. R104 源码中不出现 "scense"
#>

[CmdletBinding()]
param(
    # 单个源码文件的最大行数（R26）
    [int]$MaxLines = 500,
    # 跳过 cargo check（只跑静态检查时用，加快速度）
    [switch]$SkipCheck
)

$ErrorActionPreference = 'Continue'

# ---------------------------------------------------------------- 基础工具

function Get-RepoRoot {
    $dir = $PSScriptRoot
    while ($dir -and -not (Test-Path (Join-Path $dir 'Cargo.toml'))) {
        $parent = Split-Path $dir -Parent
        if ($parent -eq $dir) { return $null }
        $dir = $parent
    }
    return $dir
}

$RepoRoot = Get-RepoRoot
if (-not $RepoRoot) {
    Write-Host 'FATAL: 找不到 Cargo.toml，请在仓库内运行本脚本。' -ForegroundColor Red
    exit 2
}

$Results = @()

function Add-Result {
    param(
        [string]$Id,
        [string]$Name,
        [bool]$Passed,
        [string]$Detail = ''
    )
    $script:Results += [pscustomobject]@{ Id = $Id; Name = $Name; Passed = $Passed; Detail = $Detail }
    $tag = if ($Passed) { 'PASS' } else { 'FAIL' }
    $color = if ($Passed) { 'Green' } else { 'Red' }
    Write-Host ("[{0}] {1} {2}" -f $tag, $Id, $Name) -ForegroundColor $color
    if ($Detail) {
        foreach ($line in ($Detail -split "`n")) {
            if ($line.Trim()) { Write-Host ("       $line") -ForegroundColor DarkGray }
        }
    }
}

function Test-CommandExists {
    param([string]$Name)
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

# 源码文件（排除 target / .git）
function Get-SourceFiles {
    param([string]$Root)
    $crates = Join-Path $Root 'crates'
    if (-not (Test-Path $crates)) { return @() }
    return Get-ChildItem $crates -Recurse -File -Include *.rs, *.toml |
        Where-Object { $_.FullName -notmatch '[\\/]target[\\/]' }
}

# ---------------------------------------------------------------- 前置检查

Write-Host ''
Write-Host '=== Voxelith 架构守卫 ===' -ForegroundColor Cyan
Write-Host "仓库根：$RepoRoot"
Write-Host ''

if (-not (Test-CommandExists 'cargo')) {
    Write-Host 'FATAL: 找不到 cargo。' -ForegroundColor Red
    exit 2
}

$HasTokei = Test-CommandExists 'tokei'

# ---------------------------------------------------------------- 1. cargo check / test

if ($SkipCheck) {
    Add-Result 'R90' 'cargo check --workspace' $true '（已跳过）'
    Add-Result '测试' 'cargo test --workspace' $true '（已跳过）'
}
else {
    Write-Host '... 运行 cargo check --workspace（可能较慢）' -ForegroundColor DarkGray
    $checkOutput = & cargo check --workspace --message-format short 2>&1
    $checkExit = $LASTEXITCODE
    if ($checkExit -eq 0) {
        Add-Result 'R90' 'cargo check --workspace' $true
    }
    else {
        $tail = ($checkOutput | Select-Object -Last 25) -join "`n"
        Add-Result 'R90' 'cargo check --workspace' $false $tail
    }

    Write-Host '... 运行 cargo test --workspace' -ForegroundColor DarkGray
    $testOutput = & cargo test --workspace 2>&1
    if ($LASTEXITCODE -eq 0) {
        $passed = (($testOutput | Select-String -Pattern '^test result: .* (\d+) passed' -AllMatches).Matches |
            ForEach-Object { [int]$_.Groups[1].Value } | Measure-Object -Sum).Sum
        Add-Result '测试' 'cargo test --workspace' $true "$passed 个测试通过"
    }
    else {
        $tail = ($testOutput | Select-String -Pattern 'FAILED|panicked|error\[|^error' | Select-Object -First 20) -join "`n"
        Add-Result '测试' 'cargo test --workspace' $false $tail
    }
}

# ---------------------------------------------------------------- 2/3. 依赖树

Write-Host '... 运行 cargo tree -p voxelith-axiom' -ForegroundColor DarkGray
$treeOutput = & cargo tree -p voxelith-axiom 2>&1
$treeExit = $LASTEXITCODE

if ($treeExit -ne 0) {
    $tail = ($treeOutput | Select-Object -Last 15) -join "`n"
    Add-Result 'R91' 'voxelith-axiom 无渲染 crate 依赖' $false "cargo tree 执行失败：`n$tail"
    Add-Result 'R92' 'voxelith-axiom 不依赖 voxelith-prime' $false 'cargo tree 执行失败'
}
else {
    $treeText = $treeOutput -join "`n"

    $renderHits = ($treeText -split "`n") |
        Where-Object { $_ -match 'bevy_render|bevy_ui|bevy_sprite|bevy_pbr|bevy_core_pipeline|bevy_gizmos' }
    if ($renderHits) {
        Add-Result 'R91' 'voxelith-axiom 无渲染 crate 依赖' $false (($renderHits | Select-Object -First 10) -join "`n")
    }
    else {
        Add-Result 'R91' 'voxelith-axiom 无渲染 crate 依赖' $true
    }

    $primeHits = ($treeText -split "`n") | Where-Object { $_ -match 'voxelith-prime' }
    if ($primeHits) {
        Add-Result 'R92' 'voxelith-axiom 不依赖 voxelith-prime' $false (($primeHits | Select-Object -First 10) -join "`n")
    }
    else {
        Add-Result 'R92' 'voxelith-axiom 不依赖 voxelith-prime' $true
    }
}

# ---------------------------------------------------------------- 4. 文件行数（R93/R26）

$sourceFiles = Get-SourceFiles -Root $RepoRoot
$rustFiles = $sourceFiles | Where-Object { $_.Extension -eq '.rs' }

if ($HasTokei) {
    $tokeiJson = & tokei --sort code --output json 2>$null
    if ($LASTEXITCODE -eq 0 -and $tokeiJson) {
        $data = $tokeiJson | ConvertFrom-Json
        $overLimit = @()
        foreach ($lang in $data.PSObject.Properties) {
            $reports = $lang.Value.reports
            if (-not $reports) { continue }
            foreach ($report in $reports) {
                if ($report.stats.code -gt $MaxLines) {
                    $rel = $report.name.Replace($RepoRoot, '').TrimStart('\', '/')
                    $overLimit += ("{0}  {1} 行代码" -f $rel, $report.stats.code)
                }
            }
        }
        if ($overLimit) {
            Add-Result 'R93' "tokei：无文件 > $MaxLines 行" $false ($overLimit -join "`n")
        }
        else {
            Add-Result 'R93' "tokei：无文件 > $MaxLines 行" $true
        }
    }
    else {
        Write-Host '  ! tokei 运行失败，回退到内置行数统计。' -ForegroundColor Yellow
        $HasTokei = $false
    }
}

if (-not $HasTokei) {
    $overLimit = @()
    foreach ($file in $rustFiles) {
        $count = (Get-Content -LiteralPath $file.FullName | Measure-Object -Line).Lines
        if ($count -gt $MaxLines) {
            $rel = $file.FullName.Replace($RepoRoot, '').TrimStart('\', '/')
            $overLimit += ("{0}  {1} 行" -f $rel, $count)
        }
    }
    if ($overLimit) {
        Add-Result 'R93' "行数：无文件 > $MaxLines 行" $false ($overLimit -join "`n")
    }
    else {
        Add-Result 'R93' "行数：无文件 > $MaxLines 行" $true '（未安装 tokei，使用内置统计；建议 cargo install tokei）'
    }
}

# ---------------------------------------------------------------- 5. axiom 不得依赖完整 bevy（R5）

$axiomToml = Join-Path $RepoRoot 'crates/voxelith-axiom/Cargo.toml'
if (-not (Test-Path $axiomToml)) {
    Add-Result 'R5' 'voxelith-axiom 不依赖完整 bevy' $false '找不到 crates/voxelith-axiom/Cargo.toml'
}
else {
    $tomlText = Get-Content -LiteralPath $axiomToml -Raw
    $hits = @()

    # R5（Q21 / Q23 修订）：bevy 家族严格白名单；非 bevy 工具 crate 走"登记制"——
    # 先登记进 docs/architecture.md 的表，再改 Cargo.toml，否则这里直接报未登记。
    $bevyAllowed = @('bevy_ecs', 'bevy_app', 'bevy_reflect', 'bevy_time')
    $registered = @('exn', 'derive_more')
    $inDependencies = $false
    foreach ($line in ($tomlText -split "`n")) {
        $trimmed = $line.Trim()
        if ($trimmed -match '^\[(.+)\]$') {
            $inDependencies = ($matches[1] -eq 'dependencies')
            continue
        }
        if (-not $inDependencies -or -not $trimmed -or $trimmed.StartsWith('#')) { continue }
        if ($trimmed -match '^([A-Za-z0-9_\-]+)\s*=') {
            $depName = $matches[1]
            if ($bevyAllowed -contains $depName -or $registered -contains $depName) { continue }
            if ($depName -like 'bevy*') {
                $hits += "bevy 家族白名单外：$depName（只允许 $($bevyAllowed -join ' / ')）"
            }
            else {
                $hits += "未登记的第三方依赖：$depName（先在 docs/architecture.md 的登记表里登记，再改 Cargo.toml）"
            }
        }
    }

    # 调试设施（BRP / egui 检查器）属 L2，禁止下沉到 L0/L1（R5、R99）——单独给出更直白的报错。
    foreach ($forbidden in @('bevy_brp_extras', 'bevy-inspector-egui', 'bevy_remote', 'bevy_egui')) {
        if ($tomlText -match ("(?m)^\s*" + [regex]::Escape($forbidden) + "\s*=")) {
            $hits += "发现调试设施依赖 `$forbidden`（属 L2，禁止进 axiom）"
        }
    }
    $r5Name = 'voxelith-axiom 依赖（bevy 白名单 + 非 bevy 登记制）'
    if ($hits) {
        Add-Result 'R5' $r5Name $false ($hits -join "`n")
    }
    else {
        Add-Result 'R5' $r5Name $true
    }
}

# ---------------------------------------------------------------- 6. 规则注释（R95）

$entryFiles = @(
    'crates/voxelith-axiom/src/lib.rs',
    'crates/voxelith-prime/src/lib.rs',
    'crates/voxelith-prime/src/main.rs'
)
$missing = @()
foreach ($rel in $entryFiles) {
    $path = Join-Path $RepoRoot $rel
    if (-not (Test-Path $path)) { continue }
    $head = (Get-Content -LiteralPath $path -TotalCount 20) -join "`n"
    if ($head -notmatch '(#!\[doc|//!|//\s*R\d)') {
        $missing += "$rel 顶部缺少规则注释（R95）"
    }
}
# main.rs 是二进制入口，只在没有 lib.rs 时要求
if ($missing) {
    Add-Result 'R95' 'crate 入口文件含规则注释' $false ($missing -join "`n")
}
else {
    Add-Result 'R95' 'crate 入口文件含规则注释' $true
}

# ---------------------------------------------------------------- 7/8. 命名反模式（R96/R104）

$badModules = @()
$badSpelling = @()
foreach ($file in $rustFiles) {
    $rel = $file.FullName.Replace($RepoRoot, '').TrimStart('\', '/')
    $lineNo = 0
    foreach ($line in (Get-Content -LiteralPath $file.FullName)) {
        $lineNo++
        if ($line -match '^\s*(pub\s+)?mod\s+(base|common|utils|helpers|misc|shared)\s*[;{]') {
            $badModules += "{0}:{1}  {2}" -f $rel, $lineNo, $line.Trim()
        }
    }
    if (Select-String -LiteralPath $file.FullName -Pattern 'scense' -Quiet) {
        $badSpelling += "$rel 出现 'scense'（应为 presentation/levels）"
    }
}
if ($badModules) {
    Add-Result 'R96' '无 base/common/utils/helpers 模块名' $false ($badModules -join "`n")
}
else {
    Add-Result 'R96' '无 base/common/utils/helpers 模块名' $true
}
if ($badSpelling) {
    Add-Result 'R104' "无 'scense' 命名" $false ($badSpelling -join "`n")
}
else {
    Add-Result 'R104' "无 'scense' 命名" $true
}

# ---------------------------------------------------------------- 9. 格式（R26 配套）

$fmtOutput = & cargo fmt --all -- --check 2>&1
if ($LASTEXITCODE -eq 0) {
    Add-Result 'R90+' 'cargo fmt --all -- --check' $true
}
else {
    $tail = ($fmtOutput | Select-Object -First 20) -join "`n"
    Add-Result 'R90+' 'cargo fmt --all -- --check' $false "格式不一致，运行 `cargo fmt --all`：`n$tail"
}

# ---------------------------------------------------------------- 汇总

$failed = @($Results | Where-Object { -not $_.Passed })
Write-Host ''
Write-Host ("=== 汇总：{0}/{1} 通过 ===" -f ($Results.Count - $failed.Count), $Results.Count) -ForegroundColor Cyan
if ($failed) {
    Write-Host '失败项：' -ForegroundColor Red
    foreach ($f in $failed) { Write-Host ("  - [{0}] {1}" -f $f.Id, $f.Name) -ForegroundColor Red }
    exit 1
}
Write-Host '全部通过。' -ForegroundColor Green
exit 0
