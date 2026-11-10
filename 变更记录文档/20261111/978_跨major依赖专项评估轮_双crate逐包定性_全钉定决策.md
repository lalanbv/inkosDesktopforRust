# 978 号 · 跨 major 依赖专项评估轮：双 crate 逐包定性，全钉定决策

> 977 备案清偿（「engine-rs 11 包/src-tauri 15 包 unchanged behind latest=
> semver 不兼容不进 compatible 面，跨 major 留专项不盲推」）。本轮=评估定性轮，
> **零代码改动**（Cargo.toml/lockfile/源码全不动）。枚举 22 包（去重后）跨
> semver behind latest，逐包用面勘察+族绑定验证+决策——结论：**全部钉定**，
> 无一满足升级触发条件。必红形态=audit:npm 四夹具+audit:rust 双夹具自检活体
> （903/977 巡检族先例，无 NEUTER/stash 形）。

## 一、枚举面（口径与清单）

- **口径**：直接依赖（Cargo.toml `[dependencies]`+`[dev-dependencies]`+
  `[build-dependencies]`）× Cargo.lock 现版本 × crates.io API
  `max_stable_version`，semver 判定（major 不同即不兼容；0.x 按 minor）。
  977 的 11/15 为全图口径（含传递依赖），本轮 10/12 为直接依赖可控面口径
  ——不冲突，本轮更精确指向决策面。
- engine-rs 35 直接依赖 → **10 包 INCOMPAT**；src-tauri 42 直接依赖 →
  **12 包 INCOMPAT**；去重后 22 包（reqwest/sha2/thiserror/criterion 双 crate
  同现）。wasmtime 首扫 UNKNOWN（API 抖动）补查=50.0.1 确认。
- 全清单：engine-rs=axum 0.7.9→0.8.9、base64 0.22.1→0.23.1、criterion
  0.5.1→0.8.2、hmac 0.12.1→0.13.0、lopdf 0.42.0→0.47.0、reqwest
  0.12.28→0.13.5、sha2 0.10.9→0.11.1、thiserror 1.0.69→2.0.21、tower-http
  0.5.2→0.7.1、ts-rs 10.1.0→12.0.1；src-tauri 另有 dirs 5.0.1→7.0.0、
  ed25519-dalek 2.2.0→3.0.0、keyring 3.6.3→4.2.0、notify 6.1.1→8.2.2、
  rand 0.8.8→0.10.3、toml 0.8.23→1.2.1、ureq 2.12.1→3.5.1、wasmtime-wasi
  36.0.20→50.0.1（连带 wasmtime 36→50）。

## 二、逐包定性台账（决策=全部钉定）

| 包（族） | 用面密度 | 定性理由 |
|---|---|---|
| axum | 457 calls/33 files（server 路由全链） | 高密度用面=大专项；0.8 为破坏性发布；无安全触发 |
| tower-http | CORS 两函数（server/mod.rs:244/251） | 与 axum 族配套——axum 钉则族钉，单升无意义 |
| reqwest（双栈） | engine 37 calls/11 files + tauri 18/6 | 双 crate 同步升级专项；LLM 请求/下载/通知全链过手；无触发 |
| thiserror | 38 处 derive/37 文件（双 crate）；`cargo tree -i` 反转面=仅自身 | 纯版本卫生零收益（无新能力需求） |
| sha2 ↔ hmac | sha2 10+ 文件（工件修订/local_search/权限）；hmac 单站 notify/dispatcher.rs:96 `Hmac<Sha256>` | digest 族绑定（0.10 族）——单升不互通，族升触碰 sha2 全哈希面 |
| base64 | 12 calls/5 files | 低收益无触发 |
| lopdf | 单站（PDF 域） | 跨 5 minor 无触发 |
| ts-rs | 49 文件 `#[derive(TS)]`=schema 单一真源（models 模块） | 升级=双栈契约面全量重建（全部 .ts 导出+golden 重锁）——须专项且无触发 |
| criterion / rand | dev-only（bench/测试） | 非产品面，不投资 |
| dirs | 11 calls/2 files（目录定位） | 低收益无触发 |
| ed25519-dalek | 14 calls/5 files（updater 签名验证） | **安全敏感面**——升级须重验签名链活体，无触发不轻动 |
| keyring | 4 calls/2 files（凭据存储） | **安全敏感面**同上 |
| notify | 9 calls/2 files（书库监视） | 跨 2 major 无触发 |
| toml | 18 calls/6 files | 1.0 为稳定化发布但 0.8 无 RUSTSEC 无功能缺口 |
| ureq | 单站 | 低收益无触发 |
| wasmtime ↔ wasmtime-wasi | plugin/runtime.rs 单文件但 `bindgen!`+36 特有 API（「wasmtime-wasi 36：table 并入 WasiCtxView」注释自证） | 跨 14 minor=插件运行时大专项；族绑定必须同升 |

**共同前提**：audit:rust 双 0（RUSTSEC 零压力）——全部 22 包均无安全紧迫性；
升级唯一正当动机（安全清偿）不存在，「不盲推」纪律落定为全钉定。

## 三、升级触发条件备案（满足任一按族专项）

1. 任一钉定包出现 RUSTSEC（audit:rust 由 0 转非 0）；
2. 上游协议扩展或新真实需求需要某包新 major 能力；
3. 巡检轮 audit/advisory 面提示。

族绑定表（专项时整族推进）：hmac↔sha2（digest 族）、axum↔tower-http、
wasmtime↔wasmtime-wasi、reqwest 双 crate 同步、ts-rs 须配双栈契约面
golden 全量重建、ed25519-dalek/keyring 须配安全链活体重验。

## 四、必红形态与门禁

- **必红形态**（903/977 巡检族先例：评估/巡检轮无行为改动，无 NEUTER/stash）：
  - audit:npm 四夹具全活（未白名单红/错误形状红/白名单钉定绿/未钉定新 id 红）。
  - audit:rust 双 0（engine 2 条/src-tauri 9 条 unmaintained/yanked 警告维持，
    yanked 联网核验超时 335 次为网络抖动）+双夹具自检活体（7+1 RUSTSEC 检出）。
- **门禁引用**：本轮零代码改动（git status 干净、双 Cargo.toml/lockfile 未动）
  ——clippy 双 0/testgate 2110+584/gate:ts 十步/U+FFFD 引用 977 收口全绿面
  （965/976 先例：树同则面不变）；本轮新增活体=audit 双栈自检（上）。
- 真引擎活体豁免：无行为改动面（枚举为 crates.io API 网络勘察，非引擎面）。

## 五、结论

- 977 备案「跨 major 留专项不盲推」清偿：22 包逐包定性全钉定，触发条件备案。
- 出口回归常备：上游协议扩展/外部事件/新真实需求/下一巡检周期；EPUB 字节级
  维持 974「如需」。

**下一号自 979 起；用户侧待办=Fork 推送（含 955-978）。**
