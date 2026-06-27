use crate::tui::app::{AppState, Phase, ToolCard, ToolCardStatus, TranscriptBlock};
use crate::tui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn render(frame: &mut Frame<'_>, state: &AppState, theme: &Theme) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.bg_base)),
        area,
    );

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(permission_height(state)),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(frame, rows[0], theme);
    render_transcript(frame, rows[1], state, theme);
    if state.pending_permission.is_some() {
        render_permission(frame, rows[2], state, theme);
    }
    render_status(frame, rows[3], state, theme);
    render_input(frame, rows[4], state, theme);
}

fn permission_height(state: &AppState) -> u16 {
    if state.pending_permission.is_some() {
        7
    } else {
        0
    }
}

fn render_header(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(theme.border_strong).bg(theme.bg_base))
        .style(Style::default().fg(theme.text_secondary).bg(theme.bg_base));
    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled(
            "✦ mysteries",
            Style::default()
                .fg(theme.text_secondary)
                .bg(theme.bg_base)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  agent · v1.0",
            Style::default().fg(theme.text_muted).bg(theme.bg_base),
        ),
    ]))
    .block(block);
    frame.render_widget(paragraph, area);
}

fn render_transcript(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let lines = if state.transcript.is_empty() && state.tool_cards.is_empty() {
        welcome_lines(theme)
    } else {
        transcript_lines(state, theme)
    };
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .style(Style::default().fg(theme.text_primary).bg(theme.bg_base))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn welcome_lines(theme: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "✦ MYSTERIES",
            Style::default()
                .fg(theme.text_title)
                .bg(theme.bg_base)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "AGENT · v1.0 · 终端编码助手",
            Style::default().fg(theme.accent_primary).bg(theme.bg_base),
        )),
        Line::from(Span::styled(
            "读只读,写必询 —— 每一次文件改动与命令执行,都先把 diff 摊给你,等你按下 y 才动手。",
            Style::default().fg(theme.text_body).bg(theme.bg_base),
        )),
        Line::from(Span::styled(
            "试试 ↓",
            Style::default().fg(theme.text_muted).bg(theme.bg_base),
        )),
        suggestion_line("任务", "给 Config 加 timeout_secs 字段", theme),
        suggestion_line("/help", "查看内置命令", theme),
        suggestion_line("/status", "当前会话快照", theme),
        suggestion_line("错误", "演示:鉴权失败(致命错误,终止 Loop)", theme),
    ]
}

fn suggestion_line(tag: &str, text: &str, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("〔{tag}〕"),
            Style::default().fg(theme.accent_primary).bg(theme.bg_base),
        ),
        Span::styled(
            format!(" {text}"),
            Style::default().fg(theme.text_primary).bg(theme.bg_base),
        ),
    ])
}

fn transcript_lines(state: &AppState, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for block in &state.transcript {
        match block {
            TranscriptBlock::User(text) => {
                lines.push(Line::from(vec![
                    Span::styled(
                        "> ",
                        Style::default().fg(theme.accent_primary).bg(theme.bg_base),
                    ),
                    Span::styled(
                        text.clone(),
                        Style::default().fg(theme.text_primary).bg(theme.bg_base),
                    ),
                ]));
            }
            TranscriptBlock::Assistant(text) => {
                lines.push(Line::from(vec![
                    Span::styled("m ", Style::default().fg(theme.info_fg).bg(theme.bg_base)),
                    Span::styled(
                        text.clone(),
                        Style::default().fg(theme.text_body).bg(theme.bg_base),
                    ),
                ]));
            }
            TranscriptBlock::Error(text) => {
                lines.push(Line::from(vec![
                    Span::styled("! ", Style::default().fg(theme.error_fg).bg(theme.bg_base)),
                    Span::styled(
                        text.clone(),
                        Style::default().fg(theme.error_fg).bg(theme.bg_base),
                    ),
                ]));
            }
        }
        lines.push(Line::from(""));
    }
    for card in &state.tool_cards {
        lines.extend(tool_card_lines(card, theme));
        lines.push(Line::from(""));
    }
    lines
}

fn tool_card_lines(card: &ToolCard, theme: &Theme) -> Vec<Line<'static>> {
    let (glyph, glyph_color) = match card.status {
        ToolCardStatus::Running => ("◇", theme.accent_primary),
        ToolCardStatus::Done => ("✓", theme.success_fg),
        ToolCardStatus::Error => ("✗", theme.error_fg),
    };
    let mut head = vec![
        Span::styled(
            "┌─ ",
            Style::default().fg(theme.border_subtle).bg(theme.bg_base),
        ),
        Span::styled(glyph, Style::default().fg(glyph_color).bg(theme.bg_base)),
        Span::styled(" ", Style::default().fg(theme.text_muted).bg(theme.bg_base)),
        Span::styled(
            card.name.clone(),
            Style::default()
                .fg(theme.accent_primary)
                .bg(theme.bg_base)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", card.args),
            Style::default().fg(theme.text_muted).bg(theme.bg_base),
        ),
    ];
    if card.readonly {
        head.push(Span::styled(
            "  只读 · 自动运行",
            Style::default().fg(theme.text_secondary).bg(theme.bg_base),
        ));
    }
    let mut lines = vec![Line::from(head)];

    match &card.output {
        Some(output) if output.is_empty() => lines.push(tool_output_line("", theme)),
        Some(output) => {
            for line in output.lines() {
                lines.push(tool_output_line(line, theme));
            }
        }
        None => lines.push(tool_output_line("运行中…", theme)),
    }

    if card.truncated {
        lines.push(Line::from(vec![
            Span::styled(
                "│ ",
                Style::default().fg(theme.border_subtle).bg(theme.bg_base),
            ),
            Span::styled(
                "⋯ 输出已截断(超出 max_output_bytes)",
                Style::default().fg(theme.warning_fg).bg(theme.bg_base),
            ),
        ]));
    }

    lines.push(Line::from(Span::styled(
        "└──────────────────────────────────────────────────────────────────────────────",
        Style::default().fg(theme.border_subtle).bg(theme.bg_base),
    )));
    lines
}

fn tool_output_line(text: &str, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "│ ",
            Style::default().fg(theme.border_subtle).bg(theme.bg_base),
        ),
        Span::styled(
            "output: ",
            Style::default().fg(theme.text_secondary).bg(theme.bg_base),
        ),
        Span::styled(
            text.to_string(),
            Style::default().fg(theme.text_body).bg(theme.bg_base),
        ),
    ])
}

fn render_permission(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let Some(request) = &state.pending_permission else {
        return;
    };
    let paragraph = Paragraph::new(vec![
        Line::from(Span::styled(
            "▲ 需要授权",
            Style::default()
                .fg(theme.warning_fg)
                .bg(theme.warning_bg)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                "tool: ",
                Style::default()
                    .fg(theme.text_secondary)
                    .bg(theme.warning_bg),
            ),
            Span::styled(
                request.tool_name.clone(),
                Style::default()
                    .fg(theme.accent_primary)
                    .bg(theme.warning_bg),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "args: ",
                Style::default()
                    .fg(theme.text_secondary)
                    .bg(theme.warning_bg),
            ),
            Span::styled(
                request.args.to_string(),
                Style::default().fg(theme.text_body).bg(theme.warning_bg),
            ),
        ]),
        Line::from(Span::styled(
            "[y · 允许]   [n · 拒绝]",
            Style::default().fg(theme.warning_fg).bg(theme.warning_bg),
        )),
        Line::from(Span::styled(
            "提示:Enter = 允许 · Esc = 拒绝",
            Style::default()
                .fg(theme.text_secondary)
                .bg(theme.warning_bg),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.warning_fg).bg(theme.warning_bg))
            .style(Style::default().fg(theme.text_primary).bg(theme.warning_bg)),
    )
    .style(Style::default().fg(theme.text_primary).bg(theme.warning_bg));
    frame.render_widget(paragraph, area);
}

fn render_status(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let (label, color) = match &state.phase {
        Phase::Ready => ("◇ 就绪".to_string(), theme.text_muted),
        Phase::Busy => ("忙".to_string(), theme.accent_primary),
        Phase::CallingModel => ("调用模型…".to_string(), theme.accent_primary),
        Phase::ExecutingTool(name) => (format!("执行 {name}…"), theme.accent_primary),
        Phase::WaitingForPermission => ("▲ 等待授权…".to_string(), theme.warning_fg),
    };
    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled(
            "status: ",
            Style::default().fg(theme.text_secondary).bg(theme.bg_base),
        ),
        Span::styled(label, Style::default().fg(color).bg(theme.bg_base)),
    ]))
    .style(Style::default().fg(theme.text_primary).bg(theme.bg_base));
    frame.render_widget(paragraph, area);
}

fn render_input(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let content = if state.input.is_empty() {
        Line::from(vec![
            Span::styled(
                "mysteries ▸ ",
                Style::default().fg(theme.accent_primary).bg(theme.bg_base),
            ),
            Span::styled(
                "输入任务,或 / 执行命令…",
                Style::default().fg(theme.text_muted).bg(theme.bg_base),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                "mysteries ▸ ",
                Style::default().fg(theme.accent_primary).bg(theme.bg_base),
            ),
            Span::styled(
                state.input.clone(),
                Style::default().fg(theme.text_primary).bg(theme.bg_base),
            ),
        ])
    };
    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border_subtle).bg(theme.bg_base))
                .style(Style::default().fg(theme.text_primary).bg(theme.bg_base)),
        )
        .style(Style::default().fg(theme.text_primary).bg(theme.bg_base));
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::tui::app::{AppState, Phase, ToolCard, ToolCardStatus};
    use crate::tui::channel::{AgentEvent, PermissionRequest};
    use crate::tui::theme::Theme;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Modifier};
    use ratatui::Terminal;
    use serde_json::json;
    use tokio::sync::oneshot;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct StyleKey {
        fg: Option<&'static str>,
        bg: Option<&'static str>,
        bold: bool,
        dim: bool,
        reversed: bool,
    }

    fn render_to_styled(state: &AppState, theme: &Theme) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, state, theme)).unwrap();
        buffer_to_styled(terminal.backend().buffer(), theme)
    }

    fn buffer_to_styled(buffer: &Buffer, theme: &Theme) -> String {
        let area = *buffer.area();
        let mut lines = Vec::new();
        for y in area.y..area.y + area.height {
            let mut cells = Vec::new();
            let mut x = area.x;
            while x < area.x + area.width {
                let cell = &buffer[(x, y)];
                let symbol = cell.symbol().to_string();
                cells.push((
                    symbol.clone(),
                    style_key(cell.fg, cell.bg, cell.modifier, theme),
                ));
                x += if is_cjk_wide(&symbol) { 2 } else { 1 };
            }

            while matches!(cells.last(), Some((symbol, _)) if symbol == " ") {
                cells.pop();
            }

            lines.push(styled_line(&cells));
        }
        lines.join("\n")
    }

    fn style_key(fg: Color, bg: Color, modifier: Modifier, theme: &Theme) -> StyleKey {
        StyleKey {
            fg: token_name(fg, theme),
            bg: token_name(bg, theme).filter(|name| *name != "bg.base"),
            bold: modifier.contains(Modifier::BOLD),
            dim: modifier.contains(Modifier::DIM),
            reversed: modifier.contains(Modifier::REVERSED),
        }
    }

    fn token_name(color: Color, theme: &Theme) -> Option<&'static str> {
        if color == Color::Reset {
            None
        } else {
            theme.token_name(color)
        }
    }

    fn styled_line(cells: &[(String, StyleKey)]) -> String {
        let mut output = String::new();
        let mut current = StyleKey::default();
        let mut open = false;

        for (symbol, key) in cells {
            if *key != current {
                if open {
                    output.push_str("‹/›");
                    open = false;
                }
                if let Some(marker) = open_marker(key) {
                    output.push_str(&marker);
                    open = true;
                }
                current = *key;
            }
            output.push_str(symbol);
        }

        if open {
            output.push_str("‹/›");
        }

        output
    }

    fn open_marker(key: &StyleKey) -> Option<String> {
        if *key == StyleKey::default() {
            return None;
        }

        let mut parts = Vec::new();
        if let Some(fg) = key.fg {
            parts.push(fg.to_string());
        }
        if let Some(bg) = key.bg {
            parts.push(format!("bg={bg}"));
        }
        if key.bold {
            parts.push("+bold".to_string());
        }
        if key.dim {
            parts.push("+dim".to_string());
        }
        if key.reversed {
            parts.push("+reversed".to_string());
        }

        Some(format!("‹{}›", parts.join(" ")))
    }

    fn is_cjk_wide(symbol: &str) -> bool {
        symbol.chars().any(|ch| {
            matches!(
                ch as u32,
                0x2E80..=0xA4CF
                    | 0xAC00..=0xD7A3
                    | 0xF900..=0xFAFF
                    | 0xFE10..=0xFE19
                    | 0xFE30..=0xFE6F
                    | 0xFF00..=0xFF60
                    | 0xFFE0..=0xFFE6
            )
        })
    }

    fn status_line(text: &str) -> String {
        text.lines()
            .find(|line| line.contains("status:"))
            .unwrap()
            .to_string()
    }

    fn tool_card(
        name: &str,
        status: ToolCardStatus,
        output: Option<&str>,
        readonly: bool,
        truncated: bool,
    ) -> ToolCard {
        ToolCard {
            id: format!("{name}-1"),
            name: name.to_string(),
            args: json!({ "path": "note.txt" }),
            readonly,
            status,
            output: output.map(str::to_string),
            truncated,
        }
    }

    #[test]
    fn welcome_state_snapshot() {
        let state = AppState::new();
        let theme = Theme::midnight();
        let text = render_to_styled(&state, &theme);

        println!("\n--- welcome frame ---\n{text}\n--- end welcome frame ---");
        insta::assert_snapshot!("tui_welcome_state", text);
    }

    #[test]
    fn welcome_state_daylight_snapshot() {
        let state = AppState::new();
        let daylight = Theme::daylight();
        let midnight = Theme::midnight();
        let text = render_to_styled(&state, &daylight);
        let midnight_text = render_to_styled(&state, &midnight);

        assert_ne!(text, midnight_text);
        println!("\n--- welcome daylight frame ---\n{text}\n--- end welcome daylight frame ---");
        insta::assert_snapshot!("tui_welcome_state_daylight", text);
    }

    #[test]
    fn permission_state_snapshot() {
        let (tx, _rx) = oneshot::channel();
        let mut state = AppState::new();
        state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({ "path": "note.txt", "content": "hello" }),
            responder: tx,
        }));
        let theme = Theme::midnight();
        let text = render_to_styled(&state, &theme);

        println!("\n--- permission frame ---\n{text}\n--- end permission frame ---");
        insta::assert_snapshot!("tui_permission_state", text);
    }

    #[test]
    fn tool_card_running_snapshot() {
        let mut state = AppState::new();
        state.tool_cards.push(tool_card(
            "read_file",
            ToolCardStatus::Running,
            None,
            true,
            false,
        ));
        let theme = Theme::midnight();
        let text = render_to_styled(&state, &theme);

        println!("\n--- tool card running frame ---\n{text}\n--- end tool card running frame ---");
        insta::assert_snapshot!("tui_tool_card_running", text);
    }

    #[test]
    fn tool_card_done_snapshot() {
        let mut state = AppState::new();
        state.tool_cards.push(tool_card(
            "write_file",
            ToolCardStatus::Done,
            Some("wrote note.txt"),
            false,
            false,
        ));
        let theme = Theme::midnight();
        let text = render_to_styled(&state, &theme);

        println!("\n--- tool card done frame ---\n{text}\n--- end tool card done frame ---");
        insta::assert_snapshot!("tui_tool_card_done", text);
    }

    #[test]
    fn tool_card_error_snapshot() {
        let mut state = AppState::new();
        state.tool_cards.push(tool_card(
            "run_shell",
            ToolCardStatus::Error,
            Some("command failed: permission denied"),
            false,
            true,
        ));
        let theme = Theme::midnight();
        let text = render_to_styled(&state, &theme);

        println!("\n--- tool card error frame ---\n{text}\n--- end tool card error frame ---");
        insta::assert_snapshot!("tui_tool_card_error", text);
    }

    #[test]
    fn phase_status_lines_snapshot() {
        let phases = [
            ("idle", Phase::Ready),
            ("calling", Phase::CallingModel),
            ("executing", Phase::ExecutingTool("write_file".to_string())),
            ("waiting", Phase::WaitingForPermission),
        ];
        let mut text = String::new();

        for (label, phase) in phases {
            let mut state = AppState::new();
            state.phase = phase;
            text.push_str(label);
            text.push_str(": ");
            text.push_str(&status_line(&render_to_styled(&state, &Theme::midnight())));
            text.push('\n');
        }

        println!("\n--- phase status lines ---\n{text}--- end phase status lines ---");
        insta::assert_snapshot!("tui_phase_status_lines", text);
    }
}
