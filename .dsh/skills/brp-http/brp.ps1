<#
.SYNOPSIS
    通过 HTTP 直连 BRP 抓取/修改运行中 Voxelith app 的 ECS 状态。

.DESCRIPTION
    MCP（bevy_brp_mcp）只是 BRP HTTP 接口的包装，本脚本直接打同一个接口，
    因此不依赖 MCP 是否配置、也不依赖联网。

    接口：JSON-RPC 2.0，POST 到 http://127.0.0.1:15702（根路径，无后缀）

.EXAMPLE
    # 服务在不在
    .\brp.ps1 -Status

    # 列出所有可反射类型 / 所有 BRP 方法
    .\brp.ps1 -Schema
    .\brp.ps1 -Discover

    # 读组件（传完整类型路径）
    .\brp.ps1 -Query "voxelith_axiom::atoms::health::Health"
    .\brp.ps1 -Query "bevy_ecs::name::Name","voxelith_axiom::atoms::health::Health"

    # 读单个实体
    .\brp.ps1 -Entity 4294966862 -Components "bevy_ecs::name::Name"

    # 改组件字段（path 形如 ".current"）
    .\brp.ps1 -Mutate -Entity 4294966862 `
              -Component "voxelith_axiom::atoms::health::Health" -Path ".current" -Value 42

    # 列出某实体的组件
    .\brp.ps1 -ListComponents -Entity 4294966862

    # 任意方法逃生口
    .\brp.ps1 -Method "world.list_resources" -Params '{}'
#>
[CmdletBinding()]
param(
    # BRP 端口（主 app 15702；渲染子 app 15703）
    [int]$Port = 15702,

    # 检查服务是否可用
    [switch]$Status,

    # 列出所有 BRP 方法（rpc.discover）
    [switch]$Discover,

    # 列出所有可反射类型（registry.schema）
    [switch]$Schema,

    # 按组件筛选实体并读值（world.query），传完整类型路径
    [string[]]$Query,

    # 读单个实体的指定组件（world.get_components）
    [long]$Entity,

    # 与 -Entity 搭配的组件类型路径
    [string[]]$Components,

    # 列出某实体上的组件（world.list_components）
    [switch]$ListComponents,

    # 修改组件字段（world.mutate_components）
    [switch]$Mutate,

    # 要修改的组件类型路径（配合 -Mutate）
    [string]$Component,

    # 字段路径，形如 ".current"（配合 -Mutate）
    [string]$Path,

    # 新值（配合 -Mutate）
    $Value,

    # 任意 BRP 方法的逃生口（离线方法或未封装方法）
    [string]$Method,

    # 配合 -Method 的 params（JSON 字符串或对象）
    $Params,

    # 打印原始 JSON 响应而不是解析后的对象
    [switch]$Raw
)

$ErrorActionPreference = 'Stop'
$base = "http://127.0.0.1:$Port"

function Invoke-Brp {
    param([string]$MethodName, $MethodParams)

    $payload = [ordered]@{ jsonrpc = '2.0'; id = 1; method = $MethodName }
    if ($null -ne $MethodParams) { $payload.params = $MethodParams }
    $json = $payload | ConvertTo-Json -Depth 12 -Compress

    try {
        $resp = Invoke-RestMethod -Uri $base -Method Post -Body $json `
            -ContentType 'application/json' -TimeoutSec 20
    }
    catch {
        Write-Host "请求失败: $($_.Exception.Message)" -ForegroundColor Red
        Write-Host "提示：确认 app 在跑  ->  cargo run -p voxelith-prime" -ForegroundColor DarkGray
        Write-Host "     端口是否监听    ->  Get-NetTCPConnection -LocalPort $Port -State Listen" -ForegroundColor DarkGray
        return $null
    }

    if ($resp.PSObject.Properties.Name -contains 'error') {
        Write-Host "BRP 错误 [$($resp.error.code)]: $($resp.error.message)" -ForegroundColor Red
        if ($resp.error.data) { Write-Host ($resp.error.data | ConvertTo-Json -Depth 6) -ForegroundColor DarkGray }
        return $null
    }

    # PowerShell 会把数组“展开”输出：空的 JSON 数组 result:[] 会变成 $null，
    # 从而无法区分「查到了 0 条」和「调用失败」。用逗号包一层抵消展开。
    if ($resp.result -is [System.Array]) { return , $resp.result }
    return $resp.result
}

# ------------------------------------------------------------------ 1. 连通性
if ($Status) {
    $listening = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
    if (-not $listening) {
        Write-Host "端口 $Port 未监听：app 没在跑。" -ForegroundColor Yellow
        exit 1
    }
    $r = Invoke-Brp 'rpc.discover' $null
    if ($null -eq $r) { exit 1 }
    $names = @($r.methods | ForEach-Object { $_.name })
    Write-Host "BRP 可用：$base" -ForegroundColor Green
    Write-Host "  监听 PID : $($listening.OwningProcess)"
    Write-Host "  方法总数 : $($names.Count)"
    Write-Host "  extras   : $(@($names | Where-Object { $_ -like 'brp_extras/*' }).Count) 个"

    # 注意：局部变量不能叫 $schema —— PowerShell 变量名大小写不敏感，
    # 会和开关参数 -Schema 撞名，导致把对象赋给 SwitchParameter 而报错。
    $typeTable = Invoke-Brp 'registry.schema' $null
    $voxelithTypes = @()
    if ($typeTable) {
        $voxelithTypes = @($typeTable.PSObject.Properties.Name | Where-Object { $_ -match 'voxelith' })
    }
    Write-Host "  Voxelith 类型：$($voxelithTypes.Count) 个" -ForegroundColor Cyan
    foreach ($t in $voxelithTypes) { Write-Host "    $t" }
    exit 0
}

# ------------------------------------------------------------------ 2. 方法表
if ($Discover) {
    $r = Invoke-Brp 'rpc.discover' $null
    if ($r) { $r.methods | ForEach-Object { $_.name } | Sort-Object }
    exit 0
}

# ------------------------------------------------------------------ 3. 类型表
if ($Schema) {
    $r = Invoke-Brp 'registry.schema' $null
    if ($r) { $r.PSObject.Properties.Name | Sort-Object }
    exit 0
}

# ------------------------------------------------------------------ 4. 任意方法
if ($Method) {
    $p = $null
    if ($Params) {
        $p = if ($Params -is [string]) { $Params | ConvertFrom-Json } else { $Params }
    }
    $r = Invoke-Brp $Method $p
    if ($null -ne $r) {
        if ($Raw) { $r | ConvertTo-Json -Depth 12 } else { $r }
    }
    exit 0
}

# ------------------------------------------------------------------ 5. 按组件查实体
if ($Query) {
    # 用 strict=true：类型名写错或未注册反射时直接报错，
    # 而不是静默返回空数组（那是本接口最容易踩的坑）。
    $r = Invoke-Brp 'world.query' @{
        data   = @{ components = @($Query) }
        filter = @{}
        strict = $true
    }
    if ($null -ne $r) {
        if (@($r).Count -eq 0) {
            # 类型正确、也注册了反射，只是当前没有实体带它。
            Write-Host "类型已注册，但当前没有实体带这些组件：$($Query -join ', ')" -ForegroundColor Yellow
            Write-Host "看看世界里有什么：  .\brp.ps1 -Query `"bevy_ecs::name::Name`"" -ForegroundColor DarkGray
        }
        else {
            Write-Host "命中 $(@($r).Count) 个实体：" -ForegroundColor Green
            foreach ($row in @($r)) { $row | ConvertTo-Json -Depth 10 }
        }
    }
    else {
        Write-Host "提示：类型路径必须是全路径（含模块），例如" -ForegroundColor DarkGray
        Write-Host "  voxelith_axiom::atoms::health::Health   而不是  Health" -ForegroundColor DarkGray
        Write-Host "列出已注册类型：  .\brp.ps1 -Schema | Select-String voxelith" -ForegroundColor DarkGray
    }
    exit 0
}

# ------------------------------------------------------------------ 6. 列某实体的组件
if ($ListComponents) {
    if ($Entity -eq 0) { Write-Host "-ListComponents 需要 -Entity" -ForegroundColor Red; exit 1 }
    # 实测：world.list_components 返回的是**裸字符串数组**，不是 { components: [...] }
    $r = Invoke-Brp 'world.list_components' @{ entity = $Entity }
    if ($null -ne $r) {
        $list = @($r)
        Write-Host "实体 $Entity 上的组件（$($list.Count) 个）：" -ForegroundColor Green
        foreach ($c in ($list | Sort-Object)) { Write-Host "  $c" }
    }
    exit 0
}

# ------------------------------------------------------------------ 7. 读单实体
if ($Entity -ne 0 -and $Components -and -not $Mutate) {
    $r = Invoke-Brp 'world.get_components' @{ entity = $Entity; components = @($Components) }
    if ($r) { $r | ConvertTo-Json -Depth 10 }
    exit 0
}

# ------------------------------------------------------------------ 8. 改组件字段
if ($Mutate) {
    if ($Entity -eq 0 -or -not $Component) {
        Write-Host "-Mutate 需要 -Entity 与 -Component" -ForegroundColor Red
        exit 1
    }
    if (-not $PSBoundParameters.ContainsKey('Path')) { $Path = '' }
    $r = Invoke-Brp 'world.mutate_components' @{
        entity    = $Entity
        component = $Component
        path      = $Path
        value     = $Value
    }
    Write-Host "已提交修改：entity=$Entity $Component$Path = $Value" -ForegroundColor Green

    # 立刻回读确认（BRP 的修改是即时的）
    $back = Invoke-Brp 'world.get_components' @{ entity = $Entity; components = @($Component) }
    if ($back) { Write-Host "回读确认："; $back | ConvertTo-Json -Depth 10 }
    exit 0
}

Write-Host "没有指定动作。常用示例：" -ForegroundColor Yellow
Write-Host "  .\brp.ps1 -Status"
Write-Host "  .\brp.ps1 -Schema"
Write-Host "  .\brp.ps1 -Query `"voxelith_axiom::atoms::health::Health`""
Write-Host "  .\brp.ps1 -Entity 4294966862 -Components `"bevy_ecs::name::Name`""
Write-Host "  .\brp.ps1 -Mutate -Entity 4294966862 -Component `"...Health`" -Path `".current`" -Value 42"
Write-Host "用 -Method 可调用任意未封装方法。"
exit 0