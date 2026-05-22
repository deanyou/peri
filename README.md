# Peri

A Rust-powered AI agent that speaks Claude Code's language.

Runs on **DeepSeek-V4-Pro** and **GLM-5.1** — not locked to a single provider. Extensive harness engineering (prompt tuning, tool-calling adaptation, reasoning pipeline) ensures open-source models perform reliably. 99% of Peri's own codebase was written by them.

## Why Peri

- **Rust, not Node.js or Bun.** Fast startup, ~50MB memory footprint, no runtime overhead.
- **Any LLM, not just one.** Anthropic, OpenAI-compatible APIs — DeepSeek, GLM, whatever works for you.
- **Drop-in compatible.** Your `.claude/` agents, skills, MCP servers, hooks, and plugins just work. Zero migration.
- **IDE-ready via [ACP](https://github.com/Azure/agent-client-protocol).** Connects to [Zed](https://zed.dev) and other ACP clients out of the box. We're also building a cloud coding platform where any ACP-compatible agent can plug in and go.

## Install

Binaries available for macOS (x86_64 / Apple Silicon), Linux (x86_64 / aarch64 / riscv64), and Windows (x86_64).

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.ps1 | iex
```

## Architecture

```text
peri-tui  ──ACP──>  peri-acp  ──>  peri-agent (ReAct loop)
    │                   │               │
 peri-widgets     peri-middlewares   langfuse-client
                       │
                  peri-lsp / MCP / Plugins
```

## License

Apache 2.0
