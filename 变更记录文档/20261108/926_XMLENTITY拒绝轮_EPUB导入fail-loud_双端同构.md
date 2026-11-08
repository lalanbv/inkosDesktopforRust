# 926 号 · XML ENTITY 拒绝轮：EPUB 导入 fail-loud（双端同构）——DOCTYPE 放行的兼容性取舍入档

> Mimosa 注入约束（解析不可信 XML 拒绝 DOCTYPE/ENTITY）指向的 XML 面
> 勘察：全仓无 XML 解析库依赖——EPUB 导入（translation/source.rs+
> epub.ts 双端）为**手写正则解析**，无实体展开机制=经典 XXE（外部实
> 体读本地文件）天然免疫。但 `<!ENTITY` 声明静默保持字面=损坏/恶意包
> 得到怪异行为而非明确错误——本轮补 **fail-loud 显式拒绝**（双端同
> 构）。零生产行为破坏：DOCTYPE 放行（真实 EPUB 普遍携带 XHTML DTD
> 声明，拒之破坏兼容性——取舍入档）。
> 验证：Rust 新测试 1/1（DOCTYPE 兼容性锚+ENTITY 拒绝双形态）+TS
> translation-ingest 6/6（+1）+testgate 2053+584+clippy 双 0+gate:ts
> 十步全绿+U+FFFD 净。

## 一、设计取舍（约束字面 vs 兼容性的工程平衡）

- **`<!ENTITY` 声明=拒绝**：XXE 唯一注入源是实体声明（正则解析虽不展
  开，声明在场即恶意/损坏信号——fail-loud 让其得到明确错误「declares
  <!ENTITY> (unsupported XML dialect)」而非静默怪异行为）。
- **DOCTYPE=放行**：真实 EPUB 章节普遍携带 XHTML DTD 声明；手写解析
  不 fetch 外部 DTD/不展开内部子集——DOCTYPE 本身无注入机制，拒绝会
  拒掉合法包（兼容性破坏）。约束字面「拒绝 DOCTYPE/ENTITY」在此域的
  安全等效实现=拒 ENTITY+放行 DOCTYPE，取舍与论据入档。

## 二、实现（双端同构）

### Rust（translation/source.rs）

- `read_entry` 闭包改 `Result<Option<String>, String>`：**Ok(None)=
  entry 缺失**（调用点按既有语义跳过/报缺包，不致命）；**Err=ENTITY
  声明**（致命传播）——语义保留修正（首版 Err-return 误把「损坏章节
  跳过」改致命，clippy ? 建议复核时当场纠正）。
- 读取点三处：container.xml（缺失=报缺包）、opf（同）、章节 XHTML
  （缺失=continue 跳过、ENTITY=Err 致命）——let-else+? 惯用形。

### TS（translation/epub.ts）

- `rejectEntityDeclarations(name, text)` 同文案三读取点接入
  （container/opf/章节 XHTML）。

## 三、测试（双端双形态）

- **兼容性锚**：章节带 `<!DOCTYPE html PUBLIC ...>` 的 EPUB 正常提取
  （title/章数断言）——放行语义锁定。
- **XXE 拒绝**：`<!DOCTYPE html [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>`
  →Err 含 `declares <!ENTITY>`+entry 名（溯源）。
- Rust：内存 zip 构造（zip crate Writer，write_epub helper）+let-else
  形态（ExtractedEpub 无 Debug，unwrap_err 不可用）。

## 四、验证汇总

- Rust translation::source 1/1（双形态单测）；TS translation-ingest
  6/6（+1）；testgate engine-rs **2053**（+1）/584；typecheck 净；
  clippy:gate 双 0；gate:ts 十步全绿；U+FFFD 净。

## 五、后续出口

- XML 面勘察闭合（无解析库依赖/手写正则+ENTITY 拒绝双端同构）。
- 出口清空回归常备。

**下一号自 927 起；出口清空回归常备；用户侧待办=Fork 推送 59 个提交。**
