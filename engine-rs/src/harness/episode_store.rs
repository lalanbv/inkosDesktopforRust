//! CreativeEpisode / 事件存储（882 号 M1a）：`packages/core/src/harness/
//! episode-store.ts` 的 rusqlite 移植——同 schema、同连接参数（WAL/foreign_keys/
//! busy_timeout=5000）、同事务与排序语义。
//!
//! 与 TS 的差异（备案）：
//! - TS 的 per-session enqueue 串行 → Rust 用 `Mutex<Connection>` 整体串行
//!   （超集：跨会话也串行；M1a 写入频率为操作级，无热循环）。
//! - `owner_pid` 列迁移沿用（存量库缺列时 ALTER 补齐）。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;
use serde_json::{Map, Value};

use super::contracts::{
    validate_harness_id, validate_work_resource_id, CreativeEpisode, CreativeEpisodeEvent,
    EpisodeStatus, HARNESS_VERSION,
};

pub struct ListEpisodesOptions {
    pub work_id: Option<String>,
    pub profile_id: Option<String>,
    pub status: Option<String>,
    pub limit: Option<u64>,
}

pub struct CreativeEpisodeStore {
    connection: Mutex<Connection>,
    path: PathBuf,
}

fn episode_status_from_db(value: &str) -> rusqlite::Result<EpisodeStatus> {
    match value {
        "running" => Ok(EpisodeStatus::Running),
        "completed" => Ok(EpisodeStatus::Completed),
        "failed" => Ok(EpisodeStatus::Failed),
        "cancelled" => Ok(EpisodeStatus::Cancelled),
        // rusqlite 0.32 的 InvalidColumnType 第三参是 Type（不携带值文本）。
        _other => Err(rusqlite::Error::InvalidColumnType(
            0,
            "status".into(),
            rusqlite::types::Type::Text,
        )),
    }
}

fn parse_episode_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CreativeEpisode> {
    Ok(CreativeEpisode {
        version: HARNESS_VERSION,
        id: row.get("id")?,
        work_id: row.get("workId")?,
        profile_id: row.get("profileId")?,
        status: episode_status_from_db(&row.get::<_, String>("status")?)?,
        started_at: row.get("startedAt")?,
        completed_at: row.get("completedAt")?,
    })
}

fn parse_event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CreativeEpisodeEvent> {
    let capability_id: Option<String> = row.get("capabilityId")?;
    let action_id: Option<String> = row.get("actionId")?;
    let payload_json: String = row.get("payloadJson")?;
    Ok(CreativeEpisodeEvent {
        version: HARNESS_VERSION,
        episode_id: row.get("episodeId")?,
        // rusqlite 0.33 起 FromSql 移除 u64 实现（SQLite 整列语义 i64）——
        // RUSTSEC-2026-0459 升级面：经 i64 读取转 u64（负值=损坏数据归 0，不回绕）。
        seq: u64::try_from(row.get::<_, i64>("seq")?).unwrap_or(0),
        timestamp: row.get("timestamp")?,
        event_type: row.get("type")?,
        work_id: row.get("workId")?,
        capability_id,
        action_id,
        payload: serde_json::from_str::<Map<String, Value>>(&payload_json)
            .unwrap_or_else(|_| Map::new()),
    })
}

impl CreativeEpisodeStore {
    pub fn new(path: &Path) -> rusqlite::Result<Self> {
        if path != Path::new(":memory:") {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        let this = Self {
            connection: Mutex::new(connection),
            path: path.to_path_buf(),
        };
        {
            let conn = this.lock();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS creative_episodes (
                    episode_id TEXT PRIMARY KEY,
                    work_id TEXT,
                    profile_id TEXT,
                    status TEXT NOT NULL,
                    started_at TEXT NOT NULL,
                    completed_at TEXT
                );

                CREATE TABLE IF NOT EXISTS creative_episode_events (
                    episode_id TEXT NOT NULL,
                    seq INTEGER NOT NULL,
                    timestamp TEXT NOT NULL,
                    type TEXT NOT NULL,
                    work_id TEXT,
                    capability_id TEXT,
                    action_id TEXT,
                    payload_json TEXT NOT NULL,
                    PRIMARY KEY (episode_id, seq),
                    FOREIGN KEY (episode_id) REFERENCES creative_episodes(episode_id) ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_creative_episode_events_type
                    ON creative_episode_events(type);
                CREATE INDEX IF NOT EXISTS idx_creative_episodes_work
                    ON creative_episodes(work_id, started_at);",
            )?;
            // 存量库缺 owner_pid 列时补齐（TS 同款迁移，包裹在事务里）。
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let has_column = conn
                .prepare("PRAGMA table_info(creative_episodes)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .any(|name| name.as_deref() == Ok("owner_pid"));
            if !has_column {
                conn.execute("ALTER TABLE creative_episodes ADD COLUMN owner_pid INTEGER", [])?;
            }
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS creative_episode_event_work
                    ON creative_episode_events(work_id, episode_id);
                 COMMIT",
            )?;
        }
        Ok(this)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn create(&self, episode: &CreativeEpisode) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO creative_episodes (\
                episode_id, work_id, profile_id, status, started_at, completed_at, owner_pid\
            ) VALUES (?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                episode.id,
                episode.work_id,
                episode.profile_id,
                match episode.status {
                    EpisodeStatus::Running => "running",
                    EpisodeStatus::Completed => "completed",
                    EpisodeStatus::Failed => "failed",
                    EpisodeStatus::Cancelled => "cancelled",
                },
                episode.started_at,
                episode.completed_at,
                std::process::id(),
            ],
        )?;
        Ok(())
    }

    fn next_seq(conn: &Connection, episode_id: &str) -> rusqlite::Result<u64> {
        // rusqlite 0.33 起 FromSql 移除 u64 实现——i64 读取转 u64（COALESCE(MAX,-1)+1 恒非负）。
        let seq: i64 = conn
            .prepare("SELECT COALESCE(MAX(seq), -1) + 1 AS seq FROM creative_episode_events WHERE episode_id = ?")?
            .query_row([episode_id], |row| row.get("seq"))?;
        Ok(u64::try_from(seq).unwrap_or(0))
    }

    fn insert_event(conn: &Connection, event: &CreativeEpisodeEvent) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO creative_episode_events (\
                episode_id, seq, timestamp, type, work_id,\
                capability_id, action_id, payload_json\
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                event.episode_id,
                event.seq as i64,
                event.timestamp,
                event.event_type,
                event.work_id,
                event.capability_id,
                event.action_id,
                serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".into()),
            ],
        )?;
        Ok(())
    }

    /// TS `append`：事务内校验 episode 存在 + 分配次 seq + 写事件。
    #[allow(clippy::too_many_arguments)] // 与 TS append(episodeId, type, workId, capabilityId, actionId, payload, timestamp) 七参对齐
    pub fn append(
        &self,
        episode_id: &str,
        event_type: &str,
        work_id: Option<&str>,
        capability_id: Option<&str>,
        action_id: Option<&str>,
        payload: &Value,
        timestamp: &str,
    ) -> Result<CreativeEpisodeEvent, String> {
        validate_harness_id(episode_id)?;
        let conn = self.lock();
        conn.execute_batch("BEGIN IMMEDIATE").map_err(|e| e.to_string())?;
        let result = (|| {
            let exists: Option<String> = conn
                .prepare("SELECT episode_id FROM creative_episodes WHERE episode_id = ?").map_err(|e| e.to_string())?
                .query_row([episode_id], |row| row.get::<_, String>("episode_id"))
                .ok();
            if exists.is_none() {
                return Err(format!("Unknown creative episode: {episode_id}"));
            }
            let seq = Self::next_seq(&conn, episode_id).map_err(|e| e.to_string())?;
            let payload = payload.as_object().cloned().unwrap_or_default();
            let event = CreativeEpisodeEvent {
                version: HARNESS_VERSION,
                episode_id: episode_id.to_string(),
                seq,
                timestamp: timestamp.to_string(),
                event_type: event_type.to_string(),
                work_id: work_id.map(str::to_string),
                capability_id: capability_id.map(str::to_string),
                action_id: action_id.map(str::to_string),
                payload,
            };
            Self::insert_event(&conn, &event).map_err(|e| e.to_string())?;
            Ok(event)
        })();
        match result {
            Ok(event) => {
                conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
                Ok(event)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub fn finish(
        &self,
        episode_id: &str,
        status: EpisodeStatus,
        completed_at: &str,
    ) -> Result<CreativeEpisode, String> {
        let status_text = match status {
            EpisodeStatus::Running => "running",
            EpisodeStatus::Completed => "completed",
            EpisodeStatus::Failed => "failed",
            EpisodeStatus::Cancelled => "cancelled",
        };
        if status_text == "running" {
            return Err("Cannot finish into running".into());
        }
        {
            // 锁作用域收紧：std Mutex 不可重入——require_episode 在锁外调用
            //（884 号根因修复：M1a 备案的「episode 测试进程挂起」即本方法的
            // 持锁重入自死锁，非 WAL 竞态）。
            let conn = self.lock();
            let changed = conn
                .execute(
                    "UPDATE creative_episodes SET status = ?, completed_at = ? WHERE episode_id = ? AND status = 'running'",
                    rusqlite::params![status_text, completed_at, episode_id],
                )
                .map_err(|e| e.to_string())?;
            if changed != 1 {
                return Err(format!(
                    "Creative episode is missing or already terminal: {episode_id}"
                ));
            }
        }
        self.require_episode(episode_id)
    }

    pub fn get_episode(&self, episode_id: &str) -> Option<CreativeEpisode> {
        let conn = self.lock();
        let mut statement = conn
            .prepare(
                "SELECT episode_id AS id, work_id AS workId, profile_id AS profileId,
                    status, started_at AS startedAt, completed_at AS completedAt
             FROM creative_episodes WHERE episode_id = ?",
            )
            .ok()?;
        statement
            .query_row([episode_id], parse_episode_row)
            .ok()
    }

    pub fn require_episode(&self, episode_id: &str) -> Result<CreativeEpisode, String> {
        self.get_episode(episode_id)
            .ok_or_else(|| format!("Unknown creative episode: {episode_id}"))
    }

    /// TS `bindWork`：事务内校验存在/未绑定/running，写 `episode-work-bound`
    /// 事件并更新 work_id。
    pub fn bind_work(
        &self,
        episode_id: &str,
        work_id: &str,
        timestamp: &str,
    ) -> Result<CreativeEpisode, String> {
        validate_harness_id(episode_id)?;
        validate_work_resource_id(work_id)?;
        let conn = self.lock();
        conn.execute_batch("BEGIN IMMEDIATE").map_err(|e| e.to_string())?;
        let result = (|| {
            let row: Option<(Option<String>, String)> = conn
                .prepare("SELECT work_id, status FROM creative_episodes WHERE episode_id = ?").map_err(|e| e.to_string())?
                .query_row([episode_id], |row| {
                    Ok((row.get("workId")?, row.get("status")?))
                })
                .ok();
            let Some((current_work_id, status)) = row else {
                return Err(format!("Unknown creative episode: {episode_id}"));
            };
            if let Some(current) = current_work_id.as_deref() {
                if current != work_id {
                    return Err(format!(
                        "Creative episode \"{episode_id}\" is already bound to Work \"{current}\""
                    ));
                }
                conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
                return Ok(());
            }
            if status != "running" {
                return Err(format!("Cannot bind terminal creative episode: {episode_id}"));
            }
            let seq = Self::next_seq(&conn, episode_id).map_err(|e| e.to_string())?;
            let event = CreativeEpisodeEvent {
                version: HARNESS_VERSION,
                episode_id: episode_id.to_string(),
                seq,
                timestamp: timestamp.to_string(),
                event_type: "episode-work-bound".into(),
                work_id: Some(work_id.to_string()),
                capability_id: None,
                action_id: None,
                payload: serde_json::Map::new(),
            };
            conn.execute(
                "UPDATE creative_episodes SET work_id = ? WHERE episode_id = ? AND work_id IS NULL AND status = 'running'",
                rusqlite::params![work_id, episode_id],
            ).map_err(|e| e.to_string())?;
            Self::insert_event(&conn, &event).map_err(|e| e.to_string())?;
            conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
            Ok(())
        })();
        match result {
            // Ok 臂先放锁再取行（std Mutex 不可重入，884 号同族修复）。
            Ok(()) => {
                drop(conn);
                self.require_episode(episode_id)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// TS `listEpisodes`：workId 匹配 = work_id 相等或事件轨迹出现该 work
    ///（含 action-started 的 parameters.workId/projectId/bookId），profileId/
    /// status 精确过滤，startedAt 降序 + episode_id 降序，limit 钳制 1..=500。
    pub fn list_episodes(
        &self,
        options: &ListEpisodesOptions,
    ) -> Result<Vec<CreativeEpisode>, String> {
        let work_id = options.work_id.as_deref();
        let profile_id = options.profile_id.as_deref();
        let status = options.status.as_deref();
        let limit = options.limit.unwrap_or(100).clamp(1, 500) as i64;
        let conn = self.lock();
        let mut statement = conn
            .prepare(
                "SELECT episode_id AS id, work_id AS workId, profile_id AS profileId,
                    status, started_at AS startedAt, completed_at AS completedAt
             FROM creative_episodes
             WHERE (?1 IS NULL OR work_id = ?1 OR EXISTS (
                 SELECT 1 FROM creative_episode_events e WHERE e.episode_id=creative_episodes.episode_id
                     AND (e.work_id=?1 OR (e.type='action-started' AND (
                         json_extract(e.payload_json,'$.parameters.workId')=?1
                         OR json_extract(e.payload_json,'$.parameters.projectId')=?1
                         OR json_extract(e.payload_json,'$.parameters.bookId')=?1
                     )))
             ))
                 AND (?2 IS NULL OR profile_id = ?2)
                 AND (?3 IS NULL OR status = ?3)
             ORDER BY started_at DESC, episode_id DESC
             LIMIT ?4",
            )
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map(
                // SQL 用编号参数 ?1..?4——占位 4 个，重复引用同一 ?n 不增参数计数
                //（884 号修正：M1a 误按出现次数传 11 参，且该路径被 #[ignore]
                // 遮蔽从未真跑——死锁修复后端点测试揪出）。
                rusqlite::params![work_id, profile_id, status, limit],
                parse_episode_row,
            )
            .map_err(|e| e.to_string())?;
        let mut episodes = Vec::new();
        for row in rows {
            episodes.push(row.map_err(|e| e.to_string())?);
        }
        Ok(episodes)
    }

    pub fn list_events(&self, episode_id: &str) -> Result<Vec<CreativeEpisodeEvent>, String> {
        validate_harness_id(episode_id)?;
        let conn = self.lock();
        let mut statement = conn
            .prepare(
                "SELECT episode_id AS episodeId, seq, timestamp, type, work_id AS workId,
                    capability_id AS capabilityId, action_id AS actionId, payload_json AS payloadJson
             FROM creative_episode_events
             WHERE episode_id = ?
             ORDER BY seq ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([episode_id], parse_event_row)
            .map_err(|e| e.to_string())?;
        let mut events = Vec::new();
        for row in rows {
            events.push(row.map_err(|e| e.to_string())?);
        }
        Ok(events)
    }

    /// TS `recoverInterruptedEpisodes`：非本进程持有且 running 的 episode 标记
    /// failed + `episode-failed` 事件。
    pub fn recover_interrupted_episodes(
        &self,
        completed_at: &str,
        reason: &str,
    ) -> Result<u32, String> {
        let conn = self.lock();
        conn.execute_batch("BEGIN IMMEDIATE").map_err(|e| e.to_string())?;
        let result = (|| {
            let rows: Vec<(String, Option<String>)> = conn
                .prepare(
                    "SELECT episode_id, work_id FROM creative_episodes WHERE status = 'running'
                     ORDER BY started_at ASC, episode_id ASC",
                )
                .map_err(|e| e.to_string())?
                .query_map([], |row| {
                    Ok((row.get::<_, String>("episode_id")?, row.get::<_, Option<String>>("work_id")?))
                })
                .map_err(|e| e.to_string())?
                .collect::<rusqlite::Result<_>>()
                .map_err(|e| e.to_string())?;
            let mut recovered = 0u32;
            for (episode_id, work_id) in rows {
                // owner_pid 是 TS/Node 写入时的宿主 pid；Rust 引擎与 Node 各自
                // 持库，非自身进程写下的 running 一律视为中断（与 TS 对比：
                // TS 有 isLiveProcess 存活检查——同库双引擎下按不可存活处理）。
                let seq = Self::next_seq(&conn, &episode_id).map_err(|e| e.to_string())?;
                let event = CreativeEpisodeEvent {
                    version: HARNESS_VERSION,
                    episode_id: episode_id.clone(),
                    seq,
                    timestamp: completed_at.to_string(),
                    event_type: "episode-failed".into(),
                    work_id: work_id.clone(),
                    capability_id: None,
                    action_id: None,
                    payload: serde_json::json!({ "reason": reason }).as_object().cloned().unwrap_or_default(),
                };
                Self::insert_event(&conn, &event).map_err(|e| e.to_string())?;
                conn.execute(
                    "UPDATE creative_episodes SET status = 'failed', completed_at = ? WHERE episode_id = ? AND status = 'running'",
                    rusqlite::params![completed_at, episode_id],
                ).map_err(|e| e.to_string())?;
                recovered += 1;
            }
            Ok(recovered)
        })();
        match result {
            Ok(count) => {
                conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
                Ok(count)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub fn close(&self) {
        // rusqlite 的 Connection 在 drop 时关闭；close() 面为 TS 对齐保留。
        let _ = &self.connection;
    }
}
