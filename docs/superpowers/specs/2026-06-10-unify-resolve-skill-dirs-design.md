# SkillPreloadMiddleware 统一 resolve_dirs 设计

## 背景

`SkillPreloadMiddleware` 无法加载插件提供的 skill 全文（issue `2026-06-10-skill-preload-cannot-load-plugin-skills`）。根因：`resolve_dirs()` 硬编码 3 个搜索路径，不包含插件 skill 目录。而 `SkillsMiddleware` 通过 `with_extra_dirs()` 已支持插件目录，两者不一致。

本次修复同时消除 `resolve_dirs` 逻辑重复：将搜索路径解析抽取为公共函数，两个中间件共享。

## 设计

### 1. 公共函数 `resolve_skill_dirs`

位置：`peri-middlewares/src/skills/loader.rs`

```rust
/// 统一的 skill 搜索目录解析
/// 优先级：~/.claude/skills → globalConfig skillsDir → ./.claude/skills → extra_dirs
pub fn resolve_skill_dirs(cwd: &str, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let user_dir = dirs_next::home_dir()
        .map(|h| h.join(".claude").join("skills"))
        .unwrap_or_default();
    let global_dir = crate::skills::load_global_skills_dir();
    let project_dir = PathBuf::from(cwd).join(".claude").join("skills");

    let mut dirs = vec![user_dir];
    if let Some(global) = global_dir {
        dirs.push(global);
    }
    dirs.push(project_dir);
    for dir in extra_dirs {
        if dir.is_dir() {
            dirs.push(dir.clone());
        }
    }
    dirs
}
```

`load_global_skills_dir()` 保持原位（`skills/mod.rs`），通过 `crate::skills::load_global_skills_dir()` 引用。无需移动。

### 2. SkillsMiddleware 委托

`SkillsMiddleware` 的 `resolve_dirs_static()` 委托公共函数：

```rust
pub fn resolve_dirs_static(cwd: &str, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    loader::resolve_skill_dirs(cwd, extra_dirs)
}
```

`resolve_dirs()` 有 override 字段时走自己的逻辑（测试隔离），无 override 时委托公共函数。

### 3. SkillPreloadMiddleware 改造

- 新增 `extra_dirs: Vec<PathBuf>` 字段 + `with_extra_dirs()` builder
- 删除 `resolve_dirs()` 方法
- `before_agent` 中直接调用 `crate::skills::loader::resolve_skill_dirs(&self.cwd, &self.extra_dirs)`
- 现有测试不需要改（`extra_dirs` 默认空，行为不变）

### 4. 主 Agent 调用点

```rust
// peri-acp/src/agent/builder.rs:417
.add_middleware(Box::new(
    SkillPreloadMiddleware::new(preload_skills, &cwd)
        .with_extra_dirs(plugin_skill_dirs.clone())
))
```

### 不改范围

- **SubAgent 路径**：SubAgent 的 `SkillsMiddleware` 也没有 `plugin_skill_dirs`（`build_subagent_middlewares` 中只有 `.with_global_config()`），SubAgent 无法发现插件 skill，preload 无意义。SubAgent 插件 skill 支持应作为独立 issue 整体解决（发现 + 预加载一起做）
- **SkillsMiddleware override 字段**：`with_user_dir()`/`with_global_dir()`/`with_project_dir()` 保留（测试使用）
- **现有测试**：不改动

## 涉及文件

| 文件 | 改动类型 |
|------|----------|
| `peri-middlewares/src/skills/loader.rs` | 新增 `resolve_skill_dirs()` |
| `peri-middlewares/src/skills/mod.rs` | `resolve_dirs_static()` 委托公共函数 |
| `peri-middlewares/src/subagent/skill_preload.rs` | 加 `extra_dirs` + `with_extra_dirs()`，删除 `resolve_dirs()`，委托公共函数 |
| `peri-acp/src/agent/builder.rs` | 一行传参 |

## 测试

- 现有测试不受影响（`extra_dirs` 默认空）
- 可选：新增测试验证 `resolve_skill_dirs()` 包含 extra_dirs 路径
- 可选：新增测试验证 `SkillPreloadMiddleware::with_extra_dirs()` 能从插件目录加载 skill
