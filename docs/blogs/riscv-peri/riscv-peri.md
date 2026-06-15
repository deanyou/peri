# Peri Code：RISC-V 上的 Coding Agent

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。`curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh | bash`

手里有一块昉星光开发板，8GB 内存，想在上面跑 Coding Agent。Claude Code 没有 RISC-V 版本，Bun 也没有。Node.js 在 RISC-V 上停留在旧版本，很多现代工具链根本跑不起来——Claude Code 依赖的原生模块直接编译失败，完全没有可用的选项。

因为 Peri Code 是用 Rust 写的，我们顺手做了 RISC-V 的交叉编译支持。装完直接 `peri` 回车进 TUI，功能全的。

## 安装

安装脚本自动识别架构，`uname -m` 返回 `riscv64` 直接拉对应包：

```bash
curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh | bash
```

GitHub Release 页面能找到 `peri-linux-riscv64.tar.gz`，和 x86_64、aarch64、macOS、Windows 的包放在一起。

## 实际体验

内存稳定在 70MB，跑了很久不涨。Claude Code 随便跑跑就好几百 MB 乃至 2GB，70MB 在 8GB 的板子上完全不叫事。

TUI 功能没有任何裁剪：鼠标滚动翻聊天记录、浏览代码输出、流式输出实时显示，跟在 x86 上没区别。`crossterm` + `ratatui` 是纯 Rust 实现，不感知 CPU 架构。模型吐字延迟的瓶颈在服务端，本地这边没有任何额外开销。

## 为什么 Node.js 生态做不到

RISC-V 上的 Node.js 生态长期滞后。官方 Node.js 二进制没有 RISC-V 构建，发行版自带的往往是老版本，许多现代原生模块（特别是需要 N-API 或预编译 `.node` 文件的）直接无法使用。Claude Code 和 Bun 都没有 `linux-riscv64` 的构建产物，上游也没有明确的支持计划。

Rust 这边情况完全不同。`rustup target add riscv64gc-unknown-linux-gnu` 一行准备好交叉编译环境，在 x86 开发机上直接出 RISC-V 的包，无需在板子上本地编译。Peri Code 项目里没有内联汇编、没有 SIMD、没有架构专属的原生代码。仅有的 C 依赖——`libsqlite3-sys` 是纯 C89 源码编译——对目标架构无感知。

## 用 Peri 开发板子

Peri Code 支持 RISC-V 之后，我直接用板子上的 Peri Code 来开发 Peri Code 本身。SSH 进昉星光，启动 Peri，让它帮我写代码、跑测试、提 issue。

有几件事挺有意思。一是板子 CPU 性能确实有限，编译 Rust 项目得等一会儿，但 Peri Code 本身的响应丝毫不受影响——它的延迟完全取决于模型服务端，本地只是一个 TUI 壳。挂在 SSH 里连着跑了几个小时，内存一直在 70MB 附近晃，没有任何漂移的迹象。

二是鼠标在 SSH 终端里也能用。滚动翻代码输出、点击选择消息，`crossterm` 的鼠标事件在 RISC-V 的终端里跟在本机上完全一致。这个有点出乎意料，毕竟这块板子的软件生态在很多地方还有坑。

三是有一次让 Peri 在板子上帮我修一个并发 bug，它用 SubAgent 并行跑了三个搜索任务。RISC-V 板子的四核同时跑着三个 SubAgent，内存峰值也没超过 120MB。同样的任务在 Claude Code 上早就几百 MB 起跳了。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)