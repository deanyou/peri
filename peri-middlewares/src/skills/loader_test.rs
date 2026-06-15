    fn write_skill(dir: &Path, name: &str, desc: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let content = format!(
            "---\nname: '{}'\ndescription: '{}'\n---\n\n# {}\n\nContent here.\n",
            name, desc, name
        );
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn test_load_skill_metadata() {
        let dir = tempdir().unwrap();
        write_skill(dir.path(), "my-skill", "A test skill");
        let skill_file = dir.path().join("my-skill").join("SKILL.md");
        let meta = load_skill_metadata(&skill_file).unwrap();
        assert_eq!(meta.name, "my-skill");
        assert_eq!(meta.description, "A test skill");
    }

    #[test]
    fn test_list_skills_dedup() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        write_skill(dir1.path(), "skill-a", "from dir1");
        write_skill(dir1.path(), "skill-b", "from dir1");
        write_skill(dir2.path(), "skill-a", "from dir2"); // 重复，应被忽略
        write_skill(dir2.path(), "skill-c", "from dir2");

        let skills = list_skills(&[dir1.path().to_path_buf(), dir2.path().to_path_buf()]);
        assert_eq!(skills.len(), 3);

        let skill_a = skills.iter().find(|s| s.name == "skill-a").unwrap();
        assert_eq!(skill_a.description, "from dir1"); // dir1 优先
    }

    #[test]
    fn test_resolve_skill_dirs_returns_standard_paths() {
        let cwd = "/tmp/test-project";
        let dirs = resolve_skill_dirs(cwd, &[]);
        // 应包含用户目录和项目目录
        assert!(dirs.iter().any(|d| d.ends_with(".claude/skills")), "应包含 ~/.claude/skills");
        assert!(dirs.iter().any(|d| d == &PathBuf::from("/tmp/test-project/.claude/skills")), "应包含项目 .claude/skills");
    }

    #[test]
    fn test_resolve_skill_dirs_includes_extra_dirs() {
        let extra = tempfile::tempdir().unwrap();
        let dirs = resolve_skill_dirs("/tmp", &[extra.path().to_path_buf()]);
        assert!(dirs.contains(&extra.path().to_path_buf()), "应包含 extra_dirs 中的目录");
    }

    #[test]
    fn test_resolve_skill_dirs_skips_nonexistent_extra_dirs() {
        let dirs = resolve_skill_dirs("/tmp", &[PathBuf::from("/nonexistent/path")]);
        assert!(!dirs.iter().any(|d| d.to_str() == Some("/nonexistent/path")), "不存在的 extra_dirs 应被跳过");
    }
