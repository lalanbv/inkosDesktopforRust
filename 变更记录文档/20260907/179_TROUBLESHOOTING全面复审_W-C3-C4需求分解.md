# 179 · TROUBLESHOOTING 全面复审 + W-C3/C4 产品级需求分解

- 日期：2026-09-07
- 模块：docs/TROUBLESHOOTING.md（本地生效不入库）、开发时SpecCoding'sPlan/inkosDesktop/05_UI体验设计/W-C3-C4_产品级功能需求分解_v1.md（新增）
- 类型：docs + 规划
- 关联：164 号（Rust 直启）、170/173 号（回环守卫/CORS）、174–178 号（检查点/停止/门禁/三语 README）、[全局分析与案例对比_v1](../../开发时SpecCoding'sPlan/inkosDesktop/06_全局重构优化/全局分析与案例对比_v1.md)

## 一、TROUBLESHOOTING.md 全面复审（逐条对码）

旧文是上游继承内容 + 早期虚构细节，本次逐条对照真实代码取证后重写。主要修正：

| 旧文内容 | 取证结论 | 处置 |
| --- | --- | --- |
| 日志路径 `~/Library/Logs/InkOS Desktop/` | 实际 `{data_dir}/inkosDesktop/logs`（main.rs `APP_DATA_DIR_NAME="inkosDesktop"` + init_logging）；panic 快照在 `…/crashes/` | 全文路径更正（macOS/Win/Linux 三形态） |
| Engine 整章「下载失败/手动下载 engine-*.tar.gz/manifest+node+index.js」 | Rust 直启（164 号）零下载；Node 回退链路引擎代码随包分发，bootstrap 只下载 **Node 运行时**（nodejs.org LTS 22.x + SHASUMS 校验，失败回退系统 node） | 章节重写：Rust 直启为主（engine-rust 三级探测、updater .bak 回滚），Node 回退单列；`INKOS_ENGINE_MIRROR`/`engineMirror` 配置确认不存在，删除 |
| 端口 3000 | `DEFAULT_STUDIO_PORT=4567` + `pick_free_port` 自动探测 | 更正，并说明「被占自动后移，一般无需手工处理」 |
| Keychain service=`inkos-desktop`、account=`api-key` | 实际 service=**`inkosDesktop`**（main.rs L664）+ `__index__` 元条目；keychain 不可用降级为警告不阻塞 | 更正 + 补降级语义 |
| 配置键 `maxConcurrentChapters` / `autoAudit` / `debug` | 均不存在（桌面壳配置是 `logging.level` 分层配置） | 删除，换成真实优化项（日志清理/RUST_LOG/模型选择） |
| 「更新后回滚 .app.bak」 | 实际 updater 回滚点是 `app_data/engine-rust.bak`（166 号） | 改写为引擎资源回滚步骤 |
| 联系渠道：微信群「见主 README」、`support@inkos.ai` | 均虚构（主 README 无微信群） | 只留 GitHub Issues，强调脱敏 |
| `docs/API.md` 链接 | 文件不存在（断链） | 移除 |
| `.inkos/backups/` 章节备份恢复 | 目录不存在；真实机制是安全点落盘 + 原子替换（atomic_file_set） | 重写为「中断不产生半章」的真实语义说明 |
| 状态栏文案「Engine 下载失败」等 | UI 无此文案 | 删除 |

**新增章节「任务与写作问题」**（174–178 号新能力的用户侧说明）：重启后「任务已中断」是预期对账行为 + 残留快照手动清理路径（`.inkos/tasks/`，URL 编码文件名）；「停止写作」的阶段边界语义（Rust 在途 LLM 完成后停 / Node 诚实不可停）与耗时预期；SSE 断连时重连补发对账快照的恢复语义。另补 CORS 回环守卫的「远端 403 是预期安全行为」条目与 `INKOS_ENGINE_ALLOWED_ORIGINS` 出口。

**注意**：`docs/*` 被根 .gitignore 有意忽略（上游继承）——本文件改动**本地生效、不入库**，不随提交出现；叙事权威仍是根 README + 变更记录。

## 二、W-C3/C4 产品级需求分解（新增规划文档）

落位 `开发时SpecCoding'sPlan/inkosDesktop/05_UI体验设计/W-C3-C4_产品级功能需求分解_v1.md`。核心结论：

- **现状盘点**：番外/同人/仿写三入口已有（ImportManager 三 tab + 引擎 59 号端点），但**系列模型不存在**（book.json 无 series）、**时间线模型不存在**；章节 index（status/wordCount/时间戳）是 W-C4 可直接复用的现成数据面。
- **W-C3 切分**：C3-a series 字段+Dashboard 分组（完全自主）→ C3-b 从既有书抽取设定的回填引擎管道 → C3-c 向导化（复用 174–176 任务卡基建）。唯一需用户决策点：回填写入粒度（建议默认「预览+逐项勾选」）。
- **W-C4 切分**：C4-a 只读时间线网格（纯前端、章节 index 兜底、零引擎改动、完全自主）→ C4-b timeline.json 数据面 + 可选生成端点（建议默认关闭）→ C4-c 编辑回写与「按 beat 写章」接线。
- **180 号推荐**：W-C4-a（+可带 C3-a）——自主可落地度最高、零契约风险、为后续切分打地基。
- **边界登记**：不做拖拽规划编辑器、不做跨项目系列聚合、不建日历语义时间轴。

## 三、验证

| 项 | 结果 |
| --- | --- |
| TROUBLESHOOTING 事实核对 | 表列 10 项全部对码（main.rs/constants.rs/secrets/store.rs/supervisor.rs/engine/node/mod.rs/diagnostics.rs 等） |
| 链接与文件 | `docs/API.md` 断链移除；保留链接（USER_GUIDE/QUICK_START）存在 |
| 需求分解的事实基础 | ImportManager 四 tab、initSpinoffBook/importFanficCanon、chapters/index.json 字段、StoryGraphTree 归属均经 grep 取证 |
| 入库面 | 本批入库变更仅规划文档 1 份（docs/ 不入库）；无代码改动，测试基线不变 |

## 四、遗留

- 180 号：W-C4-a（推荐）± C3-a。
- 产品决策待用户：C3 回填写入粒度；C4-b 生成端点默认开关。
- W-A4b secrets 掩码（需 UI 决策）、W-B4/B5（插件生态）维持 backlog。
- 推送须在 Fork 图形端执行（既有约定）。
