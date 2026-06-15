# AGM — Agent Package Manager

pnpm-style package manager for AI agent dependencies. One `agm.json`, any tool.

## Install

```bash
# Unix / macOS
curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/agm/install.sh | bash

# Windows
irm https://raw.githubusercontent.com/konghayao/peri/main/agm/install.ps1 | iex
```

## Quick Start

```bash
# 1. Init a project
agm init

# 2. Install a skill from GitHub
agm install --git https://github.com/obra/superpowers --tool claude

# 3. Install from a registry package
agm install some-package --tool claude

# 4. See what's installed
agm list

# 5. Remove a package
agm uninstall --package @git/obra/superpowers --tool claude
```

## How It Works

```
agm.json          # Your project manifest — what you depend on
agm.lock.json     # Lock file — exact versions resolved
~/.agm/store/     # Content-addressable store — one copy of each package
.claude/skills/   # Symlinks created by agm (tool-agnostic adapters)
```

**Add a dependency** → agm clones to `~/.agm/store/` → creates symlinks in your tool's directory → records the version in `agm.json` and `agm.lock.json`.

## Commands

| Command | Description |
|---------|-------------|
| `agm init` | Create `agm.json` |
| `agm install` | Install all dependencies from `agm.json` |
| `agm install --git <url> --tool <tool>` | Install directly from a git repo |
| `agm uninstall --package <name> --tool <tool>` | Remove a package |
| `agm list` | List installed packages |
| `agm update` | Update packages interactively |
| `agm gc` | Clean up unused store packages |
| `agm self-update` | Update agm itself |

## agm.json

```json
{
  "name": "my-project",
  "targets": ["claude"],
  "skills": {
    "@git/obra/superpowers": "6fd4507...",
    "some-registry-pkg": "^1.0.0",
    "@git/obra/another-repo": {
      "version": "abc1234...",
      "pick": ["grill-*", "interview"],
      "omit": ["**/*-test"]
    },
    "@git/peakdong68/toolkit-agent-skills": {
      "version": "abc1234...",
      "base": "plugins/kit-core",
      "pick": ["autonomous-loop"]
    }
  },
  "agents": {},
  "mcp": {},
  "overrides": {}
}
```

### Pick / Omit

Large repositories can expose many skills/agents. Use `pick` and `omit` with glob patterns to install only what you need:

- `pick` — only install items matching any of these globs.
- `omit` — exclude items matching any of these globs.
- Both are optional arrays; when both are present, `pick` is applied first, then `omit`.
- Patterns match both the item directory name (e.g., `grill-me`) and its relative path inside the package (e.g., `skills/engineering/*`).

Example: install only `grill-*` and `interview`, but exclude anything ending in `-test`.

```json
{
  "skills": {
    "@git/obra/superpowers": {
      "version": "6fd4507...",
      "pick": ["grill-*", "interview"],
      "omit": ["**/*-test"]
    }
  }
}
```

Re-running `agm install` after changing `pick`/`omit` will remove stale symlinks and create new ones to match the updated filters.

### Base path

By default agm discovers skills/agents under `.claude/skills/`, `.claude/agents/`, `skills/`, and `agents/` relative to the package root. If a repository places them deeper — for example `plugins/kit-core/skills/autonomous-loop/SKILL.md` — use `base` to tell agm where to start discovery:

```json
{
  "skills": {
    "@git/peakdong68/toolkit-agent-skills": {
      "version": "abc1234...",
      "base": "plugins/kit-core",
      "pick": ["autonomous-loop"]
    }
  }
}
```

`base` can also be a glob pattern to scan multiple directories at once:

```json
{
  "skills": {
    "@git/peakdong68/toolkit-agent-skills": {
      "version": "abc1234...",
      "base": "plugins/*",
      "pick": ["autonomous-loop", "extra-skill"]
    }
  }
}
```

`base` is relative to the package root and only affects auto-discovery. It is ignored when the package has an explicit `agm.package.json` manifest. `pick` and `omit` still match against the detected glob paths relative to the package root.

## Supported Tools

| Tool | Adapter |
|------|---------|
| Claude | `.claude/skills/`, `.claude/agents/` |

## Release

```bash
git tag agm-v0.1.0
git push origin agm-v0.1.0
```

CI builds 6 platforms (linux/macos/windows × x86_64/aarch64/riscv64) and publishes to GitHub Releases.
