# AGM pick/omit 范围过滤实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `agm.json` 在声明依赖时支持 `pick`/`omit` glob 过滤，实现只安装包内部分 skill/agent/mcp。

**Architecture:** 新增 `DependencySpec` 表示依赖的字符串或对象写法；新增 `filter` 模块统一处理 glob 过滤；在 `installer` 创建 symlink 前调用过滤；`resolver`/`list`/`uninstall` 同步适配新类型。

**Tech Stack:** Rust, serde, glob, clap

---

## 文件结构

| 文件 | 变更 | 说明 |
|------|------|------|
| `agm/Cargo.toml` | 修改 | 新增 `glob = "0.3"` 依赖 |
| `agm/src/types.rs` | 修改 | 新增 `DependencySpec` enum，修改 `ProjectManifest` 三个依赖字段类型 |
| `agm/src/error.rs` | 修改 | 新增 `InvalidGlobPattern` 错误变体 |
| `agm/src/filter.rs` | 新建 | glob 过滤逻辑 |
| `agm/src/filter_test.rs` | 新建 | 过滤单元测试 |
| `agm/src/resolver.rs` | 修改 | `collect_dependencies` 返回 `DependencySpec` |
| `agm/src/installer.rs` | 修改 | 安装前按 `DependencySpec` 过滤 skill/agent |
| `agm/src/commands/list.rs` | 修改 | 显示 `pick`/`omit` 摘要 |
| `agm/src/commands/uninstall.rs` | 修改 | 适配 `DependencySpec` |
| `agm/src/config_test.rs` | 修改 | 新增 `DependencySpec` 序列化测试 |
| `agm/src/store_test.rs` | 修改 | 新增过滤函数测试 |
| `agm/tests/integration_test.rs` | 修改 | 新增 `--git` 安装 pick/omit 集成测试 |
| `agm/src/mod.rs`（或 `agm/src/lib.rs`） | 修改 | 注册 `filter` 模块 |

---

### Task 1: 添加 `glob` 依赖并定义 `DependencySpec`

**Files:**
- Modify: `agm/Cargo.toml`
- Modify: `agm/src/types.rs`
- Modify: `agm/src/error.rs`

- [ ] **Step 1: 在 `agm/Cargo.toml` 添加 glob 依赖**

```toml
[dependencies]
glob = "0.3"
```

完整 `[dependencies]` 应变为：

```toml
[dependencies]
tokio.workspace = true
serde.workspace = true
serde_json.workspace = true
reqwest.workspace = true
tempfile.workspace = true
thiserror.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
dirs-next.workspace = true
clap.workspace = true
semver.workspace = true
sha2.workspace = true
flate2 = "1"
tar = "0.4"
glob = "0.3"
```

- [ ] **Step 2: 在 `agm/src/error.rs` 新增 InvalidGlobPattern 错误变体**

```rust
#[error("Invalid glob pattern '{pattern}': {reason}")]
InvalidGlobPattern { pattern: String, reason: String },
```

插入到 `Other(String)` 之前。

- [ ] **Step 3: 在 `agm/src/types.rs` 新增 DependencySpec**

在 `ProjectManifest` 之前添加：

```rust
/// Dependency declaration: either a plain version string or a detailed object with pick/omit filters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    Simple(String),
    Detailed {
        version: String,
        #[serde(default)]
        pick: Vec<String>,
        #[serde(default)]
        omit: Vec<String>,
    },
}

impl DependencySpec {
    pub fn version(&self) -> &str {
        match self {
            DependencySpec::Simple(v) => v,
            DependencySpec::Detailed { version, .. } => version,
        }
    }
}
```

- [ ] **Step 4: 修改 ProjectManifest 三个依赖字段类型**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub registry: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub skills: BTreeMap<String, DependencySpec>,
    #[serde(default)]
    pub agents: BTreeMap<String, DependencySpec>,
    #[serde(default)]
    pub mcp: BTreeMap<String, DependencySpec>,
    #[serde(default)]
    pub overrides: BTreeMap<String, String>,
}
```

- [ ] **Step 5: 验证编译**

Run: `cargo check -p agm`
Expected: 出现多处类型错误（因为 resolver/installer 还未适配），这是预期的。下一步开始修复。

- [ ] **Step 6: Commit**

```bash
git add agm/Cargo.toml agm/src/types.rs agm/src/error.rs
git commit -m "feat(agm): add DependencySpec and glob dependency"
```

---

### Task 2: 实现过滤模块

**Files:**
- Create: `agm/src/filter.rs`
- Create: `agm/src/filter_test.rs`
- Modify: `agm/src/lib.rs`（或入口模块文件）

- [ ] **Step 1: 新建 `agm/src/filter.rs`**

```rust
use crate::error::{AgmError, Result};
use crate::types::DependencySpec;

/// Filter a list of (name, glob) items according to pick/omit patterns.
/// Matching is done against both the item name and its glob path.
pub fn filter_items(items: &[(String, String)], spec: &DependencySpec) -> Result<Vec<(String, String)>> {
    let (pick_patterns, omit_patterns) = match spec {
        DependencySpec::Simple(_) => return Ok(items.to_vec()),
        DependencySpec::Detailed { pick, omit, .. } => (pick, omit),
    };

    let pick_compiled = compile_patterns(pick_patterns)?;
    let omit_compiled = compile_patterns(omit_patterns)?;

    let mut result = Vec::new();
    for (name, glob) in items {
        let matched_pick = pick_compiled.is_empty()
            || pick_compiled.iter().any(|p| p.matches(name) || p.matches(glob));
        let matched_omit = omit_compiled
            .iter()
            .any(|p| p.matches(name) || p.matches(glob));

        if matched_pick && !matched_omit {
            result.push((name.clone(), glob.clone()));
        }
    }

    Ok(result)
}

fn compile_patterns(patterns: &[String]) -> Result<Vec<glob::Pattern>> {
    patterns
        .iter()
        .map(|p| {
            glob::Pattern::new(p).map_err(|e| AgmError::InvalidGlobPattern {
                pattern: p.clone(),
                reason: e.to_string(),
            })
        })
        .collect()
}
```

- [ ] **Step 2: 新建 `agm/src/filter_test.rs`**

```rust
use crate::filter::filter_items;
use crate::types::DependencySpec;

#[test]
fn test_filter_simple_passes_through() {
    let spec = DependencySpec::Simple("abc123".into());
    let items = vec![("interview".into(), "skills/interview/SKILL.md".into())];
    let out = filter_items(&items, &spec).unwrap();
    assert_eq!(out.len(), 1);
}

#[test]
fn test_filter_pick_by_name() {
    let spec = DependencySpec::Detailed {
        version: "abc123".into(),
        pick: vec!["grill-*".into()],
        omit: vec![],
    };
    let items = vec![
        ("grill-me".into(), "skills/grill-me/SKILL.md".into()),
        ("interview".into(), "skills/interview/SKILL.md".into()),
    ];
    let out = filter_items(&items, &spec).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "grill-me");
}

#[test]
fn test_filter_omit_by_path() {
    let spec = DependencySpec::Detailed {
        version: "abc123".into(),
        pick: vec![],
        omit: vec!["skills/test/**".into()],
    };
    let items = vec![
        ("grill-me".into(), "skills/grill-me/SKILL.md".into()),
        ("foo".into(), "skills/test/foo/SKILL.md".into()),
    ];
    let out = filter_items(&items, &spec).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "grill-me");
}

#[test]
fn test_filter_pick_and_omit() {
    let spec = DependencySpec::Detailed {
        version: "abc123".into(),
        pick: vec!["skill-*".into()],
        omit: vec!["skill-test".into()],
    };
    let items = vec![
        ("skill-a".into(), "skills/skill-a/SKILL.md".into()),
        ("skill-test".into(), "skills/skill-test/SKILL.md".into()),
        ("other".into(), "skills/other/SKILL.md".into()),
    ];
    let out = filter_items(&items, &spec).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "skill-a");
}

#[test]
fn test_filter_invalid_glob_errors() {
    let spec = DependencySpec::Detailed {
        version: "abc123".into(),
        pick: vec!["[invalid".into()],
        omit: vec![],
    };
    let items = vec![("a".into(), "skills/a/SKILL.md".into())];
    assert!(filter_items(&items, &spec).is_err());
}
```

- [ ] **Step 3: 在 `agm/src/lib.rs` 注册 filter 模块**

```rust
pub mod filter;
```

如果 `agm/src/lib.rs` 不存在（当前 agm 可能是 bin crate），需要先创建 `agm/src/lib.rs` 并暴露现有模块。当前 agm 的 `main.rs` 直接使用 `agm::commands::*`，说明已存在 `lib.rs`。查看后把 `pub mod filter;` 加入其中。

- [ ] **Step 4: 运行 filter 测试**

Run: `cargo test -p agm --lib filter`
Expected: 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add agm/src/filter.rs agm/src/filter_test.rs agm/src/lib.rs
git commit -m "feat(agm): add pick/omit filter module with tests"
```

---

### Task 3: 适配 resolver

**Files:**
- Modify: `agm/src/resolver.rs`

- [ ] **Step 1: 修改 `collect_dependencies` 返回类型**

```rust
pub fn collect_dependencies(manifest: &ProjectManifest) -> Vec<(String, DependencySpec, PackageType)> {
    let mut deps = Vec::new();
    for (name, spec) in &manifest.skills {
        deps.push((name.clone(), spec.clone(), PackageType::Skills));
    }
    for (name, spec) in &manifest.agents {
        deps.push((name.clone(), spec.clone(), PackageType::Agents));
    }
    for (name, spec) in &manifest.mcp {
        deps.push((name.clone(), spec.clone(), PackageType::Mcp));
    }
    deps
}
```

- [ ] **Step 2: 验证编译**

Run: `cargo check -p agm`
Expected: installer.rs 仍报错，resolver.rs 自身通过。

- [ ] **Step 3: Commit**

```bash
git add agm/src/resolver.rs
git commit -m "refactor(agm): resolver returns DependencySpec"
```

---

### Task 4: 适配 installer 安装流程

**Files:**
- Modify: `agm/src/installer.rs`

- [ ] **Step 1: 引入 filter 和 DependencySpec**

在文件顶部添加：

```rust
use crate::filter::filter_items;
use crate::types::DependencySpec;
```

- [ ] **Step 2: 修改 `install_from_git` 在 symlink 前过滤**

`install_from_git` 目前接受 `--git` 直接安装。pick/omit 配置应来自已有 manifest（如果用户之前编辑过），否则不过滤。

在解析出 `pkg_name` 后、创建 symlinks 前加入：

```rust
let existing_spec = self
    .manifest
    .skills
    .get(&pkg_name)
    .or_else(|| self.manifest.agents.get(&pkg_name))
    .or_else(|| self.manifest.mcp.get(&pkg_name))
    .cloned();
let spec = existing_spec.unwrap_or_else(|| DependencySpec::Simple(actual_commit.clone()));
let skills = filter_items(&skills, &spec)?;
let agents = filter_items(&agents, &spec)?;

if skills.is_empty() && agents.is_empty() && !matches!(spec, DependencySpec::Simple(_)) {
    println!("  (no skills/agents matched pick/omit filters for {})", pkg_name);
}
```

- [ ] **Step 3: 修改 `install_all` 在 symlink 前过滤**

`install_all` 已遍历 `deps`，其中每个元素现在是 `(name, spec, typ)`。将循环内部：

```rust
for (name, spec, _) in &deps_of_type {
```

并在创建 symlink 前根据类型获取已检测到的 items。

但 `install_all` 当前不预先检测包内 items，而是直接 symlink 整个 store 路径。需要先读取 `agm.package.json` 或自动检测，得到 items 列表，再过滤。

修改思路：在 `install_to_store` 之后、创建 symlink 之前，读取 package manifest 或自动检测 items，然后过滤，最后只为命中的项创建 symlink。

具体地，把原来的直接 symlink 替换为：

```rust
let store_path = match &resolution {
    Resolution::Git { repo, commit, .. } => self.store.git_package_path(repo, commit),
    Resolution::Registry { .. } => self.store.registry_package_path(name, &lock_version),
};

// Detect and filter items
let (items, target_subdir): (Vec<(String, String)>, _) = match typ {
    PackageType::Skills => {
        let detected = detect_package_items(&store_path, PackageType::Skills, name)?;
        (filter_items(&detected, spec)?, adapter.map_dir(*typ, &self.project_root))
    }
    PackageType::Agents => {
        let detected = detect_package_items(&store_path, PackageType::Agents, name)?;
        (filter_items(&detected, spec)?, adapter.map_dir(*typ, &self.project_root))
    }
    PackageType::Mcp => {
        let detected = detect_package_items(&store_path, PackageType::Mcp, name)?;
        (filter_items(&detected, spec)?, adapter.map_dir(*typ, &self.project_root))
    }
};

if items.is_empty() && !matches!(spec, DependencySpec::Simple(_)) {
    println!("  (no items matched pick/omit filters for {})", name);
}

for (item_name, item_glob) in &items {
    let link_name = symlink_name(item_name, &[]);
    let (store_item_path, install_source): (PathBuf, &Path) = if item_glob == "." {
        (store_path.to_path_buf(), store_path)
    } else {
        let p = store_path.join(item_glob);
        let source = p.parent().unwrap_or(&p);
        (p, source)
    };
    if store_item_path.exists() {
        adapter.install(install_source, &target_subdir, &link_name)?;
        println!("  ✓ {}: {} → .{}/{}/{}", typ_label(typ), item_name, self.target, typ_subdir(typ), link_name);
    }
}
```

需要新增辅助函数：

```rust
fn detect_package_items(
    store_path: &Path,
    typ: PackageType,
    package_name: &str,
) -> Result<Vec<(String, String)>> {
    let pkg_manifest_path = store_path.join("agm.package.json");
    if pkg_manifest_path.exists() {
        let pkg = PackageManifest::load(&pkg_manifest_path)?;
        match typ {
            PackageType::Skills => Ok(pkg
                .skills
                .into_iter()
                .map(|g| (extract_skill_name(&g), g))
                .collect()),
            PackageType::Agents => Ok(pkg
                .agents
                .into_iter()
                .map(|g| (extract_skill_name(&g), g))
                .collect()),
            PackageType::Mcp => Ok(pkg
                .mcp
                .into_iter()
                .map(|g| (extract_skill_name(&g), g))
                .collect()),
        }
    } else {
        // Auto-detect from store path
        let (detected_skills, detected_agents) = auto_detect_types(store_path);
        let detected = match typ {
            PackageType::Skills => detected_skills,
            PackageType::Agents => detected_agents,
            PackageType::Mcp => Vec::new(),
        };
        // Fallback: if the package has no manifest and no auto-detected items,
        // treat the whole package directory as a single item to preserve existing behavior.
        if detected.is_empty() {
            Ok(vec![(package_name.into(), ".".into())])
        } else {
            Ok(detected)
        }
    }
}

fn typ_label(typ: PackageType) -> &'static str {
    match typ {
        PackageType::Skills => "skill",
        PackageType::Agents => "agent",
        PackageType::Mcp => "mcp",
    }
}

fn typ_subdir(typ: PackageType) -> &'static str {
    match typ {
        PackageType::Skills => "skills",
        PackageType::Agents => "agents",
        PackageType::Mcp => "mcp",
    }
}
```

- [ ] **Step 4: 处理 install_from_git 的 manifest 写入**

`install_from_git` 当前在循环内对同一个 `pkg_name` 重复 `insert`（每个 skill/agent 都 insert 一次）。这不影响正确性，但建议改为循环外统一 insert。

由于 `--git` 模式安装后需要把包写入 manifest，而 pick/omit 来自已有配置，所以 manifest 中该包对应的 spec 应该保持不变。如果之前不存在，写入 `Simple(actual_commit.clone())`。

修改安装循环内的 manifest 写入：

```rust
// 循环前记录是否已有 spec
let final_spec = self
    .manifest
    .skills
    .get(&pkg_name)
    .or_else(|| self.manifest.agents.get(&pkg_name))
    .or_else(|| self.manifest.mcp.get(&pkg_name))
    .cloned()
    .unwrap_or_else(|| DependencySpec::Simple(actual_commit.clone()));
```

循环结束后，根据是否安装了 skill/agent 把 `final_spec` 写入对应 map：

```rust
if !skills.is_empty() {
    self.manifest.skills.insert(pkg_name.clone(), final_spec.clone());
}
if !agents.is_empty() {
    self.manifest.agents.insert(pkg_name.clone(), final_spec.clone());
}
```

- [ ] **Step 5: 验证编译**

Run: `cargo check -p agm`
Expected: 通过

- [ ] **Step 6: Commit**

```bash
git add agm/src/installer.rs
git commit -m "feat(agm): apply pick/omit filters during install"
```

---

### Task 5: 适配 list 命令

**Files:**
- Modify: `agm/src/commands/list.rs`

- [ ] **Step 1: 修改 `print_section` 签名和实现**

```rust
fn print_section(
    label: &str,
    deps: &BTreeMap<String, DependencySpec>,
    lock: &Option<LockFile>,
) {
    if deps.is_empty() {
        return;
    }
    println!("[{}]", label);
    for (name, spec) in deps {
        let source = if is_git_dep(name) { "git" } else { "registry" };
        let installed = lock.as_ref().and_then(|l| {
            l.packages
                .iter()
                .find(|(k, _)| k.starts_with(name))
                .map(|(_, p)| p.targets.join(", "))
        });

        let filters = match spec {
            DependencySpec::Simple(_) => String::new(),
            DependencySpec::Detailed { pick, omit, .. } => {
                let mut parts = Vec::new();
                if !pick.is_empty() {
                    parts.push(format!("pick=[{}]", pick.join(", ")));
                }
                if !omit.is_empty() {
                    parts.push(format!("omit=[{}]", omit.join(", ")));
                }
                if parts.is_empty() {
                    String::new()
                } else {
                    format!(" {}", parts.join(" "))
                }
            }
        };

        match installed {
            Some(targets) if !targets.is_empty() => {
                println!(
                    "  ✓ {} {} ({}) [installed: {}]{}",
                    name, spec.version(), source, targets, filters
                );
            }
            _ => {
                println!(
                    "  ✗ {} {} ({}) [pending]{}",
                    name, spec.version(), source, filters
                );
            }
        }
    }
}
```

- [ ] **Step 2: 验证 list 命令编译**

Run: `cargo check -p agm`
Expected: 通过

- [ ] **Step 3: Commit**

```bash
git add agm/src/commands/list.rs
git commit -m "feat(agm): show pick/omit filters in list output"
```

---

### Task 6: 适配 uninstall 命令

**Files:**
- Modify: `agm/src/commands/uninstall.rs`

- [ ] **Step 1: 修改 removed_* 类型**

```rust
let removed_skills = manifest.skills.remove(package);
let removed_agents = manifest.agents.remove(package);
let removed_mcp = manifest.mcp.remove(package);
```

类型自动变为 `Option<DependencySpec>`，后续判断逻辑不变。

- [ ] **Step 2: 验证编译**

Run: `cargo check -p agm`
Expected: 通过

- [ ] **Step 3: Commit**

```bash
git add agm/src/commands/uninstall.rs
git commit -m "refactor(agm): uninstall adapts to DependencySpec"
```

---

### Task 7: 添加单元测试

**Files:**
- Modify: `agm/src/types_test.rs`
- Modify: `agm/src/config_test.rs`
- Modify: `agm/src/store_test.rs`

- [ ] **Step 1: 更新 `types_test.rs` 以适配 DependencySpec**

`types_test.rs` 中现有测试仍假设 `skills` 等字段是 `BTreeMap<String, String>`，需要改为 `DependencySpec::Simple(...)`：

```rust
use crate::types::*;
use std::collections::BTreeMap;

#[test]
fn test_parse_project_manifest() {
    let json = r#"{
        "name": "my-agent-project",
        "version": "1.0.0",
        "skills": {
            "@git/konghayao/peri/blog-writer": "abc123def456",
            "some-pkg": "^1.2.3"
        },
        "agents": {},
        "mcp": {}
    }"#;
    let manifest: ProjectManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.name, "my-agent-project");
    assert_eq!(manifest.skills.len(), 2);
    assert_eq!(
        manifest.skills["@git/konghayao/peri/blog-writer"],
        DependencySpec::Simple("abc123def456".into())
    );
    assert_eq!(
        manifest.skills["some-pkg"],
        DependencySpec::Simple("^1.2.3".into())
    );
}
```

同时 `test_project_manifest_roundtrip` 中的：

```rust
skills: [("pkg".into(), "^1.0.0".into())].into(),
```

改为：

```rust
skills: [("pkg".into(), DependencySpec::Simple("^1.0.0".into()))].into(),
```

- [ ] **Step 2: 在 `config_test.rs` 新增 DependencySpec 测试**

```rust
use crate::types::DependencySpec;

#[test]
fn test_dependency_spec_simple_roundtrip() {
    let spec = DependencySpec::Simple("abc123".into());
    let json = serde_json::to_string(&spec).unwrap();
    assert_eq!(json, "\"abc123\"");
    let parsed: DependencySpec = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, spec);
}

#[test]
fn test_dependency_spec_detailed_roundtrip() {
    let spec = DependencySpec::Detailed {
        version: "^1.0.0".into(),
        pick: vec!["grill-*".into()],
        omit: vec!["**/*-test".into()],
    };
    let json = serde_json::to_string(&spec).unwrap();
    let parsed: DependencySpec = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, spec);
}

#[test]
fn test_project_manifest_mixed_deps() {
    use crate::types::ProjectManifest;
    use std::collections::BTreeMap;

    let mut skills = BTreeMap::new();
    skills.insert("@git/owner/repo".into(), DependencySpec::Simple("abc123".into()));
    skills.insert(
        "some-pkg".into(),
        DependencySpec::Detailed {
            version: "^1.0.0".into(),
            pick: vec!["interview".into()],
            omit: vec![],
        },
    );

    let manifest = ProjectManifest {
        name: "test".into(),
        version: "0.1.0".into(),
        description: String::new(),
        author: String::new(),
        registry: None,
        targets: vec!["claude".into()],
        skills,
        agents: BTreeMap::new(),
        mcp: BTreeMap::new(),
        overrides: BTreeMap::new(),
    };

    let json = serde_json::to_string_pretty(&manifest).unwrap();
    let parsed: ProjectManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, manifest);
}
```

- [ ] **Step 3: 在 `store_test.rs` 测试过滤集成**

`filter` 模块已有独立测试，这里补充一个通过 `ProjectManifest` 解析 JSON 的测试：

```rust
use crate::types::{DependencySpec, ProjectManifest};
use std::collections::BTreeMap;

#[test]
fn test_manifest_parses_detailed_dependency() {
    let json = r#"{
        "name": "test",
        "skills": {
            "some-pkg": {
                "version": "^1.0.0",
                "pick": ["interview", "grill-*"],
                "omit": ["**/*-test"]
            }
        }
    }"#;

    let manifest: ProjectManifest = serde_json::from_str(json).unwrap();
    let spec = manifest.skills.get("some-pkg").unwrap();
    assert!(matches!(spec, DependencySpec::Detailed { .. }));
    assert_eq!(spec.version(), "^1.0.0");
}
```

- [ ] **Step 4: 运行单元测试**

Run: `cargo test -p agm --lib`
Expected: 全部 PASS

- [ ] **Step 5: Commit**

```bash
git add agm/src/types_test.rs agm/src/config_test.rs agm/src/store_test.rs
git commit -m "test(agm): add DependencySpec and manifest parsing tests"
```

---

### Task 8: 添加集成测试

**Files:**
- Modify: `agm/tests/integration_test.rs`

- [ ] **Step 1: 新增 pick/omit 集成测试**

```rust
use std::process::Command;
use tempfile::TempDir;

fn setup_git_repo_with_skills(tmp: &TempDir) -> String {
    let repo = tmp.path().join("remote-repo");
    std::fs::create_dir_all(repo.join("skills/grill-me")).unwrap();
    std::fs::create_dir_all(repo.join("skills/interview")).unwrap();
    std::fs::create_dir_all(repo.join("skills/skill-test")).unwrap();
    std::fs::write(repo.join("skills/grill-me/SKILL.md"), "# grill-me").unwrap();
    std::fs::write(repo.join("skills/interview/SKILL.md"), "# interview").unwrap();
    std::fs::write(repo.join("skills/skill-test/SKILL.md"), "# skill-test").unwrap();

    // init git repo and commit
    let init = Command::new("git")
        .args(["init"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(init.status.success());

    let config = Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(config.status.success());

    let config = Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(config.status.success());

    let add = Command::new("git")
        .args(["add", "."])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(add.status.success());

    let commit = Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(commit.status.success());

    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repo)
        .output()
        .unwrap();
    String::from_utf8_lossy(&head.stdout).trim().to_string()
}

#[test]
fn test_install_git_with_pick_omit() {
    let tmp = TempDir::new().unwrap();
    let commit = setup_git_repo_with_skills(&tmp);
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();

    // Write agm.json with pick/omit
    let agm_json = format!(
        r#"{{
            "name": "project",
            "targets": ["claude"],
            "skills": {{
                "@git/owner/repo": {{
                    "version": "{}",
                    "pick": ["grill-*", "interview"],
                    "omit": ["**/*-test"]
                }}
            }}
        }}"#,
        commit
    );
    std::fs::write(project.join("agm.json"), agm_json).unwrap();

    let output = Command::new("cargo")
        .args([
            "run", "-p", "agm", "--", "install", "--tool", "claude", "--git",
        ])
        .arg(tmp.path().join("remote-repo").to_str().unwrap())
        .arg("-C")
        .arg(&project)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let skills_dir = project.join(".claude/skills");
    assert!(skills_dir.join("grill-me").exists() || skills_dir.join("grill-me").read_link().is_ok());
    assert!(skills_dir.join("interview").exists() || skills_dir.join("interview").read_link().is_ok());
    assert!(!skills_dir.join("skill-test").exists());
}
```

- [ ] **Step 2: 运行集成测试**

Run: `cargo test -p agm --test integration_test test_install_git_with_pick_omit`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add agm/tests/integration_test.rs
git commit -m "test(agm): add git install pick/omit integration test"
```

---

### Task 9: 全量验证

**Files:**
- 所有已修改文件

- [ ] **Step 1: 运行全部 agm 测试**

Run: `cargo test -p agm`
Expected: 全部 PASS

- [ ] **Step 2: 运行 clippy**

Run: `cargo clippy -p agm -- -D warnings`
Expected: 无警告

- [ ] **Step 3: 格式化代码**

Run: `cargo fmt -p agm`
Expected: 无变更（或已自动修正）

- [ ] **Step 4: Commit 任何格式修复**

```bash
git add -A
git commit -m "style(agm): format and clippy fixes" || echo "nothing to commit"
```

---

## Self-Review Checklist

- [ ] **Spec coverage:**
  - agm.json 支持字符串/对象双写法 → Task 1, Task 7
  - glob pick/omit 过滤 → Task 2, Task 4
  - 支持 skills/agents/mcp → Task 4
  - 保持向后兼容 → Task 1, Task 7
  - list 显示摘要 → Task 5
- [ ] **Placeholder scan:** 无 TBD/TODO/"implement later" / 无 "add appropriate error handling" 等模糊描述 / 每个代码步骤都包含实际代码
- [ ] **Type consistency:** `DependencySpec` 在 Task 1 定义后，后续 task 均使用相同变体名和字段；`filter_items` 签名一致
