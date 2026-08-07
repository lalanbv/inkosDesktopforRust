# 安全：插件子进程环境变量白名单

> 日期：2026-08-07
> 范围：`src/plugin/host_api.rs`、`src/plugin/process.rs`
> commit：`0c879217`

## 缺陷

两处子进程 spawn 均继承宿主**全部**环境变量：

| 位置 | 用途 |
|------|------|
| `host_api::exec_command` | 插件经 `system_command` 能力执行命令 |
| `process::PluginProcess::spawn` | 进程隔离插件的常驻子进程 |

插件是不可信第三方代码。宿主 env 可能含 `GITHUB_TOKEN`、`AWS_ACCESS_KEY_ID`、
`ANTHROPIC_API_KEY`、代理凭据等——一次 `std::env::vars()` 即可全部读出，
**完全绕过能力模型**。`system_command` 白名单管的是「能跑哪个命令」，
不管「跑起来能看到什么」。

这是真实的密钥泄露面：即便插件只被授权跑 `echo`，它也能通过 `echo $GITHUB_TOKEN`
或直接读自身环境取走密钥。

## 修复

`env_clear()` + 显式白名单，两处共用 `apply_env_allowlist` 单点定义（防漂移）：

保留项限于**运行必需**：`PATH`（找可执行文件）、`HOME`、`TMPDIR`/`TMP`/`TEMP`、
`LANG`/`LC_ALL`（编码）、`TZ`。其余一律不传。

白名单（而非黑名单）是唯一正确方向：黑名单需穷举所有密钥变量名，
新增一个 `FOO_API_KEY` 就漏一个；白名单默认拒绝，新增密钥自动被挡。

## 测试设计：必须读实际环境

断言白名单常量的内容是**同义反复**——它只验证「常量等于常量」，
`env_clear()` 漏调也照样通过。

改为真实 `env` 子进程读取实际环境：

1. 注入 `INKOS_TEST_FAKE_TOKEN=super-secret-value-must-not-leak` 到宿主
2. 经 `exec_command` 跑 `env`
3. 断言 stdout **不含**密钥值、**不含**密钥名
4. 断言 `PATH=` **存在**（反向保证白名单不过严——过严会让子进程找不到可执行文件）

`std::env::remove_var` 在断言**之前**执行，断言失败也不污染后续测试。

## 反向验证（防假绿）

临时把 `cmd.env_clear()` 改成 `if false { ... }` 重跑：

```
test ..._env_isolation_hides_host_secrets ... FAILED
子进程环境泄露了宿主密钥值，stdout: AI_AGENT=claude-code_2-1-223_agent
```

测试确实捕获缺陷，非环境巧合。这一步是必要的——本轮多次遇到
「测试通过只因环境恰好如此」的假绿（见 `27_`、`29_`、`32_`）。

## 验证

- `cargo test`：**548 passed / 0 failed**
- `cargo clippy --all-targets -- -D warnings`：零警告
