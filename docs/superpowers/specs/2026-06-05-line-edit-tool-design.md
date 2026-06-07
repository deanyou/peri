# LineEdit 工具设计

**日期**：2026-06-05
**状态**：Draft

## 问题

现有两个编辑工具都有根本性缺陷：

**Edit（old_string 模型）**：
- Agent 想说"改第 N 行"，被迫表达为"找到这段文字"——行级操作伪装成内容搜索
- LLM 生成的 old_string 有天然微小差异（空格、tab、尾随换行），失败率 4.6%（284/6233）
- 唯一性约束逼 Agent 扩大 old_string 到 20+ 行，浪费 token
- Write 后必须 Read 才能构造 old_string

**HashlineEdit（hash + 行号模型）**：
- hash 是全文件级校验，粒度太粗——第 100 行变了，第 5 行的安全编辑被拒绝
- Write 后拿不到 hash，被迫 Read 中转，或凭空捏造 hash 导致双重报错
- hash 没解决 Edit 的核心问题——Edit 失败主因是 LLM 构造差异，不是"文件变了"

## 设计

### 核心思路

用行号定位替换内容搜索，用 `start_word`/`end_word` 提供行内精度，不依赖 hash。

### 参数

```json
{
  "type": "object",
  "properties": {
    "edits": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "file_path": { "type": "string" },
          "start_line": { "type": "integer" },
          "end_line": { "type": "integer" },
          "start_word": { "type": "string" },
          "end_word": { "type": "string" },
          "new_string": { "type": "string" },
          "insert": { "type": "boolean" }
        },
        "required": ["file_path", "start_line", "new_string"]
      }
    }
  },
  "required": ["edits"]
}
```

### 语义规则

| 插入 | `insert: true` | 在 start_line 前插入，原行不动。`end_line`/`start_word`/`end_word` 忽略 |
|------|------|------|
| 替换单行 | `start_line: 5` | 替换第 5 行全部，`end_line` 默认 = `start_line` |
| 替换多行 | `start_line: 5, end_line: 8` | 替换第 5-8 行 |
| 行内替换 | + `start_word` + `end_word` | 只替换 start_word 到 end_word 之间的内容，行首行尾保留 |
| 插入 | `insert: true` | 在 start_line 前插入，原行不动。`end_line`/`end_word` 忽略 |
| 删除 | `new_string: ""` | 删除目标范围 |

### 行内定位：start_word / end_word

- **可选**。不提供则从行首开始 / 到行尾结束。
- **必须在该行内唯一**。不唯一则报错，告知匹配次数，要求 Agent 提供更长前缀重试。
- 不使用列号——列号对 LLM 不直观，start_word 是语义化的。
- 示例：
  ```
  Line 42: pub async fn handle_request(&self, req: Request, config: &Config) -> Result {
  
  start_line: 42, start_word: "req:", end_word: "Config)"
  → 只替换 "req:" 到 "Config)" 之间，行首 "pub async fn handle_request(&self, " 和行尾 ") → Result {" 保留
  ```

### 多编辑策略

**为什么需要多编辑**：Agent 修改同一文件多处时，如果并发调用多个单编辑，前面的编辑会改变行号，后面的行号失效。单次调用多编辑是解决同文件并发编辑的正确机制。

**应用顺序**：
1. 按 `file_path` 分组
2. 组内按 `start_line` 降序排列（从最大行号往最小行号）
3. 从后往前依次应用——前面的行号不受影响

**失败策略**：best-effort。
- 成功的编辑正常应用并报告
- 失败的编辑报错（行号越界、start_word/end_word 不唯一等），Agent 重新 Read 后重试
- 同文件中从后往前应用时，如果前面某个编辑失败，后面的已经写入。但失败编辑在更小行号，已成功的在更大行号，Agent 只需重试失败的部分

**跨文件**：不同文件互不影响，组内各自从后往前。

### 与 Grep 联动

Grep 输出天然带行号（`file.rs:42: matching content`），Agent 直接拿行号构造 LineEdit 批量编辑。

这取代了旧 Edit 的 `replace_all` 场景，且更强大：
- 旧方式：`old_string + replace_all`（精确字符串，无正则）
- 新方式：`Grep(正则) → 拿行号 → LineEdit 批量`

### Beta 开关

`settings.json` → `config.betas.lineEdit: true` 时 Edit 工具替换为 LineEdit。不开启则保留旧 Edit。

## 清理工作

- Revert `HashlineEdit` 全部代码（`peri-middlewares/src/tools/hashline/`）
- 删除 `hashline` beta 配置项
- 移除 Read 输出中的 `¶path#HASH` 哈希锚点（hashline beta 专属功能）
- 更新 system prompt 中的工具描述

## 架构

### 模块位置

新增 `peri-middlewares/src/tools/filesystem/line_edit.rs`，与现有 `edit.rs` 平行。

### 注册逻辑

在 `mod.rs` 的工具注册处，根据 `betas.lineEdit` 选择注册 `EditFileTool` 或 `LineEditTool`。与当前 `hashline` beta 的注册方式一致。

### 原子写入

复用现有 `atomic_write` 模式（临时文件 + rename）。

## 错误处理

| 错误场景 | 行为 |
|----------|------|
| `start_line` 超出文件行数 | 报错，告知文件总行数 |
| `end_line < start_line`（非 insert 模式） | 报错，提示检查参数 |
| `start_word` 在行内不匹配 | 报错，提示该行实际内容 |
| `start_word` 在行内匹配多处 | 报错，告知匹配次数，要求加长 |
| `end_word` 同理 | 同上 |
| 文件不存在 | 报错 |
| `new_string` 与原内容相同 | 正常执行（幂等） |

## 涉及文件

| 文件 | 变更 |
|------|------|
| `peri-middlewares/src/tools/filesystem/line_edit.rs` | 新增 LineEdit 实现 |
| `peri-middlewares/src/tools/filesystem/edit.rs` | 保留，beta 关闭时使用 |
| `peri-middlewares/src/tools/filesystem/mod.rs` | 注册逻辑分支 |
| `peri-middlewares/src/tools/hashline/` | 整个目录 revert |
| `peri-middlewares/src/middleware/filesystem.rs` | 工具选择逻辑 |
| `peri-tui/prompts/sections/` | 工具描述更新 |
| `peri-tui/src/app/edit_utils.rs` | TUI 编辑辅助 |
| `spec/issues/2026-06-05-hashline-edit-missing-hash-after-write.md` | 关联 issue |

## 关联 Issue

- `spec/issues/2026-06-05-hashline-edit-missing-hash-after-write.md`
- `spec/issues/2026-06-03-edit-tool-errors-invisible-and-retry-inefficient.md`（Edit 错误率分析的原始 issue）
- `spec/issues/2026-06-03-edit-tool-tab-indent-mismatch.md`（Tab 缩进匹配失败，LineEdit 用行号规避）