# 595 号：592 结论勘误——env 伪象实锤+custom 服务边界发现（双端不一致方向精确化）

- 日期：2026-10-02
- 类型：audit(实测勘误)（实测+提案文档 595 再勘误节，零代码改动）
- 路线：592 号实测的干净复测（去 env 后证伪+边界归因）
- 状态：✅ 收口

## 勘误过程

592 的「Rust 零显式直发直接成功」经**去 env 重测**证伪：592 探针环境带了 `INKOS_LLM_BASE_URL`（env 层优先于配置解析），去掉后 Rust 零显式直发**失败**——请求打到缺省 `127.0.0.1:9`。

## 归因实锤（双端真实图景）

- Rust 层 3（`resolve_agent_model_override` 层 3）实现完整，但 **`custom:*` 服务被排除**：`list_models_for_service` 只查 preset bank，custom 不在 bank → 模型发现为空 → 下落缺省——`agent_production.rs:3911` 测试锁定该行为；
- Node 第 3 层**支持 custom**（`resolveConfiguredServiceBaseUrl` 读 inkos `llm.services` 的 baseUrl → /models 发现模型）——593 标准布局实测成功即为证；
- **双端不一致方向实锤=Rust 层 3 缺 custom 服务的 baseUrl 感知模型发现**（Node 有、Rust 无）。593 的「双端同构」结论勘误为「第 3 层 custom 支持双端不同构」。

## 裁决更新（提案文档 595 节已同步）

B 恢复「待评估」但方向精确化：若实施，落点=Rust 层 3 为 `custom:*` 服务补 baseUrl 感知的模型发现（对齐 Node 第 3 层）；影响面=Rust 解析行为变更（custom 服务零显式直发从失败变成功）+层 3 测试更新。590 错误注解不受影响保留。

## 教训

- 592 实测设计的 env 干扰：探针环境必须**最小化 env 兜底面**（INKOS_LLM_BASE_URL 这类覆盖变量恰是待测能力本身）——「成功」结论要归因到目标机制而非环境层；
- 实测结论发布前归因检查：592 把成功归给「四层解析第三层」却未排除 env 层命中。

## 下一号自 596 起
