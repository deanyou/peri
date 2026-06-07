# /plugin 斜杠命令 Marketplace 子命令支持

**日期**: 2026-06-06
**关联 Issue**: spec/issues/2026-06-06-plugin-slash-command-marketplace-support.md

## 背景

`/plugin` 目前只打开面板，不支持子命令。CLI 已有 `plugin install name@marketplace`，Plugin Panel UI 已有 marketplace add/update/delete，斜杠命令路径缺失。

## 目标

支持三种新子命令，静默执行 + 系统提示反馈：

```
/plugin marketplace add <url>
/plugin install <name@marketplace>
/plugin marketplace update <name>
```

## 设计

### 子命令解析

`PluginCommand::execute(app, args)` 中按 whitespace 分词路由：

| args | 行为 |
|------|------|
| 空 | `app.open_plugin_panel()`（现有行为） |
| `marketplace add <url>` | 调用 `app.marketplace_add_and_save(&input)` |
| `install <name@marketplace>` | 调用 `app.plugin_install_by_marketplace(name, marketplace)` |
| `marketplace update <name>` | 调用 `app.marketplace_update_and_refresh(name)` |
| 其他 | `push_system_note` 显示用法提示 |

### Async 执行策略

沿用 `App::marketplace_add_and_save()` 的同步+异步分离模式：
- 同步阶段（`&mut App`）：解析、保存、push system note
- 异步阶段：clone `services.bg_event_tx` → `tokio::spawn` → 通过 channel 发 `PluginActionCompleted`

不使用 raw pointer 或 `unsafe`。

### 各子命令详情

**`/plugin marketplace add <url>`**

已有 `App::marketplace_add_and_save(&input)`，一行调用。该方法内部：
1. `parse_marketplace_input` 解析
2. 去重检查
3. `save_known_marketplaces` 持久化
4. `push_system_note` 反馈
5. `tokio::spawn` 后台 `refresh_marketplace` → 更新 install_location → 发 `PluginActionCompleted`

**`/plugin install <name@marketplace>`**

新增 `App::plugin_install_by_marketplace(name, marketplace)`：
1. 同步：从 `MarketplaceManager` 缓存查找插件 → 不在缓存则 push error
2. 同步：检查是否已安装 → 已安装则 push 提示
3. 同步：`push_system_note("正在安装 <name>@<marketplace> ...")`
4. `tokio::spawn { install_plugin(name, marketplace, User, ...).await }` → 发 `PluginActionCompleted`

**`/plugin marketplace update <name>`**

新增 `App::marketplace_update_and_refresh(name)`：
1. 同步：`load_known_marketplaces` 查找匹配 name → 找不到则 error
2. 同步：`push_system_note("正在刷新 marketplace <name> ...")`
3. `tokio::spawn { refresh_marketplace(&source, name).await }` → 更新元数据 → 发 `PluginActionCompleted`

### 错误处理

同步错误（解析失败、未找到）：`push_system_note` 显示错误。
异步错误：`PluginActionCompleted { success: false, message }` 事件，已有处理逻辑。

### 测试

- `PluginCommand` 参数路由：5 种分支全覆盖
- `plugin_install_by_marketplace`：正常/已安装/marketplace 不存在
- `marketplace_update_and_refresh`：匹配/不匹配

## 涉及文件

| 文件 | 改动 |
|------|------|
| `peri-tui/src/command/panel/plugin.rs` | 子命令解析逻辑 |
| `peri-tui/src/app/panel_plugin.rs` | 新增 2 个方法 |
| `peri-tui/src/command/panel/plugin_test.rs`（如超过 30 行）| 测试 |

无新增依赖。
