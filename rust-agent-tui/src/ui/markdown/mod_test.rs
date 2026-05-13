use super::*;

#[test]
fn test_find_last_block_boundary_basic() {
    let text = "paragraph one\n\nparagraph two\n\nparagraph three";
    let prefix_len = "paragraph one\n\nparagraph two\n\n".len();
    let result = find_last_block_boundary(text, prefix_len);
    assert_eq!(result, "paragraph one\n\nparagraph two\n\n".len());
}

#[test]
fn test_find_last_block_boundary_code_fence() {
    // 代码围栏内跳过空行
    let text = "before\n\n```\ncode\n\nmore code\n```\nafter";
    let prefix_len = "before\n\n```\ncode\n\nmore code\n```\n".len();
    let result = find_last_block_boundary(text, prefix_len);
    assert_eq!(result, "before\n\n".len());
}

#[test]
fn test_find_last_block_boundary_unclosed_fence() {
    // 未闭合围栏：边界应在围栏前的 \n\n
    let text = "before\n\n```\nstill open";
    let prefix_len = text.len();
    let result = find_last_block_boundary(text, prefix_len);
    assert_eq!(result, "before\n\n".len());
}

#[test]
fn test_find_last_block_boundary_empty() {
    assert_eq!(find_last_block_boundary("", 0), 0);
    assert_eq!(find_last_block_boundary("hello", 5), 0);
}

#[test]
fn test_find_last_block_boundary_single_paragraph() {
    let text = "just one line";
    assert_eq!(find_last_block_boundary(text, text.len()), 0);
}

#[test]
fn test_find_last_block_boundary_prefix_at_boundary() {
    let text = "aaa\n\nbbb\n\nccc";
    let prefix_len = "aaa\n\nbbb\n\n".len();
    let result = find_last_block_boundary(text, prefix_len);
    assert_eq!(result, "aaa\n\nbbb\n\n".len());
}

#[test]
fn test_find_last_block_boundary_fence_open_close() {
    let text = "para1\n\n```\ncode\n```\n\npara2";
    let prefix_len = text.len();
    let result = find_last_block_boundary(text, prefix_len);
    assert_eq!(result, "para1\n\n```\ncode\n```\n\n".len());
}

/// 辅助：设置 dirty 标志
fn set_dirty(block: &mut ContentBlockView, value: bool) {
    if let ContentBlockView::Text { dirty, .. } = block {
        *dirty = value;
    }
}

/// 辅助：追加文本并标记 dirty
fn append_to_block(block: &mut ContentBlockView, text: &str) {
    if let ContentBlockView::Text { raw, dirty, .. } = block {
        raw.push_str(text);
        *dirty = true;
    }
}

/// 辅助：获取 rendered 行数
fn rendered_line_count(block: &ContentBlockView) -> usize {
    if let ContentBlockView::Text { rendered, .. } = block {
        rendered.lines.len()
    } else {
        0
    }
}

/// 辅助：获取 rendered_prefix_len
fn get_prefix_len(block: &ContentBlockView) -> usize {
    if let ContentBlockView::Text {
        rendered_prefix_len,
        ..
    } = block
    {
        *rendered_prefix_len
    } else {
        0
    }
}

#[test]
fn test_ensure_rendered_incremental_basic() {
    let mut block = ContentBlockView::Text {
        raw: "hello".to_string(),
        rendered: parse_markdown("hello", 80),
        dirty: false,
        rendered_prefix_len: "hello".len(),
        rendered_prefix_lines: 0,
    };
    // 先全量渲染建立基线
    set_dirty(&mut block, true);
    ensure_rendered_incremental(&mut block, 80);
    let baseline_lines = rendered_line_count(&block);

    // 追加新段落，增量解析
    append_to_block(&mut block, "\n\nworld");
    ensure_rendered_incremental(&mut block, 80);

    assert!(rendered_line_count(&block) > baseline_lines, "应该有更多行");
    assert_eq!(get_prefix_len(&block), "hello\n\nworld".len());
}

#[test]
fn test_ensure_rendered_incremental_full_fallback() {
    // rendered_prefix_len==0 且无双换行 → 走全量重解析
    let mut block = ContentBlockView::Text {
        raw: "no boundary".to_string(),
        rendered: Text::raw(""),
        dirty: true,
        rendered_prefix_len: 0,
        rendered_prefix_lines: 0,
    };
    ensure_rendered_incremental(&mut block, 80);

    assert_ne!(rendered_line_count(&block), 0, "应该有渲染输出");
    assert_eq!(get_prefix_len(&block), "no boundary".len());
}

#[test]
fn test_ensure_rendered_incremental_not_dirty() {
    // dirty=false → 直接返回，不触发渲染
    let mut block = ContentBlockView::Text {
        raw: "unchanged".to_string(),
        rendered: Text::raw(""),
        dirty: false,
        rendered_prefix_len: 0,
        rendered_prefix_lines: 0,
    };
    let lines_before = rendered_line_count(&block);
    ensure_rendered_incremental(&mut block, 80);
    assert_eq!(
        rendered_line_count(&block),
        lines_before,
        "不 dirty 时不应修改渲染"
    );
}

#[test]
fn test_ensure_rendered_incremental_no_new_content() {
    // dirty=false 且 raw.len() == rendered_prefix_len → 直接返回
    let mut block = ContentBlockView::Text {
        raw: "hello".to_string(),
        rendered: parse_markdown("hello", 80),
        dirty: false,
        rendered_prefix_len: "hello".len(),
        rendered_prefix_lines: 1,
    };
    let lines_before = rendered_line_count(&block);
    ensure_rendered_incremental(&mut block, 80);
    assert_eq!(
        rendered_line_count(&block),
        lines_before,
        "无新内容时行数不变"
    );
}

#[test]
fn test_ensure_rendered_incremental_code_block_recovery() {
    // 代码块闭合后追加新内容，增量解析应正确工作
    let mut block = ContentBlockView::Text {
        raw: "intro\n\n```\ncode\n```".to_string(),
        rendered: parse_markdown("intro\n\n```\ncode\n```", 80),
        dirty: false,
        rendered_prefix_len: "intro\n\n```\ncode\n```".len(),
        rendered_prefix_lines: 0,
    };
    // 先全量渲染
    set_dirty(&mut block, true);
    ensure_rendered_incremental(&mut block, 80);

    // 追加新内容
    append_to_block(&mut block, "\n\nnew paragraph");
    ensure_rendered_incremental(&mut block, 80);

    assert!(rendered_line_count(&block) > 0);
    assert_eq!(
        get_prefix_len(&block),
        "intro\n\n```\ncode\n```\n\nnew paragraph".len()
    );
}
