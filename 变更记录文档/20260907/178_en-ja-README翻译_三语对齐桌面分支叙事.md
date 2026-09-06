# 178 · en/ja README 翻译——三语 README 全部对齐桌面分支叙事

- 日期：2026-09-07
- 模块：README.md / README.en.md / README.ja.md
- 类型：docs（收口 168 号遗留）
- 关联：168 号（根 README 桌面分支重写——本次翻译的底本）、[readme-docs-alignment 记忆](../../../.zcode/cli/memories/projects/inkosdesktopforrust-338e69db7066fda4/memory/readme-docs-alignment.md)

## 一、背景

168 号重写根 README 后，en/ja 两个文件仍是上游 InkOS v0.4.0-alpha 的旧产品内容（npm 徽章、Kimi 推广横幅、ClawHub、brew/msi 假渠道叙事），与「桌面客户端分支」的实际形态矛盾；zh 头部挂有「英文 / 日文 README 目前仍为上游内容」的临时提示。本批全量重写两份文件，三语对齐。

## 二、改动

1. **README.en.md / README.ja.md 全量替换**：逐节对齐 zh 版 12 个二级 + 6 个三级标题骨架（这是什么 / 当前状态 / 桌面端能力（引擎双后端 / 系统集成 / Studio UI / 插件系统）/ 从源码运行 / 安装包 / 构建与发布 / 测试 / 文档 / 开发约定 / 致谢与许可证）；徽章组换成桌面分支事实（v0.2.0 / 上游 Narcooo/inkos / Rust 直启+Node 回退）；pnpm 10.x 锁版本、dev 探测序、变更记录编号等事实逐项对应。上游 npm/Kimi/ClawHub 推广内容全部移除。
2. **zh 版两处收尾**：删除头部未译提示行（其使命完成）；「当前至 173 号」更新为 177 号（174~177 各批未及同步，本批顺手纠错）。
3. **专有名词处理**：中文目录名（`变更记录文档/`、`开发时SpecCoding'sPlan/`）链接路径保持原样（真实路径），说明文字加注（en: "directories kept in Chinese" / ja: ディレクトリ名は中国語のまま）；「收尾」概念 ja 译「収尾」、编号约定 en 用 `#NNN` / ja 用「第 NNN 号」与各自行文习惯一致。

## 三、验证

| 项 | 结果 |
| --- | --- |
| 三语标题骨架比对 | `grep -E "^#{1,3} "` 逐节一致（12×`##` + 6×`###` 同构） |
| 本地链接目标存在性 | 14 个相对链接 × 3 文件全部 `OK`（docs/ 5、模块 README 3、LICENSE、中文目录 2 等） |
| 语言切换行 | 三语互链齐备且各自高亮当前语言（zh 中间、en 中间、ja 末位） |
| 旧推广残留清查 | npm / Kimi / ClawHub / kimi-file 图床引用在 en/ja 中零残留 |

## 四、遗留

- docs/ 下英文内容（USER_GUIDE 等）仍为中文——docs/* 被 gitignore 本地生效，不入库不阻塞。
- TROUBLESHOOTING 复审仍挂（同上，本地文档债）。
- W-A4b（需 UI 决策）、W-B4/B5（插件生态）、W-C3/C4（产品级）——产品 backlog。
- 推送须在 Fork 图形端执行（既有约定）。
