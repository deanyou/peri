//! Unified diff 解析器
//! 将标准 unified diff 字符串解析为结构化的 Hunk 列表。

/// diff 中的单行类型
#[derive(Debug, Clone, PartialEq)]
pub enum DiffLine {
    Context(String), // ' ' 前缀
    Remove(String),  // '-' 前缀
    Add(String),     // '+' 前缀
}

/// hunk header 信息
#[derive(Debug, Clone)]
pub struct HunkHeader {
    pub old_start: usize, // @@ -L,N 中的 L
    pub old_count: usize, // @@ -L,N 中的 N
    pub new_start: usize, // @@ +L,N 中的 L
    pub new_count: usize, // @@ +L,N 中的 N
}

/// 单个 hunk
#[derive(Debug, Clone)]
pub struct Hunk {
    pub header: HunkHeader,
    pub lines: Vec<DiffLine>,
}

/// 单个 patch（一个文件的完整 diff）
#[derive(Debug, Clone)]
pub struct ParsedPatch {
    pub hunks: Vec<Hunk>,
}

/// 解析错误
#[derive(Debug)]
pub enum ParseError {
    NoHunkFound,
    InvalidHunkHeader(String),
    EmptyPatch,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::NoHunkFound => write!(f, "diff 中未找到 hunk（缺少 @@ 标记）"),
            ParseError::InvalidHunkHeader(s) => write!(f, "无效的 hunk header: {}", s),
            ParseError::EmptyPatch => write!(f, "diff 内容为空"),
        }
    }
}

/// 解析 unified diff 字符串为 ParsedPatch
pub fn parse_unified_diff(diff: &str) -> Result<ParsedPatch, ParseError> {
    if diff.trim().is_empty() {
        return Err(ParseError::EmptyPatch);
    }

    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current_lines: Vec<DiffLine> = Vec::new();
    let mut current_header: Option<HunkHeader> = None;
    let mut found_hunk = false;

    for line in diff.lines() {
        // 跳过 --- / +++ 头部
        if line.starts_with("--- ") || line.starts_with("+++ ") {
            continue;
        }

        // 解析 hunk header
        if line.starts_with("@@") {
            // 保存前一个 hunk
            if let Some(header) = current_header.take() {
                hunks.push(Hunk {
                    header,
                    lines: std::mem::take(&mut current_lines),
                });
            }

            let header = parse_hunk_header(line)?;
            current_header = Some(header);
            found_hunk = true;
            continue;
        }

        // 解析 diff 行（仅在 hunk 内）
        if current_header.is_some() {
            if let Some(ch) = line.chars().next() {
                match ch {
                    ' ' => current_lines.push(DiffLine::Context(line[1..].to_string())),
                    '-' => current_lines.push(DiffLine::Remove(line[1..].to_string())),
                    '+' => current_lines.push(DiffLine::Add(line[1..].to_string())),
                    _ => {
                        // `\ No newline at end of file` 等元信息行，跳过
                    }
                }
            }
        }
    }

    // 保存最后一个 hunk
    if let Some(header) = current_header.take() {
        hunks.push(Hunk {
            header,
            lines: current_lines,
        });
    }

    if !found_hunk {
        return Err(ParseError::NoHunkFound);
    }

    Ok(ParsedPatch { hunks })
}

/// 解析 hunk header: @@ -L,N +L,N @@
fn parse_hunk_header(line: &str) -> Result<HunkHeader, ParseError> {
    let re = regex::Regex::new(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")
        .map_err(|e| ParseError::InvalidHunkHeader(e.to_string()))?;

    let caps = re
        .captures(line)
        .ok_or_else(|| ParseError::InvalidHunkHeader(line.to_string()))?;

    let old_start: usize = caps[1]
        .parse()
        .map_err(|e: std::num::ParseIntError| ParseError::InvalidHunkHeader(e.to_string()))?;
    let old_count: usize = caps
        .get(2)
        .map(|m| m.as_str().parse().unwrap_or(1))
        .unwrap_or(1);
    let new_start: usize = caps[3]
        .parse()
        .map_err(|e: std::num::ParseIntError| ParseError::InvalidHunkHeader(e.to_string()))?;
    let new_count: usize = caps
        .get(4)
        .map(|m| m.as_str().parse().unwrap_or(1))
        .unwrap_or(1);

    Ok(HunkHeader {
        old_start,
        old_count,
        new_start,
        new_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_hunk() {
        let diff = "--- a/file.rs\n+++ b/file.rs\n@@ -1,3 +1,3 @@\n line1\n-old\n+new\n line3";
        let patch = parse_unified_diff(diff).unwrap();
        assert_eq!(patch.hunks.len(), 1);
        let hunk = &patch.hunks[0];
        assert_eq!(hunk.header.old_start, 1);
        assert_eq!(hunk.header.old_count, 3);
        assert_eq!(hunk.header.new_start, 1);
        assert_eq!(hunk.header.new_count, 3);
        assert_eq!(hunk.lines.len(), 4);
        assert_eq!(hunk.lines[0], DiffLine::Context("line1".to_string()));
        assert_eq!(hunk.lines[1], DiffLine::Remove("old".to_string()));
        assert_eq!(hunk.lines[2], DiffLine::Add("new".to_string()));
        assert_eq!(hunk.lines[3], DiffLine::Context("line3".to_string()));
    }

    #[test]
    fn test_parse_multiple_hunks() {
        let diff = "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n@@ -10,1 +10,1 @@\n x\n-y\n+z";
        let patch = parse_unified_diff(diff).unwrap();
        assert_eq!(patch.hunks.len(), 2);
        assert_eq!(patch.hunks[0].header.old_start, 1);
        assert_eq!(patch.hunks[1].header.old_start, 10);
    }

    #[test]
    fn test_parse_hunk_without_count() {
        let diff = "@@ -5 +5 @@\n-old\n+new";
        let patch = parse_unified_diff(diff).unwrap();
        let h = &patch.hunks[0];
        assert_eq!(h.header.old_start, 5);
        assert_eq!(h.header.old_count, 1);
    }

    #[test]
    fn test_parse_empty_diff() {
        assert!(matches!(
            parse_unified_diff(""),
            Err(ParseError::EmptyPatch)
        ));
        assert!(matches!(
            parse_unified_diff("  "),
            Err(ParseError::EmptyPatch)
        ));
    }

    #[test]
    fn test_parse_no_hunk() {
        let diff = "--- a/f\n+++ b/f\njust some text";
        assert!(matches!(
            parse_unified_diff(diff),
            Err(ParseError::NoHunkFound)
        ));
    }

    #[test]
    fn test_parse_invalid_header() {
        let diff = "@@ invalid @@\n line";
        assert!(matches!(
            parse_unified_diff(diff),
            Err(ParseError::InvalidHunkHeader(_))
        ));
    }
}
