# 变更记录：ABI 版本兼容校验

## 日期
2026-08-07

## 变更类型
feat(security): parse_manifest ABI 版本兼容性校验

## 问题
`abi_version` 字段在 manifest 中存储但从未验证。宿主升级 WIT 接口后，
旧版插件（abi_version=1）仍能被旧宿主加载，接口不匹配导致静默 UB。

## 修复
- `pub const HOST_ABI_VERSION: &str = "1"` —— 宿主 ABI 版本常量
- `parse_manifest` 新增校验：`abi_version != HOST_ABI_VERSION → InvalidManifest`
- 测试 `test_parse_manifest_rejects_incompatible_abi`：
  - abi_version=99 → Err（含 "abi_version" 信息）
  - abi_version=HOST_ABI_VERSION → Ok

## 累计测试数
512 passed, 0 failed（全量）
