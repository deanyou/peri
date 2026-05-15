    #[test]
    fn test_highlight_diff_add() {
        let spans = highlight_diff_line("+ added line");
        assert!(!spans.is_empty());
        assert_eq!(spans[0].style.fg, Some(DIFF_ADD_COLOR));
    }

    #[test]
    fn test_highlight_diff_remove() {
        let spans = highlight_diff_line("- removed line");
        assert!(!spans.is_empty());
        assert_eq!(spans[0].style.fg, Some(DIFF_REMOVE_COLOR));
    }

    #[test]
    fn test_highlight_diff_hunk() {
        let spans = highlight_diff_line("@@ -1,3 +1,4 @@");
        assert!(!spans.is_empty());
        assert_eq!(spans[0].style.fg, Some(DIFF_HUNK_COLOR));
    }

    #[test]
    fn test_is_diff_true() {
        let text = "some line\n@@ -1,3 +1,4 @@\n+ added";
        assert!(is_diff_content(text));
    }

    #[test]
    fn test_is_diff_false() {
        let text = "fn main() {\n    println!(\"hello\");\n}";
        assert!(!is_diff_content(text));
    }
