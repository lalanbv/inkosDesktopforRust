import { estimateTextTokens } from "../llm/provider.js";

/**
 * R41 token 计量服务（564 号，TS 镜像；563 号 Rust 侧同批语义）。
 *
 * dsh token-meter `measure(session, requestHeader?)` 语义的同位落位：
 * - 启发式计价：复用 `estimateTextTokens`（CJK 1:1、其余 4:1，双端同源已锁），
 *   追加 O(1)（节点表 push + 计数累加）；
 * - usage 锚点修正：最近成功调用的 usage 总量 ÷ 记锚时刻启发式总量 = 校准比
 *   （clamp 0.25..=4.0 防离群），`measure()` 即取即弃——快照不可变；
 * - 锚点复用条件（收缩防御）：新锚总量较前锚腰斩且表不缩（启发式总量不降）
 *   → 判定异常重置，拒绝换锚沿用旧比；否则接受替换；
 * - 覆盖率：锚点记锚时刻启发式总量 / 当前启发式总量（min 1.0，4 位舍入）。
 *
 * 表语义 = 当前请求面（聊天面每请求新建，与 Rust run_agent_loop 挂线同构）。
 * 隐私红线与 RunLog/R33 一致：只计量不存文——节点表只留启发式 token 数与
 * 序号，不留原文。
 * 双端共享 golden：`__tests__/golden/token-meter-vectors.json`（563 号）。
 */

export interface SurfaceNode {
  readonly seq: number;
  readonly heuristicTokens: number;
}

export interface UsageAnchor {
  readonly model: string | null;
  /** 权威总量（usage.total 优先，缺省 input+output）。 */
  readonly usageTotal: number;
  /** 记锚时刻表面启发式总量（校准比分母）。 */
  readonly heuristicTotal: number;
}

/** 校准比 clamp（设计文档 0.25–4.0：防 usage 离群值污染估算）。 */
const RATIO_CLAMP_LOW = 0.25;
const RATIO_CLAMP_HIGH = 4.0;

/** 计量来源：usage 锚点生效 = "usage"，纯启发式 = "estimate"。 */
export type MeterSource = "usage" | "estimate";

/** 计量快照（值语义；camelCase 对齐 Rust serde 面/双端 golden 向量）。 */
export interface TokenMeterSnapshot {
  /** 锚点模型（无锚 = null）。 */
  readonly model: string | null;
  /** 当前表面启发式总量。 */
  readonly heuristicTokens: number;
  /** 锚点校准后总量（无锚时 = heuristicTokens）。 */
  readonly anchoredTokens: number;
  /** 采用值：锚点生效 = anchoredTokens，否则 heuristicTokens。 */
  readonly tokens: number;
  readonly anchorValid: boolean;
  readonly source: MeterSource;
  /** 锚点覆盖率（0.0..=1.0，4 位小数舍入；无锚 = 0.0）。 */
  readonly coverage: number;
  /** 输入窗（构造时注入；0 = 未知不判超窗）。 */
  readonly inputWindow: number;
  /** tokens > inputWindow（窗口未知时恒 false）。 */
  readonly overWindow: boolean;
  /** 表面节点数（追加次数）。 */
  readonly surfaceNodes: number;
}

interface SurfaceFold {
  nodes: SurfaceNode[];
  heuristicTotal: number;
  nextSeq: number;
  anchor: UsageAnchor | null;
}

/** token 计量器：追加 O(1) / 快照 O(surface) 即取即弃。 */
export class TokenMeter {
  private readonly fold: SurfaceFold = { nodes: [], heuristicTotal: 0, nextSeq: 0, anchor: null };

  constructor(private readonly inputWindow: number) {}

  /** 追加一段文本入表面，返回节点序号。O(1)。 */
  append(text: string): number {
    const tokens = estimateTextTokens(text);
    const seq = this.fold.nextSeq;
    this.fold.nextSeq += 1;
    this.fold.heuristicTotal += tokens;
    this.fold.nodes.push({ seq, heuristicTokens: tokens });
    return seq;
  }

  /**
   * usage 权威值入账（最近成功调用）。返回是否接受为新锚（收缩防御拒绝
   * 时沿用旧锚，见模块头）。`model` 为 null（执行器未透出）时锚点仍成立，
   * 仅快照 model 面为 null。
   */
  noteUsage(
    model: string | null,
    promptTokens: number,
    completionTokens: number,
    totalTokens: number,
  ): boolean {
    const usageTotal = totalTokens > 0 ? totalTokens : promptTokens + completionTokens;
    if (usageTotal === 0) return false;
    const prev = this.fold.anchor;
    if (prev) {
      // 收缩防御：总量腰斩且表面未缩 → 异常重置（缓存错配/路由切换伪
      // usage），拒绝换锚。
      if (usageTotal * 2 < prev.usageTotal && this.fold.heuristicTotal >= prev.heuristicTotal) {
        return false;
      }
    }
    this.fold.anchor = {
      model,
      usageTotal,
      heuristicTotal: this.fold.heuristicTotal,
    };
    return true;
  }

  /** 计量快照：即取即弃（O(surface)）。 */
  measure(): TokenMeterSnapshot {
    const { anchor, heuristicTotal } = this.fold;
    let anchored = heuristicTotal;
    let coverage = 0;
    let anchorValid = false;
    let model: string | null = null;
    if (anchor && heuristicTotal > 0 && anchor.heuristicTotal > 0) {
      const ratio = Math.min(
        RATIO_CLAMP_HIGH,
        Math.max(RATIO_CLAMP_LOW, anchor.usageTotal / anchor.heuristicTotal),
      );
      anchored = Math.round(heuristicTotal * ratio);
      coverage = Math.round((Math.min(anchor.heuristicTotal, heuristicTotal) / heuristicTotal) * 10_000) / 10_000;
      anchorValid = true;
      model = anchor.model;
    } else if (anchor) {
      // 表空或锚点记锚时表空：校准比未定义，退纯启发式（锚仍在场，
      // anchorValid 保留 true——锚点身份可观测）。
      anchorValid = true;
      model = anchor.model;
    }
    const source: MeterSource = anchorValid ? "usage" : "estimate";
    const tokens = anchorValid ? anchored : heuristicTotal;
    return {
      model,
      heuristicTokens: heuristicTotal,
      anchoredTokens: anchored,
      tokens,
      anchorValid,
      source,
      coverage,
      inputWindow: this.inputWindow,
      overWindow: this.inputWindow > 0 && tokens > this.inputWindow,
      surfaceNodes: this.fold.nodes.length,
    };
  }
}
