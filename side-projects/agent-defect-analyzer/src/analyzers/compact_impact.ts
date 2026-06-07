//! Compact 影响评估分析器。
//!
//! 评估 compact 机制对 SQLite 持久化数据的影响：
//! 1. Micro compact 痕迹：`[compacted: N chars]` / `[compacted: image ~N tokens]`
//! 2. Full compact 痕迹：消息数量骤降
//! 3. 受影响 session 的 compact 比例
//! 4. 原始数据丢失量估算
//!
//! 注意：多轮 compact 后，SQLite 中只保留最后一次 compact 后的状态，
//! 无法还原中间过程。因此本分析只关注"是否经历过 compact"和"影响范围"。

import type { MessageRow, DefectReport } from "../../types.js";
import { DataLoader } from "../utils/data_loader.js";
import {
  printSection,
  printMetric,
  printTable,
  printWarning,
} from "../utils/report.js";

// ── Micro compact 占位符正则 ──
const COMPACTED_CHARS_RE = /^\[compacted:\s*(\d+)\s*chars\]$/;
const COMPACTED_IMAGE_RE = /^\[compacted:\s*image\s*~(\d+)\s*tokens\]$/;
const COMPACTED_DOCUMENT_RE = /^\[compacted:\s*document\s*~(\d+)\s*tokens\]$/;
const COMPACTED_GENERIC_RE = /^\[compacted:/;

// ── 数据结构 ──
interface CompactedResult {
  /** compacted 占位符匹配到的原始大小 */
  originalChars: number;
  /** 类型：chars / image / document */
  kind: string;
}

interface SessionCompactInfo {
  threadId: string;
  title: string;
  totalMessages: number;
  /** 包含 compacted 占位符的消息数 */
  compactedCount: number;
  /** 所有 compacted 占位符的原始字符数总和 */
  totalLostChars: number;
  /** compacted 占位符占比 */
  compactedRatio: number;
}

// ── 主分析 ──

export function analyzeCompactImpact(loader: DataLoader): DefectReport[] {
  printSection("Compact 影响评估");

  const threads = loader.loadVisibleThreads();
  const sessionInfos: SessionCompactInfo[] = [];
  let globalCompactedCount = 0;
  let globalTotalLostChars = 0;
  let globalTotalMessages = 0;

  for (const thread of threads) {
    const messages = loader.loadMessages(thread.id);
    let compactedCount = 0;
    let totalLostChars = 0;

    for (const msg of messages) {
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed) continue;

      // 检查 tool 消息中的 compacted 占位符
      if ((parsed as any).role === "tool") {
        const toolContent = (parsed as any).content;
        const result = parseCompacted(toolContent);
        if (result) {
          compactedCount++;
          totalLostChars += result.originalChars;
        }
      }
    }

    if (compactedCount > 0) {
      sessionInfos.push({
        threadId: thread.id,
        title: thread.title,
        totalMessages: messages.length,
        compactedCount,
        totalLostChars,
        compactedRatio: compactedCount / messages.length,
      });
    }

    globalCompactedCount += compactedCount;
    globalTotalLostChars += totalLostChars;
    globalTotalMessages += messages.length;
  }

  if (globalCompactedCount === 0) {
    console.log("  ✅ 未发现 compact 痕迹");
    return [];
  }

  // ── 全局统计 ──

  printSection("全局 Compact 统计");
  printMetric("受影响 Session", `${sessionInfos.length} / ${threads.length}`);
  printMetric("Compacted 消息总数", globalCompactedCount);
  printMetric("估算丢失字符总量", formatSize(globalTotalLostChars));
  printMetric(
    "估算丢失 Token 量",
    `~${Math.round(globalTotalLostChars / 4).toLocaleString()}`
  );
  printMetric(
    "Compacted 占全局消息比",
    `${(globalCompactedCount / globalTotalMessages * 100).toFixed(2)}%`
  );

  // ── 受影响 Session 排行 ──

  printSection("受影响 Session（按 compacted 数量排序）");
  const sortedSessions = [...sessionInfos].sort(
    (a, b) => b.compactedCount - a.compactedCount
  );
  const sessionRows = sortedSessions.map((s) => [
    s.threadId.slice(0, 12) + "...",
    s.title.slice(0, 30),
    String(s.totalMessages),
    String(s.compactedCount),
    (s.compactedRatio * 100).toFixed(1) + "%",
    formatSize(s.totalLostChars),
  ]);
  printTable(
    ["Session", "标题", "总消息", "Compacted", "占比", "丢失量"],
    sessionRows
  );

  // ── 丢失量估算 ──

  printSection("数据丢失量估算");
  const affectedMessages = sessionInfos.reduce(
    (a, s) => a + s.totalMessages,
    0
  );
  const avgLostPerSession =
    sessionInfos.length > 0
      ? Math.round(globalTotalLostChars / sessionInfos.length)
      : 0;
  const avgCompactedPerSession =
    sessionInfos.length > 0
      ? (globalCompactedCount / sessionInfos.length).toFixed(1)
      : "0";

  printMetric("受影响 Session 平均丢失量", formatSize(avgLostPerSession));
  printMetric(
    "受影响 Session 平均 compacted 数",
    avgCompactedPerSession
  );

  // 上下文占比估算
  const CONTEXT_WINDOW_TOKENS = 128_000;
  const avgLostTokens = Math.round(globalTotalLostChars / 4 / sessionInfos.length);
  const pct = (avgLostTokens / CONTEXT_WINDOW_TOKENS * 100).toFixed(2);
  printMetric(
    "平均每受影响 Session 丢失上下文占比",
    `${pct}% (${avgLostTokens.toLocaleString()} tokens / ${CONTEXT_WINDOW_TOKENS.toLocaleString()} tokens)`
  );

  // ── 对分析数据可靠性的影响 ──

  printSection("对缺陷分析数据可靠性的影响");

  // 计算如果排除受影响 session，样本量还剩多少
  const unaffectedMessages =
    globalTotalMessages -
    sessionInfos.reduce((a, s) => a + s.totalMessages, 0);
  const unaffectedSessions = threads.length - sessionInfos.length;
  printMetric(
    "排除受影响后剩余 Session",
    `${unaffectedSessions} / ${threads.length} (${(unaffectedSessions / threads.length * 100).toFixed(1)}%)`
  );
  printMetric(
    "排除受影响后剩余消息",
    `${unaffectedMessages.toLocaleString()} / ${globalTotalMessages.toLocaleString()} (${(unaffectedMessages / globalTotalMessages * 100).toFixed(1)}%)`
  );

  if (sessionInfos.length <= threads.length * 0.3) {
    console.log(
      "  ✅ 受影响 session 占比 <30%，分析数据整体可靠"
    );
  } else {
    printWarning(
      "数据可靠性",
      `受影响 session 占比 ${(sessionInfos.length / threads.length * 100).toFixed(1)}%，建议在分析时排除这些 session 或标注 compact 影响`
    );
  }

  // ── 缺陷报告 ──

  const reports: DefectReport[] = [];

  if (sessionInfos.length > 0) {
    const affectedPct = (sessionInfos.length / threads.length * 100).toFixed(1);
    reports.push({
      id: "COMPACT-001",
      severity: "low",
      category: "数据完整性",
      title: "Micro compact 导致 SQLite 中工具结果被占位符替换",
      description: `${sessionInfos.length} 个 session（${affectedPct}%）存在 micro compact 痕迹，共 ${globalCompactedCount} 条工具结果被替换为 [compacted: N chars] 占位符。估算丢失 ${(globalTotalLostChars / 1024).toFixed(1)}KB / ~${Math.round(globalTotalLostChars / 4).toLocaleString()} tokens 的原始工具输出。这些数据在 SQLite 中不可恢复，影响 payload_size、death_loop 等依赖完整工具输出的分析模块准确性。`,
      evidence: sortedSessions
        .slice(0, 5)
        .map(
          (s) =>
            `${s.title.slice(0, 30)}: ${s.compactedCount} 条 compacted, 丢失 ${formatSize(s.totalLostChars)}`
        ),
      affectedSessions: sessionInfos.map((s) => s.threadId),
      recommendation:
        "1) 缺陷分析器中增加 compact 影响标注：受影响 session 的 payload/loop 分析结果应添加 '受 compact 影响' 标记；" +
        "2) 考虑在 compacted 占位符中编码工具名和参数摘要（如 [compacted: Bash 'ls -la' → 11939 chars]），方便分析器识别被压缩的是哪个工具；" +
        "3) 对关键分析（如 Write 大文件写入）排除经历过 compact 的 session 以保证数据准确性。",
      confidence: 0.95,
    });
  }

  return reports;
}

// ── 辅助函数 ──

/** 解析 compacted 占位符，提取原始大小 */
function parseCompacted(text: string): CompactedResult | null {
  if (typeof text !== "string") return null;

  let m = text.match(COMPACTED_CHARS_RE);
  if (m) return { originalChars: parseInt(m[1]), kind: "chars" };

  m = text.match(COMPACTED_IMAGE_RE);
  if (m)
    return { originalChars: parseInt(m[1]) * 4, kind: "image" }; // tokens → chars 估算

  m = text.match(COMPACTED_DOCUMENT_RE);
  if (m)
    return { originalChars: parseInt(m[1]) * 4, kind: "document" };

  if (COMPACTED_GENERIC_RE.test(text)) {
    // 无法解析具体大小的通用占位符
    return { originalChars: 0, kind: "generic" };
  }

  return null;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
}
