# 767 号：技能包契约公共件——双栈版本常量 + 可选 min-engine-version 诊断降级（路线④③公共件先行）

> 目标线：可扩展性轴实质落地第一步。三项设计决策五轮征询未获答复，
> 重新审计决策空间后确认：**③公共件与全部答案兼容**（767 施工预备
> cbac45d4 已裁决"任一分支都先做、与①②正交"）——能力件先行，
> 正式冻结承诺仍留待决策③。①信任门/②分发形态维持待用户，不预设。

## 一、变更内容（双栈 + golden）

1. **TS**（`packages/core/src/skills/external-loader.ts`）：
   - `SKILL_CONTRACT_VERSION = "2.0"`（取 core 正典 2.0.0 前两段）；
   - `parseSkillContractVersion`（"major[.minor][.patch]" → [major,minor]，
     1-3 段纯数字，第三段忽略）+ `skillContractSatisfied`（前两段比较，
     `<` 即不满足）；
   - `parseAgentSkillDocument` 门禁：可选 `min-engine-version`（kebab
     对齐 disable-model-invocation 惯例）——非法/不满足 → 抛错成
     diagnostic（拒这一份技能、加载循环继续、人侧可见原因 = 766 草案
     的"降级非硬拒"）；YAML 数值形态归一（`2.0` 数值 → "2"，推荐带引号）。
   - skills/index.ts 透出三个新符号。
2. **Rust**（`engine-rs/src/skills/external_loader.rs`）：逐字镜像
   （`SKILL_CONTRACT_VERSION`/`parse_skill_contract_version`/
   `skill_contract_satisfied` + parse 门禁；serde_yaml_ng 数值经 f64
   `{}` 格式与 TS String(number) 同形）。
3. **golden**（`golden/skill-contract-vectors.json`，第 33 守门域）：
   contractVersion=双栈常量唯一事实源 + 9 case 比较向量（相等/第三段
   忽略/older 双形态/newer 双形态/垃圾/空/四段非法）。TS 测试锁常量
   **且与 package.json 前两段比对（防常量与包版本漂移）**；Rust
   `tests/golden_skill_contract_diff.rs` 读同一文件。

## 二、语义边界（不预设决策③的依据）

- 能力（键处理+诊断降级）已落地；**兼容性承诺**（该键是否进公开契约）
  未承诺——766 草案节已更新为"能力在位、承诺待决策③"。
- 决策①（信任模型）②（分发形态）零触碰：本件不改变任何载入行为
  （缺键技能行为逐字不变）。
- 版本源裁决（core 2.0.0 线为比较基准）是技术事实非用户偏好：契约
  源头在 TS 腿，Rust 经 golden 锁镜像。

## 三、验证

- TS：external-skill-loader 26 passed（+2 新测：四技能门禁行为
  ok/numeric 过、future/garbage 成 diagnostic 且消息可见；golden 锁）。
- Rust：`golden_skill_contract_diff` 3 passed（版本锁+向量逐 case+边界
  形态）；skills 域 33 passed；全 lib **1504 passed**。
- 全目标 testgate：engine-rs **46 目标 1941 passed**（=1938+3 新测对账）
  + src-tauri **583 passed**——首轮 `health_probe_succeeds_when_server_up`
  一过性失败（起服务+探针类测试在 engine-rs 大批并行后的资源竞争窗口；
  本会话 src-tauri 零触碰），单测复跑即绿、全量复跑全绿，定性 flake
  如实入档。
- clippy 双 crate 0 告警；gate:ts 八步全绿（rust-bin 指纹重建）。
- bench 面：skills 载入非 21 基准组内，无需重锚。

## 四、剩余（待用户）

- 决策①信任模型 / ②分发形态 / ③正式冻结承诺——施工预备
  （`07_产品演进规划/技能包公开扩展点_767施工预备_*.md`）已铺全部
  分支路线，答复到达即按分支实施差量。
