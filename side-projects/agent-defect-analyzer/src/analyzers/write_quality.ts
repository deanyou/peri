//! Write 工具质量专项分析器。
//!
//! 专门分析 Write 工具写入长文件时的行为模式和问题：
//! 1. **入参大小分布**：按行数和字节数分桶，识别大文件写入频率
//! 2. **内容重复度**：Write 的 content 与之前 Read 的内容重叠度（全量覆写 vs 增量）
//! 3. **写入后上下文影响**：Write 大文件后是否触发 compact、是否出现 death loop
//! 4. **文件类型分布**：哪些文件类型倾向大写入
//! 5. **Write vs Edit 选择**：LLM 是否在应该用 Edit 时用了 Write

import type { MessageRow, DefectReport } from "../../types.js";
import { DataLoader } from "../utils/data_loader.js";
import {
  printSection,
  printMetric,
  printTable,
  printWarning,
  printProgressBar,
} from "../utils/report.js";

// ── 数据结构 ──

interface WriteRecord {
  threadId: string;
  callId: string;
  /** content 字段的字节数 */
  contentBytes: number;
  /** content 的行数 */
  lineCount: number;
  /** 文件路径 */
  filePath: string;
  /** 文件扩展名 */
  fileExt: string;
  /** 在会话中的消息序号 */
  messageIndex: number;
  /** 该会话的总消息数 */
  totalMessages: number;
  /** 写入后是否触发 compact（启发式：后续出现 system 消息含 summary/摘要 关键词） */
  triggeredCompact: boolean;
  /** 写入后 N 条消息内是否有重复 Write 同一文件 */
  repeatedWrite: boolean;
  /** 前序最近的 Read 是否读取了同一文件 */
  precededByRead: boolean;
  /** 前序 Read 到此 Write 之间经过的消息数 */
  messagesSinceRead: number;
  /** Write 后续工具调用是否出现 death loop（3+ 次相同调用） */
  followedByLoop: boolean;
}

interface WriteStats {
  totalWrites: number;
  avgContentBytes: number;
  maxContentBytes: number;
  avgLineCount: number;
  maxLineCount: number;
  /** 大文件写入次数 (>500 行) */
  largeFileWrites: number;
  /** 超大文件写入次数 (>1000 行) */
  hugeFileWrites: number;
  /** 重复写入同一文件的次数 */
  repeatedWrites: number;
  /** Write 前有 Read 的比例 */
  readBeforeRatio: number;
}

// ── 行数分桶 ──

const LINE_BUCKETS = [
  { label: "1-10 行", max: 10 },
  { label: "11-50 行", max: 50 },
  { label: "51-100 行", max: 100 },
  { label: "101-200 行", max: 200 },
  { label: "201-500 行", max: 500 },
  { label: "501-1000 行", max: 1000 },
  { label: ">1000 行", max: Infinity },
];

const SIZE_BUCKETS = [
  { label: "<1KB", max: 1024 },
  { label: "1-5KB", max: 5 * 1024 },
  { label: "5-20KB", max: 20 * 1024 },
  { label: "20-50KB", max: 50 * 1024 },
  { label: ">50KB", max: Infinity },
];

// ── 主分析 ──

export function analyzeWriteQuality(loader: DataLoader): DefectReport[] {
  printSection("Write 工具质量专项分析");

  const threads = loader.loadVisibleThreads();
  const writeRecords: WriteRecord[] = [];

  // 全局统计
  let totalReads = 0;
  let totalEdits = 0;

  for (const thread of threads) {
    const messages = loader.loadMessages(thread.id);
    const threadWrites = analyzeThreadWrites(thread.id, messages);
    writeRecords.push(...threadWrites);

    // 统计 Read/Edit 数量
    for (const msg of messages) {
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed || parsed.role !== "assistant") continue;
      const ai = parsed as any;
      const blocks = Array.isArray(ai.content) ? ai.content : [];
      for (const block of blocks) {
        if (block.type === "tool_use") {
          if (block.name === "Read") totalReads++;
          if (block.name === "Edit" || block.name === "LineEdit")
            totalEdits++;
        }
      }
    }
  }

  if (writeRecords.length === 0) {
    console.log("  ✅ 未发现 Write 工具调用");
    return [];
  }

  // ── 基础统计 ──

  printSection("Write 工具基础统计");
  const stats = computeStats(writeRecords);
  printMetric("Write 总调用数", stats.totalWrites);
  printMetric("平均内容大小", formatSize(stats.avgContentBytes));
  printMetric("最大内容大小", formatSize(stats.maxContentBytes));
  printMetric("平均行数", stats.avgLineCount.toFixed(0));
  printMetric("最大行数", stats.maxLineCount.toLocaleString());
  printMetric("大文件写入 (>500 行)", stats.largeFileWrites);
  printMetric("超大文件写入 (>1000 行)", stats.hugeFileWrites);
  printMetric(
    "Read:Write:Edit 比",
    `${totalReads}:${stats.totalWrites}:${totalEdits}`
  );

  // ── 行数分布 ──

  printSection("Write 内容行数分布");
  const lineDistribution = new Map<string, number>();
  for (const b of LINE_BUCKETS) lineDistribution.set(b.label, 0);

  for (const rec of writeRecords) {
    for (const b of LINE_BUCKETS) {
      if (rec.lineCount <= b.max) {
        lineDistribution.set(b.label, (lineDistribution.get(b.label) || 0) + 1);
        break;
      }
    }
  }

  const lineRows = LINE_BUCKETS.map((b) => {
    const count = lineDistribution.get(b.label) || 0;
    const pct = (count / writeRecords.length * 100).toFixed(1);
    return [b.label, String(count), `${pct}%`];
  });
  printTable(["行数范围", "次数", "占比"], lineRows);

  // ── 大小分布 ──

  printSection("Write 入参大小分布");
  const sizeDistribution = new Map<string, number>();
  for (const b of SIZE_BUCKETS) sizeDistribution.set(b.label, 0);

  for (const rec of writeRecords) {
    for (const b of SIZE_BUCKETS) {
      if (rec.contentBytes <= b.max) {
        sizeDistribution.set(b.label, (sizeDistribution.get(b.label) || 0) + 1);
        break;
      }
    }
  }

  const sizeRows = SIZE_BUCKETS.map((b) => {
    const count = sizeDistribution.get(b.label) || 0;
    const pct = (count / writeRecords.length * 100).toFixed(1);
    return [b.label, String(count), `${pct}%`];
  });
  printTable(["大小范围", "次数", "占比"], sizeRows);

  // ── 文件类型分布 ──

  printSection("Write 文件类型分布");
  const extStats = new Map<
    string,
    { count: number; totalLines: number; maxLines: number; totalBytes: number }
  >();
  for (const rec of writeRecords) {
    const ext = rec.fileExt || "(无扩展名)";
    if (!extStats.has(ext)) {
      extStats.set(ext, { count: 0, totalLines: 0, maxLines: 0, totalBytes: 0 });
    }
    const s = extStats.get(ext)!;
    s.count++;
    s.totalLines += rec.lineCount;
    s.maxLines = Math.max(s.maxLines, rec.lineCount);
    s.totalBytes += rec.contentBytes;
  }

  const extRows = [...extStats.entries()]
    .sort((a, b) => b[1].count - a[1].count)
    .slice(0, 10)
    .map(([ext, s]) => [
      ext,
      String(s.count),
      String(Math.round(s.totalLines / s.count)),
      String(s.maxLines),
      formatSize(Math.round(s.totalBytes / s.count)),
    ]);
  printTable(["扩展名", "写入次数", "平均行数", "最大行数", "平均大小"], extRows);

  // ── Write 前是否有 Read ──

  printSection("Write 前的 Read 行为");
  const withRead = writeRecords.filter((r) => r.precededByRead).length;
  const withoutRead = writeRecords.length - withRead;
  const readRatio = (withRead / writeRecords.length * 100).toFixed(1);
  printMetric("Write 前有 Read", `${withRead} (${readRatio}%)`);
  printMetric("Write 前无 Read（盲写）", withoutRead);

  // Read → Write 间隔分布
  const withReadRecords = writeRecords.filter((r) => r.precededByRead);
  if (withReadRecords.length > 0) {
    const avgGap = (
      withReadRecords.reduce((a, r) => a + r.messagesSinceRead, 0) /
      withReadRecords.length
    ).toFixed(1);
    printMetric("Read→Write 平均间隔", `${avgGap} 条消息`);
  }

  // ── 重复写入分析 ──

  printSection("同一文件重复写入");
  const repeatedWrites = writeRecords.filter((r) => r.repeatedWrite);
  if (repeatedWrites.length > 0) {
    printWarning("重复写入", `同一会话中对同一文件进行了多次 Write: ${repeatedWrites.length} 次`);
    const repeatedByFile = new Map<string, number>();
    for (const r of repeatedWrites) {
      const key = r.filePath;
      repeatedByFile.set(key, (repeatedByFile.get(key) || 0) + 1);
    }
    const repeatedRows = [...repeatedByFile.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, 10)
      .map(([path, count]) => [path.slice(-50), String(count)]);
    printTable(["文件路径", "重复写入次数"], repeatedRows);
  } else {
    console.log("  ✅ 未发现同一文件重复写入");
  }

  // ── 大文件写入详情 ──

  printSection("大文件写入详情 (>500 行)");
  const largeWrites = writeRecords
    .filter((r) => r.lineCount > 500)
    .sort((a, b) => b.lineCount - a.lineCount);

  if (largeWrites.length > 0) {
    printMetric("大文件写入总数", largeWrites.length);
    const largeRows = largeWrites.slice(0, 15).map((r) => [
      r.threadId.slice(0, 12) + "...",
      String(r.lineCount),
      formatSize(r.contentBytes),
      r.fileExt,
      r.filePath.slice(-40),
    ]);
    printTable(
      ["Session", "行数", "大小", "类型", "文件路径"],
      largeRows
    );
  } else {
    console.log("  ✅ 未发现超过 500 行的文件写入");
  }

  // ── 上下文影响分析 ──

  printSection("Write 对上下文的影响");

  // 按 Write 大小分组，对比后续行为
  const smallWrites = writeRecords.filter((r) => r.lineCount <= 50);
  const mediumWrites = writeRecords.filter(
    (r) => r.lineCount > 50 && r.lineCount <= 200
  );
  const largeWritesAll = writeRecords.filter((r) => r.lineCount > 200);

  const groups = [
    { label: "小型 (≤50行)", records: smallWrites },
    { label: "中型 (51-200行)", records: mediumWrites },
    { label: "大型 (>200行)", records: largeWritesAll },
  ];

  const impactRows = groups
    .filter((g) => g.records.length > 0)
    .map((g) => {
      const loopRate = (
        (g.records.filter((r) => r.followedByLoop).length /
          g.records.length) *
        100
      ).toFixed(1);
      const repeatRate = (
        (g.records.filter((r) => r.repeatedWrite).length /
          g.records.length) *
        100
      ).toFixed(1);
      return [g.label, String(g.records.length), `${loopRate}%`, `${repeatRate}%`];
    });
  printTable(
    ["Write 大小", "次数", "后续 Loop 率", "重复写入率"],
    impactRows
  );

  // ── 上下文占比估算 ──

  printSection("Write 入参的上下文占比估算");
  // 假设 128K context window，1 token ≈ 4 bytes
  const CONTEXT_WINDOW_TOKENS = 128_000;
  const BYTES_PER_TOKEN = 4;
  const contextBytes = CONTEXT_WINDOW_TOKENS * BYTES_PER_TOKEN;

  for (const g of groups) {
    if (g.records.length === 0) continue;
    const avgBytes = g.records.reduce((a, r) => a + r.contentBytes, 0) / g.records.length;
    const maxBytes = Math.max(...g.records.map((r) => r.contentBytes));
    const avgPct = ((avgBytes / contextBytes) * 100).toFixed(2);
    const maxPct = ((maxBytes / contextBytes) * 100).toFixed(2);
    console.log(`  ${g.label}:`);
    printProgressBar("  平均占比", Number(avgPct) / 100);
    console.log(`    平均 ${formatSize(avgBytes)} (${avgPct}%), 最大 ${formatSize(maxBytes)} (${maxPct}%)`);
  }

  // ── Top 大文件写入会话追踪 ──

  printSection("Top 5 大写入的会话上下文");
  const topWrites = writeRecords
    .sort((a, b) => b.contentBytes - a.contentBytes)
    .slice(0, 5);

  for (const w of topWrites) {
    console.log(
      `  Session ${w.threadId.slice(0, 12)}... | ` +
        `${w.lineCount} 行 / ${formatSize(w.contentBytes)} | ` +
        `${w.filePath.slice(-40)}`
    );
    console.log(
      `    会话位置: ${w.messageIndex}/${w.totalMessages} | ` +
        `前有Read: ${w.precededByRead ? `是(${w.messagesSinceRead}条前)` : "否"} | ` +
        `重复写入: ${w.repeatedWrite ? "是" : "否"} | ` +
        `后续Loop: ${w.followedByLoop ? "是" : "否"}`
    );
  }

  // ── 缺陷报告 ──

  const reports: DefectReport[] = [];

  // 大文件写入报告
  if (stats.largeFileWrites > 0) {
    const largeFilePct = (stats.largeFileWrites / stats.totalWrites * 100).toFixed(1);
    reports.push({
      id: "WRITE-001",
      severity: stats.hugeFileWrites > 0 ? "high" : "medium",
      category: "上下文膨胀",
      title: "Write 工具写入超大文件消耗大量上下文",
      description: `${stats.largeFileWrites} 次写入超过 500 行（占 ${largeFilePct}%），其中 ${stats.hugeFileWrites} 次超过 1000 行。最大 ${stats.maxLineCount} 行 / ${formatSize(stats.maxContentBytes)}。大文件 Write 的 content 在 tool_use input 中出现一次（入参），tool_result 回显后再次出现在历史中，单次大 Write 可能消耗 2x 的上下文空间。`,
      evidence: largeWrites.slice(0, 5).map(
        (r) => `${r.lineCount} 行 / ${formatSize(r.contentBytes)} — ${r.filePath.slice(-40)}`
      ),
      affectedSessions: [...new Set(largeWrites.map((r) => r.threadId))],
      recommendation:
        "1) 在系统提示中引导 LLM：超过 100 行的新文件，考虑分块 Write 或先写骨架再用 LineEdit 追加；" +
        "2) Write 工具描述中添加：'对于超过 200 行的文件，优先考虑先用 Write 写骨架，再用 LineEdit 逐步补充'；" +
        "3) 在 Write 执行后对 content 做 hash 缓存，tool_result 不回传完整内容，改为 'Wrote N lines to <path>'（当前已实现）。" +
        "注意：真正的上下文消耗在 LLM 生成 tool_use 时的 input 字段（入参端），非 tool_result（出参端）。",
      confidence: 0.85,
    });
  }

  // 重复写入报告
  if (repeatedWrites.length > 0) {
    reports.push({
      id: "WRITE-002",
      severity: "medium",
      category: "策略低效",
      title: "同一文件被多次全量 Write",
      description: `${repeatedWrites.length} 次对同一文件的重复 Write。LLM 可能在应该用 Edit/LineEdit 做局部修改时，选择了全量覆写。每次重复 Write 都把完整文件内容作为入参发送，浪费上下文。`,
      evidence: repeatedWrites.slice(0, 5).map(
        (r) => `${r.filePath.slice(-40)} — ${r.lineCount} 行 x 重写`
      ),
      affectedSessions: [...new Set(repeatedWrites.map((r) => r.threadId))],
      recommendation:
        "在系统提示中强化：'对已有文件的修改必须用 Edit/LineEdit，禁止用 Write 覆写已有文件，除非文件需要完全重写'。" +
        "在 Write 工具执行时检测文件是否已存在且内容变化 <50%，如果是则返回警告提示使用 Edit。",
      confidence: 0.75,
    });
  }

  // 上下文占比警告
  const bigImpactWrites = writeRecords.filter((r) => r.contentBytes > 20 * 1024);
  if (bigImpactWrites.length > 0) {
    reports.push({
      id: "WRITE-003",
      severity: "medium",
      category: "上下文效率",
      title: "Write 入参超过 20KB 占用大量上下文",
      description: `${bigImpactWrites.length} 次 Write 的入参超过 20KB。在 128K 上下文窗口中，20KB ≈ 5K tokens，占 3.9%。考虑到 tool_use input 在 LLM 生成后保留在消息历史中，实际影响更大。`,
      evidence: bigImpactWrites.slice(0, 5).map(
        (r) => `${formatSize(r.contentBytes)} — ${r.filePath.slice(-40)}`
      ),
      affectedSessions: [...new Set(bigImpactWrites.map((r) => r.threadId))],
      recommendation:
        "引导 LLM 将大文件创建拆分为多步：Write 写骨架（<100 行）→ LineEdit 逐步补充。" +
        "这与人类程序员的工作方式一致：先建框架，再填充细节。",
      confidence: 0.7,
    });
  }

  return reports;
}

// ── 线程级分析 ──

function analyzeThreadWrites(
  threadId: string,
  messages: MessageRow[]
): WriteRecord[] {
  const records: WriteRecord[] = [];

  // 第一遍：建立 callId → tool_result 映射
  const callResults = new Map<string, { size: number; isError: boolean }>();
  for (const msg of messages) {
    if (msg.role === "tool") {
      const parsed = DataLoader.parseContent(msg.content);
      if (parsed && "tool_call_id" in parsed) {
        callResults.set((parsed as any).tool_call_id, {
          size: msg.content.length,
          isError: (parsed as any).is_error || false,
        });
      }
    }
  }

  // 第二遍：提取所有 Write 调用 + Read 文件记录
  interface ReadRecord {
    filePath: string;
    messageIndex: number;
  }

  interface WriteCall {
    callId: string;
    content: string;
    filePath: string;
    messageIndex: number;
  }

  const readHistory: ReadRecord[] = [];
  const writeCalls: WriteCall[] = [];

  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i];
    if (msg.role !== "assistant") continue;

    const parsed = DataLoader.parseContent(msg.content);
    if (!parsed || parsed.role !== "assistant") continue;

    const ai = parsed as any;
    const blocks = Array.isArray(ai.content) ? ai.content : [];

    for (const block of blocks) {
      if (block.type !== "tool_use") continue;

      if (block.name === "Read") {
        const fp = block.input?.file_path || block.input?.path || "";
        if (fp) {
          readHistory.push({ filePath: fp, messageIndex: i });
        }
      }

      if (block.name === "Write") {
        const content = block.input?.content || "";
        const filePath = block.input?.file_path || "";
        if (content && filePath) {
          writeCalls.push({
            callId: block.id,
            content,
            filePath,
            messageIndex: i,
          });
        }
      }
    }
  }

  // 第三遍：为每个 Write 调用补充上下文信息
  for (const wc of writeCalls) {
    const lineCount = wc.content.split("\n").length;
    const contentBytes = Buffer.byteLength(wc.content, "utf8");
    const filePath = wc.filePath;
    const fileExt = filePath.includes(".")
      ? "." + filePath.split(".").pop()!.toLowerCase()
      : "";

    // 查找前序最近的 Read
    const precedingRead = [...readHistory]
      .reverse()
      .find(
        (r) =>
          r.messageIndex < wc.messageIndex &&
          pathsMatch(r.filePath, filePath)
      );
    const precededByRead = precedingRead !== undefined;
    const messagesSinceRead = precededByRead
      ? wc.messageIndex - precedingRead.messageIndex
      : 0;

    // 检查是否有重复 Write 同一文件
    const repeatedWrite = writeCalls.some(
      (other) =>
        other.callId !== wc.callId &&
        other.messageIndex < wc.messageIndex &&
        pathsMatch(other.filePath, filePath) &&
        other.messageIndex > wc.messageIndex - 20 // 20 条消息内的重复
    );

    // 检查后续是否出现 death loop（简化：检查 Write 后 10 条消息内是否有 3+ 次同工具调用）
    const followedByLoop = detectLoopAfter(wc.messageIndex, messages);

    records.push({
      threadId,
      callId: wc.callId,
      contentBytes,
      lineCount,
      filePath,
      fileExt,
      messageIndex: wc.messageIndex,
      totalMessages: messages.length,
      triggeredCompact: false, // 需要更精确的检测
      repeatedWrite,
      precededByRead,
      messagesSinceRead,
      followedByLoop,
    });
  }

  return records;
}

// ── 辅助函数 ──

/** 路径匹配（忽略 cwd 前缀差异） */
function pathsMatch(a: string, b: string): boolean {
  const normalize = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  const na = normalize(a);
  const nb = normalize(b);
  // 完全匹配
  if (na === nb) return true;
  // 尾部匹配（处理相对/绝对路径差异）
  if (na.length > 10 && nb.length > 10) {
    return na.endsWith(nb.slice(-Math.min(na.length, nb.length))) ||
      nb.endsWith(na.slice(-Math.min(na.length, nb.length)));
  }
  return false;
}

/** 简化 loop 检测：Write 后 10 条消息内是否有 3+ 次同工具调用 */
function detectLoopAfter(
  writeIndex: number,
  messages: MessageRow[]
): boolean {
  const windowStart = writeIndex + 1;
  const windowEnd = Math.min(writeIndex + 30, messages.length);

  const toolCounts = new Map<string, number>();

  for (let i = windowStart; i < windowEnd; i++) {
    const msg = messages[i];
    if (msg.role !== "assistant") continue;
    const parsed = DataLoader.parseContent(msg.content);
    if (!parsed || parsed.role !== "assistant") continue;

    const ai = parsed as any;
    const blocks = Array.isArray(ai.content) ? ai.content : [];
    for (const block of blocks) {
      if (block.type === "tool_use") {
        const key = `${block.name}:${JSON.stringify(block.input || {}).slice(0, 100)}`;
        toolCounts.set(key, (toolCounts.get(key) || 0) + 1);
        if ((toolCounts.get(key) || 0) >= 3) return true;
      }
    }
  }
  return false;
}

function computeStats(records: WriteRecord[]): WriteStats {
  if (records.length === 0) {
    return {
      totalWrites: 0, avgContentBytes: 0, maxContentBytes: 0,
      avgLineCount: 0, maxLineCount: 0,
      largeFileWrites: 0, hugeFileWrites: 0,
      repeatedWrites: 0, readBeforeRatio: 0,
    };
  }

  const totalBytes = records.reduce((a, r) => a + r.contentBytes, 0);
  const totalLines = records.reduce((a, r) => a + r.lineCount, 0);

  return {
    totalWrites: records.length,
    avgContentBytes: Math.round(totalBytes / records.length),
    maxContentBytes: Math.max(...records.map((r) => r.contentBytes)),
    avgLineCount: totalLines / records.length,
    maxLineCount: Math.max(...records.map((r) => r.lineCount)),
    largeFileWrites: records.filter((r) => r.lineCount > 500).length,
    hugeFileWrites: records.filter((r) => r.lineCount > 1000).length,
    repeatedWrites: records.filter((r) => r.repeatedWrite).length,
    readBeforeRatio: records.filter((r) => r.precededByRead).length / records.length,
  };
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
}
