# 950 号 · 跨端备份包互灌兼容性轮：GNU 长路径与 USTAR prefix 双端三通道

> 快照核验开场（上游 fetch 双 remote 零落后+npm audit 6 白名单 0 拦截
> +必红自检活体）中发现结构缺口：备份域 export 面九轮饱和，跨端互灌
> 零覆盖。深察定型长路径患面四项并双端修复——此前 Rust 导出的长路径
> 备份包在 Node 端导入整包 400、TS 导出对 >100 字节路径直接抛错。
> 验证：tar-read 9/9+backup-routes 9/9+Rust backup_routes 12/12（+2）
> +testgate 2059/584+typecheck 净+clippy 双 0+gate:ts 十步全绿+U+FFFD 净。

## 一、患面定型（勘察）

双端 tar 形态差异：Rust 导出=`tar::Builder`+`Header::new_gnu()`（GNU
格式，>100 字节路径写 typeflag `'L'` LongLink 扩展条目）；TS untar 自
称 USTAR 解析器且 typeflag 白名单仅 `'0'`/`'5'`/`'\0'`。真实 works 路
径 `inkos-backup/works/<uuid>/revisions/<uuid>/...`≈133 字节超 USTAR
name 字段 100 上限。四患：

1. **Rust 导出长路径包 → TS import 整包 400**（'L' 进 rejected →
   unsafe entries 拒收）——Rust 引擎导出→Node 回退端导入=断裂，最重。
2. **TS export >100 字节路径直接抛错**（writeTarString 上限 throw=
   export 500）——长路径 works 形态 export 不可用。
3. TS untar 无 POSIX prefix 字段处理（带 prefix 的包名错）。
4. TS 写侧未利用 USTAR prefix 拆分（≤255 可表达路径被放弃）。

## 二、修复（双端）

- **TS 读侧 tar-read.ts**：GNU 'L' LongLink 支持（payload=下一条目完
  整名，payload 同受解压上限）；POSIX USTAR prefix 拼接（magic 逐字节
  判 `ustar\0`，GNU 头该区无 prefix 语义不拼）；**安全检查一律作用于
  'L' 覆盖与 prefix 拼接后的最终名**（长名/prefix 通道不得绕过 `..`/
  绝对路径守卫）；PAX 'x' 无产出者维持拒绝（注释备案）。
- **TS 写侧 server.ts**：长路径三通道——≤100 单头；≤255 有 '/' 边界
  走 POSIX prefix 拆分（splitUstarPath）；其余（>255 或无边界）走 GNU
  'L' 写法（truncateTarName 字节安全截断真条目名，读取侧以 'L' payload
  为准）——与 Rust 导出形态对齐，export 抛错患清偿。
- **Rust 零生产代码改动**（tar::Archive 天然全格式读），仅回归锁。

## 三、测试（四向互灌矩阵）

| 写\读 | TS untar | Rust Archive |
|---|---|---|
| TS（prefix/'L'） | export 回灌用例（133B prefix 通道+>255 'L' 通道→另一 root 落盘） | import_accepts_ustar_prefix_entries |
| Rust（GNU 'L'） | GNU 包灌 TS import 200（修复前 400）+tar-read 'L' 用例 | export_long_paths_roundtrip_via_gnu_longname |

安全用例：'L' payload=`../../evil` 与 prefix=`..` 均被最终名守卫拒收；
'L' payload 计入 maxTotalBytes。Rust 侧新 helper tar_entry_block（POSIX
prefix+任意 typeflag 单条目块，simple_tar 因末尾双零块不可拼接）。

## 四、影响面核查

- server.ts 另一 buildTarArchive 调用点=/api/v1/projects/:id/export
  （交互电影 source 打包下载）——包由用户在外部解压（系统 tar 均支持
  prefix/'L'），不回流 import 端点；改动纯正向（长路径 500→可用）。
- 插曲：tar_entry_block 初版放 tests 模块外→clippy 拦（dead_code+
  items after test module），移回模块内与 simple_tar 并列。

## 五、快照核验（本轮开场，净结论）

- 上游 fetch：origin/upstream 双 remote 领先 0 提交——零落后。
- npm audit：6 advisory 全白名单（dev 工具链），拦截 0；必红自检 4 夹
  具活体。
- audit:rust/engine-rs+src-tauri 0 漏洞、clippy 双 0、gate:ts 十步连
  绿（work-alternation 5.2s/check-routes 0.1s 新哨稳定）。

**下一号自 951 起；出口清空回归常备；用户侧待办=Fork 推送（含 948/949/950）。**
