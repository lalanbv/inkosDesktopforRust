# 1062 号 · mz 弃坑标记响应与跨 major 复查轮

> 1061 出口执行（上游协议事件响应+npm 跨 major 触发条件周期复查）。零代码改动轮。

## 一、上游协议事件：mz 双件官方 deprecated 标记（1036 单线化定性的官方确认）

```
@mariozechner/pi-agent-core 0.73.1 → Deprecated
  "please use @earendil-works/pi-agent-core instead going forward"
@mariozechner/pi-ai 0.73.1 → Deprecated
  "please use @earendil-works/pi-ai instead going forward"
```

- **1036「上游已实质单线化」定性获 mz 官方背书确认**——弃坑标记直指 earendil 继任
- **触发条件制结构更新**：「mz 复活」条件**官方永久关闭**（弃坑标记=永不复活）；
  等待条件收敛为「earendil breaking/能力需求/安全事件」单翼+「安装警告用户面」观察
- **不构成迁移启动充分条件**：deprecated 是元数据动作，0.73.1 运行时代码零变化
  （1039 重估维持：功能稳定=维持现状合理）；迁移大专项启动条件=earendil 侧事件逼迁
- 仓内 lockfile 锁 0.73.1 安装不受影响（pnpm deprecation warning 非错误）；
  advisory 监控对 deprecated 包仍有效

## 二、npm 跨 major 面周期复查（978 触发条件制）

| 包 | 现状 | 触发条件 |
|---|---|---|
| commander 13→15 / ink 7→8 / dotenv 16→18 / js-yaml 4→5 / @hono/node-server 1→2 / ai 6→7 / @streamdown 1→2 / @vitejs/plugin-react 4→6 / @types/node 22→26 | 全部钉定维持 | **安全=未命中**（audit:npm 绿 0 advisory）/能力=无新需求 |
| @playwright/test 1.65.0→1.65.1 | patch 级 | 非跨 major 备案范围；随手升级低风险但须全量验证——下轮快检顺手项 |
| @mariozechner 双件 | 0.73.1 deprecated（见上） | 见第一节 |

## 三、docs 对齐抽查（1057 触及面）

grep localizeKnownRuntimeMessage/error-copy/错误文案 于 docs/*.md+README*.md **零命中**
——1057 cli TUI zh 承接未触及文档描述面，零对齐义务。

## 四、下一号自 1063 起

出口=earendil-mz 单包合并（**触发条件制已更新：mz 复活翼关闭，收敛为 earendil 侧
事件翼**）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1062）。
