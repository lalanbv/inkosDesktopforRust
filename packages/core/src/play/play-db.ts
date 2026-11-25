import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import {
  PlayEdgeSchema,
  PlayEntitySchema,
  PlayEventSchema,
  PlayStateSlotSchema,
  type PlayEdge,
  type PlayEdgeInput,
  type PlayEntity,
  type PlayEntityInput,
  type PlayEvent,
  type PlayEventInput,
  type PlayStateSlot,
  type PlayStateSlotInput,
} from "../models/play.js";
import { z } from "zod";

export interface PlayGraphSnapshot {
  readonly entities: PlayEntity[];
  readonly edges: PlayEdge[];
  readonly stateSlots: PlayStateSlot[];
  readonly events: PlayEvent[];
}

const require = createRequire(import.meta.url);

const ENTITY_SELECT_COLUMNS = `
  entities.id,
  entities.type,
  entities.label,
  entities.summary,
  entities.status,
  entities.created_event AS createdEventId,
  entities.updated_event AS updatedEventId
`;

const EDGE_SELECT_COLUMNS = `
  id,
  from_id AS fromId,
  type,
  to_id AS toId,
  value_json AS valueJson,
  valid_from_event AS validFromEventId,
  valid_until_event AS validUntilEventId,
  source_event_id AS sourceEventId,
  visibility_json AS visibilityJson,
  strength
`;

const STATE_SLOT_SELECT_COLUMNS = `
  id,
  owner_entity_id AS ownerEntityId,
  kind,
  label,
  value_json AS valueJson,
  updated_event AS updatedEventId
`;

const EVENT_SELECT_COLUMNS = `
  id,
  turn,
  action_kind AS actionKind,
  raw_input AS rawInput,
  outcome_summary AS outcomeSummary,
  created_at AS createdAt
`;

export class PlayDB {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private readonly db: any;

  constructor(runDir: string) {
    const databasePath = join(runDir, "play.db");
    // 只有全新的库才导入文件图谱：之后再开不重跑，免得抹掉导入之后写进去的东西。
    // 先读图谱再开库——图谱读不出来就抛，不留下一个空库冒充它。
    const fileGraph = existsSync(databasePath) ? null : readFileGraph(join(runDir, "play-graph.json"));
    mkdirSync(runDir, { recursive: true });
    const { DatabaseSync } = require("node:sqlite");
    this.db = new DatabaseSync(databasePath);
    this.db.exec("PRAGMA journal_mode = WAL");
    this.initializeSchema();
    if (fileGraph) this.importFileGraph(fileGraph);
  }

  private importFileGraph(graph: FileGraph): void {
    this.transaction(() => {
      for (const event of graph.events) this.recordEvent(event);
      for (const entity of graph.entities) this.upsertEntity(entity);
      for (const edge of graph.edges) this.upsertEdge(edge);
      for (const slot of graph.stateSlots) this.upsertStateSlot(slot);
    });
  }

  private initializeSchema(): void {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS entities (
        id TEXT PRIMARY KEY,
        type TEXT NOT NULL,
        label TEXT NOT NULL,
        summary TEXT NOT NULL DEFAULT '',
        status TEXT NOT NULL DEFAULT '',
        created_event TEXT,
        updated_event TEXT
      );

      CREATE TABLE IF NOT EXISTS edges (
        id TEXT PRIMARY KEY,
        from_id TEXT NOT NULL,
        type TEXT NOT NULL,
        to_id TEXT NOT NULL,
        value_json TEXT NOT NULL DEFAULT '{}',
        valid_from_event TEXT NOT NULL,
        valid_until_event TEXT,
        source_event_id TEXT NOT NULL,
        visibility_json TEXT NOT NULL DEFAULT '{}',
        strength REAL
      );

      CREATE TABLE IF NOT EXISTS state_slots (
        id TEXT PRIMARY KEY,
        owner_entity_id TEXT,
        kind TEXT NOT NULL,
        label TEXT NOT NULL,
        value_json TEXT NOT NULL,
        updated_event TEXT NOT NULL
      );

      CREATE TABLE IF NOT EXISTS events (
        id TEXT PRIMARY KEY,
        turn INTEGER NOT NULL,
        action_kind TEXT NOT NULL,
        raw_input TEXT NOT NULL,
        outcome_summary TEXT NOT NULL DEFAULT '',
        created_at TEXT NOT NULL
      );

      CREATE INDEX IF NOT EXISTS idx_play_edges_from ON edges(from_id, valid_until_event);
      CREATE INDEX IF NOT EXISTS idx_play_edges_to ON edges(to_id, type, valid_until_event);
      CREATE INDEX IF NOT EXISTS idx_play_state_owner ON state_slots(owner_entity_id);
      CREATE INDEX IF NOT EXISTS idx_play_events_turn ON events(turn);
    `);
  }

  upsertEntity(entity: PlayEntityInput): void {
    const parsed = PlayEntitySchema.parse(entity);
    this.db.prepare(
      `INSERT OR REPLACE INTO entities (id, type, label, summary, status, created_event, updated_event)
       VALUES (?, ?, ?, ?, ?, ?, ?)`,
    ).run(
      parsed.id,
      parsed.type,
      parsed.label,
      parsed.summary,
      parsed.status,
      parsed.createdEventId ?? null,
      parsed.updatedEventId ?? null,
    );
  }

  getEntity(id: string): PlayEntity | null {
    const row = this.db.prepare(
      `SELECT ${ENTITY_SELECT_COLUMNS} FROM entities WHERE id = ?`,
    ).get(id) as EntityRow | undefined;
    return row ? rowToEntity(row) : null;
  }

  upsertEdge(edge: PlayEdgeInput): void {
    const parsed = PlayEdgeSchema.parse(edge);
    this.db.prepare(
      `INSERT OR REPLACE INTO edges (
         id, from_id, type, to_id, value_json, valid_from_event, valid_until_event,
         source_event_id, visibility_json, strength
       ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    ).run(
      parsed.id,
      parsed.fromId,
      parsed.type,
      parsed.toId,
      JSON.stringify(parsed.value),
      parsed.validFromEventId,
      parsed.validUntilEventId,
      parsed.sourceEventId,
      JSON.stringify(parsed.visibility),
      parsed.strength ?? null,
    );
  }

  expireEdge(edgeId: string, validUntilEventId: string): void {
    this.db.prepare(
      "UPDATE edges SET valid_until_event = ? WHERE id = ?",
    ).run(validUntilEventId, edgeId);
  }

  getCurrentEdgesForEntity(entityId: string): PlayEdge[] {
    const rows = this.db.prepare(
      `SELECT ${EDGE_SELECT_COLUMNS}
       FROM edges
       WHERE (from_id = ? OR to_id = ?) AND valid_until_event IS NULL
       ORDER BY type, id`,
    ).all(entityId, entityId) as EdgeRow[];
    return rows.map(rowToEdge);
  }

  getEvidenceForClaim(claimId: string): PlayEntity[] {
    const rows = this.db.prepare(
      `SELECT ${ENTITY_SELECT_COLUMNS}
       FROM entities
       INNER JOIN edges ON entities.id = edges.from_id
       WHERE edges.to_id = ?
         AND edges.type = 'supports'
         AND edges.valid_until_event IS NULL
         AND entities.type IN ('evidence', 'clue')
       ORDER BY COALESCE(edges.strength, 0) DESC, entities.id ASC`,
    ).all(claimId) as EntityRow[];
    return rows.map(rowToEntity);
  }

  upsertStateSlot(slot: PlayStateSlotInput): void {
    const parsed = PlayStateSlotSchema.parse(slot);
    this.db.prepare(
      `INSERT OR REPLACE INTO state_slots (id, owner_entity_id, kind, label, value_json, updated_event)
       VALUES (?, ?, ?, ?, ?, ?)`,
    ).run(
      parsed.id,
      parsed.ownerEntityId ?? null,
      parsed.kind,
      parsed.label,
      JSON.stringify(parsed.value),
      parsed.updatedEventId,
    );
  }

  getStateSlotsForEntity(entityId: string): PlayStateSlot[] {
    const rows = this.db.prepare(
      `SELECT ${STATE_SLOT_SELECT_COLUMNS}
       FROM state_slots
       WHERE owner_entity_id = ?
       ORDER BY kind, label, id`,
    ).all(entityId) as StateSlotRow[];
    return rows.map(rowToStateSlot);
  }

  recordEvent(event: PlayEventInput): void {
    const parsed = PlayEventSchema.parse(event);
    this.db.prepare(
      `INSERT OR REPLACE INTO events (id, turn, action_kind, raw_input, outcome_summary, created_at)
       VALUES (?, ?, ?, ?, ?, ?)`,
    ).run(
      parsed.id,
      parsed.turn,
      parsed.actionKind,
      parsed.rawInput,
      parsed.outcomeSummary,
      parsed.createdAt,
    );
  }

  getEvent(id: string): PlayEvent | null {
    const row = this.db.prepare(
      `SELECT ${EVENT_SELECT_COLUMNS} FROM events WHERE id = ?`,
    ).get(id) as EventRow | undefined;
    return row ? PlayEventSchema.parse(row) : null;
  }

  snapshot(): PlayGraphSnapshot {
    const entities = this.db.prepare(
      `SELECT ${ENTITY_SELECT_COLUMNS} FROM entities ORDER BY id`,
    ).all() as EntityRow[];
    const edges = this.db.prepare(
      `SELECT ${EDGE_SELECT_COLUMNS} FROM edges ORDER BY id`,
    ).all() as EdgeRow[];
    const stateSlots = this.db.prepare(
      `SELECT ${STATE_SLOT_SELECT_COLUMNS} FROM state_slots ORDER BY id`,
    ).all() as StateSlotRow[];
    const events = this.db.prepare(
      `SELECT ${EVENT_SELECT_COLUMNS} FROM events ORDER BY turn, id`,
    ).all() as EventRow[];
    return {
      entities: entities.map(rowToEntity),
      edges: edges.map(rowToEdge),
      stateSlots: stateSlots.map(rowToStateSlot),
      events: events.map((row) => PlayEventSchema.parse(row)),
    };
  }

  replaceWithSnapshot(snapshot: PlayGraphSnapshot): void {
    this.transaction(() => {
      this.db.prepare("DELETE FROM state_slots").run();
      this.db.prepare("DELETE FROM edges").run();
      this.db.prepare("DELETE FROM entities").run();
      this.db.prepare("DELETE FROM events").run();
      for (const event of snapshot.events) this.recordEvent(event);
      for (const entity of snapshot.entities) this.upsertEntity(entity);
      for (const edge of snapshot.edges) this.upsertEdge(edge);
      for (const slot of snapshot.stateSlots) this.upsertStateSlot(slot);
    });
  }

  transaction<T>(fn: () => T): T {
    this.db.exec("BEGIN IMMEDIATE");
    try {
      const result = fn();
      this.db.exec("COMMIT");
      return result;
    } catch (error) {
      this.db.exec("ROLLBACK");
      throw error;
    }
  }

  close(): void {
    this.db.close();
  }
}

/** 文件图谱的四张记录表（产品引擎给新运行写的那份，形态同 Rust `persist_file`）。 */
interface FileGraph {
  readonly entities: readonly PlayEntity[];
  readonly edges: readonly PlayEdge[];
  readonly stateSlots: readonly PlayStateSlot[];
  readonly events: readonly PlayEvent[];
}

const ENTITY_COLUMNS = ["id", "type", "label", "summary", "status", "createdEventId", "updatedEventId"] as const;
const EDGE_COLUMNS = [
  "id", "fromId", "type", "toId", "value", "validFromEventId", "validUntilEventId",
  "sourceEventId", "visibility", "strength",
] as const;
const STATE_SLOT_COLUMNS = ["id", "ownerEntityId", "kind", "label", "value", "updatedEventId"] as const;
const EVENT_COLUMNS = ["id", "turn", "actionKind", "rawInput", "outcomeSummary", "createdAt"] as const;

/**
 * 读产品引擎的文件后端图谱（`play-graph.json`）；没有这个文件返回 null。
 *
 * 记录是产品引擎存下的模型输出原样对象：别名键（`from`/`to`/`relation`…）和模型多给
 * 的键都还在，所以先按列投影再过 schema——产品引擎自己写 sqlite 时也只取这几列，
 * `null` 的可选键当作缺省。投影之后仍不合 schema 的记录直接抛：宁可在开库之前失败，
 * 也不留下一个缺记录的库。库一旦建出来产品引擎就改读它，文件里那份会被丢在后面。
 */
function readFileGraph(path: string): FileGraph | null {
  let raw: string;
  try {
    raw = readFileSync(path, "utf-8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
  // 函数声明而不是箭头函数：类型收窄只认显式标注类型的声明。
  function invalid(detail: string): never {
    throw Object.assign(new Error(`File play graph cannot be imported: ${detail}`), {
      code: "PLAY_GRAPH_INVALID",
    });
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    invalid("not valid JSON");
  }
  const table = (name: string, columns: readonly string[]): Record<string, unknown>[] => {
    const records = (parsed as Record<string, unknown> | null)?.[name];
    if (records === undefined) return [];
    if (records === null || typeof records !== "object" || Array.isArray(records)) {
      invalid(`${name} is not a record table`);
    }
    return Object.values(records as Record<string, unknown>).map((record, index) => {
      if (record === null || typeof record !== "object" || Array.isArray(record)) {
        invalid(`${name}[${index}] is not a record`);
      }
      return pickColumns(record as Record<string, unknown>, columns);
    });
  };
  const check = <T>(
    kind: string,
    records: Record<string, unknown>[],
    schema: { safeParse(value: unknown): { success: true; data: T } | { success: false; error: z.ZodError } },
  ): T[] =>
    records.map((record) => {
      const result = schema.safeParse(record);
      if (!result.success) {
        const issue = result.error.issues[0];
        invalid(`${kind} ${JSON.stringify(record.id ?? "?")}: ${issue?.path.map(String).join(".") ?? ""} ${issue?.message ?? ""}`);
      }
      return result.data;
    });
  return {
    entities: check("entity", table("entities", ENTITY_COLUMNS), PlayEntitySchema),
    edges: check("edge", table("edges", EDGE_COLUMNS), PlayEdgeSchema),
    stateSlots: check("state slot", table("stateSlots", STATE_SLOT_COLUMNS), PlayStateSlotSchema),
    events: check("event", table("events", EVENT_COLUMNS), PlayEventSchema),
  };
}

/** 只取 `columns` 里在场的键；值为 `null` 的键当作缺省（产品引擎写 sqlite 时即存 NULL）。 */
function pickColumns(
  record: Record<string, unknown>,
  columns: readonly string[],
): Record<string, unknown> {
  return Object.fromEntries(
    columns
      .filter((column) => record[column] !== null && record[column] !== undefined)
      .map((column) => [column, record[column]]),
  );
}

interface EntityRow {
  readonly id: string;
  readonly type: PlayEntity["type"];
  readonly label: string;
  readonly summary: string;
  readonly status: string;
  readonly createdEventId: string | null;
  readonly updatedEventId: string | null;
}

interface EdgeRow {
  readonly id: string;
  readonly fromId: string;
  readonly type: string;
  readonly toId: string;
  readonly valueJson: string;
  readonly validFromEventId: string;
  readonly validUntilEventId: string | null;
  readonly sourceEventId: string;
  readonly visibilityJson: string;
  readonly strength: number | null;
}

interface StateSlotRow {
  readonly id: string;
  readonly ownerEntityId: string | null;
  readonly kind: PlayStateSlot["kind"];
  readonly label: string;
  readonly valueJson: string;
  readonly updatedEventId: string;
}

interface EventRow {
  readonly id: string;
  readonly turn: number;
  readonly actionKind: PlayEvent["actionKind"];
  readonly rawInput: string;
  readonly outcomeSummary: string;
  readonly createdAt: string;
}

function rowToEntity(row: EntityRow): PlayEntity {
  return PlayEntitySchema.parse({
    id: row.id,
    type: row.type,
    label: row.label,
    summary: row.summary,
    status: row.status,
    ...(row.createdEventId ? { createdEventId: row.createdEventId } : {}),
    ...(row.updatedEventId ? { updatedEventId: row.updatedEventId } : {}),
  });
}

function rowToEdge(row: EdgeRow): PlayEdge {
  return PlayEdgeSchema.parse({
    id: row.id,
    fromId: row.fromId,
    type: row.type,
    toId: row.toId,
    value: parseJsonObject(row.valueJson),
    validFromEventId: row.validFromEventId,
    validUntilEventId: row.validUntilEventId,
    sourceEventId: row.sourceEventId,
    visibility: parseJsonObject(row.visibilityJson),
    ...(row.strength === null ? {} : { strength: row.strength }),
  });
}

function rowToStateSlot(row: StateSlotRow): PlayStateSlot {
  return PlayStateSlotSchema.parse({
    id: row.id,
    ownerEntityId: row.ownerEntityId,
    kind: row.kind,
    label: row.label,
    value: JSON.parse(row.valueJson) as unknown,
    updatedEventId: row.updatedEventId,
  });
}

function parseJsonObject(value: string): Record<string, unknown> {
  const parsed = JSON.parse(value) as unknown;
  return parsed && typeof parsed === "object" && !Array.isArray(parsed)
    ? parsed as Record<string, unknown>
    : {};
}
