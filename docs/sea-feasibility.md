# Node.js SEA 可行性调研与决策（M4f）

> 调研对象：把 inkos engine（Node runtime + 应用）打包为 Single Executable Application。
> 结论：**不采用 SEA**，维持 `tar.gz` bundle + 增量 delta（Phase 6.1）。理由见下。

## 一、SEA 原理

Node 22+ 提供 Single Executable Application（SEA）：
1. `node --experimental-sea-config sea-config.json` 把应用 blob 注入 `node` 二进制
2. `postsea` 去除签名/重签名（macOS 需 codesign）
3. 产物：单个可执行文件 = `node` 二进制 + 应用 blob

适用于「Node + 单入口脚本」的小型应用。

## 二、inkos engine 现状

`scripts/desktop-package-engine.sh` 组装的 engine：
```
src-tauri/engine/
├── dist/            packages/cli/dist（入口 dist/index.js）
├── node_modules      自包含（prod 真实，dev 符号链接）
└── manifest.json
```
运行时：M3 bootstrap 下载的 `node` 二进制（~40MB macOS）spawn `dist/index.js`。

`packages/cli` 依赖：`@actalk/inkos-core/studio`（monorepo workspace）、`commander`、`react`、`ink`（TUI）、`epub-gen-memory`、`marked` 等，pnpm workspace 协议 + overrides。

## 三、体积分析（核心论据）

| 组成 | 当前（tar.gz） | SEA |
|---|---|---|
| Node 二进制 | ~40MB（占大头） | ~40MB（同样占大头，SEA 不压缩 node） |
| 应用代码 | dist + node_modules，tar.gz 压缩 | bundle 成单 blob 注入 |
| 分发形态 | 单个 .tar.gz | 单个可执行文件 |
| **总体积** | **≈ node + 应用** | **≈ node + 应用（基本相同）** |

**SEA 不减小首装体积**——node 二进制占绝对大头，SEA 只是把它和应用拼成一个文件。

**SEA 不减小更新体积**——这正是 inkos 的真实痛点，而 Phase 6.1 的 bsdiff delta 已解决（相邻版本高度相似，delta 远小于全量）。

## 四、可行性障碍

即便决定采用 SEA，仍有显著工程成本：

1. **应用需先 bundle**：inkos 是 pnpm monorepo + workspace 协议 + overrides，需 esbuild/rollup 把 `dist/index.js` + 依赖打成单文件——动态 `require`、workspace 软链、`@actalk/*` 解析都要处理
2. **native 模块**：`epub-gen-memory` 等若含 native addon，SEA 注入需特殊处理
3. **跨平台构建**：每平台独立 `sea-config.json` + `postsea` + macOS codesign
4. **Node SEA 仍 evolving**：22 GA 但 native/CJS 边界 case 多

## 五、与 delta 方案对比

| 维度 | SEA | tar.gz + delta（已实现） |
|---|---|---|
| 首装体积 | ≈ 当前 | ≈ 当前 |
| 更新体积 | 全量替换 | delta（大幅减小）✅ |
| 分发文件数 | 1 | 1（.tar.gz） |
| 实现成本 | 高（bundle + 跨平台 + native） | 已完成 ✅ |
| 成熟度 | evolving | 成熟 |

delta 在「更新体积」（真实痛点）上完胜；SEA 仅在「单文件」上与 tar.gz 持平。性价比上 SEA 不占优。

## 六、决策

**短期不采用 SEA。** 维持 `tar.gz` bundle（M3）+ bsdiff delta 增量（Phase 6.1）。

理由：不减体积（node 占大头）、更新痛点已被 delta 解决、采用成本高（monorepo bundle + native + 跨平台）。

## 七、fallback（已就位）

- 首装：`engine-*.tar.gz`（M3，已生产）
- 更新：优先 delta（6.1），失败降级全量（6.1 apply_delta + 6.5 签名校验）
- 完整性：SHA256（M3a）+ Ed25519 来源认证（M4c）

## 八、体积对比工具（供未来重评估）

`scripts/measure-sea-vs-bundle.sh`（骨架）：在具备 Node 22 + esbuild 的环境，实测 SEA 单文件 vs `engine-*.tar.gz` 体积，验证本调研结论。

## 九、未来重评估触发条件

满足任一可重开 SEA 调研：
1. 首装体积成为核心痛点（用户投诉下载慢）——但更可能是优化 node binary（裁剪未用模块）
2. Node SEA 对 native 模块 + monorepo bundle 的支持成熟到零成本
3. 出现 SEA 独有的强需求（如无 Node 运行时环境的目标平台）

当前均不满足，故 M4f 结论为：**调研完成，决策不采用，fallback（delta）已就位**。
