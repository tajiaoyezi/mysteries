use crate::tui::app::{
    compute_diff, AppState, DiffKind, DiffLine, Phase, StatusSnapshot, ToolCard, ToolCardStatus,
    TranscriptBlock,
};
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

    let rows = layout_rows(area, state);

    render_header(frame, rows[0], theme);
    render_transcript(frame, rows[1], state, theme);
    if state.pending_permission.is_some() {
        render_permission(frame, rows[2], state, theme);
    }
    render_status(frame, rows[3], state, theme);
    render_input(frame, rows[4], state, theme);
}

pub(crate) fn transcript_line_count(state: &AppState, theme: &Theme) -> usize {
    transcript_content_lines(state, theme).len()
}

pub(crate) fn transcript_viewport_height(area: Rect, state: &AppState) -> usize {
    layout_rows(area, state)[1].height as usize
}

fn layout_rows(area: Rect, state: &AppState) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(permission_height(state)),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(area)
}

fn permission_height(state: &AppState) -> u16 {
    let Some(request) = &state.pending_permission else {
        return 0;
    };
    let diff_rows = compute_diff(&request.tool_name, &request.args).len() as u16;
    7 + diff_rows
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
    let lines = visible_transcript_lines(state, theme, area.height as usize);
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .style(Style::default().fg(theme.text_primary).bg(theme.bg_base))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn transcript_content_lines(state: &AppState, theme: &Theme) -> Vec<Line<'static>> {
    if state.transcript.is_empty() && state.tool_cards.is_empty() {
        welcome_lines(theme)
    } else {
        transcript_lines(state, theme)
    }
}

fn visible_transcript_lines(
    state: &AppState,
    theme: &Theme,
    viewport_lines: usize,
) -> Vec<Line<'static>> {
    let lines = transcript_content_lines(state, theme);
    let offset = state.visible_scroll_offset(lines.len(), viewport_lines);
    lines
        .into_iter()
        .skip(offset)
        .take(viewport_lines)
        .collect()
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
                lines.extend(error_block_lines(text, theme));
            }
            TranscriptBlock::Help => {
                lines.extend(help_block_lines(theme));
            }
            TranscriptBlock::Status(snapshot) => {
                lines.extend(status_block_lines(snapshot, theme));
            }
            TranscriptBlock::Notice(text) => {
                lines.extend(notice_block_lines(text, theme));
            }
        }
        lines.push(Line::from(""));
    }
    for card in &state.tool_cards {
        lines.extend(tool_card_lines(card, state, theme));
        lines.push(Line::from(""));
    }
    lines
}

fn help_block_lines(theme: &Theme) -> Vec<Line<'static>> {
    let commands = [
        ("/help", "查看内置命令"),
        ("/clear", "清空当前 transcript"),
        ("/model", "查看当前 model"),
        ("/model <name>", "切换后续请求 model"),
        ("/status", "当前会话快照"),
        ("/exit", "退出 TUI"),
        ("/login /logout", "凭据占位"),
    ];
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "┌─ ",
            Style::default().fg(theme.border_subtle).bg(theme.bg_base),
        ),
        Span::styled(
            "帮助",
            Style::default()
                .fg(theme.info_fg)
                .bg(theme.bg_base)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " ───────────────────────────────────────────────────────────────",
            Style::default().fg(theme.border_subtle).bg(theme.bg_base),
        ),
    ])];

    for row in commands.chunks(2) {
        let mut spans = vec![Span::styled(
            "│ ",
            Style::default().fg(theme.border_subtle).bg(theme.bg_base),
        )];
        for (index, (cmd, desc)) in row.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(
                    "   ",
                    Style::default().fg(theme.text_body).bg(theme.bg_base),
                ));
            }
            spans.push(Span::styled(
                format!("{cmd:<14}"),
                Style::default()
                    .fg(theme.accent_primary)
                    .bg(theme.bg_base)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!("{desc:<18}"),
                Style::default().fg(theme.text_body).bg(theme.bg_base),
            ));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(Span::styled(
        "└──────────────────────────────────────────────────────────────────────────────",
        Style::default().fg(theme.border_subtle).bg(theme.bg_base),
    )));
    lines
}

fn status_block_lines(snapshot: &StatusSnapshot, theme: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled(
                "┌─ ",
                Style::default().fg(theme.border_subtle).bg(theme.bg_base),
            ),
            Span::styled(
                "会话快照",
                Style::default()
                    .fg(theme.info_fg)
                    .bg(theme.bg_base)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " ───────────────────────────────────────────────────────────",
                Style::default().fg(theme.border_subtle).bg(theme.bg_base),
            ),
        ]),
        status_field_line(
            "provider",
            &snapshot.provider,
            "model",
            &snapshot.model,
            theme,
        ),
        status_field_line(
            "iter",
            &format!("{}/{}", snapshot.iteration, snapshot.max_iterations),
            "msgs",
            &snapshot.messages.to_string(),
            theme,
        ),
        status_field_line(
            "cwd",
            &snapshot.cwd.display().to_string(),
            "tools",
            &snapshot.tools.to_string(),
            theme,
        ),
        Line::from(Span::styled(
            "└──────────────────────────────────────────────────────────────────────────────",
            Style::default().fg(theme.border_subtle).bg(theme.bg_base),
        )),
    ]
}

fn status_field_line(
    left_label: &str,
    left_value: &str,
    right_label: &str,
    right_value: &str,
    theme: &Theme,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "│ ",
            Style::default().fg(theme.border_subtle).bg(theme.bg_base),
        ),
        Span::styled(
            format!("{left_label}: "),
            Style::default().fg(theme.text_secondary).bg(theme.bg_base),
        ),
        Span::styled(
            format!("{left_value:<24}"),
            Style::default().fg(theme.text_primary).bg(theme.bg_base),
        ),
        Span::styled(
            format!("{right_label}: "),
            Style::default().fg(theme.text_secondary).bg(theme.bg_base),
        ),
        Span::styled(
            right_value.to_string(),
            Style::default().fg(theme.text_primary).bg(theme.bg_base),
        ),
    ])
}

fn notice_block_lines(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    vec![Line::from(vec![
        Span::styled("◇ ", Style::default().fg(theme.info_fg).bg(theme.bg_base)),
        Span::styled(
            text.to_string(),
            Style::default().fg(theme.text_body).bg(theme.bg_base),
        ),
    ])]
}

fn error_block_lines(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled(
                "┌─ ",
                Style::default().fg(theme.error_border).bg(theme.error_bg),
            ),
            Span::styled(
                "致命错误",
                Style::default()
                    .fg(theme.error_fg)
                    .bg(theme.error_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " ───────────────────────────────────────────────────────────────",
                Style::default().fg(theme.error_border).bg(theme.error_bg),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "│ ",
                Style::default().fg(theme.error_border).bg(theme.error_bg),
            ),
            Span::styled(
                text.to_string(),
                Style::default().fg(theme.error_fg).bg(theme.error_bg),
            ),
        ]),
        Line::from(Span::styled(
            "└──────────────────────────────────────────────────────────────────────────────",
            Style::default().fg(theme.error_border).bg(theme.error_bg),
        )),
    ]
}

fn tool_card_lines(card: &ToolCard, state: &AppState, theme: &Theme) -> Vec<Line<'static>> {
    let (glyph, glyph_color) = match card.status {
        ToolCardStatus::Running => (state.spinner_glyph(), theme.accent_primary),
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
            for line in visible_tool_output_lines(card, output) {
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

    if let Some(exit) = card.exit {
        let color = if exit == 0 {
            theme.success_fg
        } else {
            theme.error_fg
        };
        lines.push(Line::from(vec![
            Span::styled(
                "│ ",
                Style::default().fg(theme.border_subtle).bg(theme.bg_base),
            ),
            Span::styled(
                format!("exit {exit}"),
                Style::default().fg(color).bg(theme.bg_base),
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
            text.to_string(),
            Style::default().fg(theme.text_body).bg(theme.bg_base),
        ),
    ])
}

fn visible_tool_output_lines<'a>(card: &ToolCard, output: &'a str) -> Vec<&'a str> {
    let mut lines = output.lines().collect::<Vec<_>>();
    if let Some(exit) = card.exit {
        let expected_exit = format!("exit: {exit}");
        if lines.first().is_some_and(|line| *line == expected_exit) {
            lines.remove(0);
        }
    }
    lines
}

fn render_permission(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let Some(request) = &state.pending_permission else {
        return;
    };
    let mut lines = vec![
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
                permission_args_preview(&request.tool_name, &request.args),
                Style::default().fg(theme.text_body).bg(theme.warning_bg),
            ),
        ]),
    ];
    lines.extend(
        compute_diff(&request.tool_name, &request.args)
            .into_iter()
            .map(|line| diff_line(line, theme)),
    );
    lines.extend([
        Line::from(vec![
            Span::styled(
                "[y · 允许]",
                Style::default().fg(theme.warning_fg).bg(theme.warning_bg),
            ),
            Span::styled(
                "   ",
                Style::default().fg(theme.text_body).bg(theme.warning_bg),
            ),
            Span::styled(
                "[n · 拒绝]",
                Style::default().fg(theme.error_fg).bg(theme.warning_bg),
            ),
        ]),
        Line::from(Span::styled(
            "提示:Enter = 允许 · Esc = 拒绝",
            Style::default()
                .fg(theme.text_secondary)
                .bg(theme.warning_bg),
        )),
    ]);

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.warning_fg).bg(theme.warning_bg))
                .style(Style::default().fg(theme.text_primary).bg(theme.warning_bg)),
        )
        .style(Style::default().fg(theme.text_primary).bg(theme.warning_bg));
    frame.render_widget(paragraph, area);
}

fn permission_args_preview(tool_name: &str, args: &serde_json::Value) -> String {
    match tool_name {
        "write_file" | "edit_file" => args
            .get("path")
            .and_then(serde_json::Value::as_str)
            .map(|path| format!("path={path}"))
            .unwrap_or_else(|| args.to_string()),
        "run_shell" => args
            .get("command")
            .and_then(serde_json::Value::as_str)
            .map(|command| format!("command={command}"))
            .unwrap_or_else(|| args.to_string()),
        _ => args.to_string(),
    }
}

fn diff_line(line: DiffLine, theme: &Theme) -> Line<'static> {
    let (prefix, color) = match line.kind {
        DiffKind::Add => ("+ ", theme.success_fg),
        DiffKind::Del => ("− ", theme.error_fg),
        DiffKind::Ctx => ("  ", theme.text_body),
    };

    Line::from(vec![
        Span::styled(prefix, Style::default().fg(color).bg(theme.warning_bg)),
        Span::styled(line.text, Style::default().fg(color).bg(theme.warning_bg)),
    ])
}

fn render_status(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let (label, color) = match &state.phase {
        Phase::Ready => ("◇ 就绪".to_string(), theme.text_muted),
        Phase::Busy => ("忙".to_string(), theme.accent_primary),
        Phase::CallingModel => (
            format!("{} 调用模型…", state.spinner_glyph()),
            theme.accent_primary,
        ),
        Phase::ExecutingTool(name) => (
            format!("{} 执行 {name}…", state.spinner_glyph()),
            theme.accent_primary,
        ),
        Phase::WaitingForPermission => ("▲ 等待授权…".to_string(), theme.warning_fg),
    };
    let meta = status_meta(state);
    let left_plain = format!("status: {label}");
    let padding = area
        .width
        .saturating_sub((display_width(&left_plain) + display_width(&meta)) as u16)
        as usize;
    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled(
            "status: ",
            Style::default().fg(theme.text_secondary).bg(theme.bg_base),
        ),
        Span::styled(label, Style::default().fg(color).bg(theme.bg_base)),
        Span::styled(
            " ".repeat(padding),
            Style::default().fg(theme.text_muted).bg(theme.bg_base),
        ),
        Span::styled(
            meta,
            Style::default().fg(theme.text_muted).bg(theme.bg_base),
        ),
    ]))
    .style(Style::default().fg(theme.text_primary).bg(theme.bg_base));
    frame.render_widget(paragraph, area);
}

fn status_meta(state: &AppState) -> String {
    format!(
        "{} · {} · iter {}/{} · {} msgs · {}",
        state.session.provider,
        state.session.model,
        state.iteration,
        state.session.max_iterations,
        state.dialog_message_count(),
        state.session.cwd.display()
    )
}

fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

fn char_width(ch: char) -> usize {
    if matches!(
        ch as u32,
        0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
    ) {
        2
    } else {
        1
    }
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
    use super::{render, transcript_line_count};
    use crate::tui::app::{
        AppState, Phase, SessionSnapshot, ToolCard, ToolCardStatus, TranscriptBlock,
    };
    use crate::tui::channel::{AgentEvent, PermissionRequest};
    use crate::tui::theme::Theme;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Modifier};
    use ratatui::Terminal;
    use serde_json::json;
    use std::path::PathBuf;
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
            exit: None,
        }
    }

    fn session() -> SessionSnapshot {
        SessionSnapshot {
            provider: "anthropic".to_string(),
            model: "claude-test".to_string(),
            max_iterations: 8,
            cwd: PathBuf::from("workspace"),
            tools: 7,
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
        let state = permission_state();
        let theme = Theme::midnight();
        let text = render_to_styled(&state, &theme);

        println!("\n--- permission frame ---\n{text}\n--- end permission frame ---");
        insta::assert_snapshot!("tui_permission_state", text);
    }

    #[test]
    fn permission_state_daylight_snapshot() {
        let state = permission_state();
        let text = render_to_styled(&state, &Theme::daylight());

        println!(
            "\n--- permission daylight frame ---\n{text}\n--- end permission daylight frame ---"
        );
        insta::assert_snapshot!("tui_permission_state_daylight", text);
    }

    #[test]
    fn help_block_snapshot() {
        let mut state = AppState::with_session(session());
        state.transcript.push(TranscriptBlock::Help);
        let text = render_to_styled(&state, &Theme::midnight());

        println!("\n--- help block frame ---\n{text}\n--- end help block frame ---");
        insta::assert_snapshot!("tui_help_block", text);
    }

    #[test]
    fn status_block_snapshot() {
        let mut state = AppState::with_session(session());
        state
            .transcript
            .push(TranscriptBlock::User("给我看一下当前会话状态".to_string()));
        state.iteration = 2;
        let snapshot = state.status_snapshot();
        state.transcript.push(TranscriptBlock::Status(snapshot));
        let text = render_to_styled(&state, &Theme::midnight());

        println!("\n--- status block frame ---\n{text}\n--- end status block frame ---");
        insta::assert_snapshot!("tui_status_block", text);
    }

    #[test]
    fn tool_card_running_snapshot() {
        let mut state = AppState::new();
        state.spinner_frame = 3;
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
            Some("pub struct Config {\n    pub timeout_secs: u64,\n}"),
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
    fn run_shell_exit_foot_snapshot() {
        let mut state = AppState::new();
        state.tool_cards.push(ToolCard {
            id: "run-shell-1".to_string(),
            name: "run_shell".to_string(),
            args: json!({ "command": "exit 7" }),
            readonly: false,
            status: ToolCardStatus::Error,
            output: Some("exit: 7\n--- stdout ---\nfailed\n--- stderr ---\n".to_string()),
            truncated: false,
            exit: Some(7),
        });
        let text = render_to_styled(&state, &Theme::midnight());

        println!(
            "\n--- run shell exit foot frame ---\n{text}\n--- end run shell exit foot frame ---"
        );
        insta::assert_snapshot!("tui_run_shell_exit_foot", text);
    }

    #[test]
    fn fatal_error_snapshot() {
        let state = fatal_error_state();
        let theme = Theme::midnight();
        let text = render_to_styled(&state, &theme);

        println!("\n--- fatal error frame ---\n{text}\n--- end fatal error frame ---");
        insta::assert_snapshot!("tui_fatal_error", text);
    }

    #[test]
    fn fatal_error_daylight_snapshot() {
        let state = fatal_error_state();
        let text = render_to_styled(&state, &Theme::daylight());

        println!(
            "\n--- fatal error daylight frame ---\n{text}\n--- end fatal error daylight frame ---"
        );
        insta::assert_snapshot!("tui_fatal_error_daylight", text);
    }

    fn permission_state() -> AppState {
        let (tx, _rx) = oneshot::channel();
        let mut state = AppState::new();
        state.transcript.push(TranscriptBlock::User(
            "在 Config 里加一个 timeout_secs 字段,默认 30 秒".to_string(),
        ));
        state.transcript.push(TranscriptBlock::Assistant(
            "好的。我先读一下 src/config/schema.rs 里 Config 的结构。".to_string(),
        ));
        state.tool_cards.push(ToolCard {
            id: "read-file-1".to_string(),
            name: "read_file".to_string(),
            args: json!({ "path": "schema.rs", "offset": 0, "limit": 40 }),
            readonly: true,
            status: ToolCardStatus::Done,
            output: Some(
                "pub struct Config {\n    pub model: String,\n    pub max_iterations: u32,\n}"
                    .to_string(),
            ),
            truncated: false,
            exit: None,
        });
        state.transcript.push(TranscriptBlock::Assistant(
            "结构清楚了。我在 Config 上加 timeout_secs: u64,并在 Default 里给默认值 30。"
                .to_string(),
        ));
        state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "edit_file".to_string(),
            args: json!({
                "path": "src/config/mod.rs",
                "old_string": "    pub max_iterations: u32,",
                "new_string": "    pub timeout_secs: u64,"
            }),
            responder: tx,
        }));
        state
    }

    fn fatal_error_state() -> AppState {
        let mut state = AppState::new();
        state.transcript.push(TranscriptBlock::User(
            "换 anthropic provider 再跑一次".to_string(),
        ));
        state.transcript.push(TranscriptBlock::Error(
            "鉴权失败:未找到 OPENAI_API_KEY。Agent Loop 已终止。".to_string(),
        ));
        state
    }

    #[test]
    fn transcript_scroll_window_snapshot() {
        let mut state = AppState::new();
        for index in 1..=30 {
            state
                .transcript
                .push(TranscriptBlock::User(format!("line {index:02}")));
        }
        let theme = Theme::midnight();
        let total_lines = transcript_line_count(&state, &theme);
        state.page_up(total_lines, 17);
        let text = render_to_styled(&state, &theme);

        println!("\n--- transcript scroll frame ---\n{text}\n--- end transcript scroll frame ---");
        insta::assert_snapshot!("tui_transcript_scroll_window", text);
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
            state.spinner_frame = 3;
            text.push_str(label);
            text.push_str(": ");
            text.push_str(&status_line(&render_to_styled(&state, &Theme::midnight())));
            text.push('\n');
        }

        println!("\n--- phase status lines ---\n{text}--- end phase status lines ---");
        insta::assert_snapshot!("tui_phase_status_lines", text);
    }
}
