use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use peri_widgets::{BorderedPanel, ScrollState, ScrollableArea};

use crate::app::plugin_panel::{DiscoverDetailAction, PluginPanel, PluginPanelView};
use crate::app::App;
use crate::ui::theme;

mod plugin_render;

pub fn render_plugin_panel(f: &mut Frame, panel: &PluginPanel, app: &mut App, area: Rect) {
    let is_detail = panel.is_detail();

    if is_detail {
        let is_add_marketplace = panel.add_marketplace_active;
        if is_add_marketplace {
            render_add_marketplace(f, panel, app, area);
            return;
        }

        let is_discover_detail = panel.discover_detail_index.is_some();
        if is_discover_detail {
            render_discover_detail(f, panel, app, area);
        } else {
            plugin_render::detail::render_detail(f, panel, app, area);
        }
    } else {
        let is_discover = panel.view == PluginPanelView::Discover;
        if is_discover {
            render_discover_list(f, panel, app, area);
        } else {
            plugin_render::list::render_list(f, panel, app, area);
        }
    }
}

fn render_discover_detail(f: &mut Frame, panel: &PluginPanel, app: &mut App, area: Rect) {
    let (lines, scroll_offset) = {
        let plugin_idx = match panel.discover_detail_index {
            Some(i) => i,
            None => return,
        };
        let filtered = panel.discover_filtered_plugins();
        let plugin = match filtered.get(plugin_idx) {
            Some(p) => p,
            None => return,
        };
        let scroll_offset = panel.discover_list.scroll_offset();
        let detail_cursor = panel.discover_detail_cursor;
        let mut lines: Vec<Line> = Vec::new();

        // Header
        let header_text = if plugin.marketplace.is_empty() {
            plugin.name.clone()
        } else {
            format!("{} @ {}", plugin.name, plugin.marketplace)
        };
        lines.push(Line::from(Span::styled(
            format!("  {}", header_text),
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD),
        )));

        // Version
        lines.push(detail_kv_line("Version:", &plugin.version));

        // Description
        if !plugin.description.is_empty() {
            lines.push(Line::from(""));
            for desc_line in plugin.description.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {}", desc_line),
                    Style::default().fg(theme::MUTED),
                )));
            }
        }

        // Author
        if let Some(ref author) = plugin.author {
            lines.push(Line::from(""));
            lines.push(detail_kv_line("Author:", author));
        }

        // Status
        lines.push(Line::from(""));
        let (status_icon, status_style, status_text) = if plugin.installed {
            ("\u{2714}", Style::default().fg(theme::SAGE), "Installed")
        } else {
            (
                "\u{25CB}",
                Style::default().fg(theme::MUTED),
                "Not installed",
            )
        };
        lines.push(Line::from(vec![
            Span::styled("  Status: ".to_string(), Style::default().fg(theme::MUTED)),
            Span::styled(format!("{} {}", status_icon, status_text), status_style),
        ]));

        // Action menu
        lines.push(Line::from(""));
        lines.push(Line::from(""));

        let actions = if plugin.installed {
            &[DiscoverDetailAction::BackToList] as &[DiscoverDetailAction]
        } else {
            &DiscoverDetailAction::ALL
        };

        for (i, action) in actions.iter().enumerate() {
            let is_cursor = i == detail_cursor;
            let cursor_char = if is_cursor { "\u{276F} " } else { "  " };
            let label = action.label();
            let style = if is_cursor {
                Style::default()
                    .fg(theme::THINKING)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    cursor_char.to_string(),
                    Style::default().fg(theme::THINKING),
                ),
                Span::styled(label.to_string(), style),
            ]));
        }

        (lines, scroll_offset)
    };

    let inner = BorderedPanel::new(Span::styled(
        " Plugins ",
        Style::default()
            .fg(theme::THINKING)
            .add_modifier(Modifier::BOLD),
    ))
    .border_style(Style::default().fg(theme::BORDER))
    .render(f, area);

    app.session_mgr.sessions[app.session_mgr.active]
        .ui
        .panel_area = Some(inner);
    app.session_mgr.sessions[app.session_mgr.active]
        .ui
        .panel_scroll_offset = 0;
    app.session_mgr.sessions[app.session_mgr.active]
        .ui
        .panel_plain_lines = lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();

    let mut scroll_state = ScrollState::with_offset(scroll_offset);
    ScrollableArea::new(Text::from(lines))
        .scrollbar_style(Style::default().fg(theme::MUTED))
        .render(f, inner, &mut scroll_state);
}

/// Tab 行占用的固定高度（Tab 行 + 空行）
const DISCOVER_TAB_OVERHEAD: u16 = 2;
/// 搜索框占用的固定高度（搜索框 3 行 + 空行 1 行）
const DISCOVER_SEARCH_OVERHEAD: u16 = 4;
/// Tab + 搜索框合计固定高度
const DISCOVER_FIXED_OVERHEAD: u16 = DISCOVER_TAB_OVERHEAD + DISCOVER_SEARCH_OVERHEAD; // 6

/// 渲染搜索框到固定区域（不参与滚动）
fn render_discover_search_box(f: &mut Frame, panel: &PluginPanel, area: Rect) {
    if area.width < 4 || area.height < 3 {
        return;
    }

    let search_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if panel.discover_searching {
            theme::ACCENT
        } else {
            theme::DIM
        }));

    let search_inner = search_block.inner(area);

    let query_val = panel.discover_search.value();
    let content_line = if query_val.is_empty() && !panel.discover_searching {
        Line::from(vec![
            Span::styled(" \u{2315} ", Style::default().fg(theme::MUTED)),
            Span::styled("Search plugins\u{2026}", Style::default().fg(theme::DIM)),
        ])
    } else {
        let mut spans = vec![
            Span::styled(" \u{2315} ", Style::default().fg(theme::MUTED)),
            Span::styled(
                panel.discover_search.display_text('\u{2022}'),
                Style::default().fg(theme::TEXT),
            ),
        ];
        if panel.discover_searching {
            spans.push(Span::styled("\u{2588}", Style::default().fg(theme::TEXT)));
        }
        Line::from(spans)
    };

    let search_para = Paragraph::new(content_line);
    f.render_widget(search_block, area);
    f.render_widget(search_para, search_inner);
}

/// Discover 视图：Tab 行 -> 搜索框（固定） -> 可滚动插件列表（带跟随）
fn render_discover_list(f: &mut Frame, panel: &PluginPanel, app: &mut App, area: Rect) {
    // Tab 行 Spans
    let tab_labels: Vec<Span> = PluginPanelView::ALL
        .iter()
        .map(|v| {
            let label = v.label();
            let is_active = panel.view == *v;
            let style = if is_active {
                Style::default()
                    .fg(theme::TEXT)
                    .bg(theme::THINKING)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::MUTED)
            };
            Span::styled(format!(" {} ", label), style)
        })
        .collect();

    let title_text = if panel.discover_loading {
        " Plugins \u{2026} "
    } else {
        " Plugins "
    };

    let inner = BorderedPanel::new(Span::styled(
        title_text,
        Style::default()
            .fg(theme::THINKING)
            .add_modifier(Modifier::BOLD),
    ))
    .border_style(Style::default().fg(theme::BORDER))
    .render(f, area);

    let tab_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: DISCOVER_TAB_OVERHEAD,
    };
    let search_area = Rect {
        x: inner.x + 1,
        y: inner.y + DISCOVER_TAB_OVERHEAD,
        width: inner.width.saturating_sub(2),
        height: 3,
    };
    let list_area = Rect {
        x: inner.x,
        y: inner.y + DISCOVER_FIXED_OVERHEAD,
        width: inner.width,
        height: inner.height.saturating_sub(DISCOVER_FIXED_OVERHEAD),
    };

    let tab_para = Paragraph::new(vec![Line::from(tab_labels), Line::from("")]);
    f.render_widget(tab_para, tab_area);

    render_discover_search_box(f, panel, search_area);

    let mut lines: Vec<Line> = Vec::new();

    let filtered = panel.discover_filtered_plugins();
    let max_name_width = list_area.width.saturating_sub(8) as usize;

    if panel.discover_loading && filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Loading marketplace data\u{2026}",
            Style::default().fg(theme::MUTED),
        )));
    } else if filtered.is_empty() {
        let msg = if panel.discover_search.value().is_empty() {
            "  No plugins available"
        } else {
            "  No matching plugins"
        };
        lines.push(Line::from(Span::styled(
            msg.to_string(),
            Style::default().fg(theme::MUTED),
        )));
    } else {
        for (i, plugin) in filtered.iter().enumerate() {
            let is_cursor = i == panel.discover_list.cursor();
            let is_selected = panel.discover_selected.contains(&plugin.plugin_id);
            let is_installing = panel.installing.contains(&plugin.plugin_id);
            let is_uninstalling = panel.uninstalling.contains(&plugin.plugin_id);
            let cursor_char = if is_cursor { "\u{276F} " } else { "  " };
            let check_char = if is_selected { "\u{25C9}" } else { "\u{25CB}" };

            let name_style = if is_cursor {
                Style::default()
                    .fg(theme::THINKING)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT)
            };

            let display_name = truncate_display(&plugin.name, max_name_width);

            let mut spans = vec![
                Span::styled(
                    cursor_char.to_string(),
                    Style::default().fg(theme::THINKING),
                ),
                Span::styled(
                    format!("{} ", check_char),
                    if is_selected {
                        Style::default().fg(theme::ACCENT)
                    } else {
                        Style::default().fg(theme::MUTED)
                    },
                ),
                Span::styled(display_name.clone(), name_style),
            ];

            if !plugin.marketplace.is_empty() {
                spans.push(Span::styled(
                    format!(" \u{00B7} {}", plugin.marketplace),
                    Style::default().fg(theme::MUTED),
                ));
            }

            let mut right_parts: Vec<Span> = Vec::new();

            if let Some(count) = plugin.install_count {
                right_parts.push(Span::styled(
                    format!(
                        " {} {} installs",
                        peri_middlewares::plugin::format_install_count(count),
                        "\u{00B7}"
                    ),
                    Style::default().fg(theme::MUTED),
                ));
            }

            if is_installing {
                right_parts.push(Span::styled(
                    " installing\u{2026}",
                    Style::default().fg(theme::WARNING),
                ));
            } else if is_uninstalling {
                right_parts.push(Span::styled(
                    " uninstalling\u{2026}",
                    Style::default().fg(theme::WARNING),
                ));
            } else if plugin.installed {
                right_parts.push(Span::styled(" \u{2714}", Style::default().fg(theme::SAGE)));
            }

            if !right_parts.is_empty() {
                let content_width: usize = spans
                    .iter()
                    .map(|s| unicode_width::UnicodeWidthStr::width(&*s.content))
                    .sum();
                let right_width: usize = right_parts
                    .iter()
                    .map(|s| unicode_width::UnicodeWidthStr::width(&*s.content))
                    .sum();
                let available_width = list_area.width.saturating_sub(2) as usize;
                let padding = if content_width + right_width < available_width {
                    " ".repeat(available_width.saturating_sub(content_width + right_width))
                } else {
                    " ".repeat(2)
                };
                spans.push(Span::styled(padding, Style::default()));
                spans.extend(right_parts);
            }

            lines.push(Line::from(spans));

            let desc_width = list_area.width.saturating_sub(6) as usize;
            let desc = if plugin.description.is_empty() {
                String::new()
            } else {
                truncate_display(&plugin.description, desc_width)
            };
            if !desc.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("     ", Style::default()),
                    Span::styled(desc, Style::default().fg(theme::MUTED)),
                ]));
            } else {
                lines.push(Line::from(""));
            }
        }
    }

    let cursor_row = (panel.discover_list.cursor() * 2) as u16;
    let visible_height = list_area.height;
    let mut scroll_state = ScrollState::with_offset(panel.discover_list.scroll_offset());
    scroll_state.ensure_visible(cursor_row, visible_height);

    if let Some(p) = app.global_panels.get_mut::<PluginPanel>() {
        p.discover_list.set_scroll_offset(scroll_state.offset());
    }

    app.session_mgr.sessions[app.session_mgr.active]
        .ui
        .panel_area = Some(inner);
    app.session_mgr.sessions[app.session_mgr.active]
        .ui
        .panel_scroll_offset = 0;
    app.session_mgr.sessions[app.session_mgr.active]
        .ui
        .panel_plain_lines = lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();

    ScrollableArea::new(Text::from(lines))
        .scrollbar_style(Style::default().fg(theme::MUTED))
        .render(f, list_area, &mut scroll_state);
}

/// 基于显示宽度的安全截断
fn truncate_display(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if UnicodeWidthStr::width(s) <= max_width {
        s.to_string()
    } else {
        let mut width = 0;
        let end = s
            .char_indices()
            .find(|&(_, c)| {
                width += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                width > max_width.saturating_sub(1)
            })
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}\u{2026}", &s[..end])
    }
}

fn detail_kv_line<'a>(key: &str, value: &str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {}: ", key), Style::default().fg(theme::MUTED)),
        Span::styled(value.to_string(), Style::default().fg(theme::TEXT)),
    ])
}

/// 渲染 Add Marketplace 面板
fn render_add_marketplace(f: &mut Frame, panel: &PluginPanel, app: &mut App, area: Rect) {
    let input_value = panel.add_marketplace_input.value();
    let display_text = panel.add_marketplace_input.display_text('\u{2022}');

    let inner = BorderedPanel::new(Span::styled(
        " Add Marketplace ",
        Style::default()
            .fg(theme::THINKING)
            .add_modifier(Modifier::BOLD),
    ))
    .border_style(Style::default().fg(theme::BORDER))
    .render(f, area);

    app.session_mgr.sessions[app.session_mgr.active]
        .ui
        .panel_area = Some(inner);

    let mut lines = Vec::new();

    lines.push(Line::from(""));

    lines.push(Line::from(vec![Span::styled(
        "  Enter marketplace source:",
        Style::default().fg(theme::TEXT),
    )]));

    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("Examples:", Style::default().fg(theme::MUTED)),
    ]));

    let examples = [
        ("owner/repo", "GitHub"),
        ("git@github.com:owner/repo.git", "SSH"),
        ("https://example.com/marketplace.json", ""),
        ("./path/to/marketplace", ""),
    ];

    for (example, desc) in &examples {
        if desc.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("   \u{00B7} ", Style::default().fg(theme::MUTED)),
                Span::styled(*example, Style::default().fg(theme::MUTED)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("   \u{00B7} ", Style::default().fg(theme::MUTED)),
                Span::styled(*example, Style::default().fg(theme::MUTED)),
                Span::styled(format!(" ({})", desc), Style::default().fg(theme::MUTED)),
            ]));
        }
    }

    lines.push(Line::from(""));

    let input_line = if input_value.is_empty() {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("\u{2588}", Style::default().fg(theme::TEXT)),
        ])
    } else {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(display_text, Style::default().fg(theme::TEXT)),
            Span::styled("\u{2588}", Style::default().fg(theme::TEXT)),
        ])
    };
    lines.push(input_line);

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled(
            "Enter to add",
            Style::default()
                .fg(theme::MUTED)
                .add_modifier(Modifier::ITALIC),
        ),
        Span::styled(" \u{00B7} ", Style::default().fg(theme::MUTED)),
        Span::styled(
            "Esc to cancel",
            Style::default()
                .fg(theme::MUTED)
                .add_modifier(Modifier::ITALIC),
        ),
    ]));

    app.session_mgr.sessions[app.session_mgr.active]
        .ui
        .panel_plain_lines = lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}
