# Peri 生态工具 AGM：别再手动复制 .claude/skills 了

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。<https://github.com/KonghaYao/peri>

之前装 superpowers 那 14 个 skill——每台设备上 clone 仓库，找到技能目录，一个个搬进 `.claude/skills/`。装完还得在 Claude 里跑一遍 setup 命令，换台机器整个流程重来一遍。搬完还得记住装的是哪个 commit，下次更新全忘了。

AGM 把这件事压成一行。`agm install --git https://github.com/obra/superpowers --tool claude`——回车之后，14 个 symlink（符号链接，相当于文件系统的快捷方式）到位，仓库的 commit hash 自动写进 `agm.json`。下次 `agm install` 重新跑一遍就是最新版，不用记上次装的哪个版本。AGM 自己更新也省事——`agm self-update` 跑一遍 install 脚本拿最新版。

装 AGM 本身一行就够：Unix 下 `curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/agm/install.sh | bash`，Windows 下 `irm ... | iex`。进项目目录，`agm init` 初始化，然后 `agm install --git <仓库地址> --tool claude` 一行安装一套技能。`agm list` 看一眼装了什么，不用了 `agm uninstall --package @git/obra/superpowers --tool claude` 删掉。

---

## symlink + commit hash，一份存，多处用

AGM 的存储思路很直接：每个包在 `~/.agm/store/` 里只存一份，symlink 到项目目录——不复制，不膨胀。包来源目前是 GitHub，直接用 `git clone` 拉到 store，以 commit hash 作为版本标识。Registry 分发（像 npm registry 那样的集中式包索引）也在计划中。

没有 `agm.package.json` 的仓库，AGM 自动扫描目录找 skill。从 `.claude/skills/` 到裸 `skills/` 目录都能认，mattpocock 那种 `skills/engineering/grill-me/SKILL.md` 两层嵌套也递归进去找。如果仓库里 skill 太多，还可以用 `pick`/`omit` 加 glob 范围只装需要的，避免把整仓库 skill 全塞进项目。

依赖声明收敛在一份 `agm.json` 里，结构很直白。skills、agents、mcp 三类都管，一个 `--tool claude` 参数全到位——skills 装到 `.claude/skills/`，agents 到 `.claude/agents/`，MCP 配置到对应目录，不用分别跑三个命令。

```json
{
  "name": "my-project",
  "skills": {
    "@git/obra/superpowers": "6fd4507..."
  },
  "agents": {},
  "mcp": {}
}
```

---

AGM 已经在 Peri 项目里用上了——装 superpowers 和 mattpocock 两套技能，各一行命令。更新也简单，`agm install` 重新跑一遍就是最新 commit。换设备不用再手动操作——`agm.json` 和 `agm.lock.json` 往仓库一提交，新机器 `agm install` 一把复原，连 Claude 命令都不用再跑了。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
