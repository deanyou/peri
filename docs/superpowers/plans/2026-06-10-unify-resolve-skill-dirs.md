# 统一 resolve_skill_dirs 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 skill 搜索目录解析逻辑抽取为公共函数，修复 SkillPreloadMiddleware 无法加载插件 skill 的 bug。

**Architecture:** 在 `skills/loader.rs` 新增 `resolve_skill_dirs()` 公共函数，`SkillsMiddleware` 和 `SkillPreloadMiddleware` 均委托此函数。主 Agent builder 传入 `plugin_skill_dirs`。SubAgent 路径不改。

**Tech Stack:** Rust，无新依赖

---

### Task 1: 公共函数 `resolve_skill_dirs`

**Files:**
- Modify: `peri-middlewares/src/skills/loader.rs`
- Test: `peri-middlewares/src/skills/loader_test.rs`

- [ ] **Step 1: 在 `loader.rs` 中新增 `resolve_skill_dirs` 函数**

在 `loader.rs` 文件末尾（`#[cfg(test)]` 之前）添加：

```rust
/// 统一的 skill 搜索目录解析
///
/// 优先级：~/.claude/skills → globalConfig skillsDir → ./.claude/skills → extra_dirs
/// 这是 skill 目录解析的 single source of truth，SkillsMiddleware 和 SkillPreloadMiddleware 都应委托此函数。
pub fn resolve_skill_dirs(cwd: &str, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let user_dir = dirs_next::home_dir()
        .map(|h| h.join(".claude").join("skills"))
        .unwrap_or_default();

    let global_dir = super::load_global_skills_dir();

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

- [ ] **Step 2: 在 `loader_test.rs` 末尾添加测试**

```rust
#[test]
fn test_resolve_skill_dirs_returns_standard_paths() {
    let cwd = "/tmp/test-project";
    let dirs = resolve_skill_dirs(cwd, &[]);
    // 应包含用户目录和项目目录
    assert!(dirs.iter().any(|d| d.ends_with(".claude/skills")), "应包含 ~/.claude/skills");
    assert!(dirs.iter().any(|d| d == &PathBuf::from("/tmp/test-project/.claude/skills")), "应包含项目 .claude/skills");
}

#[test]
fn test_resolve_skill_dirs_includes_extra_dirs() {
    let extra = tempfile::tempdir().unwrap();
    let dirs = resolve_skill_dirs("/tmp", &[extra.path().to_path_buf()]);
    assert!(dirs.contains(&extra.path().to_path_buf()), "应包含 extra_dirs 中的目录");
}

#[test]
fn test_resolve_skill_dirs_skips_nonexistent_extra_dirs() {
    let dirs = resolve_skill_dirs("/tmp", &[PathBuf::from("/nonexistent/path")]);
    assert!(!dirs.iter().any(|d| d.to_str() == Some("/nonexistent/path")), "不存在的 extra_dirs 应被跳过");
}
```

- [ ] **Step 3: 运行测试验证通过**

Run: `cargo test -p peri-middlewares --lib -- skills::loader::tests::test_resolve_skill_dirs`
Expected: 3 passed

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/skills/loader.rs peri-middlewares/src/skills/loader_test.rs
git commit -m "feat(skills): 新增 resolve_skill_dirs 公共函数，统一搜索目录解析"
```

---

### Task 2: SkillsMiddleware 委托公共函数

**Files:**
- Modify: `peri-middlewares/src/skills/mod.rs:124-145`

- [ ] **Step 1: 替换 `resolve_dirs_static()` 实现**

将 `mod.rs:124-145` 的 `resolve_dirs_static` 方法体替换为委托：

```rust
    pub fn resolve_dirs_static(cwd: &str, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
        loader::resolve_skill_dirs(cwd, extra_dirs)
    }
```

- [ ] **Step 2: 替换 `resolve_dirs()` 无 override 时的逻辑**

将 `mod.rs:148-174` 的 `resolve_dirs` 方法替换为：

```rust
    fn resolve_dirs(&self, cwd: &str) -> Vec<PathBuf> {
        // 有 override 字段时走测试隔离路径
        if self.user_skills_dir.is_some()
            || self.global_skills_dir.is_some()
            || self.project_skills_dir.is_some()
        {
            let user_dir = self.user_skills_dir.clone().unwrap_or_else(|| {
                dirs_next::home_dir()
                    .map(|h| h.join(".claude").join("skills"))
                    .unwrap_or_default()
            });
            let global_dir = self.global_skills_dir.clone();
            let project_dir = self
                .project_skills_dir
                .clone()
                .unwrap_or_else(|| PathBuf::from(cwd).join(".claude").join("skills"));
            let mut dirs = vec![user_dir];
            if let Some(global) = global_dir {
                dirs.push(global);
            }
            dirs.push(project_dir);
            for dir in &self.extra_dirs {
                if dir.is_dir() {
                    dirs.push(dir.clone());
                }
            }
            dirs
        } else {
            Self::resolve_dirs_static(cwd, &self.extra_dirs)
        }
    }
```

- [ ] **Step 3: 运行现有 SkillsMiddleware 测试验证无回归**

Run: `cargo test -p peri-middlewares --lib -- skills::tests`
Expected: 全部通过

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/skills/mod.rs
git commit -m "refactor(skills): SkillsMiddleware 委托 resolve_skill_dirs 公共函数"
```

---

### Task 3: SkillPreloadMiddleware 使用公共函数 + 支持 extra_dirs

**Files:**
- Modify: `peri-middlewares/src/subagent/skill_preload.rs`
- Test: `peri-middlewares/src/subagent/skill_preload_test.rs`

- [ ] **Step 1: 修改 SkillPreloadMiddleware 结构体和构造**

将 `skill_preload.rs:61-72` 替换为：

```rust
pub struct SkillPreloadMiddleware {
    skill_names: Vec<String>,
    cwd: String,
    extra_dirs: Vec<PathBuf>,
}

impl SkillPreloadMiddleware {
    pub fn new(skill_names: Vec<String>, cwd: &str) -> Self {
        Self {
            skill_names,
            cwd: cwd.to_string(),
            extra_dirs: Vec::new(),
        }
    }

    /// 追加额外 skills 搜索目录（用于插件 skills 路径注入）
    pub fn with_extra_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.extra_dirs = dirs;
        self
    }
}
```

- [ ] **Step 2: 删除 `resolve_dirs()` 方法，替换 import**

将 `skill_preload.rs:11` 的 import：
```rust
use crate::skills::{list_skills, load_global_skills_dir};
```
替换为：
```rust
use crate::skills::{list_skills, loader::resolve_skill_dirs};
```

删除 `skill_preload.rs:74-90` 的 `resolve_dirs()` 方法。

- [ ] **Step 3: 修改 `before_agent` 中的目录解析调用**

将 `skill_preload.rs` 中 `before_agent` 里的：
```rust
let dirs = self.resolve_dirs();
```
替换为：
```rust
let dirs = resolve_skill_dirs(&self.cwd, &self.extra_dirs);
```

- [ ] **Step 4: 添加 extra_dirs 测试**

在 `skill_preload_test.rs` 末尾添加：

```rust
    #[tokio::test]
    async fn test_preload_from_extra_dirs() {
        // Arrange: skill 不在标准路径，只在 extra_dirs 中
        let dir = tempdir().unwrap();
        let extra_dir = dir.path().join("plugin-skills");
        std::fs::create_dir_all(&extra_dir).unwrap();
        write_skill(&extra_dir, "plugin-skill", "插件技能");

        let mw = SkillPreloadMiddleware::new(
            vec!["plugin-skill".to_string()],
            "/nonexistent/cwd", // cwd 下没有 skill
        )
        .with_extra_dirs(vec![extra_dir]);
        let mut state = AgentState::new("/nonexistent/cwd");

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert: 应从 extra_dirs 找到并注入 Ai + Tool = 2 条消息
        assert_eq!(state.messages().len(), 2, "应从 extra_dirs 找到 skill 并注入");
        let tool_content = state.messages()[1].content();
        assert!(
            tool_content.contains("Skill content for plugin-skill"),
            "Tool 结果应包含插件 skill 全文"
        );
    }
```

- [ ] **Step 5: 运行全部 SkillPreloadMiddleware 测试**

Run: `cargo test -p peri-middlewares --lib -- subagent::skill_preload::tests`
Expected: 全部通过（含新增 1 个）

- [ ] **Step 6: Commit**

```bash
git add peri-middlewares/src/subagent/skill_preload.rs peri-middlewares/src/subagent/skill_preload_test.rs
git commit -m "fix(skills): SkillPreloadMiddleware 支持 extra_dirs，委托 resolve_skill_dirs 公共函数"
```

---

### Task 4: 主 Agent builder 传入 plugin_skill_dirs

**Files:**
- Modify: `peri-acp/src/agent/builder.rs:417`

- [ ] **Step 1: 修改 builder.rs 中的 SkillPreloadMiddleware 构造**

将 `builder.rs:417` 的：
```rust
        .add_middleware(Box::new(SkillPreloadMiddleware::new(preload_skills, &cwd)))
```
替换为：
```rust
        .add_middleware(Box::new(
            SkillPreloadMiddleware::new(preload_skills, &cwd)
                .with_extra_dirs(plugin_skill_dirs.clone()),
        ))
```

- [ ] **Step 2: 编译验证**

Run: `cargo build -p peri-acp`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add peri-acp/src/agent/builder.rs
git commit -m "fix(acp): 主 Agent SkillPreloadMiddleware 传入 plugin_skill_dirs"
```

---

### Task 5: 全量测试 + 编译验证

- [ ] **Step 1: 全量编译**

Run: `cargo build`
Expected: 成功

- [ ] **Step 2: 运行相关 crate 测试**

Run: `cargo test -p peri-middlewares && cargo test -p peri-acp`
Expected: 全部通过

- [ ] **Step 3: 更新 issue 状态**

将 `spec/issues/2026-06-10-skill-preload-cannot-load-plugin-skills.md` 状态更新为 `Fixed`，追加修复记录。
