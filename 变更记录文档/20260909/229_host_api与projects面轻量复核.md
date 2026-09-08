# 229 号：plugin host_api 深度抽查 + projects/workspace 管理面轻量复核——零缺口

- 日期：2026-09-09
- 分支：develop
- 关联：228 号（plugin 面初探——本批深挖 host_api 网络面）、src-tauri M3b（atomic_write_0600）
- 推送核验：origin/develop 仍停 6bc65564——**201–228 共 28 提交待推送**；本批次后本地领先 29。两项默认值无新答复。

## 一、plugin host_api 深度抽查（2k 行，网络/执行安全面）

| 审计点 | 结论 |
|---|---|
| **域名白名单算法** | ✓ `check_network_domain`：精确匹配 ∨ `*` 全开放 ∨ `.{a}` 后缀匹配（带点前缀）——经典绕过全部被拒：`evil-example.com` ⊁ `example.com`（前缀粘连）、`example.com.evil.com` ⊁ `example.com`（后缀注入）；大小写统一 lowercase |
| **SSRF 三层纵深** | ①IP 字面量拦截（loopback/private/link-local/unspecified + IPv4-mapped/6to4/NAT64 解封装判定）；②`ssrf_resolve` 连接期 IP pinning——关闭「解析-连接」DNS rebinding 窗口；③`"*"` 全开放插件也拦云元数据 169.254.169.254 |
| 配额 | ✓ 请求数/字节数双维独立；被拒请求先检查后计数——不耗配额不产生出网流量 |
| exec_command | ✓ 白名单 fail-closed（未声明 SystemCommand = 零命令可执行）；`is_safe_command_name` 拒路径/shell 元字符；成功执行也记审计日志 |

## 二、projects/workspace 管理面（632 行）

| 审计点 | 结论 |
|---|---|
| 持久化 | ✓ projects.json 与 workspace 状态均走 `atomic_write_0600`（原子替换 + 0600，M3b 共享 util） |
| 会话隔离 | ✓ workspace 按 project 独立状态文件 |

## 三、验证

纯复核批次（零代码改动）：src-tauri check/test/clippy 持续绿；engine 门禁于 227 号已绿（lib 1311/集成 196/clippy 0/duel 10/10），core 1917 / studio 793 / 双 typecheck 已绿。

## 四、遗留

无新增。轻量质量面复核至此已覆盖全部主要子系统，后续转向真实反馈驱动或新一轮演进方向探索。
