# shot.ps1 — 截图并**证明它真的是这一帧**，而不是上一次留下的旧文件。
#
# ## 为什么需要这个脚本（踩过两次）
#
# 用 BRP 截图时，如果请求失败（进程没起、端口没监听、路径写错），
# `brp_extras/screenshot` 会**什么都不做**，而磁盘上**上一次的 PNG 还在**。
# 于是你读到的是一张**看起来很正常的旧图**，并据此得出错误结论。
#
# 本脚本把"防陈旧"做成机械检查：
#
#   1. 截图前**删掉目标文件**（旧文件不可能被误读）；
#   2. 截图后**确认文件真的出现了**、且大小 > 0；
#   3. 打印 sha256 —— 改了可见的东西之后，哈希**必须**变；
#      没变就说明你看到的还是旧帧（或改动根本没生效）。
#
# ## 用法
#
#   pwsh ./scripts/shot.ps1 -Name before
#   ... 改点东西 ...
#   pwsh ./scripts/shot.ps1 -Name after
#
# 两次的哈希会打出来；**一样就说明改动没有可见效果**（或截图没成功）。
#
# ## 退出码
#
#   0 = 截图成功
#   1 = 进程没在跑 / 端口没监听
#   2 = 截图请求失败或文件没出现（**不要**在这时读图）

[CmdletBinding()]
param(
    # 输出文件名（不含扩展名）。放在仓库根的 `work/shots/` 下。
    [Parameter(Mandatory = $true)]
    [string]$Name,

    # BRP 端口。
    [int]$Port = 15702
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $repo 'work/shots'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$out = Join-Path $dir "$Name.png"

# ---- 1. 前置检查：进程与端口 ----
$proc = Get-Process voxelith-prime -ErrorAction SilentlyContinue
if (-not $proc) {
    Write-Host "[截图] 失败：voxelith-prime 没在跑。先启动它。" -ForegroundColor Red
    exit 1
}
$listening = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
if (-not $listening) {
    Write-Host "[截图] 失败：端口 $Port 没在监听（BRP 没就绪）。" -ForegroundColor Red
    exit 1
}

# ---- 2. 删掉旧文件：旧文件不可能被误读 ----
if (Test-Path $out) { Remove-Item $out -Force }

# ---- 3. 发出截图请求 ----
# 路径要转义成 JSON 里的反斜杠。
$jsonPath = $out -replace '\\', '\\\\'
$body = @{
    jsonrpc = '2.0'
    id      = 1
    method  = 'brp_extras/screenshot'
    params  = @{ path = $out }
} | ConvertTo-Json -Depth 5 -Compress

try {
    $response = Invoke-RestMethod -Uri "http://127.0.0.1:$Port" -Method Post -Body $body `
        -ContentType 'application/json' -TimeoutSec 20
} catch {
    Write-Host "[截图] 请求失败：$($_.Exception.Message)" -ForegroundColor Red
    exit 2
}
if ($response.error) {
    Write-Host "[截图] BRP 报错：$($response.error.message)" -ForegroundColor Red
    exit 2
}

# ---- 4. 文件真的出现了吗 ----
# 给渲染/写盘一点时间（截图不是同步落盘的）。
$deadline = (Get-Date).AddSeconds(5)
while (-not (Test-Path $out) -and (Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 100
}
if (-not (Test-Path $out)) {
    Write-Host "[截图] 失败：文件没出现（$out）。**不要**在这时读图 —— 那会读到旧帧。" -ForegroundColor Red
    exit 2
}
$len = (Get-Item $out).Length
if ($len -le 0) {
    Write-Host "[截图] 失败：文件是空的。" -ForegroundColor Red
    exit 2
}

# ---- 5. 哈希：改了可见的东西之后它必须变 ----
$hash = (Get-FileHash $out -Algorithm SHA256).Hash
Write-Host ("[截图] {0}  {1} 字节  sha256={2}" -f $out, $len, $hash.Substring(0, 16)) -ForegroundColor Green
exit 0
