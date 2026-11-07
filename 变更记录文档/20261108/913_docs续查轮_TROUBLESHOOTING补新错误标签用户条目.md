# 913 号 · docs 续查轮：TROUBLESHOOTING 补新错误标签用户条目（898-900 修复族文档闭环）

> 912 文档对齐续查（QUICK_START/TROUBLESHOOTING/RECOVERY/DESKTOP_
> WALKTHROUGH 四文件对照现状）：QUICK_START 构建与引擎切换声明健康、
> RECOVERY/WALKTHROUGH（桌壳域）与 11 月改动无交集净——**真实缺口=
> TROUBLESHOOTING 缺 11 月新错误标签的用户条目**（898-900 修复族不留
> 用户文档=半闭环）。本轮补两节，修复族文档闭环。
> 验证：gate:ts 八步全绿；U+FFFD 净。

## 一、补条（docs/TROUBLESHOOTING.md，任务与写作问题族尾）

1. **`COVER_DOWNLOAD_ORIGIN_BLOCKED`**（900 号策略的用户语义）：现象
   （封面生成成功但下载被 origin 策略拦截）→说明（同 provider origin
   放行=本地网关不受影响/其余 host 公网放行/环回私网保留地址与解析
   到私网拒绝）→部署方操作（`INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS`
   allowlist、`INKOS_COVER_DOWNLOAD_ORIGIN_POLICY=strict` 收紧）+
   相关预算（封面 600s/通知 15s env 提及）。
2. **`LLM_REQUEST_TIMEOUT`**（898 号看门狗）：现象（约 300 秒终止）
   →说明（buffered/流式同族有界不挂死；非可重试语义）→操作（端点停
   摆排查；`INKOS_LLM_REQUEST_TIMEOUT_MS` 放宽长耗时请求）。

## 二、其余三文件净结论

- QUICK_START：构建三步（上游前端/引擎 release/src-tauri run）与引擎
  切换、`INKOS_ENGINE_BACKEND`、回退告警声明均与现状一致。
- RECOVERY（42 行）/DESKTOP_WALKTHROUGH（67 行，桌壳 workspace 命令
  族人工清单）：与 11 月 works/管线改动无交集，无漂移。

## 三、验证

- gate:ts 八步全绿；U+FFFD 净；文档改动零代码。

**下一号自 914 起；出口清空回归常备；用户侧待办=Fork 推送 47 个提交。**
