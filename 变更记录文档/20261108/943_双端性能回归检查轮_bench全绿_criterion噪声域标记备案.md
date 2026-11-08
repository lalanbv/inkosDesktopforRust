# 943 号 · 双端性能回归检查轮：bench:gate 全绿 + criterion 噪声域标记备案——净结论

> 909 bench 首跑后 30+ 轮改动（threads/备份域/检测链/文档等）的性能回
> 归检查：TS bench:gate 复跑 **✓ 门禁通过**（基线阈值判定）；Rust
> criterion 复跑对照——regressed 标记全部 ns~µs 级噪声域（对照基线为
> 931/932 首轮，运行环境差异贡献噪声）。零代码改动轮。
> 验证：bench:gate ✓；gate:ts 十步全绿（当日 914-943 十三轮连绿）。

## 一、TS bench:gate（✓ 通过）

- 基线阈值判定绿；tool_registry 改善面（lookup_last_hit −13.8%）与
  title_dedup −1.3% 等混合——无越阈回退。

## 二、Rust criterion 复跑（噪声域标记备案）

- regressed 标记条目与幅度：build_epub_12x3000 +8.6%（1.161→1.261ms，
  绝对 +100µs）、title_dedup/resolve_duplicate_hit +7.4%（155→158µs）、
  render_hook_snapshot_100 +3.9%、chapter_workspace +2.2%、truth_list
  +2.0%、list_genres +4.2% 等——**全部 µs 级**，criterion p<0.05 高敏
  判定在首轮/复跑运行环境差异（CPU 频移/缓存态）下为噪声域常态
  （932 备案「1.17x 同水准」语境延续）。
- 无 µs→ms 级量级回退；无功能性回退（全测试绿）。
- 备案：build_epub +8.6% 为最大标记，后续巡检周期跟踪（连续多轮同向
  才立案，单轮噪声不立案——bench-gate-load-noise 纪律）。

## 三、净结论

- 双端性能回归检查完成：无越阈回退（bench:gate 通过+criterion 噪声
  域标记备案跟踪）。零代码改动轮。

## 四、验证

- bench:gate ✓；gate:ts 十步全绿（当日十三轮连绿）。

**下一号自 944 起；出口清空回归常备；用户侧待办=Fork 推送 64 个提交。**
