    async fn render_headless_model_no_provider() -> (App, crate::ui::headless::HeadlessHandle) {
        let (mut app, mut handle) = App::new_headless(120, 30).await;
        let mut list = crate::app::panel_list::PanelList::new();
        list.set_items(vec![AliasTab::Opus, AliasTab::Sonnet, AliasTab::Haiku]);
        let panel = ModelPanel {
            provider_name: String::new(),
            active_tab: AliasTab::Opus,
            buf_thinking_effort: "medium".to_string(),
            list,
        };
        app.session_mgr.sessions[app.session_mgr.active]
            .session_panels
            .open(crate::app::panel_manager::PanelState::Model(panel.clone()));
        handle
            .terminal
            .draw(|f| crate::ui::main_ui::render(f, &mut app))
            .unwrap();
        (app, handle)
    }

    #[tokio::test]
    async fn test_model_panel_renders_select_model_title() {
        let (_, handle) = render_headless_model_no_provider().await;
        let snap = handle.snapshot().join("\n");
        assert!(
            snap.contains("Select model"),
            "Panel should show 'Select model' title, got:\n{}",
            snap
        );
    }

    #[tokio::test]
    async fn test_model_panel_shows_effort() {
        let (_, handle) = render_headless_model_no_provider().await;
        let snap = handle.snapshot().join("\n");
        assert!(
            snap.contains("effort"),
            "Panel should show effort setting, got:\n{}",
            snap
        );
    }
