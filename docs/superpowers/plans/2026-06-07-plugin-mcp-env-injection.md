# 插件 MCP 子进程注入 CLAUDE_PLUGIN_ROOT/DATA 环境变量

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 插件 MCP 子进程启动时自动注入 `CLAUDE_PLUGIN_ROOT` 和 `CLAUDE_PLUGIN_DATA` 环境变量，使依赖这些变量的插件 MCP 启动脚本（如 Hindsight 的 `run_mcp.sh`）能正常工作。

**Architecture:** 在 `load_merged_config_full` Step 2 展开插件 MCP config 时，将 `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` 追加到 `McpServerConfig.env` 字段。这样已有的 `spawn_stdio_transport` 中的 `cmd.envs(env)` 调用就能自动注入这些变量，无需修改 spawn 逻辑或函数签名。

**Tech Stack:** Rust 2021, peri-middlewares crate, tempfile (测试)

---

## File Structure

| 文件 | 职责 | 操作 |
|------|------|------|
| `peri-middlewares/src/mcp/config.rs:310-324` | `load_merged_config_full` Step 2：展开后追加 env | 修改 |
| `peri-middlewares/src/mcp/config_test.rs` | 验证 env 注入的单元测试 | 修改 |

---

### Task 1: 在 load_merged_config_full Step 2 追加插件环境变量到 env 字段

**Files:**
- Modify: `peri-middlewares/src/mcp/config.rs:310-324`

**背景：** 当前 Step 2 在 L311-324 遍历插件的 MCP servers，调用 `expand_server_config_with_context` 展开 args 中的 `${CLAUDE_PLUGIN_ROOT}` 占位符，然后将展开后的 config 存入 `plugin_servers`。但展开后的 config 的 `env` 字段没有追加 `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA`。子进程脚本内部（如 `run_mcp.sh` L6-L8）再次读取这些环境变量时得到空值。

**修复点：** 在 `plugin_servers.insert(namespaced.clone(), expanded_cfg);` 之前，向 `expanded_cfg.env` 注入两个键值对。

- [ ] **Step 1: 修改 load_merged_config_full Step 2，在展开后注入环境变量**

在 `peri-middlewares/src/mcp/config.rs` 中，将 L317-324 替换为：

```rust
            // 每插件独立上下文展开：在合并之前即完成 env 变量替换
            let mut expanded_cfg = expand_server_config_with_context(
                &cfg,
                Some(&plugin.install_path),
                Some(&plugin.data_path),
                None,
            );
            // 注入 CLAUDE_PLUGIN_ROOT 和 CLAUDE_PLUGIN_DATA 到子进程环境变量
            // 插件 MCP 启动脚本（如 run_mcp.sh）依赖这些变量定位 venv/resources
            let env = expanded_cfg.env.get_or_insert_with(HashMap::new);
            env.insert(
                "CLAUDE_PLUGIN_ROOT".to_string(),
                plugin.install_path.to_string_lossy().to_string(),
            );
            env.insert(
                "CLAUDE_PLUGIN_DATA".to_string(),
                plugin.data_path.to_string_lossy().to_string(),
            );
            plugin_servers.insert(namespaced.clone(), expanded_cfg);
```

关键变更：
1. `let expanded_cfg` 从不可变绑定改为 `let mut expanded_cfg`
2. 新增 `get_or_insert_with(HashMap::new)` 初始化 env 字段（如果 .mcp.json 没有 env 字段）
3. 追加两个键值对

- [ ] **Step 2: 编译验证**

Run: `cargo build -p peri-middlewares 2>&1 | tail -5`
Expected: 编译成功，无错误

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/mcp/config.rs
git commit -m "fix(mcp): inject CLAUDE_PLUGIN_ROOT/DATA env vars into plugin MCP subprocess"
```

---

### Task 2: 添加单元测试验证 env 注入

**Files:**
- Modify: `peri-middlewares/src/mcp/config_test.rs`

- [ ] **Step 1: 编写测试验证插件 MCP config 的 env 字段包含注入变量**

在 `peri-middlewares/src/mcp/config_test.rs` 的 `test_load_merged_config_full_with_plugin` 浽数中（约 L420），在 `let (_config, plugin_sources) = load_merged_config_full(&cwd, &claude_home);` 之后添加断言：

```rust
    let (config, plugin_sources) = load_merged_config_full(&cwd, &claude_home);
    // 验证 env 注入
    let srv_config = config.mcp_servers.get("plugin:p1:srv1")
        .expect("应有 plugin:p1:srv1 服务器");
    let env = srv_config.env.as_ref()
        .expect("插件 MCP server 应有 env 字段（自动注入）");
    assert_eq!(
        env.get("CLAUDE_PLUGIN_ROOT").unwrap(),
        &plugin_dir.to_string_lossy().to_string(),
        "CLAUDE_PLUGIN_ROOT 应为插件安装路径"
    );
    let expected_data = plugin_dir.join(".claude-plugin").join("data")
        .to_string_lossy().to_string();
    assert_eq!(
        env.get("CLAUDE_PLUGIN_DATA").unwrap(),
        &expected_data,
        "CLAUDE_PLUGIN_DATA 应为插件数据路径"
    );
    // 原有断言保持不变
    assert!(
        plugin_sources.contains_key("plugin:p1:srv1"),
        "plugin_sources should contain plugin:p1:srv1, got: {:?}",
        plugin_sources
    );
```

注意：需要将 `let (_config, plugin_sources)` 改为 `let (config, plugin_sources)`（去掉下划线），因为现在需要读取 config。

- [ ] **Step 2: 新增独立测试验证带自定义 env 的插件不被覆盖**

在文件末尾添加新测试，验证插件 .mcp.json 中已有自定义 env 字段时，注入不会覆盖已有值：

```rust
#[test]
fn test_load_merged_config_full_plugin_env_injection_preserves_existing() {
    use crate::plugin::types::{InstallScope, InstalledPlugin, InstalledPlugins};
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    let claude_home = dir.path().join(".claude-test");
    std::fs::create_dir_all(&claude_home).unwrap();

    // 创建插件目录和 plugin.json（含 MCP server + 自定义 env）
    let plugin_dir = claude_home
        .join("plugins")
        .join("cache")
        .join("mkt")
        .join("p2")
        .join("1.0.0");
    std::fs::create_dir_all(plugin_dir.join(".claude-plugin")).unwrap();
    std::fs::write(
        plugin_dir.join(".claude-plugin").join("plugin.json"),
        r#"{
            "name":"p2",
            "version":"1.0.0",
            "mcpServers":{
                "srv2":{
                    "command":"node",
                    "args":["server.js"],
                    "env":{"MY_VAR":"my_value","CLAUDE_PLUGIN_ROOT":"should_not_override"}
                }
            }
        }"#,
    )
    .unwrap();

    // 创建 installed_plugins.json
    std::fs::create_dir_all(claude_home.join("plugins")).unwrap();
    let installed = InstalledPlugins {
        version: 2,
        plugins: vec![InstalledPlugin {
            id: "p2@mkt".into(),
            name: "p2".into(),
            version: "1.0.0".into(),
            marketplace: "mkt".into(),
            install_path: plugin_dir.clone(),
            scope: InstallScope::User,
            project_path: None,
        }],
    };
    std::fs::write(
        claude_home.join("plugins").join("installed_plugins.json"),
        serde_json::to_string(&installed).unwrap(),
    )
    .unwrap();

    // 创建 settings.json 启用插件
    std::fs::write(
        claude_home.join("settings.json"),
        r#"{"enabledPlugins":["p2@mkt"]}"#,
    )
    .unwrap();

    let (config, _plugin_sources) = load_merged_config_full(&cwd, &claude_home);
    let srv_config = config.mcp_servers.get("plugin:p2:srv2")
        .expect("应有 plugin:p2:srv2 服务器");
    let env = srv_config.env.as_ref().expect("应有 env 字段");
    // 自定义 env 应保留
    assert_eq!(env.get("MY_VAR").unwrap(), "my_value");
    // CLAUDE_PLUGIN_ROOT 应被注入为实际路径（insert 会覆盖用户 .mcp.json 中的值）
    // 这是有意行为：运行时值优先于静态配置中的占位符
    assert_eq!(
        env.get("CLAUDE_PLUGIN_ROOT").unwrap(),
        &plugin_dir.to_string_lossy().to_string()
    );
    // CLAUDE_PLUGIN_DATA 应也被注入
    assert!(env.contains_key("CLAUDE_PLUGIN_DATA"));
}
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p peri-middlewares --lib -- mcp::config::tests 2>&1 | tail -20`
Expected: 所有测试通过，包括新增和已有的 `test_load_merged_config_full_*` 测试

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/mcp/config_test.rs
git commit -m "test(mcp): verify CLAUDE_PLUGIN_ROOT/DATA injection in plugin MCP config"
```

---

### Task 3: 端到端验证

- [ ] **Step 1: 运行全量测试确认无回归**

Run: `cargo test -p peri-middlewares 2>&1 | tail -10`
Expected: 所有测试通过

- [ ] **Step 2: 启动 TUI 验证 Hindsight MCP 连接**

Run: `cargo run -p peri-tui`

验证步骤：
1. 观察 `.tmp/agent-tui.log` 中不再出现 `MCP 连接失败 server=plugin:hindsight-memory:hindsight` 错误
2. 状态栏 MCP 状态应显示 Ready（包含 hindsight 服务器）
3. 在 SearchExtraTools 中能搜索到 `mcp__hindsight__*` 系列工具

注意：此步骤需要 Hindsight daemon 已正确配置 LLM provider（`OPENAI_API_KEY` 或 `ANTHROPIC_API_KEY`），否则 daemon 启动仍会失败，但失败原因不再是环境变量缺失。

---

## Self-Review

**1. Spec coverage:** Issue 要求插件 MCP 子进程获得 `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` 环境变量 → Task 1 实现注入逻辑，Task 2 验证正确性。覆盖完整。

**2. Placeholder scan:** 无 TBD/TODO，所有代码步骤包含完整实现代码。无占位符。

**3. Type consistency:** `McpServerConfig.env` 类型为 `Option<HashMap<String, String>>`，`get_or_insert_with(HashMap::new)` 返回 `&mut HashMap<String, String>`，`insert(key: String, value: String)` 签名匹配。`plugin.install_path` 和 `plugin.data_path` 均为 `PathBuf`，`to_string_lossy().to_string()` 产出 `String`。类型一致。
