# 169 · 入库 CodeGraph 项目级索引配置 codegraph.json

- 日期：2026-08-25
- 模块：`codegraph.json`（工具配置）
- 类型：chore
- 关联：无（新增独立配置文件，此前一直以未跟踪状态留在工作区）

## 一、背景

CodeGraph 在本仓建立代码索引（`.codegraph/`，已在 .gitignore 中忽略）。索引器同时读取项目根的 `codegraph.json` 作为项目级排除配置，该文件此前只存在于本机工作区、从未入库。`.gitignore` 末行已预留决策注释：「`.codegraph/` 本机生成不入库；`codegraph.json` 配置需团队共享故不忽略」——即入库是既定意图，本次补执行。

## 二、改动

仅新增 `codegraph.json` 一个文件，内容为排除清单：

- 双保险层：`**/node_modules/`、`**/target/`（src-tauri/target 达 46G）、`**/dist/`、`**/coverage/`、`**/.turbo/`、`src-tauri/gen/`、`src-tauri/engine/`、`.git/`、`.codegraph/`
- 补齐 .gitignore 未覆盖的非源码目录：`test-project/`、`docs/`、`books/`、`worlds/`、`.inkos/`、`prompt/`、`变更记录文档/`、`开发时SpecCoding'sPlan/`

## 三、验证

- `git status`：入库后工作区仅剩测试截图类临时产物（`gui-test-screenshots/`、`flow-after.png`，见鉴别结论，不随本次提交）
- 配置生效性由 CodeGraph 索引器自身消费，无需构建/测试

## 四、遗留与说明

- 同批鉴别出的 `gui-test-screenshots/`（6 张）与根目录 `flow-after.png` 为 GUI 测试临时截图：`flow-after.png` 曾入库（d0fc38a4）后在 9ca56ce8 被主动清除，`gui-test-screenshots/` 历史上从未入库，均维持本地不入库现状，待用户决定删除或加入 .gitignore。
