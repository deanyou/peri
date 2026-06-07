# LineEdit：基于行号的新编辑工具，替换 Edit + HashlineEdit

**状态**：Open
**优先级**：高
**创建日期**：2026-06-05
**类型**：Design

## 背景

现有编辑工具存在根本性设计缺陷：

### Edit（old_string 模型）的问题

1. **old_string 是错误的抽象**：Agent 的真实意图是"改第 N 行"，被迫表达为"找到这段文字"——行级操作伪装成内容搜索
2. **精确匹配脆弱**：LLM 生成的 old_string 有天然微小差异（空格、tab、尾随换行），失败率 4.6%（284/6233）
3. **唯一性约束浪费 token**：同样的代码出现多次时，Agent 必须扩大 old_string 到 20+ 行才唯一
4. **Write 后必须 Read**：不知道当前内容就无法构造 old_string

### HashlineEdit（hash + 行号模型）的问题

1. **hash 是全文件级校验，粒度太粗**：文件第 100 行变了，第 5 行的安全编辑被拒绝
2. **hash 不可靠**：Agent 猜哈希必然失败，Write 后拿不到哈希，被迫 Read 中转
3. **hash 没解决 Edit 的核心问题**：Edit 失败主因是 LLM 构造的 old_string 有微小差异，不是"文件变了"

### 共同问题

- Write 后都需要额外 Read 才能编辑
- 无法与 Grep 高效联动

## 方案：LineEdit 工具

**核心思路**：用行号定位替换内容搜索，用 `start_word`/`end_word` 提供行内精度，不依赖 hash。

### 参数 schema

```json
{
  "type": "object",
  "properties": {
    "edits": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "file_path": { "type": "string", "description": "绝对路径" },
          "start_line": { "type": "integer", "description": "起始行号（从 Read 输出获取）" },
          "end_line": { "type": "integer", "description": "可选，终止行号，默认 = start_line" },
          "start_word": { "type": "string", "description": "可选，start_line 内的起始定位词" },
          "end_word": { "type": "string", "description": "可选，end_line 内的终止定位词" },
          "new_string": { "type": "string", "description": "替换内容" },
          "insert": { "type": "boolean", "description": "可选，true = 在 start_line 前插入" }
        },
        "required": ["file_path", "start_line", "new_string"]
      }
    }
  },
  "required": ["edits"]
}
```

### 语义规则

| 场景 | 参数 | 行为 |
|------|------|------|
| 替换单行 | `start_line: 5, new_string: "xxx"` | 替换第 5 行全部 |
| 替换多行 | `start_line: 5, end_line: 8, new_string: "xxx"` | 替换第 5-8 行 |
| 行内替换 | + `start_word: "fn"` + `end_word: ")"` | 只替换 start_word 到 end_word 之间的内容 |
| 插入 | `insert: true` | 在 start_line 前插入，原行不动 |
| 删除 | `new_string: ""` | 删除目标范围 |

### 行内定位（start_word / end_word）

- 可选参数，不提供则从行首开始 / 到行尾结束
- **必须在该行内唯一**，不唯一则报错并告知匹配次数，要求 Agent 提供更长前缀
- Agent 看到的是 Read 输出中的行号，行号是天然锚点，无需列号

### 多编辑策略

- **同文件**：从最大行号往最小行号应用，避免前面的编辑破坏后面的行号
- **跨文件**：按文件分组，组内从后往前，不同文件互不影响
- **失败策略**：best-effort，成功报告 + 失败报错，Agent 重新 Read 后重试失败的编辑

### 工具联动：Grep → LineEdit

Grep 输出天然带行号（`file.rs:42: matching content`），Agent 直接拿行号构造 LineEdit 批量编辑，取代旧 Edit 的 `replace_all` 场景，且更强大（Grep 支持正则）。

### Beta 开关

`settings.json` → `config.betas.lineEdit: true` 时替换 Edit 工具。不开启则保留旧 Edit（old_string 模型）。

## 清理工作

- Revert `HashlineEdit` 全部代码（`peri-middlewares/src/tools/hashline/`）
- 删除 `hashline` beta 配置
- 移除 Read 输出中的 `¶path#HASH` 哈希锚点

## 涉及文件

- `peri-middlewares/src/tools/filesystem/edit.rs` — 现有 Edit 工具
- `peri-middlewares/src/tools/hashline/` — HashlineEdit 全部代码（待 revert）
- `peri-middlewares/src/tools/filesystem/mod.rs` — 工具注册
- `peri-middlewares/src/middleware/filesystem.rs` — 文件系统中间件（工具选择逻辑）
- `peri-tui/prompts/sections/` — system prompt 段落（工具描述更新）
- `peri-tui/src/app/edit_utils.rs` — TUI 编辑工具辅助

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-05 | — | Open | agent | 实测 HashlineEdit 发现 hash 问题，grill-me 后重新设计为 LineEdit |
