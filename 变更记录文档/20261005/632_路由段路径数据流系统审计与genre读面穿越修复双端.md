# 632 号：路由段→文件路径数据流系统审计（201 守卫外全命名空间）——genre 读面穿越修复双端

日期：2026-10-05。类型：fix(engine+studio)+chore(smoke)。前置：631 号 SQLite 并发面审计的方法论延伸（审计先画全数据流矩阵、判定逐点做、修复只打实锤缺口）。

## 一、审计范围与方法

201 号段守卫（`segment_guard::guard`，挂 server/mod.rs:941）只覆盖 `/api/v1/books|projects/{id}` 两命名空间。本号对**守卫外全部带 id 段路由**（全路由清单 90+ 条逐一枚举）做「id 段 → 文件系统 join」数据流审计：

| 路由面 | join 点 | 判定 |
|---|---|---|
| `books/:id/truth/*file` | `resolve_truth_file_path`：拒绝空/NUL/绝对路径/`..` + flat/outline/regex 白名单 + `strip_prefix` 复核 | ✓ 深防御 |
| `skills/:skillId`（DELETE） | `normalize_studio_skill_id` → `project_skill_path` | ✓ |
| `prompt-packs/:promptId` | `normalize_studio_prompt_id` | ✓ |
| `translations/:id`（detail/run/export） | `is_safe_book_id` 三处齐 | ✓ |
| `play/.../images/:file` | `normalize_segment` + 手拒 `/`、`..`、NUL | ✓ |
| `asset-library/:kind/assets/:id` | `parse_library_kind` + 列表过滤（id 不 join 路径） | ✓ |
| `series/:seriesId/canon` | `is_valid_series_id` | ✓ |
| `services/:service/secret` | secrets map 键查找（非路径）+ `is_header_safe_api_key` | ✓ |
| `style-profiles`、`sessions` create | 无段参数 / sessionId 形态校验（timestamp-random，625 号） | ✓ |
| **`genres/:id`（GET detail）** | **无验证**（写面 PUT/copy 有 `genre_id_is_unsafe`） | **✗ 真缺陷** |

## 二、真缺陷：genre GET 读面穿越（双端）

`read_genre_profile` 的 `project_root.join("genres").join(format!("{genre_id}.md"))`：axum `Path` 对段做 percent-decode（matchit 匹配原始段、提取时解码），`..%2Fdecoy` 解码为 `../decoy` → join 出 `genres/../decoy.md` = **项目根外任意 `.md` 文件直读**（200 带 content）。写面（PUT/CREATE/copy）双端均有同款校验，唯独 GET 读面双端都缺——**读写不对称是既有缺口而非移植回归**（TS `server.ts` genre GET 同样直读，同文件 20 行外的 copy 端点却有 regex 守卫）。

**红绿可证伪（Rust oneshot 探针）**：tempdir 根放 `decoy.md`（真仓 cozy.md 内容做 fixture，避免自造内容解析失败伪象）→ `GET /api/v1/genres/..%2Fdecoy` → **旧码 200 泄内容实锤** → 加守卫后 400；正常 id（wuxia）回归 200。

## 三、修复面

1. **engine-rs/src/server/genre_routes.rs**：`genre_detail` 补 `genre_id_is_unsafe` → 400 `INVALID_GENRE_ID`（复用写面 helper，错误形状逐字对齐 TS copy）；handler 返回类型统一 `.into_response()`（impl IntoResponse 单具体类型约束）。
2. **packages/studio/src/api/server.ts**：genre GET 补 copy 端同款 regex 守卫（同 ApiError 形状）。
3. **scripts/node-fallback-smoke.mjs**：新增双引擎 genre 穿越探针断言（诱饵 `decoy.md` 落项目根、genres 外；`GET /api/v1/genres/..%2fdecoy` 须 400）——守卫从此在门禁内永久双引擎覆盖。
4. **packages/core/src/play/play-db.ts**：`PRAGMA busy_timeout = 5000`（**631 备案清偿**：play.db 同面补齐，CLI 与桌面引擎跨进程同 run 并发写等待而非瞬时失败；Rust play 侧 rusqlite 默认 5000 已覆盖无需改动）。
5. **sqlite-pragmas.test.ts**：+PlayDB pragma 断言（3/3 绿）。

**TS 侧红验证**：`git stash` server.ts 守卫 → node 腿 smoke `✗ genre 穿越探针 400`（1 项断言失败）→ pop 恢复 → 双腿绿。**Rust 腿陈旧二进制插曲**：smoke 首跑 rust 腿红=debug 二进制未含新守卫（623/628 号「重建二进制」教训同款）——`cargo build` 重建后双腿全绿。

## 四、门禁

clippy 双 0；cargo:testgate engine 45 目标 **1904 passed**（+2 genre 测试）+ src-tauri 584 绿；gate:ts 七步全绿（build 17.8/typecheck 17.0/test 76.2/audit 2.0/node-fallback-smoke 34.6〔含新探针〕/engine-contract-diff 40.7/export-epub-smoke 34.2）；bench:gate 见收口补充（Spotlight 风暴复发轮询低谷，625/631 先例）。

## 五、教训

1. **集中式守卫的覆盖面要按路由表全量枚举复核**：201 号守卫只覆盖当时的 books/projects 两命名空间，后续新增的 genres/skills/sessions/translations/play 等 id 路由天然落在守卫外——「有守卫」≠「全守卫」，本次矩阵化枚举是补盲区的唯一手段。
2. **读写面对称性是审计的免费线索**：写面有校验而读面没有（genre 双端同病）= 自我暴露的不一致，比对同资源各 HTTP 方法间守卫差异最快锁定缺口。
3. **fixture 内容合法性是端点测试伪象的高发源**：自造「# 标题」内容触发画像解析失败 404，与穿越 404 混淆——用真仓数据做 fixture（CARGO_MANIFEST_DIR 引用）消除一类伪象。
4. smoke 双引擎探针的红绿判定必须区分「守卫缺失红」与「二进制陈旧红」——先重建再判读（623/628 号教训第三次实证）。

（632号）
