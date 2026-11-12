# 开发环境与磁盘占用治理（`target/`）

本页面向 rust-dix 仓库的贡献者，回答两个问题：**`target/` 为什么会长到几十 GB**，
以及**本仓库已经做了什么、日常怎么维护**。

> rust-ef、rust-webx 使用同一套 Cargo profile 调优；清理脚本 `scripts/clean-target.ps1`
> 支持用 `-Path` 一次处理多个仓库（三个仓库通常并排放在同一个工作目录下，例如
> `D:\GitCode\RF\`）。

## 1. `target/` 为什么会膨胀

| 来源 | 典型体积 | 说明 |
|------|----------|------|
| 依赖的完整调试信息（Windows `.pdb`） | 单个集成测试二进制 100–200 MB | 默认 `debug = 2` 会为**每个**依赖、**每个**链接产物写入 PDB；`cargo test --workspace` 会链接几十个测试二进制，每个都携带全部依赖的调试信息 |
| incremental 编译缓存 | 单仓库 40 GB 级别 | 每个「测试目标 × 特性组合 × rustflags × 工具链」组合都会生成独立的 `incremental/<target>-<hash>` 会话目录；Cargo **不会**回收旧 hash 的目录 |
| 旧 hash 的编译产物 | 几十 GB | `cargo test -p X` 与 `cargo test --workspace`、`--all-features` 与默认特性交叉构建时，依赖特性统一结果不同 → 产物 hash 不同 → 新旧产物同时保留 |

一句话：`target/` 的体积约等于「当前构建」∪「历史上所有构建方式」，**只有 `cargo clean`
（或下述修剪脚本）才会真正回收**。仅修改 profile 不会让已存在的历史产物消失。

## 2. 已经落地的 profile 配置

`Cargo.toml` 末尾（`[profile.*]`）：

```toml
[profile.dev]
debug = "line-tables-only"      # 工作区 crate 保留行号表：panic/backtrace 仍是 file:line

[profile.dev.package."*"]
debug = false                   # 第三方依赖不带调试信息（PDB 的主要来源）

[profile.test]
debug = "line-tables-only"
incremental = false             # 测试单元不再产生 incremental 缓存（最大的遗留目录来源）

[profile.test.package."*"]
debug = false

[profile.bench]
incremental = false
```

| 设置 | 解决什么 | 代价 |
|------|----------|------|
| `dev/test` 用 `line-tables-only` | 去掉 PDB 里体量最大的类型信息，保留 `file:line` | 断点看不到变量值/复杂类型结构（仍可单步） |
| 依赖 `debug = false` | 每个测试二进制不再复制全部依赖调试信息 | 断点进入第三方 crate 无源码映射；需要时临时注释该行 |
| `test`/`bench` `incremental = false` | 消除按特性/参数分叉的 incremental 会话目录堆积 | 测试/基准的全量编译略慢；日常 `dev`/`check` 仍保留 incremental，编辑-编译循环不受影响 |
| `bench.incremental = false` | 同上，bench 目标通常是额外泄漏源 | 无感 |

> rust-webx 在此之上还关闭了 `dev` 的 incremental（该仓库 crates 多、构建方式变化频繁，
> 实测历史缓存约 39 GB）。需要时用 `CARGO_PROFILE_DEV_INCREMENTAL=true` 临时打开。

修改 profile 后**第一次构建会全量重编**（profile 参与 fingerprint），属于预期行为。

## 3. 日常清理

### 3.1 修剪脚本（推荐，可覆盖整个 `D:\GitCode\RF\`）

`scripts/clean-target.ps1` 只动 `target/`：清理超过保留期的旧编译产物（`.pdb`、`.exe`、
`.rlib`、`.rmeta` 等）与旧的 incremental 单元/会话目录，保留最近一次构建，因此清理后
`cargo test` 依然可用。脚本检测到 `cargo`/`rustc` 正在运行时会拒绝执行（避免删到正在写
的产物）；确认没有构建在跑时可加 `-Force` 跳过检查。

```powershell
# 交互式 PowerShell（执行策略允许时）：先 -WhatIf 干跑，确认会回收多少
./scripts/clean-target.ps1 -Path ., ../rust-ef, ../rust-webx -WhatIf

# 清理三个仓库中超过 14 天的旧产物
./scripts/clean-target.ps1 -Path ., ../rust-ef, ../rust-webx

# 只清 incremental 缓存（最安全：纯缓存，删了只会触发一次重编）
./scripts/clean-target.ps1 -Path . -IncrementalOnly

# 全清（等价于 cargo clean，下次全量重编）
./scripts/clean-target.ps1 -Path . -All
```

Windows PowerShell 5.1 的默认执行策略是 `Restricted`，直接运行 `.ps1` 会被拒绝，此时用
下面任一方式（效果相同）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -Command "& './scripts/clean-target.ps1' -Path '.', '../rust-ef', '../rust-webx' -WhatIf"

# -File 调用时数组会折叠成一个字符串：用逗号分隔，且不要加空格
powershell -NoProfile -ExecutionPolicy Bypass -File ./scripts/clean-target.ps1 -Path .,../rust-ef,../rust-webx -WhatIf
```

常用参数：`-OlderThanDays <N>`（默认 14，`0` = 清理全部旧产物）、`-IncrementalOnly`、
`-All`、`-WhatIf`、`-Force`。

### 3.2 不依赖脚本的等价命令

```powershell
cargo clean                                    # 全清 target/
Remove-Item -Recurse -Force target\*\incremental   # 只清 incremental 缓存
Get-ChildItem target -Recurse -Filter *.pdb | Measure-Object Length -Sum  # 看看 PDB 占多少
```

### 3.3 可选：`cargo-sweep`

`cargo-sweep` 能按「最近一次构建用到的产物」做精确修剪，适合长期迭代的机器：

```powershell
cargo install cargo-sweep --locked
cargo sweep -t 14          # 删除 14 天前的产物
```

## 4. 实测数据（Windows / MSVC，2026-11）

| 仓库 | 清理前 `target/` | 清理 + 新配置全量 `cargo test --workspace --no-run` + `cargo build` 后 | 说明 |
|------|------------------|--------------------------------------------------------------------------|------|
| rust-dix | 11.58 GB | 0.34 GB | 327 个测试全部通过 |
| rust-ef | 79.63 GB | 2.84 GB | 全部测试目标编译通过；PDB 1.33 GB、incremental 0.21 GB |
| rust-webx | 147.39 GB | 3.00 GB | 全部测试目标编译通过；PDB 1.20 GB、incremental 0 |
| **合计** | **238.60 GB** | **≈ 6.19 GB** | 约 **97%** 的 `target/` 空间被回收 |

同配置下的 fresh build 对比（rust-dix）：默认 profile 0.91 GB → 调优后 0.34 GB
（incremental 0.41 GB → 0.02 GB）。历史产物才是「几十 GB」的主因，所以**清理动作不可省略**。

此外，`scripts/clean-target.ps1 -OlderThanDays 0` 的修剪路径也做过回归验证：删除全部旧产物
（含 `.rlib`/`.rmeta`/incremental）后 `cargo test --workspace` 仍能正常增量重建并通过。

## 5. 注意事项

- **CI 不受影响**：GitHub Actions 每次都是全新构建，profile 只让构建更快/产物更小；
  本地 `target/` 与 CI 缓存互不影响。
- **不要**把 `CARGO_TARGET_DIR` 指向网盘/同步盘；也不要把 `target/` 提交进 git。
- 依赖调试信息关闭后，断点进入第三方 crate 只能看到汇编；需要时临时注释
  `[profile.dev.package."*"]` 一行再构建。
- 内存/时间充裕的情况下，想换回完整调试信息：把 `debug` 临时改为 `2`（或
  `CARGO_PROFILE_DEV_DEBUG=2`）。
- 清理脚本只处理 `target/` 下的文件，不触碰源码、`Cargo.lock` 或 git 状态。
