# 1605 parity 覆盖面加深轮（2026-11-20）

## 主靶面：双守卫 fixture 扩展至全 struct 字段形态（段内键集全量进 parity）

1604 的 fixture 只覆盖顶层+各段 default 形态（services/cover/notify 段内键集未进 parity）——1602 的 detection 四键证明**段内键漂移有真实抓虫产出**，同族候选面（notify 变体/cover/services）在本轮锁定：

fixture 扩展（minimal JSON）：services 全字段条目（service/name/baseUrl/temperature/apiFormat/stream）+cover 对象三键（service/model/baseUrl）+defaultModel+notify 四变体（telegram{botToken,chatId,format}/wechat-work{webhookUrl,format}/feishu{webhookUrl,format}/webhook{url,secret,events,format}——含 secret/events 可选键形态）。

流程：Rust 守卫 minimal JSON 扩展→KEYSET_WRITE_FIXTURE env 一次性重生成（守卫测试内临时分支，写完即删）→守卫绿→TS parity 消费。

## 结果：零新漂移确认（机制化覆盖面加深的价值形态）

- **键集对比过**：services 六键/cover 三键/notify 四变体全部字段与 TS schema 键集零缺口。
- **全文 parse 一次过**：notify discriminated union 四变体（kebab tag）/secret 可选/events 数组/cover 对象 null→对象两形态全部对齐。
- parity 断言更新：cover 对象形态断言（1604 的 toBeNull 断言随 fixture 形态扩展迁移）+**cover:null 独立回归锁用例**（1604 抓虫点两形态都锁）。

## 验证链

- Rust 守卫绿（扩展形态 fixture==struct 序列化）+TS parity 3 用例绿。
- **core 全量 1085 passed**（1604 后 1084+1）+dist 重烤+typecheck 干净。
- **Node 腿活体**：扩展 fixture 全文（notify 四变体+services+cover 对象）起服 SPA 200+profiles 200。
- gate:ts 十腿全绿（smoke+差分腿吃修复后 dist）+duel 10/10+键集守卫绿+fmt 双 0。

## 清场

端口释放+tmp 清理。

## 后续

- 下号自 1606 起；用户侧待办：① Fork 图形端推送（955-1605）② 真实 Tauri build 打包验证。
