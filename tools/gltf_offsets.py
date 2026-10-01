"""从 Kenney 的 GLB 里**读出每个模型的"中心偏移"**，生成清单。

## 为什么需要这份清单

这些模型（以及 Kenney 其他 modular 套件）的**局部原点约定不统一**：

| 模型 | 包围盒（世界单位） | 原点在哪 |
|---|---|---|
| `template-floor` | `(-2,0,-2) → (2,0,2)` | 底面**中心** ✅ |
| `room-small` | `(-6,0,-6) → (6,0,6)` | 底面**中心** ✅ |
| `template-wall` | `(-2,0,-1.99) → (2,4.15,0)` | 底面**边缘**（中心在 `z = -1.0`）❌ |
| `stairs` | `(-2.2,0,-6.2) → (2.2,8.55,2.2)` | 中心在 `z = -2.0` ❌ |

按"底面中心"摆放时，墙类模型会**整体偏半个格子** ——
盖出来的房间墙会从格线上岔开。

## 为什么要生成清单，而不是在运行期算

运行期算需要访问 `Mesh` 的顶点数据，而 GLB 是**异步**加载的：
拿到顶点之前不知道该往哪摆，会先抖一下。
**用构建期生成的静态表**最简单，而且**可测试**（清单与真实 GLB 对不上时测试会失败）。

## 用法

```powershell
python tools/gltf_offsets.py
```

它写 `assets/models/dungeon/offsets.ron`。改素材后重跑。
"""

import json
import struct
import sys
from pathlib import Path

# 一个网格的顶点数上限；超过就认为读到的是别的东西（防呆）。
MAX_VERTICES = 100_000


def read_first_mesh_bounds(glb: Path):
    """读 GLB 里**第一个** mesh 的 POSITION 包围盒。

    返回 `(min, max)`（各 3 个 float）。GLB 的结构是：
    12 字节头 + JSON chunk + BIN chunk，accessor 指向 BIN 里的 bufferView。
    """
    data = glb.read_bytes()
    if data[:4] != b"glTF":
        raise ValueError(f"{glb.name} 不是 GLB（魔数不对）")
    json_len = struct.unpack_from("<I", data, 12)[0]
    gltf = json.loads(data[20 : 20 + json_len].decode("utf-8"))

    accessors = gltf["accessors"]
    best = None
    for mesh in gltf["meshes"]:
        for prim in mesh["primitives"]:
            index = prim["attributes"].get("POSITION")
            if index is None:
                continue
            accessor = accessors[index]
            count = accessor["count"]
            if count > MAX_VERTICES:
                raise ValueError(f"{glb.name} 顶点数异常：{count}")
            mn, mx = accessor.get("min"), accessor.get("max")
            if mn is None or mx is None:
                continue
            if best is None:
                best = (list(mn), list(mx))
            else:
                best = (
                    [min(best[0][i], mn[i]) for i in range(3)],
                    [max(best[1][i], mx[i]) for i in range(3)],
                )
    return best


def main():
    root = Path(__file__).resolve().parent.parent
    # **三套都生成**：偏移是"模型自身"的属性，
    # 不能只为 dungeon 做 —— 否则切到 cave / platformer 又会错位。
    kits = ["dungeon", "cave", "platformer"]
    total = 0
    for kit in kits:
        model_dir = root / "assets" / "models" / kit
        if not model_dir.is_dir():
            print(f"  跳过 {kit}：找不到 {model_dir}")
            continue
        total += generate(root, model_dir)
    return 0


def generate(root: Path, model_dir: Path) -> int:

    rows = []
    for glb in sorted(model_dir.glob("*.glb")):
        bounds = read_first_mesh_bounds(glb)
        if bounds is None:
            print(f"  跳过 {glb.name}：（没有 POSITION 的 min/max）")
            continue
        mn, mx = bounds
        # **中心偏移** = 包围盒中心的水平分量；y 取底面（-min.y），
        # 这样模型能"坐在"指定的高度上而不是陷进去。
        offset = [
            (mn[0] + mx[0]) / 2.0,
            -mn[1],
            (mn[2] + mx[2]) / 2.0,
        ]
        rows.append((glb.stem, offset, mn, mx))

    # ⚠️ 路径必须由 `model_dir` 推出来。第一版写死成 `dungeon`，
    # 结果三套**互相覆盖**（最后跑的 platformer 把 dungeon 的清单盖掉了）——
    # 而覆盖之后的症状是"偏移全丢了"，不会报任何错。
    out = model_dir / "offsets.ron"
    lines = [
        "// **自动生成，不要手改** —— 由 `tools/gltf_offsets.py` 写出。",
        "//",
        "// 每个模型的\"中心偏移\"：把它的**底面中心**对到摆放点上要补多少。",
        "//",
        "// 为什么需要：这些模型的局部原点约定**不统一** ——",
        "// 地板/房间以底面中心为原点，而墙/楼梯以**底面边缘**为原点。",
        "// 不补偏移的话，墙会整体偏半个格子，盖出来的房间对不上格线。",
        "//",
        "// 改素材后重跑 `python tools/gltf_offsets.py`。",
        "(",
        "    entries: [",
    ]
    for name, offset, _mn, _mx in rows:
        lines.append(
            f'        (name: "{name}", offset: ({offset[0]:.4f}, {offset[1]:.4f}, {offset[2]:.4f})),'
        )
    lines += ["    ],", ")", ""]
    out.write_text("\n".join(lines), encoding="utf-8")

    # 报告"哪些模型原点不在底心" —— 那才是需要偏移的。
    odd = [r for r in rows if abs(r[1][0]) > 0.05 or abs(r[1][2]) > 0.05]
    print(f"写了 {out.relative_to(root)}：{len(rows)} 个模型")
    print(f"其中**原点不在底面中心**的有 {len(odd)} 个（需要偏移）：")
    for name, offset, _mn, _mx in odd:
        print(f"   {name:<26} 中心偏移 ({offset[0]:+.2f}, {offset[1]:+.2f}, {offset[2]:+.2f})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
