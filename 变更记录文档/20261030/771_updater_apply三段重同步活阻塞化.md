# 771 号：updater apply 三段重同步活 spawn_blocking 化（770 豁免台账最大项清偿）

> 770 号豁免台账中最大的遗留面：engine 更新 apply 链——bundle 可上百 MB，
> 此前三段重同步活（整 bundle sha256 哈希 / 整 bundle 二次读+ed25519 验签
> / 解压+顶层展平）全部直接占 runtime 线程执行。770 号当时定性"重构
> async/sync 切分属大手术"缓行；本轮以最小侵入形态完成。

## 一、变更内容（2 文件）

`updater/engine.rs`：
- 新类型别名 `ExtractFn = dyn Fn(&Path, &Path) -> Result<()> + Send + Sync`
  （原 `&dyn Fn + Sync` 升格 Arc+Send 供跨线程；clippy type-complexity
  以别名消解）；
- `apply_inner` 三段插入 spawn_blocking（PathBuf owned 克隆入闭包，错误
  context 逐字保留）：
  1. SHA256 校验（读 .sha256 + `sha256_file` 整 bundle 哈希）；
  2. 签名验证（读 .sig + 读整 bundle + hex 解码 + ed25519 验签——验签
     内含全量哈希，同为 O(bundle) CPU/IO）；
  3. 解压+顶层展平（extract 回调 + flatten_single_top_dir）。
- 网络下载（download_to/fetch_release）保持 await 形态（本就 async）。

`main.rs`：
- 调用方适配：闭包升格遇 HRTB 生命周期泛型不满足（"implementation of
  Fn is not general enough"）→ 改 fn 指针形态（`engine_extract_adapter`
  + `as fn(&Path,&Path) -> Result<()>` 显式 HRTB）再 Arc 强转 dyn；
  实施插曲：适配器 fn 一度插在 `#[tauri::command]` 属性与命令函数之间
  致宏展开找不到目标——移至属性上方即解。

## 二、行为等价性

- 错误消息/context 逐字保留（含 sha 不匹配/验签拒绝/fail-closed 三分支）；
- staging 清理路径不变（apply 尾部统一清理，new_dir rename 走后清理空转）；
- 原子替换+回滚（atomic_replace_with_rollback）保持同步内联——rename 级
  操作亚毫秒，health 预检查读 manifest/入口产物为小文件读；
- 并发语义：apply 单飞（命令层无并发入口），spawn_blocking 仅改变承载
  线程，下载→校验→解压→替换顺序不变。

## 三、验证

- src-tauri cargo build + 全测 25 目标全绿（零失败）；testgate：engine-rs
  **1944 passed**（与基线同数=零触碰）+ src-tauri **583 passed**；
- clippy 双 crate **0 告警**（type-complexity 两处以 ExtractFn 别名消解）。
- bench 面：更新链非 21 基准组内（桌壳侧无 criterion），无需重锚。

## 四、收口状态

770 号豁免台账中唯一缓行项（updater 专项）已清偿；src-tauri async 宿主
阻塞面至此全部清偿或入档（engine/node bootstrap 一次性、registry 缓存/
diagnostics 亚毫秒级为记录在案的豁免）。
