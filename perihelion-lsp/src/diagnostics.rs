use crate::protocol::lsp_types::PublishDiagnosticsParams;
use lsp_types::DiagnosticSeverity as LspDiagnosticSeverity;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// 单条诊断的精简表示
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticEntry {
    pub file_uri: String,
    pub line: u32,
    pub character: u32,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum DiagnosticSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

impl From<LspDiagnosticSeverity> for DiagnosticSeverity {
    fn from(s: LspDiagnosticSeverity) -> Self {
        match s {
            LspDiagnosticSeverity::ERROR => DiagnosticSeverity::Error,
            LspDiagnosticSeverity::WARNING => DiagnosticSeverity::Warning,
            LspDiagnosticSeverity::INFORMATION => DiagnosticSeverity::Information,
            LspDiagnosticSeverity::HINT => DiagnosticSeverity::Hint,
            _ => DiagnosticSeverity::Information,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DiagnosticSummary {
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
    pub hints: usize,
    pub files_with_errors: usize,
}

impl DiagnosticSummary {
    pub fn total(&self) -> usize {
        self.errors + self.warnings + self.info + self.hints
    }

    pub fn has_issues(&self) -> bool {
        self.total() > 0
    }
}

const MAX_DIAGNOSTICS_PER_FILE: usize = 10;
const MAX_TOTAL_DIAGNOSTICS: usize = 30;
const LRU_CACHE_CAPACITY: usize = 500;

/// 诊断注册表（被动推送 + 去重 + 限流）
pub struct DiagnosticsRegistry {
    /// 当前活跃诊断（按文件 URI 索引）
    current: RwLock<HashMap<String, Vec<DiagnosticEntry>>>,
    /// 跨轮次已推送诊断的 key（用于去重）
    delivered: Mutex<lru::LruCache<String, HashSet<String>>>,
    /// 事件回调
    on_update: RwLock<Option<Box<dyn Fn(Vec<DiagnosticEntry>) + Send + Sync>>>,
}

impl DiagnosticsRegistry {
    pub fn new() -> Self {
        Self {
            current: RwLock::new(HashMap::new()),
            delivered: Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(LRU_CACHE_CAPACITY).unwrap(),
            )),
            on_update: RwLock::new(None),
        }
    }

    /// 注册更新回调
    pub fn on_update(&self, callback: Box<dyn Fn(Vec<DiagnosticEntry>) + Send + Sync>) {
        *self.on_update.write() = Some(callback);
    }

    /// 处理 textDocument/publishDiagnostics 通知
    pub fn handle_publish_diagnostics(&self, params: &PublishDiagnosticsParams) {
        let uri = params.uri.to_string();

        // 诊断为空数组表示清除
        if params.diagnostics.is_empty() {
            self.current.write().remove(&uri);
            self.delivered.lock().pop(&uri);
            return;
        }

        // 转换并排序
        let mut entries: Vec<DiagnosticEntry> = params
            .diagnostics
            .iter()
            .map(|d| DiagnosticEntry {
                file_uri: uri.clone(),
                line: d.range.start.line + 1, // 0-based -> 1-based
                character: d.range.start.character + 1, // 0-based -> 1-based
                severity: d
                    .severity
                    .unwrap_or(LspDiagnosticSeverity::INFORMATION)
                    .into(),
                message: d.message.clone(),
                source: d.source.clone(),
            })
            .collect();

        // 按严重程度排序
        entries.sort_by_key(|e| e.severity);

        // 每文件限流
        entries.truncate(MAX_DIAGNOSTICS_PER_FILE);

        // 去重：过滤掉已推送的诊断
        let mut new_entries = Vec::new();
        {
            let mut delivered = self.delivered.lock();
            let file_delivered = delivered.get_or_insert_mut(uri.clone(), HashSet::new);

            for entry in entries.into_iter() {
                let key = format!(
                    "{:?}:{}:{}:{}",
                    entry.severity, entry.line, entry.character, entry.message
                );
                if !file_delivered.contains(&key) {
                    file_delivered.insert(key);
                    new_entries.push(entry);
                }
            }
        }

        if new_entries.is_empty() {
            return;
        }

        // 更新当前诊断
        {
            let mut current = self.current.write();
            current.insert(uri.clone(), new_entries.clone());
        }

        // 总量限流
        let all_entries = {
            let current = self.current.read();
            let mut all: Vec<DiagnosticEntry> = current.values().flatten().cloned().collect();
            all.sort_by_key(|e| e.severity);
            all.truncate(MAX_TOTAL_DIAGNOSTICS);
            all
        };

        // 触发回调
        if let Some(ref callback) = *self.on_update.read() {
            callback(new_entries);
        }

        let _ = all_entries; // 可用于后续主动查询
    }

    /// 主动查询指定文件的诊断
    pub fn get_for_file(&self, uri: &str) -> Vec<DiagnosticEntry> {
        self.current.read().get(uri).cloned().unwrap_or_default()
    }

    /// 获取所有活跃诊断
    pub fn get_all(&self) -> Vec<DiagnosticEntry> {
        let current = self.current.read();
        let mut all: Vec<DiagnosticEntry> = current.values().flatten().cloned().collect();
        all.sort_by_key(|e| e.severity);
        all.truncate(MAX_TOTAL_DIAGNOSTICS);
        all
    }

    /// 获取诊断统计
    pub fn summary(&self) -> DiagnosticSummary {
        let current = self.current.read();
        let mut summary = DiagnosticSummary::default();
        for entries in current.values() {
            let has_error = entries
                .iter()
                .any(|e| e.severity == DiagnosticSeverity::Error);
            if has_error {
                summary.files_with_errors += 1;
            }
            for e in entries {
                match e.severity {
                    DiagnosticSeverity::Error => summary.errors += 1,
                    DiagnosticSeverity::Warning => summary.warnings += 1,
                    DiagnosticSeverity::Information => summary.info += 1,
                    DiagnosticSeverity::Hint => summary.hints += 1,
                }
            }
        }
        summary
    }

    /// 清除指定文件的诊断
    pub fn clear_for_file(&self, uri: &str) {
        self.current.write().remove(uri);
        self.delivered.lock().pop(uri);
    }

    /// 清除所有诊断
    pub fn clear_all(&self) {
        self.current.write().clear();
        self.delivered.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Diagnostic, Position, Range};

    fn make_params(
        uri: &str,
        diagnostics: Vec<(u32, u32, DiagnosticSeverity, &str)>,
    ) -> PublishDiagnosticsParams {
        PublishDiagnosticsParams {
            uri: format!("file://{uri}").parse().unwrap(),
            diagnostics: diagnostics
                .into_iter()
                .map(|(line, col, severity, msg)| Diagnostic {
                    range: Range {
                        start: Position {
                            line,
                            character: col,
                        },
                        end: Position {
                            line,
                            character: col + 5,
                        },
                    },
                    severity: Some(match severity {
                        DiagnosticSeverity::Error => LspDiagnosticSeverity::ERROR,
                        DiagnosticSeverity::Warning => LspDiagnosticSeverity::WARNING,
                        DiagnosticSeverity::Information => LspDiagnosticSeverity::INFORMATION,
                        DiagnosticSeverity::Hint => LspDiagnosticSeverity::HINT,
                    }),
                    message: msg.to_string(),
                    source: Some("test".to_string()),
                    ..Default::default()
                })
                .collect(),
            version: None,
        }
    }

    #[test]
    fn test_handle_diagnostics_basic() {
        let registry = DiagnosticsRegistry::new();
        let params = make_params(
            "/test.rs",
            vec![
                (0, 0, DiagnosticSeverity::Error, "error1"),
                (1, 0, DiagnosticSeverity::Warning, "warn1"),
            ],
        );
        registry.handle_publish_diagnostics(&params);

        let all = registry.get_all();
        assert_eq!(all.len(), 2);

        let summary = registry.summary();
        assert_eq!(summary.errors, 1);
        assert_eq!(summary.warnings, 1);
        assert_eq!(summary.files_with_errors, 1);
    }

    #[test]
    fn test_deduplication() {
        let registry = DiagnosticsRegistry::new();
        let params = make_params(
            "/test.rs",
            vec![
                (0, 0, DiagnosticSeverity::Error, "error1"),
                (1, 0, DiagnosticSeverity::Warning, "warn1"),
            ],
        );
        registry.handle_publish_diagnostics(&params);

        // 再次推送相同诊断
        registry.handle_publish_diagnostics(&params);

        // 应该只有 2 条（不重复）
        let all = registry.get_all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_per_file_limit() {
        let registry = DiagnosticsRegistry::new();
        let diagnostics: Vec<(u32, u32, DiagnosticSeverity, &str)> = (0..20)
            .map(|i| (i, 0, DiagnosticSeverity::Error, "error"))
            .collect();
        let params = make_params("/test.rs", diagnostics);
        registry.handle_publish_diagnostics(&params);

        let all = registry.get_all();
        assert_eq!(all.len(), MAX_DIAGNOSTICS_PER_FILE);
    }

    #[test]
    fn test_clear_diagnostics() {
        let registry = DiagnosticsRegistry::new();
        let params = make_params(
            "/test.rs",
            vec![(0, 0, DiagnosticSeverity::Error, "error1")],
        );
        registry.handle_publish_diagnostics(&params);
        assert_eq!(registry.get_all().len(), 1);

        // 发送空诊断清除
        let clear_params = PublishDiagnosticsParams {
            uri: "file:///test.rs".parse().unwrap(),
            diagnostics: vec![],
            version: None,
        };
        registry.handle_publish_diagnostics(&clear_params);
        assert!(registry.get_all().is_empty());
    }

    #[test]
    fn test_severity_sorting() {
        let registry = DiagnosticsRegistry::new();
        let params = make_params(
            "/test.rs",
            vec![
                (3, 0, DiagnosticSeverity::Hint, "hint"),
                (0, 0, DiagnosticSeverity::Error, "error"),
                (2, 0, DiagnosticSeverity::Information, "info"),
                (1, 0, DiagnosticSeverity::Warning, "warn"),
            ],
        );
        registry.handle_publish_diagnostics(&params);

        let all = registry.get_all();
        assert_eq!(all[0].severity, DiagnosticSeverity::Error);
        assert_eq!(all[1].severity, DiagnosticSeverity::Warning);
        assert_eq!(all[2].severity, DiagnosticSeverity::Information);
        assert_eq!(all[3].severity, DiagnosticSeverity::Hint);
    }
}
