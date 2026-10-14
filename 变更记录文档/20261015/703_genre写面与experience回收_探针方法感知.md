# 703 号：genre 写面 + experience 读面回收（第三批，回收清单 40→37）+ 共享面探针方法感知

日期：2026-10-15。类型：fix(studio)+fix(scripts)。前置：699 号（8552b528）。

## 一、实现

- **genre 三写路由**（create/PUT /:id/copy）：项目级 genres/{id}.md 的 frontmatter 构建（yamlScalar = JSON.stringify 字符串引号转义，YAML 标量契约），create 走内置种子目录校验 INVALID_GENRE_ID、copy 复制现有 profile 换 id；
- **experience 读面**（GET /books/:id/experience）：experience.json 纯文件读；
- **共享面探针方法感知**：SHARED_FACE 条目支持 `{path, method}` 对象形态（缺省 GET）——写面用非 GET 方法探针证明路由在位；**写面探针状态码对比即可**（空载荷的错误体形状属 fork/2.0 错误协议差=495 号 422 对齐面，非契约分歧），GET 面保持全 body 对比。

## 二、度量与门禁

- 三域报告：**共享面 22 对比 / 0 分歧**（genre 三写面 POST/PUT 探针双端状态一致 500/500、400/400）、fork-only **38/38**（52→37+experience 回归）、上游新增 9/9+2、exit 0。
- gate:ts 八步全绿 exit 0（第二轮全绿确认）。

## 三、施工实录（三断点）

1. **泄漏 tsx 占口**：先前失败运行的 detached tsx 子进程未清，持 8902 口应答旧版 server（无 genre 路由→404）——kill 模式需匹配完整 cmdline（`tsx.*api/index` 不够），后以 cwd 回正解。
2. **SHARED_FACE 对象形态两步落地**：数组条目先转对象、循环消费再改——只改数组不改循环=探测仍 GET（静默无效），以 genre 行对照验证后才继续。
3. **写面错误体**：空载荷 POST 的错误响应是 fork `{"error":string}` vs 2.0 typed shape 的协议差——状态码一致即路由在位，body 不比。

## 四、教训

1. **写面探针用非 GET 方法是路由存在性的正确证明**：GET 打 POST-only 路由时 node=404（无匹配）而 rust=405（方法不符）的方法伪影，会淹没真分歧——方法感知探针让 405/500 也成为「路由在位」的证据。
2. **泄漏子进程的端口占用会让新代码的探针打到旧进程**：每次活体运行前清理同端口残留（pkill 完整 cmdline 匹配），否则测的是已死代码。
3. **错误体形状（fork string vs 2.0 typed）是协议差不是契约分歧**：状态码对称即路由在位，body 对比只对 GET 读面有意义。
