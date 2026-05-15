use super::*;

#[test]
fn test_toggle_collapse() {
    let mut state = ToolCallState::new("Read".to_string(), Color::Blue);
    assert!(state.collapsed, "Read should collapse by default");
    state.toggle_collapse();
    assert!(!state.collapsed);
}

#[test]
fn test_bash_not_collapsed_by_default() {
    let state = ToolCallState::new("Bash".to_string(), Color::Yellow);
    assert!(!state.collapsed, "Bash should not collapse by default");
}

#[test]
fn test_advance_tick() {
    let mut state = ToolCallState::new("Read".to_string(), Color::Blue);
    assert_eq!(state.tick, 0);
    state.advance_tick();
    assert_eq!(state.tick, 1);
}

#[test]
fn test_set_result_splits_lines() {
    let mut state = ToolCallState::new("Read".to_string(), Color::Blue);
    state.set_result("line1\nline2\nline3".to_string());
    assert_eq!(state.result_lines.len(), 3);
    assert_eq!(state.result_lines[0], "line1");
}

#[test]
fn test_set_result_truncates_long_output() {
    let mut state = ToolCallState::new("Read".to_string(), Color::Blue);
    let long = (0..30).map(|i| format!("line {}", i)).collect::<Vec<_>>();
    state.set_result(long.join("\n"));
    assert!(state.result_lines.len() <= collapse::MAX_RESULT_LINES);
    assert!(state.omitted_lines.is_some());
}

#[test]
fn test_status_equality() {
    assert_eq!(ToolCallStatus::Pending, ToolCallStatus::Pending);
    assert_ne!(ToolCallStatus::Pending, ToolCallStatus::Running);
    assert_ne!(ToolCallStatus::Completed, ToolCallStatus::Failed);
}
