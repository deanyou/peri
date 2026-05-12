# Mac 上 Option+Backspace 在有可滚动内容时触发滚动而非删除整行

**状态**：Open (搁置)
**优先级**：中
**创建日期**：2026-05-12
**修复日期**：2026-05-12

## 问题描述

在 Mac 平台使用 VS Code 集成终端时，当消息区域有可滚动内容时，按 Option+Backspace（应删除整行）会触发消息区域向上滚动，而非删除输入框中的整行文字。当消息区域没有可滚动内容时，该组合键能正常删除整行。

## 症状详情

| 条件 | Option+Backspace 行为 |
|------|----------------------|
| 消息区域无内容/不可滚动 | ✅ 正常删除整行 |
| 消息区域有可滚动内容 | ❌ 消息区域向上滚动 |

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 在 Mac 上使用 VS Code 集成终端启动 rust-agent-tui
  2. 与 agent 进行几轮对话，使消息区域出现可滚动内容
  3. 在输入框中输入一些文字（多行）
  4. 按 Option+Backspace
  5. 观察到消息区域向上滚动，而非删除整行

- **环境**：
  - OS：macOS
  - 终端：VS Code 集成终端
  - 按键组合：Option+Backspace（Meta+Backspace）

## 根因分析

1. **终端层**：VS Code 终端在 Mac 上将 Option+Backspace 映射为 PageUp 转义序列（这是 VS Code 终端的已知行为）
2. **crossterm 层**：crossterm 将转义序列解释为 `Key::PageUp`
3. **事件处理层**：`rust-agent-tui/src/event.rs` 中，`PageUp` 被直接拦截用于滚动，没有先让 textarea 处理

```rust
// rust-agent-tui/src/event.rs:809-815
Input {
    key: Key::PageUp, ..
} => {
    for _ in 0..10 {
        app.scroll_up();
    }
}
```

当消息区域有可滚动内容时，`app.scroll_up()` 生效；当没有可滚动内容时，滚动操作无效果，所以 Option+Backspace 看起来"正常工作"。

## 相关代码

- `rust-agent-tui/src/event.rs:809-822` —— PageUp/PageDown 事件处理
- `rust-agent-tui/src/event.rs:824-842` —— Ctrl+U/Ctrl+D 半页滚动处理

## 外部依赖问题

- [crossterm-rs/crossterm#575](https://github.com/crossterm-rs/crossterm/issues/575) —— macOS, backspace, and modifiers
- [crossterm-rs/crossterm#504](https://github.com/crossterm-rs/crossterm/issues/504) —— Backspace with modifiers not recognised as backspace
- [microsoft/vscode#83453](https://github.com/microsoft/vscode/issues/83453) —— (Terminal) Option+delete doesn't delete previous word on MacOS

## 可能的解决方案

1. **优先让 textarea 处理**：在 `PageUp`/`PageDown` 事件处理前，先检查 textarea 是否有内容需要处理，只有当 textarea 不消费该事件时才传递给滚动处理器
2. **区分真正的 PageUp**：检测按键是否来自物理 PageUp 键（而非 Option+Backspace 的映射），但这在终端层难以实现
3. **用户配置**：允许用户禁用 Option+Backspace → PageUp 的映射（需要终端支持）

## 修复方案

**采用方案 1**：移除 `PageUp`/`PageDown` 的无条件拦截，让这些按键通过默认分支传递给 textarea 处理。

**修改文件**：`rust-agent-tui/src/event.rs`

**修改内容**：删除了 812-825 行的 `PageUp`/`PageDown` 事件处理器，并添加注释说明：

```rust
// Ctrl+U / Ctrl+D：半页滚动（无需 PageUp/PageDown 物理键，MacBook 友好）
// 注意：PageUp/PageDown 不再拦截，留给 textarea 处理其内部视口滚动。
// 这样可修复 VS Code 终端中 Option+Backspace（映射为 PageUp）触发消息滚动的问题。
```

**原理**：
- Option+Backspace 在 VS Code 终端中被映射为 PageUp 转义序列
- 移除拦截后，PageUp 事件会传递给 textarea
- textarea 的默认行为是滚动其内部视口（tui-textarea 的 PageUp 处理）
- 用户仍可使用 Ctrl+U/Ctrl+D 进行消息区域的半页滚动

**副作用**：物理 PageUp/PageDown 键不再滚动消息区域，改为滚动 textarea 内部视口。对于没有物理 PageUp/PageDown 键的 MacBook 用户，这实际上是一种改进。
