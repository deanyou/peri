//! 有状态 SSE（Server-Sent Events）解析器。
//!
//! 配合 reqwest `bytes_stream()` 使用，解析 OpenAI/Anthropic 流式响应。
//!
//! # 用法
//!
//! ```ignore
//! let mut parser = SseParser::new();
//! while let Some(chunk) = stream.next().await {
//!     for (event_type, data) in parser.push(&chunk?) {
//!         // 按协议分发事件
//!     }
//!     if parser.is_done() { break; }
//! }
//! ```

/// 有状态 SSE 解析器
pub struct SseParser {
    /// 跨 chunk 不完整行缓冲区（上一个 chunk 末尾未以 \n 结尾的部分）
    pending_line: String,
    /// 当前累积的 event type（Anthropic 格式：`event: content_block_delta`）
    event_type: Option<String>,
    /// 当前累积的 data 文本（`data:` 行内容拼接）
    data: String,
    /// [DONE] 或流终止标志
    done: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            pending_line: String::new(),
            event_type: None,
            data: String::new(),
            done: false,
        }
    }

    /// Push 新到达的字节块，返回此次推入后解析出的所有完整事件。
    /// 返回空 Vec 表示当前 chunk 内无完整事件（仍在累积中）。
    pub fn push(&mut self, bytes: &[u8]) -> Vec<(Option<String>, String)> {
        let mut events = Vec::new();

        // 将 pending_line + 新数据合并为完整文本
        let mut text = std::mem::take(&mut self.pending_line);
        text.push_str(&String::from_utf8_lossy(bytes));

        // 找到最后一个 \n，以区分完整行和不完整行
        // 仅处理完整部分（到最后一个 \n），剩余部分保存为 pending_line
        let complete_end = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let complete = &text[..complete_end];
        let incomplete = &text[complete_end..];

        // 保存不完整部分
        if !incomplete.is_empty() {
            self.pending_line = incomplete.to_string();
        }

        let lines = complete.lines();

        for mut line in lines {
            // 处理 \r\n: lines() 分割时可能残留 \r 后缀
            if line.ends_with('\r') {
                line = &line[..line.len() - 1];
            }
            // trim 掉 \r 后为空的行
            let trimmed = line.trim_end_matches('\r');

            if trimmed.is_empty() {
                // 事件边界：空行触发 commit
                if !self.data.is_empty() || self.event_type.is_some() {
                    let event = (self.event_type.take(), std::mem::take(&mut self.data));
                    events.push(event);
                }
            } else if let Some(data) = trimmed.strip_prefix("data: ") {
                if data == "[DONE]" {
                    self.done = true;
                    // [DONE] 不产出事件
                    return events;
                }
                self.data.push_str(data);
            } else if let Some(data) = trimmed.strip_prefix("data:") {
                // data: 后无空格
                if data.trim() == "[DONE]" {
                    self.done = true;
                    return events;
                }
                if !data.is_empty() {
                    self.data.push_str(data.trim_start());
                }
                // 空 data:（无内容）跳过
            } else if let Some(et) = trimmed.strip_prefix("event: ") {
                self.event_type = Some(et.to_string());
            } else if let Some(et) = trimmed.strip_prefix("event:") {
                self.event_type = Some(et.trim_start().to_string());
            }
            // 其他行（如 id:, retry:）忽略
        }

        events
    }

    /// 流是否已终止（收到 [DONE]）
    pub fn is_done(&self) -> bool {
        self.done
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_single_line_data() {
        let mut parser = SseParser::new();
        let events = parser.push(b"data: {\"key\":\"value\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, None);
        assert_eq!(events[0].1, "{\"key\":\"value\"}");
    }

    #[test]
    fn test_crlf_line_endings() {
        let mut parser = SseParser::new();
        let events = parser.push(b"data: hello\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, None);
        assert_eq!(events[0].1, "hello");
    }

    #[test]
    fn test_multi_line_data_joined() {
        let mut parser = SseParser::new();
        let events = parser.push(b"data: line1\ndata: line2\n\n");
        assert_eq!(events.len(), 1);
        // data 行被直接拼接（不添加换行符 —— 协议层自行处理）
        assert_eq!(events[0].1, "line1line2");
    }

    #[test]
    fn test_cross_chunk_line_join() {
        let mut parser = SseParser::new();
        // 第一个 chunk 以不完整行结尾
        let events1 = parser.push(b"data: {\"partial");
        assert!(events1.is_empty());

        // 第二个 chunk 补全
        let events2 = parser.push(b"_key\":\"val\"}\n\n");
        assert_eq!(events2.len(), 1);
        assert_eq!(events2[0].1, "{\"partial_key\":\"val\"}");
    }

    #[test]
    fn test_event_and_data_pair() {
        let mut parser = SseParser::new();
        let events = parser.push(b"event: content_block_delta\ndata: {\"delta\":\"text\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0.as_deref(), Some("content_block_delta"));
        assert_eq!(events[0].1, "{\"delta\":\"text\"}");
    }

    #[test]
    fn test_data_done_termination() {
        let mut parser = SseParser::new();
        let events = parser.push(b"data: {\"last\":true}\n\ndata: [DONE]\n\n");
        assert!(!events.is_empty());
        assert!(parser.is_done());
    }

    #[test]
    fn test_empty_data_line_skipped() {
        let mut parser = SseParser::new();
        let events = parser.push(b"data: \n\n");
        // 空 data: 行被跳过，不产出事件
        assert!(events.is_empty());
    }

    #[test]
    fn test_empty_stream() {
        let mut parser = SseParser::new();
        let events = parser.push(b"");
        assert!(events.is_empty());
        assert!(!parser.is_done());
    }

    #[test]
    fn test_multiple_events_in_one_chunk() {
        let mut parser = SseParser::new();
        let events = parser.push(b"data: first\n\ndata: second\n\ndata: third\n\n");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].1, "first");
        assert_eq!(events[1].1, "second");
        assert_eq!(events[2].1, "third");
    }

    #[test]
    fn test_data_without_space_after_colon() {
        let mut parser = SseParser::new();
        let events = parser.push(b"data:hello\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, "hello");
    }

    #[test]
    fn test_event_type_reset_after_commit() {
        let mut parser = SseParser::new();
        // 第一组: event + data
        let _ = parser.push(b"event: type_a\ndata: aaa\n\n");
        // 第二组: 仅 data（event_type 应已重置）
        let events = parser.push(b"data: bbb\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, None);
        assert_eq!(events[0].1, "bbb");
    }

    #[test]
    fn test_only_event_no_data_no_commit() {
        let mut parser = SseParser::new();
        // 单独 event 行不触发 commit
        let events = parser.push(b"event: content_block_start\n");
        assert!(events.is_empty());
    }

    #[test]
    fn test_done_without_space() {
        let mut parser = SseParser::new();
        let _ = parser.push(b"data:[DONE]\n\n");
        assert!(parser.is_done());
    }
}
