# ACP session/load 后历史消息不显示（Zed）

**状态**：Open
**优先级**：中
**创建日期**：2026-06-14

## 问题描述

通过 Zed 编辑器（settings.json 配置 perihelion 作为 ACP Agent）使用时，在 agent panel 的历史会话列表中点击切换到一个历史会话后，历史聊天消息不显示，界面看起来像空白新会话。用户期望看到之前的完整对话记录。

## 症状详情

| 维度 | 观察 |
|------|------|
| 客户端 | Zed 编辑器（通过 settings.json 配置 perihelion 作为 Agent，ACP stdio 模式） |
| 操作 | 在 Zed agent panel 历史列表中点击一个历史会话进行切换 |
| 期望 | 加载该会话后显示之前的完整聊天记录 |
| 实际 | 历史消息不显示，界面呈现为空白会话 |
| 频率 | 目前只试过一次，不确定是否必现 |
| 后续对话 | 未测试——不确定加载后 agent 是否仍记得之前的上下文 |

## 复现条件

- **复现频率**：未知（仅试过一次）
- **触发步骤**：
  1. 在 Zed 中通过 ACP stdio 模式连接 perihelion
  2. 进行若干轮对话后关闭或切走
  3. 在 Zed agent panel 的历史会话列表中点击该历史会话
  4. 观察消息区域——历史消息未显示
- **环境**：macOS，Zed（版本未知），perihelion 通过 Zed settings.json 配置为 Agent

## 涉及文件

- `peri-acp/src/dispatch/session_load.rs` —— ACP session/load 请求处理入口，从 ThreadStore 加载会话消息历史

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-14 | — | Open | agent | 创建 |

## 修复记录

（由 fix-issue 或 issue-verify skill 追加，创建时留空）
