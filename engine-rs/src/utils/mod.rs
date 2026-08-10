//! 工具函数域（Phase 1 叶子，纯函数，无内部依赖）。
//!
//! 自 `packages/core/src/utils/*.ts` 自下而上移植。每个子模块对应一个 TS 源文件，
//! 移植纪律：行为 1:1 复刻 + golden 差分测试守门（见 `tests/golden/utils/`）。
//!
//! ## 已移植
//! - [`book_id`]：书 ID 派生 + 安全校验（路径遍历/控制字符/shell 元字符防护）
//! - [`language`]：写作语言推断（CJK vs Latin 占比）
//! - [`path`]：项目相对路径归一化（Windows `\` → POSIX `/`）
//!
//! ## 待移植（按依赖序）
//! length-metrics / chapter-memo-parser / story-markdown / ...

pub mod book_id;
pub mod language;
pub mod path;

pub use book_id::{assert_safe_book_id, derive_book_id_from_title, is_safe_book_id};
pub use language::{infer_language, WritingLanguage};
pub use path::to_posix_path;
