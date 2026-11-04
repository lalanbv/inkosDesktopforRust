//! work 域 harness（882 号 M1a）：work.json / artifacts / profiles / episodes
//! 的存储与契约——移植自 `packages/core/src/harness/*`（上游 2.0 原生域）。
//!
//! ## 跨引擎兼容硬门禁（876/881 号）
//! 同一项目的 `works/<id>/work.json` 与 `.inkos/harness.sqlite` 会被 TS 回退端
//! 与 Rust 引擎交替读写：serde 写出的字段集必须与 TS zod strict 可读集**精确
//! 互逆**（多余/缺失字段 = 对端整份拒读）。金标准行双端锁：
//! `tests/work_domain_cross_engine.rs` ↔
//! `packages/core/src/__tests__/work-store-cross-engine.test.ts`（逐字共用）。

pub mod action_error;
pub mod adopt;
pub mod artifact_edit_policy;
pub mod artifact_reader;
pub mod artifact_revisions;
pub mod artifact_validation;
pub mod contracts;
pub mod episode_store;
pub mod profile_registry;
pub mod source_sync;
pub mod work_store;

pub use artifact_revisions::{
    promote_artifact_revision, stage_artifact_revision, StageArtifactRevisionInput,
};
pub use contracts::{
    ArtifactManifest, ArtifactRevision, ArtifactRevisionStatus, ConfirmationPolicy,
    ConfirmationValue, CreativeEpisode, CreativeEpisodeEvent, EpisodeStatus, WorkLineage,
    WorkManifest, WorkProfile, WorkStatus,
};
pub use episode_store::CreativeEpisodeStore;
pub use work_store::{
    create_work_manifest, list_work_manifests, load_work_manifest, parse_work_manifest_json,
    save_work_manifest, work_directory, work_manifest_path, CreateWorkManifestInput,
    WorkStoreError,
};
