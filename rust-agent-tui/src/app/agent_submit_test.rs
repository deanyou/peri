use super::*;

#[test]
fn test_parse_single_skill() {
    let names = parse_skill_names_from_input("/diagnose");
    assert_eq!(names, vec!["diagnose"]);
}

#[test]
fn test_parse_multiple_skills() {
    let names = parse_skill_names_from_input("/diagnose /fix-issue /caveman");
    assert_eq!(names, vec!["diagnose", "fix-issue", "caveman"]);
}

#[test]
fn test_parse_skill_in_sentence() {
    let names = parse_skill_names_from_input("帮我用 /diagnose 调试一下这个问题");
    assert_eq!(names, vec!["diagnose"]);
}

#[test]
fn test_parse_no_skill() {
    let names = parse_skill_names_from_input("普通消息没有 skill");
    assert!(names.is_empty());
}

#[test]
fn test_parse_slash_only() {
    let names = parse_skill_names_from_input("/");
    assert!(names.is_empty());
}

#[test]
fn test_parse_hash_not_matched() {
    // # 前缀不匹配，仅 / 前缀触发
    let names = parse_skill_names_from_input("#skill-name");
    assert!(names.is_empty());
}
