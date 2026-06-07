# 统一输入框为 TextArea 变体

**日期**：2026-06-07
**状态**：Approved

## 问题

TUI 中存在三种输入实现，能力参差不齐：

| 类型 | 数量 | 能力 |
|------|------|------|
| `TextArea` (tui_textarea) | 1 | 多行、粘贴、选中、CJK、删词 |
| `String + usize` + `handle_edit_key()` | 19+ | 单字符插入/删除/光标，无粘贴 |
| `InputState` (peri-widgets) | 3 | 单字符插入/删除/光标，有 masked，有 paste |

`String + usize` 方案反复出 bug（空格键被截获、CJK 光标偏移、无法粘贴），维护成本高。

## 方案

引入 `FieldTextarea`——基于 `tui_textarea::TextArea` 的配置化包装器，替代所有 `String + usize` 和 `InputState` 用法。

### FieldTextarea

```rust
// 位置：peri-tui/src/app/field_textarea.rs（新文件）

pub struct FieldTextarea {
    inner: TextArea<'static>,
    max_lines: u16,  // single_line 时 = 1
}
```

#### 构造方法

| 方法 | 用途 | max_lines | 边框 | 换行 |
|------|------|-----------|------|------|
| `single_line()` | API Key、搜索、表单字段 | 1 | 无 | Enter 不换行（由调用方处理） |
| `multi_line(max)` | AskUser 自定义输入、未来多行场景 | max | 可选 | Enter 换行 |

#### 核心方法

| 方法 | 说明 |
|------|------|
| `input(key: Input) -> bool` | 委托 `inner.input(key)` |
| `value() -> String` | 合并所有行（`\n` 连接） |
| `set_value(s: &str)` | 按行拆分加载到 textarea |
| `is_empty() -> bool` | 内容为空 |
| `render_height() -> u16` | 当前实际视觉行数，clamp 到 `[1, max_lines]` |
| `render(f, area)` | 渲染到指定区域 |

#### 渲染样式

- `single_line()`：无边框、无 padding、行内渲染。通过 `set_block(Block::default().borders(Borders::NONE))` 和 `set_cursor_line_style(Style::default())` 实现
- `multi_line()`：可选边框，由调用方通过 `with_border_style(style)` 配置

#### 粘贴处理

`TextArea` 原生支持 `Event::Paste`，不需要额外处理。但当前各面板的粘贴事件是在 `event/mod.rs` 中统一拦截的——单行模式下 paste 插入多行时，`value()` 只返回第一行（或用空格连接）。`FieldTextarea` 不做截断，由调用方决定是否允许换行。

### 迁移映射

| 现有字段 | 文件 | 替换为 |
|----------|------|--------|
| 主输入框 `UiState.textarea` | `ui_state.rs` | 不变 |
| AskUser `custom_input + custom_cursor` | `ask_user_prompt.rs` | `FieldTextarea::multi_line(5)` |
| Config `buf_threshold + cur_threshold` | `config_panel.rs` | `FieldTextarea::single_line()` |
| Config `buf_persona + cur_persona` | `config_panel.rs` | `FieldTextarea::single_line()` |
| Config `buf_tone + cur_tone` | `config_panel.rs` | `FieldTextarea::single_line()` |
| Login 6 个 buf_* + cur_* 字段 | `login_panel/mod.rs` | 各一个 `FieldTextarea::single_line()` |
| OAuth `buf_callback + cur_callback` | `oauth_prompt.rs` | `FieldTextarea::single_line()` |
| Setup Wizard 9+ 个 buf_* + cur_* 字段 | `setup_wizard/mod.rs` | 各一个 `FieldTextarea::single_line()` |
| Plugin discover_search `InputState` | `plugin_panel/types.rs` | `FieldTextarea::single_line()` |
| Plugin add_marketplace_input `InputState` | `plugin_panel/types.rs` | `FieldTextarea::single_line()` |
| Thread browser search `InputState` | `thread/browser.rs` | `FieldTextarea::single_line()` |

### 涉及文件

| 文件 | 操作 | 说明 |
|------|------|------|
| `peri-tui/src/app/field_textarea.rs` | 新建 | `FieldTextarea` 定义 |
| `peri-tui/src/app/mod.rs` | 修改 | 添加 `mod field_textarea` |
| `peri-tui/src/app/ask_user_prompt.rs` | 修改 | `custom_input/custom_cursor` → `FieldTextarea` |
| `peri-tui/src/app/ask_user_ops.rs` | 修改 | `ask_user_edit_key` 改为 `textarea.input()` |
| `peri-tui/src/ui/main_ui/popups/ask_user.rs` | 修改 | 自定义输入行渲染改为 `textarea.render()` |
| `peri-tui/src/app/config_panel.rs` | 修改 | 3 个 buf+cur → `FieldTextarea` |
| `peri-tui/src/app/login_panel/mod.rs` | 修改 | 6 个 buf+cur → `FieldTextarea` |
| `peri-tui/src/app/oauth_prompt.rs` | 修改 | 1 个 buf+cur → `FieldTextarea` |
| `peri-tui/src/app/setup_wizard/mod.rs` | 修改 | 9+ 个 buf+cur → `FieldTextarea` |
| `peri-tui/src/app/plugin_panel/types.rs` | 修改 | 2 个 `InputState` → `FieldTextarea` |
| `peri-tui/src/app/plugin_panel/` 渲染文件 | 修改 | `InputField` → `FieldTextarea` 渲染 |
| `peri-tui/src/app/thread/browser.rs` | 修改 | 1 个 `InputState` → `FieldTextarea` |
| `peri-tui/src/app/edit_utils.rs` | 修改 | 删除 `handle_edit_key()` + `edit_display_parts()` |
| `peri-tui/src/event/keyboard/popups.rs` | 修改 | AskUser 键盘处理委托 textarea |
| `peri-tui/src/event/` 各键盘处理文件 | 修改 | 各面板输入处理改为 `textarea.input()` |

### 不动的部分

- **选项列表渲染**（AskUser 的 1-N 选项行、Config 的 toggle/select 行）保持手拼 `Line<Span>`
- **`InputField` widget + `InputState`**（peri-widgets）保留定义，但 TUI 侧不再使用。不删除——其他消费者可能用到
- **主输入框** `UiState.textarea` 不变，本身就是 `TextArea`

### 渲染适配

当前表单面板（Config/Login/Setup）的渲染模式是「拼接 Span 组装 `Line`」，例如：

```rust
// 当前：手拼 Span
Line::from(vec![
    Span::styled(label, label_style),
    Span::styled(format!("{}█", buf), value_style),
])
```

替换为 TextArea 后，每个字段需要独立分配一个 1 行高的 `Rect`，然后 `f.render_widget(textarea, rect)`。这意味着面板渲染需要从「逐行拼 Span」改为「先布局再渲染」。这个改动在 Login/Setup 等已有固定行号布局的面板中比较直接——每行预分配一个 rect 即可。

### 实施顺序

1. **新建 `FieldTextarea`**——`single_line()` + `multi_line(max)` + 核心方法
2. **迁移 AskUser**——最复杂的场景（multi_line + 弹窗高度动态计算），验证 TextArea 在弹窗中能正常工作
3. **迁移 Config**——3 个字段，验证表单场景的渲染适配
4. **迁移 Login/OAuth/Setup**——批量替换，模式相同
5. **迁移 Plugin/Thread 搜索**——替换 `InputState`，验证搜索过滤
6. **清理**——删除 `handle_edit_key()`、`edit_display_parts()`、各面板的 `cur_*` cursor 字段

### 风险

| 风险 | 影响 | 缓解 |
|------|------|------|
| `TextArea` 实例比裸 `String` 重 | 20 个额外实例，每个约几百字节 | 可忽略 |
| 表单渲染从 Span 拼接改为 rect 分配 | Login/Setup 渲染逻辑需重写 | 逐面板迁移，不影响其他面板 |
| 搜索场景的 `InputState` 有 `masked` 功能 | TextArea 无原生 masked | 搜索场景不需要 masked（API Key 在 Login 中，不是搜索） |
| Tab/Enter 等键被 TextArea 截获 | TextArea 默认消费 Tab 和 Enter | `single_line()` 模式下通过 `input()` 返回值判断，未被消费的键由面板处理 |
