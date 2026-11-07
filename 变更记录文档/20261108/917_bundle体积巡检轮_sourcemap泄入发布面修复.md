# 917 号 · bundle 体积巡检轮：36 个 .map 泄入发布面修复（sourcemap/declarationMap 双关，dist −1M）

> 产品面巡检（用户可感知）：studio 构建产物体积首次建立台账——dist
> 18M、主包 index.js 2.9MB（gzip 844KB）、365 chunks（语法高亮语言包
> 按需懒加载）、路由 lazy+Suspense 在位——分包形态健康；**真患=36 个
> .map 泄入发布产物**（dist/api 20+dist/shared 16，508KB：源码可还原
> 泄露面+体积膨胀）。修复=build:server 链 sourceMap/declarationMap 双
> 关，map 36→0、dist 18M→17M、js+d.ts 产物无损。
> 验证：gate:ts 十步全绿（build/typecheck 全链重建过）。

## 一、体积台账（首次基线）

| 项 | 值 | 判定 |
|---|---|---|
| dist 总计 | 18M → 修复后 **17M** | 桌面安装包形态磁盘可接受 |
| 主包 index.js | 2.9MB（gzip 844KB） | 本地壳静态面（127.0.0.1）毫秒级读取，无用户可感知影响 |
| chunks | 365（语言包按需懒加载：emacs-lisp/cynefin/cpp/wasm/wolfram 等） | 分包充分 ✓ |
| 路由 | lazy+Suspense 在位（FlowView/FilmWizard 等） | ✓ |
| sourcemap | 36 个（api 20+shared 16）→ **0** | 修复 ✓ |

## 二、修复（packages/studio/tsconfig.server.json 单文件）

- 根因：根 tsconfig `sourceMap:true`（dev/watch 链服务）被 build:server
  的 tsc 依赖闭包连带继承——server import ../shared/* → shared 连带编
  译进 dist/shared（含 map）；**declarationMap 是第二开关**（首关
  sourceMap 后 .d.ts.map 18 个残留，二关清零）。
- `sourceMap:false + declarationMap:false`：生产产物无映射；dev 调试
  走 tsx 直跑 src 不经此链（功能面无损，js+d.ts 在位）。
- tsc 不清旧产物：残留 map 手动清理后重编验证 0。

## 三、验证汇总

- gate:ts 十步全绿（build/typecheck 全链重建过；test 101.6s 同族）；
  U+FFFD 净。

## 四、后续出口

- bundle 体积基线首轮建立（17M/2.9MB 主包/365 chunks），后续巡检对照。
- 出口清空回归常备。

**下一号自 918 起；出口清空回归常备；用户侧待办=Fork 推送 51 个提交。**
