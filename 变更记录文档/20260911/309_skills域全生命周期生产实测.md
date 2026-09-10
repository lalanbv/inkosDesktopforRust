# 309 号：skills 域全生命周期生产实测——导入/列表/删除通过（273 号 body-cap 修复实证补全）

- 日期：2026-09-11
- 分支：develop
- 关联：273 号（skills/import body 上限修复——本批补其实测）、226 号（skills 加载面复核）、240 号（use_skill 运行时）
- 编号衔接：查 20260911 目录最大号 308，顺延 309
- 推送核验：origin/develop = 498253fa（300 号）未动；本地领先 301–309 共 9 提交待推

## 一、实测（生产 bin + dataUrl 上传，全部通过）

| 步骤 | 结果 |
|---|---|
| `POST /skills/import`（SKILL.md dataUrl，含 frontmatter name/description） | ✓ 200；staging + rename 原子落盘 `.agents/skills/{id}/SKILL.md`；id 由目录名回退生成（路径回退语义与 TS parseAgentSkillDocument 一致） |
| `GET /skills` | ✓ 列表含该技能（source=project，name 正确） |
| `DELETE /skills/{id}` | ✓ `{ok:true}`，磁盘目录移除、列表消失 |

## 二、结论

skills 域（导入 273 号 body 上限修复后 / 列表 / 删除）生产引擎行为全部正确；原子导入（staging+rename）防半写状态的设计在实测中工作。273 号 body 上限修复面（translation/style/agent/skills 四域）至此**全部经生产引擎实测**。

## 三、验证性质与遗留

零代码改动批。遗留：301–309 共 9 提交待推送；并行会话三件第三十七轮在途未落库；两项默认值、ja A/B、历史瘦身待用户决策。
