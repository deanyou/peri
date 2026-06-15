# AGM pick/omit 范围过滤设计

## 背景

AGM 安装的依赖包（尤其是 git 仓库）经常包含大量 skill/agent/mcp。当前行为会把包内检测到的所有项都通过 symlink 安装到目标目录，导致用户项目里出现不需要的 skill。

本设计让 `agm.json` 在声明依赖时即可按 glob 范围选择性地安装包内资源，避免一次性引入整个仓库。

## 目标

- 支持对单个依赖声明 `pick`（只安装匹配的 skill/agent/mcp）。
- 支持对单个依赖声明 `omit`（排除匹配的项）。
- 同时支持 skills、agents、mcp 三类依赖。
- 保持现有 `"pkg": "version"` 字符串写法向后兼容。
- 范围表达式使用 glob，匹配 skill/agent 的目录名或相对路径。

## 非目标

- 不支持跨包的 `pick`/`omit`（即顶层统一过滤列表）。
- 不支持运行时 CLI 一次性过滤（如 `agm install --pick`），如需可后续扩展。
- 不修改 store 中已下载的包内容，仅在 symlink 阶段过滤。

## 数据模型

### DependencySpec

`ProjectManifest` 中的三类依赖从 `BTreeMap<String, String>` 改为 `BTreeMap<String, DependencySpec>`：

```rust
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
```

序列化示例：

```json
{
  "name": "my-project",
  "targets": ["claude"],
  "skills": {
    "@git/owner/repo": "abc123",
    "some-registry-pkg": {
      "version": "^1.0.0",
      "pick": ["interview", "grill-*"],
      "omit": ["**/*-test"]
    }
  },
  "agents": {
    "@git/owner/repo": {
      "version": "abc123",
      "pick": ["coder"]
    }
  },
  "mcp": {}
}
```

`version` 字段语义与现有字符串值完全一致：git 依赖填 commit hash，registry 依赖填 semver 范围或精确版本。

## 过滤语义

1. `pick` 为空（或缺失）表示“不排除任何项”。
2. `omit` 为空（或缺失）表示“不排除任何项”。
3. 同时声明 `pick` 和 `omit` 时，先按 `pick` 取交集，再按 `omit` 排除。
4. 某项同时命中 `pick` 和 `omit`，以 `omit` 为准。
5. glob 支持两种匹配维度：
   - **名称匹配**：skill/agent 目录名，例如 `"grill-*"` 匹配 `skills/grill-me/SKILL.md`。
   - **路径匹配**：相对包根的路径，例如 `"skills/engineering/*"` 匹配 `skills/engineering/` 下所有 skill。
6. glob 引擎使用 `glob = "0.3"` crate，需要在 `agm/Cargo.toml` 新增依赖。

## 安装流程改动

1. `resolver::collect_dependencies` 返回类型从 `(String, String, PackageType)` 改为 `(String, DependencySpec, PackageType)`。
2. `installer::install_from_git` 与 `installer::install_all` 在获取到某个包的所有 skill/agent 列表后，根据该包对应的 `DependencySpec` 做过滤，只 symlink 命中的项。
3. lock 文件只记录实际安装的包（package 级），未安装的单个 skill/agent 不进入 lock。
4. 如果 `pick` 过滤后没有任何项命中，打印 warning，但不中断安装流程。
5. `list` 命令对 Detailed 依赖展示 `pick/omit` 摘要，例如：
   ```
   [skills]
     ✓ some-registry-pkg ^1.0.0 (registry) [installed: claude] pick=[interview, grill-*] omit=[**/*-test]
   ```

## 错误处理

| 场景 | 行为 |
|------|------|
| glob 模式非法 | 安装时报错，指明具体依赖与模式 |
| `pick` 后无命中 | warning，继续 |
| Detailed 缺少 `version` | serde 反序列化时报错 |
| 同时声明 `pick` 和 `omit` 且互斥 | 按“先 pick 后 omit”规则处理，不报错 |

## 测试计划

- `agm/src/config_test.rs`：验证 `DependencySpec` 字符串/对象两种写法序列化与反序列化。
- `agm/src/store_test.rs`：构造含多个 skill/agent 的临时包目录，验证过滤函数。
- `agm/tests/integration_test.rs`：通过真实 git 安装流程验证 `pick`、`omit`、组合场景。
- 回归测试：现有 `"pkg": "version"` 字符串写法继续工作，lock 文件格式不变。

## 影响面

- 修改文件：
  - `agm/src/types.rs`：新增 `DependencySpec`。
  - `agm/src/resolver.rs`：`collect_dependencies` 返回类型与解析逻辑。
  - `agm/src/installer.rs`：安装前过滤 skill/agent/mcp。
  - `agm/src/commands/list.rs`：展示 `pick/omit` 摘要。
  - `agm/src/commands/uninstall.rs`：需要能处理 Detailed 依赖。
  - `agm/src/config_test.rs`、`agm/src/store_test.rs`、`agm/tests/integration_test.rs`：新增测试。
- 无 CLI 参数变更。
- 不修改 store 物理布局。
- 向后兼容现有 `agm.json` 字符串写法。
