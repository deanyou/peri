# 统一输入框为 TextArea 变体 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 TUI 中所有 19+ 个 `String + usize cursor` 手写输入和 3 个 `InputState` 替换为基于 `tui_textarea::TextArea` 的统一 `FieldTextarea` 组件。

**Architecture:** 引入 `FieldTextarea` 包装器（`single_line()` / `multi_line(max)`），提供 `value()` / `set_value()` / `input()` / `render()` 统一接口。各面板字段从 `buf: String + cur: usize` 对替换为单个 `FieldTextarea`。渲染从手拼 Span 改为 `f.render_widget(textarea, rect)`。键盘处理从 `handle_edit_key()` 改为 `textarea.input()`。

**Tech Stack:** Rust, tui_textarea (已有依赖), ratatui, peri-tui

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `peri-tui/src/app/field_textarea.rs` | Create | `FieldTextarea` 定义：构造、input、value、set_value、render_height、render |
| `peri-tui/src/app/field_textarea_test.rs` | Create | 单元测试 |
| `peri-tui/src/app/mod.rs` | Modify | 添加 `mod field_textarea` |
| `peri-tui/src/app/edit_utils.rs` | Modify | 删除 `handle_edit_key()` 和 `edit_display_parts()` |
| `peri-tui/src/app/ask_user_prompt.rs` | Modify | `custom_input/custom_cursor` → `FieldTextarea` |
| `peri-tui/src/app/ask_user_ops.rs` | Modify | `ask_user_edit_key` 委托 textarea |
| `peri-tui/src/ui/main_ui/popups/ask_user.rs` | Modify | 自定义输入行渲染 |
| `peri-tui/src/app/config_panel.rs` | Modify | 3 个 `buf+cur` → `FieldTextarea` |
| `peri-tui/src/ui/main_ui/panels/config.rs` | Modify | Config 渲染 |
| `peri-tui/src/app/login_panel/mod.rs` | Modify | 6 个 `buf+cur` → `FieldTextarea` |
| `peri-tui/src/app/login_panel/component.rs` | Modify | Login 键盘处理 |
| `peri-tui/src/ui/main_ui/panels/login.rs` | Modify | Login 渲染 |
| `peri-tui/src/app/oauth_prompt.rs` | Modify | `input/cursor` → `FieldTextarea` |
| `peri-tui/src/ui/main_ui/popups/oauth.rs` | Modify | OAuth 渲染 |
| `peri-tui/src/app/setup_wizard/mod.rs` | Modify | `MigratedProvider` 的 `buf+cur` → `FieldTextarea` |
| `peri-tui/src/app/setup_wizard/ops.rs` | Modify | Setup 键盘处理 |
| `peri-tui/src/ui/main_ui/popups/setup_wizard.rs` | Modify | Setup 渲染 |
| `peri-tui/src/app/plugin_panel/types.rs` | Modify | 2 个 `InputState` → `FieldTextarea` |
| `peri-tui/src/app/plugin_panel/mod.rs` | Modify | Plugin 粘贴处理 |
| `peri-tui/src/app/plugin_panel/handlers/plugin_handlers/discover_search.rs` | Modify | Discover 搜索键盘 |
| `peri-tui/src/app/plugin_panel/handlers/plugin_handlers/discover_list.rs` | Modify | Discover 列表键盘 |
| `peri-tui/src/app/plugin_panel/handlers/plugin_handlers/marketplace.rs` | Modify | Marketplace 键盘 |
| `peri-tui/src/ui/main_ui/panels/plugin/plugin_render/discover_search.rs` | Modify | Discover 搜索渲染 |
| `peri-tui/src/ui/main_ui/panels/plugin/plugin_render/add_marketplace.rs` | Modify | Marketplace 添加渲染 |
| `peri-tui/src/thread/browser.rs` | Modify | `search_query: InputState` → `FieldTextarea` |

---

### Task 1: 创建 `FieldTextarea` 组件

**Files:**
- Create: `peri-tui/src/app/field_textarea.rs`
- Create: `peri-tui/src/app/field_textarea_test.rs`
- Modify: `peri-tui/src/app/mod.rs`

- [ ] **Step 1: 创建 `field_textarea.rs`**

```rust
use ratatui::{layout::Rect, style::Style, Frame};
use tui_textarea::TextArea;

use crate::ui::theme;

/// 基于 TextArea 的统一输入字段包装器。
///
/// 替代手写的 `String + usize cursor` + `handle_edit_key()` 模式。
/// 通过 `single_line()` / `multi_line(max)` 控制行为。
pub struct FieldTextarea {
    inner: TextArea<'static>,
    max_lines: u16,
}

impl Clone for FieldTextarea {
    fn clone(&self) -> Self {
        let lines = self.inner.lines().to_vec();
        let mut new = if self.max_lines == 1 {
            Self::single_line()
        } else {
            Self::multi_line(self.max_lines)
        };
        new.set_value(&lines.join("\n"));
        new
    }
}

impl FieldTextarea {
    /// 单行输入框——用于 API Key、搜索、表单字段等
    pub fn single_line() -> Self {
        let mut inner = TextArea::default();
        // 无边框、无 padding，行内渲染
        inner.set_block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::NONE));
        inner.set_cursor_line_style(Style::default());
        inner.set_style(Style::default().fg(theme::TEXT));
        Self { inner, max_lines: 1 }
    }

    /// 多行输入框——max_lines 控制最大可视行数
    pub fn multi_line(max_lines: u16) -> Self {
        let mut inner = TextArea::default();
        inner.set_block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::NONE));
        inner.set_cursor_line_style(Style::default());
        inner.set_style(Style::default().fg(theme::TEXT));
        Self {
            inner,
            max_lines: max_lines.max(1),
        }
    }

    /// 处理输入按键，返回 true 表示已消费
    pub fn input(&mut self, key: tui_textarea::Input) -> bool {
        self.inner.input(key)
    }

    /// 获取当前文本内容（多行用 \n 连接）
    pub fn value(&self) -> String {
        self.inner.lines().join("\n")
    }

    /// 获取单行值（不含换行）。适用于 single_line 模式。
    pub fn single_line_value(&self) -> String {
        self.inner.lines().join(" ")
    }

    /// 设置文本值
    pub fn set_value(&mut self, s: &str) {
        // TextArea 没有 set_text，需要重新构造
        let max = self.max_lines;
        let new_inner = if max == 1 {
            let mut ta = TextArea::default();
            ta.set_block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::NONE));
            ta.set_cursor_line_style(Style::default());
            ta.set_style(Style::default().fg(theme::TEXT));
            ta
        } else {
            let mut ta = TextArea::default();
            ta.set_block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::NONE));
            ta.set_cursor_line_style(Style::default());
            ta.set_style(Style::default().fg(theme::TEXT));
            ta
        };
        self.inner = new_inner;
        if !s.is_empty() {
            // 逐字符插入（TextArea 没有批量设置 API）
            // 但可以按行加载
            for line in s.lines() {
                for ch in line.chars() {
                    self.inner.input(tui_textarea::Input {
                        key: tui_textarea::Key::Char(ch),
                        ctrl: false,
                        alt: false,
                        shift: false,
                    });
                }
            }
            // 在行间插入换行
            // 更好的方式：直接操作 lines
        }
        self.move_cursor_end();
    }

    /// 内容是否为空
    pub fn is_empty(&self) -> bool {
        self.inner.lines().iter().all(|l| l.is_empty())
    }

    /// 当前渲染所需高度（1 到 max_lines）
    pub fn render_height(&self) -> u16 {
        let line_count = self.inner.lines().len() as u16;
        line_count.clamp(1, self.max_lines)
    }

    /// 光标移到末尾
    pub fn move_cursor_end(&mut self) {
        self.inner.move_cursor(tui_textarea::CursorMove::End);
    }

    /// 光标移到开头
    pub fn move_cursor_home(&mut self) {
        self.inner.move_cursor(tui_textarea::CursorMove::Head);
    }

    /// 清空内容
    pub fn clear(&mut self) {
        self.inner.delete_str_by_end();
        // 更彻底：重建空 textarea
        let max = self.max_lines;
        self.inner = if max == 1 {
            let mut ta = TextArea::default();
            ta.set_block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::NONE));
            ta.set_cursor_line_style(Style::default());
            ta.set_style(Style::default().fg(theme::TEXT));
            ta
        } else {
            let mut ta = TextArea::default();
            ta.set_block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::NONE));
            ta.set_cursor_line_style(Style::default());
            ta.set_style(Style::default().fg(theme::TEXT));
            ta
        };
    }

    /// 渲染到指定区域
    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        f.render_widget(&self.inner, area);
    }

    /// 内部 TextArea 引用（高级用法，如自定义样式）
    pub fn inner_mut(&mut self) -> &mut TextArea<'static> {
        &mut self.inner
    }
}
```

注意：上面的 `set_value` 逐字符插入效率低。TextArea 支持 `delete_str_by_end()` + `insert_char()` 但没有批量设置。**更好的方案**——利用 TextArea 内部用 `Vec<String>` 管理行：直接用 `TextArea::new(lines)` 构造：

```rust
pub fn set_value(&mut self, s: &str) {
    let lines: Vec<String> = if s.is_empty() {
        vec![String::new()]
    } else {
        s.split('\n').map(|l| l.to_string()).collect()
    };
    let mut new_inner = TextArea::new(lines);
    new_inner.set_block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::NONE));
    new_inner.set_cursor_line_style(Style::default());
    new_inner.set_style(Style::default().fg(theme::TEXT));
    // 光标移到末尾
    new_inner.move_cursor(tui_textarea::CursorMove::End);
    self.inner = new_inner;
}
```

最终 `field_textarea.rs` 采用 `TextArea::new()` 构造方案。

- [ ] **Step 2: 注册模块**

在 `peri-tui/src/app/mod.rs` 中添加：

```rust
mod field_textarea;
pub use field_textarea::FieldTextarea;
```

- [ ] **Step 3: 编写测试**

创建 `peri-tui/src/app/field_textarea_test.rs`：

```rust
use super::field_textarea::FieldTextarea;

#[test]
fn test_single_line_input_char() {
    let mut ta = FieldTextarea::single_line();
    ta.input(tui_textarea::Input {
        key: tui_textarea::Key::Char('a'),
        ctrl: false,
        alt: false,
        shift: false,
    });
    assert_eq!(ta.value(), "a");
}

#[test]
fn test_single_line_backspace() {
    let mut ta = FieldTextarea::single_line();
    ta.set_value("abc");
    ta.input(tui_textarea::Input {
        key: tui_textarea::Key::Backspace,
        ctrl: false,
        alt: false,
        shift: false,
    });
    assert_eq!(ta.value(), "ab");
}

#[test]
fn test_set_value() {
    let mut ta = FieldTextarea::single_line();
    ta.set_value("hello");
    assert_eq!(ta.value(), "hello");
    ta.set_value("");
    assert!(ta.is_empty());
}

#[test]
fn test_multi_line_render_height() {
    let mut ta = FieldTextarea::multi_line(5);
    assert_eq!(ta.render_height(), 1);
    ta.set_value("line1\nline2\nline3");
    assert_eq!(ta.render_height(), 3);
}

#[test]
fn test_multi_line_clamp_height() {
    let mut ta = FieldTextarea::multi_line(3);
    ta.set_value("a\nb\nc\nd\ne");
    assert_eq!(ta.render_height(), 3); // clamp 到 max_lines
}

#[test]
fn test_clear() {
    let mut ta = FieldTextarea::single_line();
    ta.set_value("content");
    ta.clear();
    assert!(ta.is_empty());
}

#[test]
fn test_cursor_position_after_set_value() {
    let mut ta = FieldTextarea::single_line();
    ta.set_value("abc");
    // 光标应在末尾，输入新字符追加到末尾
    ta.input(tui_textarea::Input {
        key: tui_textarea::Key::Char('d'),
        ctrl: false,
        alt: false,
        shift: false,
    });
    assert_eq!(ta.value(), "abcd");
}

#[test]
fn test_clone() {
    let mut ta = FieldTextarea::single_line();
    ta.set_value("hello");
    let cloned = ta.clone();
    assert_eq!(cloned.value(), "hello");
}
```

- [ ] **Step 4: 运行测试验证**

Run: `cargo test -p peri-tui --lib -- field_textarea`
Expected: 8 tests PASS

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/app/field_textarea.rs peri-tui/src/app/field_textarea_test.rs peri-tui/src/app/mod.rs
git commit -m "feat(tui): add FieldTextarea — unified input component based on tui_textarea"
```

---

### Task 2: 迁移 AskUser 自定义输入

最复杂的场景：multi_line + 弹窗高度动态计算。验证 TextArea 在弹窗中能正常工作。

**Files:**
- Modify: `peri-tui/src/app/ask_user_prompt.rs`
- Modify: `peri-tui/src/app/ask_user_ops.rs`
- Modify: `peri-tui/src/ui/main_ui/popups/ask_user.rs`
- Modify: `peri-tui/src/event/keyboard/popups.rs`

- [ ] **Step 1: 修改 `QuestionState`**

在 `peri-tui/src/app/ask_user_prompt.rs` 中，将 `custom_input: String` + `custom_cursor: usize` 替换为 `FieldTextarea`：

```rust
use crate::app::FieldTextarea;

pub struct QuestionState {
    pub data: AskUserQuestionData,
    pub option_cursor: isize,
    pub selected: Vec<bool>,
    // 替换 custom_input + custom_cursor
    pub custom_input: FieldTextarea,
    pub in_custom_input: bool,
}
```

`QuestionState::new` 修改：

```rust
pub(super) fn new(data: AskUserQuestionData) -> Self {
    let len = data.options.len();
    Self {
        data,
        option_cursor: 0,
        selected: vec![false; len],
        custom_input: FieldTextarea::multi_line(5),
        in_custom_input: false,
    }
}
```

`answer()` 修改——`q.custom_input.trim()` 改为 `q.custom_input.value().trim().to_string()`：

```rust
pub fn answer(&self) -> String {
    let mut parts: Vec<String> = self
        .selected
        .iter()
        .enumerate()
        .filter(|(_, &v)| v)
        .map(|(i, _)| self.data.options[i].label.clone())
        .collect();
    let custom = self.custom_input.value().trim().to_string();
    if !custom.is_empty() {
        parts.push(custom);
    }
    if parts.is_empty() {
        self.custom_input.value().trim().to_string()
    } else {
        parts.join(", ")
    }
}
```

删除 `push_char` 和 `pop_char` 方法（不再需要）。

- [ ] **Step 2: 修改 `ask_user_ops.rs`**

`ask_user_edit_key` 方法从调用 `handle_edit_key()` 改为 `textarea.input()`：

```rust
pub fn ask_user_edit_key(&mut self, input: tui_textarea::Input) {
    if let Some(InteractionPrompt::Questions(p)) = self
        .session_mgr
        .current_mut()
        .agent
        .interaction_prompt
        .as_mut()
    {
        let q = p.current();
        if q.in_custom_input {
            q.custom_input.input(input);
        }
    }
}
```

`ask_user_confirm` 中的 `q.custom_input.trim()` 改为 `q.custom_input.value().trim()`。

- [ ] **Step 3: 修改 `popups.rs` 键盘处理**

在 `peri-tui/src/event/keyboard/popups.rs` 中，Space 键处理已改为同时调用 `ask_user_edit_key` 和 `ask_user_toggle`（之前的修复保持不变）。无需额外修改——`ask_user_edit_key` 内部已改为委托 textarea。

- [ ] **Step 4: 修改 AskUser 渲染**

在 `peri-tui/src/ui/main_ui/popups/ask_user.rs` 中，自定义输入行从手拼 `█` 光标改为渲染 textarea：

将当前的自定义输入渲染块（约 148-173 行）替换为：

```rust
// 自定义输入前加空行分隔
lines.push(Line::from(""));

// 自定义输入作为最后一个编号选项——渲染 textarea
{
    let custom_num = option_count + 1;
    let is_cursor = cur.in_custom_input;
    let cursor_mark = if is_cursor { "❯" } else { " " };
    let style = if is_cursor {
        Style::default()
            .fg(theme::THINKING)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::MUTED)
    };

    // label 行
    lines.push(Line::from(vec![
        Span::styled(format!("{} {}. ", cursor_mark, custom_num), style),
    ]));
}
```

然后在 `ScrollableArea::render` 之后，追加 textarea 的渲染。这需要计算 textarea 在 content_area 中的位置。

**关键改动**：不再把 textarea 内容加入 `lines` Vec，而是在 `ScrollableArea` 渲染后，单独 `f.render_widget(textarea, textarea_area)` 分配一个独立 rect。

textarea 区域高度 = `cur.custom_input.render_height()`。位置 = content_area 底部（自定义输入行的位置）。

由于 textarea 需要独立 rect，渲染逻辑需要重构为两段式：
1. 选项列表 + label 行 → `ScrollableArea`
2. textarea → `f.render_widget`

实际上更简单的做法：**把 textarea 渲染为 1 行高的内联 widget**，依然放在 `lines` 布局中。TextArea 支持无边框单行渲染——通过 `f.render_widget(&textarea, single_line_rect)`。

具体实现：记录 textarea 在 lines 中的行号偏移，渲染完 ScrollableArea 后，在对应位置 overlay textarea widget。

```rust
// 在构建 lines 时，自定义输入行记录占位行号
let textarea_line_start = lines.len() as u16;

// ... 添加 label 占位行
lines.push(Line::from(vec![
    Span::styled(format!("{} {}. ", cursor_mark, custom_num), style),
]));

// ScrollableArea 渲染
let mut scroll_state = ScrollState::with_offset(prompt.scroll_offset);
let metrics = ScrollableArea::new(Text::from(lines))
    .scrollbar_style(Style::default().fg(theme::MUTED))
    .render(f, content_area, &mut scroll_state);

// overlay textarea：在 content_area 内的 textarea 位置
if cur.in_custom_input || !cur.custom_input.is_empty() {
    let textarea_y = content_area.y + textarea_line_start.saturating_sub(prompt.scroll_offset);
    let textarea_area = Rect {
        x: content_area.x + 5, // "❯ N. " 前缀宽度
        y: textarea_y,
        width: content_area.width.saturating_sub(5),
        height: cur.custom_input.render_height(),
    };
    if textarea_area.y >= content_area.y
        && textarea_area.bottom() <= content_area.bottom()
    {
        let q = &mut prompt.questions[prompt.active_tab];
        q.custom_input.render(f, textarea_area);
    }
}
```

注意：这个 overlay 方案在滚动时需要考虑 textarea 是否在可见区域内。如果不可见则跳过渲染。

- [ ] **Step 5: 更新弹窗高度计算**

`active_panel_height` 中 AskUser 高度计算需要考虑 textarea 的动态行数。在 `render_ask_user_popup` 开头，将 textarea 高度加入高度估算：

```rust
let textarea_h = cur.custom_input.render_height();
// height_estimation 中自定义输入行从 1 行改为 textarea_h 行
```

- [ ] **Step 6: 构建验证**

Run: `cargo build -p peri-tui`
Expected: 编译成功

- [ ] **Step 7: 运行测试**

Run: `cargo test -p peri-tui --lib -- ask_user`
Expected: 所有测试 PASS

- [ ] **Step 8: Commit**

```bash
git add peri-tui/src/app/ask_user_prompt.rs peri-tui/src/app/ask_user_ops.rs peri-tui/src/ui/main_ui/popups/ask_user.rs
git commit -m "refactor(tui): migrate AskUser custom input to FieldTextarea"
```

---

### Task 3: 迁移 Config 面板

3 个字段，验证表单场景的渲染适配。

**Files:**
- Modify: `peri-tui/src/app/config_panel.rs`
- Modify: `peri-tui/src/ui/main_ui/panels/config.rs`

- [ ] **Step 1: 修改 `ConfigPanel` 结构体**

在 `peri-tui/src/app/config_panel.rs` 中，将 3 对 `buf+cur` 替换为 `FieldTextarea`：

```rust
use crate::app::FieldTextarea;

pub struct ConfigPanel {
    pub cursor: usize,
    pub buf_autocompact: bool,
    // 替换 buf_threshold + cur_threshold
    pub field_threshold: FieldTextarea,
    pub buf_language: String,
    // 替换 buf_persona + cur_persona
    pub field_persona: FieldTextarea,
    // 替换 buf_tone + cur_tone
    pub field_tone: FieldTextarea,
    pub buf_proactiveness: String,
    pub buf_diff: bool,
    pub buf_streaming: String,
}
```

`from_config` 中初始化：

```rust
field_threshold: FieldTextarea::single_line(), // 然后调用 set_value
field_persona: FieldTextarea::single_line(),
field_tone: FieldTextarea::single_line(),
```

添加辅助方法获取当前活跃字段的可变引用（替代 `is_text_row` + match 分发）：

```rust
pub fn active_field(&mut self) -> Option<&mut FieldTextarea> {
    match self.cursor {
        ROW_THRESHOLD => Some(&mut self.field_threshold),
        ROW_PERSONA => Some(&mut self.field_persona),
        ROW_TONE => Some(&mut self.field_tone),
        _ => None,
    }
}
```

`input_char` 和 `handle_text_key` 简化为：

```rust
fn input_char(&mut self, c: char) {
    if let Some(field) = self.active_field() {
        field.input(tui_textarea::Input {
            key: tui_textarea::Key::Char(c),
            ctrl: false, alt: false, shift: false,
        });
    }
}

fn handle_text_key(&mut self, input: tui_textarea::Input) {
    if let Some(field) = self.active_field() {
        field.input(input);
    }
}
```

`apply_edit` 中读取字段值：`self.field_threshold.value()` 替代 `self.buf_threshold.clone()`。

- [ ] **Step 2: 修改 Config 渲染**

在 `peri-tui/src/ui/main_ui/panels/config.rs` 中，文本行渲染从手拼 `█` 改为 overlay textarea。

当前模式（约 250-293 行）是拼 `Line<Span>` 统一渲染。替换方案：

**选项 A（推荐——最小改动）**：保持 `Line<Span>` 拼接布局，但在渲染后 overlay textarea widget 覆盖值区域。记录每行的 y 坐标，然后 `f.render_widget(textarea, value_rect)`。

```rust
ROW_THRESHOLD | ROW_PERSONA | ROW_TONE => {
    let is_active = panel.cursor == row;
    let field = match row {
        ROW_THRESHOLD => &panel.field_threshold,
        ROW_PERSONA => &panel.field_persona,
        ROW_TONE => &panel.field_tone,
        _ => unreachable!(),
    };

    // label 部分
    let value_text = if !is_active && field.is_empty() {
        "-".to_string()
    } else {
        field.value()
    };

    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(format!("{:<14}", lc.tr(field_label_key(row))), label_style),
        Span::styled(" ", Style::default()),
        Span::styled(value_text, value_style),
    ]));

    // 记录活跃字段的行号，稍后 overlay
    if is_active {
        active_textarea_overlay = Some((lines.len() as u16 - 1, row));
    }
}
```

渲染后 overlay：

```rust
// 在 ScrollableArea 渲染后
if let Some((line_idx, row)) = active_textarea_overlay {
    let y = content_area.y + line_idx.saturating_sub(scroll_offset);
    let label_width = 14 + 3; // "  " + label(14) + " "
    let value_area = Rect {
        x: area.x + label_width,
        y,
        width: area.width.saturating_sub(label_width as u16),
        height: 1,
    };
    if value_area.y >= content_area.y && value_area.bottom() <= content_area.bottom() {
        let field = match row {
            ROW_THRESHOLD => &mut panel.field_threshold,
            ROW_PERSONA => &mut panel.field_persona,
            ROW_TONE => &mut panel.field_tone,
            _ => unreachable!(),
        };
        field.render(f, value_area);
    }
}
```

- [ ] **Step 3: 构建验证**

Run: `cargo build -p peri-tui`
Expected: 编译成功

- [ ] **Step 4: Commit**

```bash
git add peri-tui/src/app/config_panel.rs peri-tui/src/ui/main_ui/panels/config.rs
git commit -m "refactor(tui): migrate Config panel inputs to FieldTextarea"
```

---

### Task 4: 迁移 Login 面板 + OAuth 弹窗

Login 有 6 个字段，OAuth 有 1 个。模式相同。

**Files:**
- Modify: `peri-tui/src/app/login_panel/mod.rs`
- Modify: `peri-tui/src/app/login_panel/component.rs`
- Modify: `peri-tui/src/ui/main_ui/panels/login.rs`
- Modify: `peri-tui/src/app/oauth_prompt.rs`
- Modify: `peri-tui/src/ui/main_ui/popups/oauth.rs`

- [ ] **Step 1: 修改 `LoginPanel` 结构体**

在 `peri-tui/src/app/login_panel/mod.rs` 中，将 6 对 `buf+cur` 替换为 `FieldTextarea`：

```rust
pub struct LoginPanel {
    // ... 其他字段保持不变
    // 替换 buf_name + cur_name 等
    pub field_name: FieldTextarea,
    pub buf_type: String,  // Type 不编辑，保持
    pub field_base_url: FieldTextarea,
    pub field_api_key: FieldTextarea,
    pub field_opus_model: FieldTextarea,
    pub field_sonnet_model: FieldTextarea,
    pub field_haiku_model: FieldTextarea,
}
```

`active_field()` 改为返回 `Option<&mut FieldTextarea>`：

```rust
pub fn active_field(&mut self) -> Option<&mut FieldTextarea> {
    match self.edit_field {
        LoginEditField::Name => Some(&mut self.field_name),
        LoginEditField::Type => None,
        LoginEditField::BaseUrl => Some(&mut self.field_base_url),
        LoginEditField::ApiKey => Some(&mut self.field_api_key),
        LoginEditField::OpusModel => Some(&mut self.field_opus_model),
        LoginEditField::SonnetModel => Some(&mut self.field_sonnet_model),
        LoginEditField::HaikuModel => Some(&mut self.field_haiku_model),
    }
}
```

- [ ] **Step 2: 修改 Login 键盘处理**

`component.rs` 中的键盘处理从 `handle_edit_key(buf, cursor, input)` 改为 `field.input(input)`：

```rust
// 简化所有 handle_edit_key 调用
if let Some(field) = self.active_field() {
    field.input(input);
}
```

`apply_edit` / `to_provider_config` 中读取值：`self.field_api_key.value()` 替代 `self.buf_api_key.clone()`。

- [ ] **Step 3: 修改 Login 渲染**

`login.rs` 中字段渲染从手拼 `edit_display_parts` 改为 overlay textarea。同 Config 的 overlay 方案。

- [ ] **Step 4: 修改 `OAuthPrompt`**

`oauth_prompt.rs` 中 `input: String` + `cursor: usize` 替换为 `field: FieldTextarea`：

```rust
pub struct OAuthPrompt {
    pub server_name: String,
    pub authorization_url: String,
    pub field: FieldTextarea, // 替换 input + cursor
    pub callback_tx: Option<tokio::sync::oneshot::Sender<OAuthCallbackResult>>,
    pub error_message: Option<String>,
}
```

`oauth.rs` 渲染改为 overlay textarea。

键盘处理（在 `event/mod.rs` 的 OAuth 分支中）改为 `prompt.field.input(input)`。

- [ ] **Step 5: 构建验证**

Run: `cargo build -p peri-tui`
Expected: 编译成功

- [ ] **Step 6: Commit**

```bash
git add peri-tui/src/app/login_panel/ peri-tui/src/ui/main_ui/panels/login.rs peri-tui/src/app/oauth_prompt.rs peri-tui/src/ui/main_ui/popups/oauth.rs
git commit -m "refactor(tui): migrate Login panel and OAuth prompt to FieldTextarea"
```

---

### Task 5: 迁移 Setup Wizard

9+ 个字段，分布在 `MigratedProvider` 和 `AliasConfig` 中。

**Files:**
- Modify: `peri-tui/src/app/setup_wizard/mod.rs`
- Modify: `peri-tui/src/app/setup_wizard/ops.rs`
- Modify: `peri-tui/src/ui/main_ui/popups/setup_wizard.rs`

- [ ] **Step 1: 修改 `MigratedProvider` 和 `AliasConfig`**

在 `setup_wizard/mod.rs` 中：

```rust
pub struct MigratedProvider {
    pub provider_type: ProviderType,
    pub field_provider_id: FieldTextarea, // 替换 provider_id + cur_provider_id
    pub field_base_url: FieldTextarea,    // 替换 base_url + cur_base_url
    pub field_api_key: FieldTextarea,     // 替换 api_key + cur_api_key
    pub aliases: [AliasConfig; 3],
    pub selected: bool,
}

pub struct AliasConfig {
    pub field_model_id: FieldTextarea, // 替换 model_id + cursor
}
```

- [ ] **Step 2: 修改 `provider_field_buf`**

`ops.rs` 中的 `provider_field_buf` 从 `Option<(&mut String, &mut usize)>` 改为 `Option<&mut FieldTextarea>`：

```rust
fn provider_field_buf(
    mp: &mut MigratedProvider,
    field: FormField,
) -> Option<&mut FieldTextarea> {
    match field {
        FormField::ProviderId => Some(&mut mp.field_provider_id),
        FormField::BaseUrl => Some(&mut mp.field_base_url),
        FormField::ApiKey => Some(&mut mp.field_api_key),
        FormField::OpusModel => Some(&mut mp.aliases[0].field_model_id),
        FormField::SonnetModel => Some(&mut mp.aliases[1].field_model_id),
        FormField::HaikuModel => Some(&mut mp.aliases[2].field_model_id),
        _ => None,
    }
}
```

所有调用点从 `handle_edit_key(buf, cursor, input)` 改为 `field.input(input)`。

- [ ] **Step 3: 修改 Setup 渲染**

`setup_wizard.rs` 中 `edit_display_parts` 调用改为 overlay textarea。API Key 的 masked 显示需要在 textarea 上设置自定义样式覆盖——但 TextArea 没有原生 masked 支持。

**解决方案**：保留当前的手拼 `•` 显示逻辑用于 unfocused 状态，只在 focused/active 状态下 overlay textarea。这样 API Key 的 masked 行为不受影响。

- [ ] **Step 4: 构建验证**

Run: `cargo build -p peri-tui`
Expected: 编译成功

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/app/setup_wizard/ peri-tui/src/ui/main_ui/popups/setup_wizard.rs
git commit -m "refactor(tui): migrate Setup Wizard to FieldTextarea"
```

---

### Task 6: 迁移 Plugin 面板 + Thread 浏览器搜索

替换 3 个 `InputState` 用法。

**Files:**
- Modify: `peri-tui/src/app/plugin_panel/types.rs`
- Modify: `peri-tui/src/app/plugin_panel/mod.rs`
- Modify: `peri-tui/src/app/plugin_panel/handlers/plugin_handlers/discover_search.rs`
- Modify: `peri-tui/src/app/plugin_panel/handlers/plugin_handlers/discover_list.rs`
- Modify: `peri-tui/src/app/plugin_panel/handlers/plugin_handlers/marketplace.rs`
- Modify: `peri-tui/src/ui/main_ui/panels/plugin/plugin_render/discover_search.rs`
- Modify: `peri-tui/src/ui/main_ui/panels/plugin/plugin_render/add_marketplace.rs`
- Modify: `peri-tui/src/thread/browser.rs`

- [ ] **Step 1: 修改 `PluginPanel`**

在 `types.rs` 中，将 2 个 `InputState` 替换：

```rust
pub struct PluginPanel {
    // ...
    pub discover_search: FieldTextarea,      // 替换 InputState
    // ...
    pub add_marketplace_input: FieldTextarea, // 替换 InputState
    // ...
}
```

所有 `discover_search.insert(c)` 改为 `discover_search.input(Input { key: Key::Char(c), .. })`。
所有 `discover_search.backspace()` 改为 `discover_search.input(Input { key: Key::Backspace, .. })`。
所有 `discover_search.value()` 改为 `discover_search.value()`（同名，无需改）。

`discover_search.cursor_left()` 等改为对应的 `input(Input { key: Key::Left, .. })`。

- [ ] **Step 2: 修改 `ThreadBrowser`**

在 `browser.rs` 中，`search_query: InputState` 替换为 `search_query: FieldTextarea`。

所有 `search_query.insert(c)` / `search_query.backspace()` 等改为 `input()` 调用。

渲染中 `search_query.display_text('•')` 需要替换——FieldTextarea 没有 masked 显示。搜索框不需要 masked，直接用 `search_query.value()` 即可。

- [ ] **Step 3: 修改搜索渲染**

`discover_search.rs` 渲染从 `InputField.to_line()` 改为 overlay textarea。
`add_marketplace.rs` 同理。

- [ ] **Step 4: 构建验证**

Run: `cargo build -p peri-tui`
Expected: 编译成功

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/app/plugin_panel/ peri-tui/src/thread/browser.rs peri-tui/src/ui/main_ui/panels/plugin/plugin_render/
git commit -m "refactor(tui): migrate Plugin panel and Thread browser search to FieldTextarea"
```

---

### Task 7: 清理旧代码

删除不再使用的 `handle_edit_key()` 和 `edit_display_parts()`。

**Files:**
- Modify: `peri-tui/src/app/edit_utils.rs`

- [ ] **Step 1: 删除 `handle_edit_key` 和 `edit_display_parts`**

在 `edit_utils.rs` 中：
- 删除 `handle_edit_key()` 函数（约 27-164 行）
- 删除 `edit_display_parts()` 函数（约 168-174 行）
- 保留 `ensure_cursor_visible()`（仍在使用）
- 保留 `build_textarea()` 和 `build_textarea_with_hint()`（主输入框仍在使用）

- [ ] **Step 2: 清理所有 `use` 引用**

全局搜索 `handle_edit_key` 和 `edit_display_parts`，确保没有残留引用。

Run: `grep -r "handle_edit_key\|edit_display_parts" peri-tui/src/`
Expected: 无结果

- [ ] **Step 3: 全量构建 + 测试**

Run: `cargo build -p peri-tui && cargo test -p peri-tui --lib`
Expected: 编译成功，所有测试通过

- [ ] **Step 4: 全量 clippy**

Run: `cargo clippy -p peri-tui -- -D warnings`
Expected: 无 warning

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/app/edit_utils.rs
git commit -m "refactor(tui): remove handle_edit_key and edit_display_parts — replaced by FieldTextarea"
```

---

## Self-Review

### Spec Coverage

| Spec 要求 | 对应 Task |
|-----------|-----------|
| 引入 `FieldTextarea` 包装器 | Task 1 |
| `single_line()` / `multi_line(max)` | Task 1 |
| AskUser 自定义输入替换 | Task 2 |
| Config 3 个字段替换 | Task 3 |
| Login 6 个字段替换 | Task 4 |
| OAuth 1 个字段替换 | Task 4 |
| Setup 9+ 个字段替换 | Task 5 |
| Plugin 搜索 2 个 InputState 替换 | Task 6 |
| Thread 搜索 1 个 InputState 替换 | Task 6 |
| 删除 `handle_edit_key()` | Task 7 |
| 删除 `edit_display_parts()` | Task 7 |

### Placeholder Scan

无 TBD / TODO / "implement later"。

### Type Consistency

- `FieldTextarea` 在所有 Task 中统一使用 `single_line()` 或 `multi_line(max)`
- `value()` 返回 `String`，`input()` 接受 `tui_textarea::Input`，`render(f, area)` 接受 `&mut Frame, Rect`——所有 Task 一致
- `active_field()` 在 Config 返回 `Option<&mut FieldTextarea>`，在 Login 返回 `Option<&mut FieldTextarea>`，在 Setup 返回 `Option<&mut FieldTextarea>`——签名一致
