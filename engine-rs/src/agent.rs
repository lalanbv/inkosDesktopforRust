//! agent 域：创建源解析（972 号：TS `agent/creation-source.ts` `loadCreationSource`
//! 翻译域切片——registered 引用 / inline 文本 / 项目内文件 + 歧义守卫 + lineage）。

pub mod creation_source;

pub use creation_source::load_creation_source;
