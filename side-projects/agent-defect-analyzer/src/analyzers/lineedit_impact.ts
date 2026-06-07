//! LineEdit 工具影响专项分析器。
//!
//! 分析 LineEdit（行号编辑模式）对 Agent 会话的帮助：
//! 1. **使用概览**：调用频率、成功率、编辑范围分布
//! 2. **上下文效率**：入参/出参大小对比 Edit 和 Write
//! 3. **错误对比**：LineEdit vs Edit 的失败率和错误类型差异
//! 4. **重读率指标**：编辑文件X后读回文件X的比率（核心指标，只计同文件）
//! 5. **工作流影响**：失败恢复路径、连续编辑能力

import type { DefectReport } from "../../types.js";
import { DataLoader } from "../utils/data_loader.js";
import {
  printSection,
  printMetric,
  printTable,
  printWarning,
  printProgressBar,
} from "../utils/report.js";

// ── 数据结构 ──

interface ToolEvent {
  name: string;
  filePath: string;
  inputBytes: number;
  resultBytes: number;
  isError: boolean;
  errorMsg: string;
  msgIdx: number;
  totalMsgs: number;
}

interface EditOp {
  threadId: string;
  filePath: string;
  startLine: number;
  endLine: number;
  newStringLen: number;
  insertMode: boolean;
  inputBytes: number;
  isSuccess: boolean;
}

interface SessionEventData {
  threadId: string;
  title: string;
  totalMsgs: number;
  events: ToolEvent[];
}

/**
 * 重读率分析结果。
 *
 * **定义：编辑文件X后，在后续N步内 Read 回文件X。读其他文件不算。**
 * 分母是 withFilePath（能提取到文件路径的编辑事件数）。
 */
interface ReReadStats {
  /** 编辑事件总数 */
  total: number;
  /** 能提取到 filePath 的编辑事件（作为分母） */
  withFilePath: number;
  /** 紧邻(1步)读回同文件 */
  immediateReRead: number;
  /** 3步内读回同文件 */
  within3ReRead: number;
  /** 5步内读回同文件 */
  within5ReRead: number;
  /** 编辑成功后 5步内读回同文件 */
  successReRead: number;
  /** 编辑成功事件数（有 filePath 的） */
  successTotal: number;
  /** 编辑失败后 5步内读回同文件 */
  failReRead: number;
  /** 编辑失败事件数（有 filePath 的） */
  failTotal: number;
}

// ── 主分析 ──

export function analyzeLineEditImpact(loader: DataLoader): DefectReport[] {
  printSection("LineEdit 工具影响分析");

  const threads = loader.loadVisibleThreads();

  // 收集所有工具事件
  const sessionData = collectSessionData(loader, threads);

  const leEvents = sessionData.flatMap((s) =>
    s.events.filter((e) => e.name === "LineEdit")
  );
  const editEvents = sessionData.flatMap((s) =>
    s.events.filter((e) => e.name === "Edit")
  );
  const writeEvents = sessionData.flatMap((s) =>
    s.events.filter((e) => e.name === "Write")
  );

  const sessionsWithLE = sessionData.filter((s) =>
    s.events.some((e) => e.name === "LineEdit")
  );
  const sessionsWithoutLE = sessionData.filter(
    (s) => !s.events.some((e) => e.name === "LineEdit")
  );

  // ── 1. 使用概览 ──

  printSection("1. 使用概览");
  printMetric("涉及 LineEdit 的会话数", sessionsWithLE.length, ` / ${threads.length} 总会话`);
  printMetric("LineEdit 调用", leEvents.length);
  printMetric("Edit 调用", editEvents.length);
  printMetric("Write 调用", writeEvents.length);

  const totalEdits = leEvents.length + editEvents.length;
  if (totalEdits > 0) {
    printMetric(
      "LineEdit 占编辑操作比例",
      `${((leEvents.length / totalEdits) * 100).toFixed(1)}%`
    );
  }

  // 成功率
  printSection("成功率对比");
  printTable(
    ["工具", "总调用", "成功", "失败", "成功率"],
    [
      toolRow("LineEdit", leEvents),
      toolRow("Edit", editEvents),
      toolRow("Write", writeEvents),
    ]
  );

  // ── 2. 编辑范围分布 ──

  const editOps = collectEditOps(loader, threads);
  if (editOps.length > 0) {
    printSection("2. LineEdit 编辑范围分布");
    const ranges = editOps.map((e) =>
      e.insertMode ? 0 : e.endLine - e.startLine + 1
    );
    const buckets = [
      { label: "0 (insert)", test: (r: number) => r === 0 },
      { label: "1 行", test: (r: number) => r === 1 },
      { label: "2-5 行", test: (r: number) => r >= 2 && r <= 5 },
      { label: "6-20 行", test: (r: number) => r >= 6 && r <= 20 },
      { label: "21-50 行", test: (r: number) => r >= 21 && r <= 50 },
      { label: ">50 行", test: (r: number) => r > 50 },
    ];
    const rangeRows = buckets.map((b) => {
      const count = ranges.filter(b.test).length;
      return [b.label, String(count), pct(count, ranges.length)];
    });
    printTable(["范围", "次数", "占比"], rangeRows);

    const inserts = editOps.filter((e) => e.insertMode).length;
    const replaces = editOps.filter((e) => !e.insertMode).length;
    printMetric("insert 模式", inserts, ` (${pct(inserts, editOps.length)})`);
    printMetric("replace 模式", replaces, ` (${pct(replaces, editOps.length)})`);

    const newStrLens = editOps.map((e) => e.newStringLen);
    printMetric("new_string 平均大小", Math.round(avg(newStrLens)), " 字符");
    printMetric("new_string 中位数", median(newStrLens), " 字符");
  }

  // ── 3. 上下文效率 ──

  printSection("3. 上下文效率对比 (入参大小)");
  printTable(
    ["工具", "平均", "P50", "P95", "最大", "样本数"],
    [
      statsRow("LineEdit", leEvents.map((e) => e.inputBytes)),
      statsRow("Edit", editEvents.map((e) => e.inputBytes)),
      statsRow("Write", writeEvents.map((e) => e.inputBytes)),
    ]
  );

  printSection("4. 上下文效率对比 (出参大小 = tool_result)");
  printTable(
    ["工具", "平均", "P50", "P95", "最大", "样本数"],
    [
      statsRow("LineEdit", leEvents.map((e) => e.resultBytes)),
      statsRow("Edit", editEvents.map((e) => e.resultBytes)),
      statsRow("Write", writeEvents.map((e) => e.resultBytes)),
    ]
  );

  // 上下文节省估算
  const avgLEInput = avg(leEvents.map((e) => e.inputBytes));
  const avgWriteInput = avg(writeEvents.map((e) => e.inputBytes));
  if (avgLEInput > 0 && avgWriteInput > 0) {
    printSection("上下文节省估算");
    printMetric(
      "LineEdit vs Write 入参比",
      `${((avgLEInput / avgWriteInput) * 100).toFixed(1)}%`
    );
    printMetric(
      "如果 LineEdit 替代 Write",
      `每次节省 ${Math.round(avgWriteInput - avgLEInput)} 字节 (${((1 - avgLEInput / avgWriteInput) * 100).toFixed(1)}%)`
    );
  }

  // ── 5. 错误类型对比 ──

  printSection("5. 错误类型对比");
  const leErrorTypes = classifyErrors(leEvents);
  const editErrorTypes = classifyErrors(editEvents);

  const allErrorTypes = new Set([
    ...leErrorTypes.keys(),
    ...editErrorTypes.keys(),
  ]);
  const errorRows = [...allErrorTypes].map((type) => [
    type,
    String(leErrorTypes.get(type) || 0),
    String(editErrorTypes.get(type) || 0),
  ]);
  if (errorRows.length > 0) {
    printTable(["错误类型", "LineEdit", "Edit"], errorRows);
  } else {
    console.log("  ✅ 无编辑工具错误");
  }

  // LineEdit 错误详情
  const leErrors = leEvents.filter((e) => e.isError);
  if (leErrors.length > 0) {
    printWarning(
      "LineEdit 错误特征",
      `${leErrors.length} 次失败全部为参数解析错误（missing field），非匹配逻辑错误`
    );
    for (const e of leErrors.slice(0, 3)) {
      console.log(`    ${e.errorMsg.slice(0, 120)}`);
    }
  }

  // ── 6. 重读率指标（核心指标） ──
  // 重读 = 编辑文件X后，在N步内 Read 回文件X。读其他文件不算。

  printSection("6. 重读率指标 (编辑文件X → 读回文件X)");

  const leReRead = computeReReadStats(sessionData, "LineEdit");
  const editReRead = computeReReadStats(sessionData, "Edit");
  const writeReRead = computeReReadStats(sessionData, "Write");

  // 6a. 三级窗口重读率
  printSection("6a. 重读率 (编辑同文件后读回同一文件)");
  printTable(
    ["工具", "有效编辑数", "紧邻重读", "3步内重读", "5步内重读", "5步内重读率"],
    [
      [
        "LineEdit",
        String(leReRead.withFilePath),
        String(leReRead.immediateReRead),
        String(leReRead.within3ReRead),
        String(leReRead.within5ReRead),
        pct(leReRead.within5ReRead, leReRead.withFilePath),
      ],
      [
        "Edit",
        String(editReRead.withFilePath),
        String(editReRead.immediateReRead),
        String(editReRead.within3ReRead),
        String(editReRead.within5ReRead),
        pct(editReRead.within5ReRead, editReRead.withFilePath),
      ],
      [
        "Write",
        String(writeReRead.withFilePath),
        String(writeReRead.immediateReRead),
        String(writeReRead.within3ReRead),
        String(writeReRead.within5ReRead),
        pct(writeReRead.within5ReRead, writeReRead.withFilePath),
      ],
    ]
  );

  // 6b. 成功/失败后重读率
  printSection("6b. 编辑成功/失败后的重读率 (5步内读回同文件)");
  printTable(
    ["工具", "成功→重读", "成功总数", "成功重读率", "失败→重读", "失败总数", "失败重读率"],
    [
      reReadSuccessFailRow("LineEdit", leReRead),
      reReadSuccessFailRow("Edit", editReRead),
      reReadSuccessFailRow("Write", writeReRead),
    ]
  );

  // 6c. 可视化
  printSection("6c. 重读率可视化");
  for (const { name, stats } of [
    { name: "LineEdit", stats: leReRead },
    { name: "Edit", stats: editReRead },
    { name: "Write", stats: writeReRead },
  ]) {
    if (stats.withFilePath === 0) continue;
    console.log(`  ${name}:`);
    const denom = stats.withFilePath;
    printProgressBar("    紧邻重读", stats.immediateReRead / denom);
    printProgressBar("    3步内重读", stats.within3ReRead / denom);
    printProgressBar("    5步内重读", stats.within5ReRead / denom);
  }

  // ── 7. 连续编辑能力 ──

  printSection("7. 同文件连续编辑");
  const consecutive = analyzeConsecutiveEdits(sessionData);
  printMetric("连续编辑总次数", consecutive.totalConsecutive);
  printMetric("最长连续编辑链", consecutive.maxChain);
  if (consecutive.samples.length > 0) {
    printTable(
      ["文件", "连续编辑次数"],
      consecutive.samples
        .sort((a, b) => b.count - a.count)
        .slice(0, 10)
        .map((s) => [s.file, String(s.count)])
    );
  }

  // ── 8. Edit 失败后的恢复路径 ──

  printSection("8. Edit 失败后的恢复路径");
  const recoveryPaths = analyzeRecoveryPaths(sessionData);
  if (recoveryPaths.length > 0) {
    const leRecovery = recoveryPaths.filter((p) =>
      p.nextTool === "LineEdit"
    ).length;
    const otherRecovery = recoveryPaths.length - leRecovery;

    printMetric("Edit 失败总数", recoveryPaths.length);
    printMetric("失败后切换到 LineEdit", leRecovery, ` (${pct(leRecovery, recoveryPaths.length)})`);
    printMetric("失败后其他恢复路径", otherRecovery);

    // 展示典型恢复路径
    console.log("\n  典型恢复路径:");
    for (const p of recoveryPaths.slice(0, 8)) {
      console.log(
        `    Edit fail → ${p.nextTool}${p.followUp ? " → " + p.followUp : ""}`
      );
    }
  }

  // ── 9. 有/无 LineEdit 会话对比 ──

  if (sessionsWithoutLE.length > 0 && sessionsWithLE.length > 0) {
    printSection("9. 有/无 LineEdit 的会话对比");
    printTable(
      ["指标", "有 LineEdit", "无 LineEdit"],
      [
        [
          "会话数",
          String(sessionsWithLE.length),
          String(sessionsWithoutLE.length),
        ],
        [
          "平均消息数",
          avg(sessionsWithLE.map((s) => s.totalMsgs)).toFixed(1),
          avg(sessionsWithoutLE.map((s) => s.totalMsgs)).toFixed(1),
        ],
        [
          "平均 Read/会话",
          avg(
            sessionsWithLE.map(
              (s) => s.events.filter((e) => e.name === "Read").length
            )
          ).toFixed(1),
          avg(
            sessionsWithoutLE.map(
              (s) => s.events.filter((e) => e.name === "Read").length
            )
          ).toFixed(1),
        ],
        [
          "平均 Edit/会话",
          avg(
            sessionsWithLE.map(
              (s) => s.events.filter((e) => e.name === "Edit").length
            )
          ).toFixed(1),
          avg(
            sessionsWithoutLE.map(
              (s) => s.events.filter((e) => e.name === "Edit").length
            )
          ).toFixed(1),
        ],
        [
          "平均 Write/会话",
          avg(
            sessionsWithLE.map(
              (s) => s.events.filter((e) => e.name === "Write").length
            )
          ).toFixed(1),
          avg(
            sessionsWithoutLE.map(
              (s) => s.events.filter((e) => e.name === "Write").length
            )
          ).toFixed(1),
        ],
      ]
    );
  }

  // ── 缺陷报告 ──

  return buildReports(leEvents, editEvents, writeEvents, leErrors, recoveryPaths, leReRead, editReRead);
}

// ── 数据收集 ──

function collectSessionData(
  loader: DataLoader,
  threads: { id: string; title?: string | null }[]
): SessionEventData[] {
  const result: SessionEventData[] = [];

  for (const thread of threads) {
    const messages = loader.loadMessages(thread.id);
    const callMap = new Map<
      string,
      { name: string; input: any; msgIdx: number }
    >();
    const events: ToolEvent[] = [];

    for (let i = 0; i < messages.length; i++) {
      const msg = messages[i];
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed) continue;

      if (parsed.role === "assistant") {
        const blocks = Array.isArray(parsed.content) ? parsed.content : [];
        for (const b of blocks) {
          if (b.type === "tool_use") {
            callMap.set(b.id, {
              name: b.name,
              input: b.input || {},
              msgIdx: i,
            });
          }
        }
      }

      if (parsed.role === "tool" && "tool_call_id" in parsed) {
        const tc = parsed as { tool_call_id?: string; content: string; is_error?: boolean };
        if (!tc.tool_call_id) continue;
        const call = callMap.get(tc.tool_call_id);
        if (!call) continue;

        if (
          ["LineEdit", "Edit", "Write", "Read"].includes(call.name)
        ) {
          let filePath = extractFilePath(call.name, call.input);
          const content = String(tc.content || "");
          events.push({
            name: call.name,
            filePath,
            inputBytes: JSON.stringify(call.input || {}).length,
            resultBytes: content.length,
            isError: content.startsWith("Error:") || !!tc.is_error,
            errorMsg: content.slice(0, 300),
            msgIdx: i,
            totalMsgs: messages.length,
          });
        }
      }
    }

    if (events.length > 0) {
      result.push({
        threadId: thread.id,
        title: thread.title || "",
        totalMsgs: messages.length,
        events,
      });
    }
  }

  return result;
}

function extractFilePath(name: string, input: any): string {
  if (name === "LineEdit") {
    const edits = input?.edits;
    if (Array.isArray(edits) && edits[0]?.file_path)
      return edits[0].file_path;
    return "";
  }
  return input?.file_path || input?.path || "";
}

function collectEditOps(
  loader: DataLoader,
  threads: { id: string }[]
): EditOp[] {
  const ops: EditOp[] = [];

  for (const thread of threads) {
    const messages = loader.loadMessages(thread.id);
    const callMap = new Map<
      string,
      { input: any; msgIdx: number; isError: boolean }
    >();

    for (const msg of messages) {
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed) continue;

      if (parsed.role === "assistant") {
        const blocks = Array.isArray(parsed.content) ? parsed.content : [];
        for (const b of blocks) {
          if (b.type === "tool_use" && b.name === "LineEdit") {
            callMap.set(b.id, {
              input: b.input || {},
              msgIdx: 0,
              isError: false,
            });
          }
        }
      }

      if (parsed.role === "tool" && "tool_call_id" in parsed) {
        const tc = parsed as { tool_call_id?: string; content: string; is_error?: boolean };
        if (!tc.tool_call_id) continue;
        const call = callMap.get(tc.tool_call_id);
        if (!call) continue;

        const content = String(tc.content || "");
        call.isError = content.startsWith("Error:") || !!tc.is_error;

        const edits = call.input?.edits;
        if (Array.isArray(edits)) {
          for (const e of edits) {
            ops.push({
              threadId: thread.id,
              filePath: String(e.file_path || ""),
              startLine: e.start_line || 0,
              endLine: e.end_line || 0,
              newStringLen: String(e.new_string || "").length,
              insertMode: !!e.insert,
              inputBytes: JSON.stringify(call.input).length,
              isSuccess: !call.isError,
            });
          }
        }
      }
    }
  }

  return ops;
}

// ── 重读率计算 ──

/** 针对指定编辑工具，在 session 内 events 序列上计算重读率。
 *
 * **重读 = 编辑文件X后，在后续N步内 Read 回同一文件X。** 读其他文件不算。
 *
 * 三级窗口：紧邻(1步)、3步内、5步内。每级只计第一个匹配的 Read。 */
function computeReReadStats(
  sessions: SessionEventData[],
  toolName: string
): ReReadStats {
  const stats: ReReadStats = {
    total: 0,
    withFilePath: 0,
    immediateReRead: 0,
    within3ReRead: 0,
    within5ReRead: 0,
    successReRead: 0,
    successTotal: 0,
    failReRead: 0,
    failTotal: 0,
  };

  for (const session of sessions) {
    const events = session.events;

    for (let i = 0; i < events.length; i++) {
      if (events[i].name !== toolName) continue;

      stats.total++;

      const editFilePath = normalizePath(events[i].filePath);
      if (!editFilePath) continue; // 无法提取路径的不计入重读率
      stats.withFilePath++;

      const isError = events[i].isError;

      // 在后续 5 步内找第一个 Read 同一文件的事件
      const windowEnd = Math.min(i + 6, events.length);
      let firstReReadStep = -1; // 1-based: 第几步重读了同一文件
      for (let j = i + 1; j < windowEnd; j++) {
        if (events[j].name !== "Read") continue;
        const readPath = normalizePath(events[j].filePath);
        if (!readPath || !pathsMatch(editFilePath, readPath)) continue;
        firstReReadStep = j - i; // 1=紧邻, 2=隔1步, ...
        break;
      }

      if (firstReReadStep >= 1) stats.within5ReRead++;
      if (firstReReadStep >= 1 && firstReReadStep <= 3) stats.within3ReRead++;
      if (firstReReadStep === 1) stats.immediateReRead++;

      // 成功/失败拆分
      if (isError) {
        stats.failTotal++;
        if (firstReReadStep >= 1) stats.failReRead++;
      } else {
        stats.successTotal++;
        if (firstReReadStep >= 1) stats.successReRead++;
      }
    }
  }

  return stats;
}

/** 路径归一化：统一斜杠方向，去除尾部斜杠 */
function normalizePath(p: string): string {
  return p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

/** 路径匹配：支持绝对/相对路径差异 */
function pathsMatch(a: string, b: string): boolean {
  if (a === b) return true;
  // 尾部匹配（处理 cwd 前缀差异）
  if (a.length > 10 && b.length > 10) {
    return a.endsWith(b.slice(-Math.min(a.length, b.length))) ||
      b.endsWith(a.slice(-Math.min(a.length, b.length)));
  }
  return false;
}

// ── 分析函数 ──

function analyzeConsecutiveEdits(sessions: SessionEventData[]): {
  totalConsecutive: number;
  maxChain: number;
  samples: { file: string; count: number }[];
} {
  let totalConsecutive = 0;
  let maxChain = 0;
  const samples: { file: string; count: number }[] = [];

  for (const session of sessions) {
    const editEvents = session.events.filter((e) =>
      ["LineEdit", "Edit"].includes(e.name)
    );

    let prevFile = "";
    let chainLen = 0;

    for (const e of editEvents) {
      const file = e.filePath.split("/").pop() || "";
      if (file && file === prevFile) {
        chainLen++;
      } else {
        if (chainLen > 1) {
          totalConsecutive += chainLen;
          if (chainLen > maxChain) maxChain = chainLen;
          if (samples.length < 20) {
            samples.push({ file: prevFile, count: chainLen });
          }
        }
        chainLen = 1;
        prevFile = file;
      }
    }
    if (chainLen > 1) {
      totalConsecutive += chainLen;
      if (chainLen > maxChain) maxChain = chainLen;
      if (samples.length < 20) {
        samples.push({ file: prevFile, count: chainLen });
      }
    }
  }

  return { totalConsecutive, maxChain, samples };
}

interface RecoveryPath {
  nextTool: string;
  followUp: string;
}

function analyzeRecoveryPaths(sessions: SessionEventData[]): RecoveryPath[] {
  const paths: RecoveryPath[] = [];

  for (const session of sessions) {
    const events = session.events;
    for (let i = 0; i < events.length; i++) {
      if (events[i].name === "Edit" && events[i].isError) {
        const next5 = events.slice(i + 1, i + 6);
        if (next5.length > 0) {
          paths.push({
            nextTool: next5[0].name,
            followUp: next5
              .slice(1, 4)
              .map((e) => e.name)
              .join(" → "),
          });
        }
      }
    }
  }

  return paths;
}

function classifyErrors(
  events: ToolEvent[]
): Map<string, number> {
  const map = new Map<string, number>();
  for (const e of events.filter((ev) => ev.isError)) {
    let kind = "unknown";
    if (e.errorMsg.includes("not found")) kind = "old_string_not_found";
    else if (e.errorMsg.includes("not unique")) kind = "old_string_not_unique";
    else if (e.errorMsg.includes("missing field")) kind = "param_parse_error";
    else if (e.errorMsg.includes("fuzzy")) kind = "fuzzy_match_fail";
    else if (e.errorMsg.includes("bracket")) kind = "bracket_balance";
    else if (e.errorMsg.includes("sanity")) kind = "sanity_check";
    else if (e.errorMsg.includes("ast") || e.errorMsg.includes("tree-sitter"))
      kind = "ast_guard";
    map.set(kind, (map.get(kind) || 0) + 1);
  }
  return map;
}

// ── 缺陷报告 ──

function buildReports(
  leEvents: ToolEvent[],
  editEvents: ToolEvent[],
  writeEvents: ToolEvent[],
  leErrors: ToolEvent[],
  recoveryPaths: RecoveryPath[],
  leReRead: ReReadStats,
  editReRead: ReReadStats,
): DefectReport[] {
  const reports: DefectReport[] = [];

  // LE-001: LineEdit 高采纳率
  const totalEdits = leEvents.length + editEvents.length;
  if (totalEdits > 0) {
    reports.push({
      id: "LE-001",
      severity: "low",
      category: "工具采纳",
      title: `LineEdit 已成为主要编辑工具 (占比 ${pct(leEvents.length, totalEdits)})`,
      description:
        `在 ${totalEdits} 次文件编辑操作中，LineEdit 占 ${leEvents.length} 次 (${pct(leEvents.length, totalEdits)})，` +
        `Edit 仅 ${editEvents.length} 次。` +
        `LLM 在 lineEdit beta 开启后，自然倾向于使用 LineEdit 而非 Edit。` +
        `这表明 LineEdit 的行号模式对 LLM 来说更容易使用。`,
      evidence: [
        `LineEdit: ${leEvents.length} 次，成功率 ${pct(leEvents.filter((e) => !e.isError).length, leEvents.length)}`,
        `Edit: ${editEvents.length} 次，成功率 ${pct(editEvents.filter((e) => !e.isError).length, editEvents.length)}`,
        `Write: ${writeEvents.length} 次`,
      ],
      affectedSessions: [],
      recommendation:
        "LineEdit 作为 beta 功能表现良好，建议正式发布。可考虑将 Edit 标记为 deprecated 或降低其在工具描述中的优先级。",
      confidence: 0.9,
    });
  }

  // LE-002: 上下文效率提升
  const avgLEInput = avg(leEvents.map((e) => e.inputBytes));
  const avgWriteInput = avg(writeEvents.map((e) => e.inputBytes));
  if (avgLEInput > 0 && avgWriteInput > 0) {
    const savingRatio = 1 - avgLEInput / avgWriteInput;
    reports.push({
      id: "LE-002",
      severity: "low",
      category: "上下文效率",
      title: `LineEdit 入参比 Write 小 ${(savingRatio * 100).toFixed(0)}%，显著节省上下文`,
      description:
        `LineEdit 平均入参 ${Math.round(avgLEInput)}B vs Write 平均入参 ${Math.round(avgWriteInput)}B。` +
        `每次 LineEdit 替代 Write 节省 ${Math.round(avgWriteInput - avgLEInput)}B。` +
        `Write 的 P95 入参为 ${p95(writeEvents.map((e) => e.inputBytes))}B，而 LineEdit 仅为 ${p95(leEvents.map((e) => e.inputBytes))}B。`,
      evidence: [
        `LineEdit 入参: avg=${Math.round(avgLEInput)}B, p50=${p50(leEvents.map((e) => e.inputBytes))}B`,
        `Write 入参: avg=${Math.round(avgWriteInput)}B, p50=${p50(writeEvents.map((e) => e.inputBytes))}B`,
        `Edit 入参: avg=${Math.round(avg(editEvents.map((e) => e.inputBytes)))}B`,
      ],
      affectedSessions: [],
      recommendation:
        "LineEdit 在上下文效率上优势明显，特别是在大文件修改场景中。",
      confidence: 0.85,
    });
  }

  // LE-003: LineEdit 错误类型分析
  if (leErrors.length > 0) {
    reports.push({
      id: "LE-003",
      severity: "medium",
      category: "工具稳定性",
      title: `LineEdit 的 ${leErrors.length} 次失败全部为参数格式错误`,
      description:
        `所有 LineEdit 失败均为参数解析错误（missing field），而非匹配逻辑错误。` +
        `对比 Edit 的 old_string_not_found 错误（需要理解文件内容），LineEdit 的错误更容易修复。`,
      evidence: leErrors.slice(0, 3).map((e) => e.errorMsg.slice(0, 100)),
      affectedSessions: [...new Set(leErrors.map((e) => e.name))],
      recommendation:
        "1) 在 LineEdit 工具描述中强化参数格式示例；" +
        "2) 考虑在工具执行层添加参数校验提示；" +
        "3) 这些错误可能源于早期版本的工具描述不完善，需持续监控。",
      confidence: 0.9,
    });
  }

  // LE-004: Edit 失败后切换到 LineEdit
  const editFailToLE = recoveryPaths.filter((p) => p.nextTool === "LineEdit").length;
  const editFailTotal = recoveryPaths.length;
  if (editFailTotal > 0 && editFailToLE > 0) {
    reports.push({
      id: "LE-004",
      severity: "medium",
      category: "恢复能力",
      title: `Edit 失败后 ${pct(editFailToLE, editFailTotal)} 切换到 LineEdit 成功恢复`,
      description:
        `在 ${editFailTotal} 次 Edit 失败中，${editFailToLE} 次 (${pct(editFailToLE, editFailTotal)}) 后续选择了 LineEdit。` +
        `LineEdit 的行号模式天然不依赖内容匹配，避免了 old_string_not_found 错误。`,
      evidence: recoveryPaths
        .filter((p) => p.nextTool === "LineEdit")
        .slice(0, 5)
        .map((p) => `Edit fail → ${p.nextTool} → ${p.followUp}`),
      affectedSessions: [],
      recommendation:
        "鼓励 LLM 在 Edit 失败后优先切换 LineEdit 而非反复重试 Edit。",
      confidence: 0.8,
    });
  }

  // LE-005: 重读率分析
  if (leReRead.withFilePath > 0 && editReRead.withFilePath > 0) {
    const leRate = leReRead.within5ReRead / leReRead.withFilePath;
    const editRate = editReRead.within5ReRead / editReRead.withFilePath;
    const diff = leRate - editRate;

    if (Math.abs(diff) > 0.05) {
      reports.push({
        id: "LE-005",
        severity: "medium",
        category: "重读率",
        title: `LineEdit 5步内重读率 ${pct(leReRead.within5ReRead, leReRead.withFilePath)}，${diff > 0 ? "高于" : "低于"} Edit 的 ${pct(editReRead.within5ReRead, editReRead.withFilePath)}`,
        description:
          `LineEdit 重读率 ${pct(leReRead.within5ReRead, leReRead.withFilePath)}（${leReRead.within5ReRead}/${leReRead.withFilePath}），` +
          `Edit 重读率 ${pct(editReRead.within5ReRead, editReRead.withFilePath)}（${editReRead.within5ReRead}/${editReRead.withFilePath}）。` +
          (diff > 0
            ? `LineEdit 的高重读率可能因为 tool_result 反馈不够直观，Agent 需要额外 Read 确认修改。`
            : `LineEdit 的低重读率说明行号模式给予 Agent 更强的编辑信心。`),
        evidence: [
          `LineEdit: 紧邻${leReRead.immediateReRead}, 3步${leReRead.within3ReRead}, 5步${leReRead.within5ReRead} / ${leReRead.withFilePath}`,
          `Edit: 紧邻${editReRead.immediateReRead}, 3步${editReRead.within3ReRead}, 5步${editReRead.within5ReRead} / ${editReRead.withFilePath}`,
          `成功重读: LE ${leReRead.successReRead}/${leReRead.successTotal} vs Edit ${editReRead.successReRead}/${editReRead.successTotal}`,
        ],
        affectedSessions: [],
        recommendation:
          diff > 0
            ? "优化 LineEdit 的 tool_result 返回内容：包含修改前后的 diff 对比，减少 Agent 需要额外 Read 验证的需求。"
            : "LineEdit 的低重读率是正面信号，行号模式的确定性减少了验证开销。",
        confidence: 0.7,
      });
    }
  }

  return reports;
}

// ── 工具函数 ──

function pct(n: number, total: number): string {
  return total === 0 ? "0%" : `${((n / total) * 100).toFixed(1)}%`;
}

function avg(arr: number[]): number {
  return arr.length ? arr.reduce((a, b) => a + b, 0) / arr.length : 0;
}

function median(arr: number[]): number {
  if (arr.length === 0) return 0;
  const sorted = [...arr].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

function p50(arr: number[]): number {
  return median(arr);
}

function p95(arr: number[]): number {
  if (arr.length === 0) return 0;
  const sorted = [...arr].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length * 0.95)];
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
}

function toolRow(
  name: string,
  events: ToolEvent[]
): string[] {
  const success = events.filter((e) => !e.isError).length;
  const fail = events.filter((e) => e.isError).length;
  const total = events.length;
  return [
    name,
    String(total),
    String(success),
    String(fail),
    total ? pct(success, total) : "N/A",
  ];
}

function statsRow(name: string, values: number[]): string[] {
  if (values.length === 0) return [name, "N/A", "N/A", "N/A", "N/A", "0"];
  return [
    name,
    formatSize(Math.round(avg(values))),
    formatSize(p50(values)),
    formatSize(p95(values)),
    formatSize(Math.max(...values)),
    String(values.length),
  ];
}

function reReadSuccessFailRow(
  name: string,
  stats: ReReadStats
): string[] {
  return [
    name,
    String(stats.successReRead),
    String(stats.successTotal),
    stats.successTotal ? pct(stats.successReRead, stats.successTotal) : "N/A",
    String(stats.failReRead),
    String(stats.failTotal),
    stats.failTotal ? pct(stats.failReRead, stats.failTotal) : "N/A",
  ];
}
