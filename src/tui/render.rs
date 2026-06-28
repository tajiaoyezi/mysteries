use crate::tui::app::{
    compute_diff, AppState, DiffKind, DiffLine, Phase, StatusSnapshot, ToolCard, ToolCardStatus,
    TranscriptBlock,
};
use crate::tui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

const STATUS_TOP_GAP_LINES: u16 = 2;

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
    render_status(frame, rows[4], state, theme);
    render_input(frame, rows[5], state, theme);
}

pub(crate) fn transcript_line_count(state: &AppState, theme: &Theme, width: usize) -> usize {
    transcript_content_lines(state, theme, width).len()
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
            Constraint::Length(status_top_gap_height(state)),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(area)
}

fn status_top_gap_height(state: &AppState) -> u16 {
    if state.pending_permission.is_none() && !state.transcript.is_empty() {
        STATUS_TOP_GAP_LINES
    } else {
        0
    }
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
    let lines = visible_transcript_lines(state, theme, area.width as usize, area.height as usize);
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .style(Style::default().fg(theme.text_primary).bg(theme.bg_base))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn transcript_content_lines(state: &AppState, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    if state.transcript.is_empty() {
        welcome_lines(theme, width)
    } else {
        transcript_lines(state, theme, width)
    }
}

fn visible_transcript_lines(
    state: &AppState,
    theme: &Theme,
    width: usize,
    viewport_lines: usize,
) -> Vec<Line<'static>> {
    let mut lines = transcript_content_lines(state, theme, width);
    if state.transcript.is_empty() && viewport_lines > lines.len() {
        let top_padding = (viewport_lines - lines.len()) / 2;
        let mut padded = vec![Line::from(""); top_padding];
        padded.extend(lines);
        lines = padded;
    }
    let offset = state.visible_scroll_offset(lines.len(), viewport_lines);
    lines
        .into_iter()
        .skip(offset)
        .take(viewport_lines)
        .collect()
}

const WELCOME_MAX_WIDTH: usize = 64;

fn welcome_lines(theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let content_width = width.clamp(1, WELCOME_MAX_WIDTH);
    let title_style = Style::default()
        .fg(theme.text_title)
        .bg(theme.bg_base)
        .add_modifier(Modifier::BOLD);
    let subtitle_style = Style::default().fg(theme.accent_primary).bg(theme.bg_base);
    let body_style = Style::default().fg(theme.text_body).bg(theme.bg_base);
    let muted_style = Style::default().fg(theme.text_muted).bg(theme.bg_base);

    let mut lines = vec![
        centered_text_line("✦ MYSTERIES", title_style, width, theme),
        centered_text_line("AGENT · v1.0 · 终端编码助手", subtitle_style, width, theme),
    ];
    for line in wrap_text(
        "读只读,写必询 —— 每一次文件改动与命令执行,都先把 diff 摊给你,等你按下 y 才动手。",
        content_width,
    ) {
        lines.push(centered_text_line(&line, body_style, width, theme));
    }
    lines.push(Line::from(""));
    lines.push(centered_text_line("试试 ↓", muted_style, width, theme));

    let suggestions = [
        ("任务", "给 Config 加 timeout_secs 字段"),
        ("/help", "查看内置命令"),
        ("/status", "当前会话快照"),
        ("错误", "演示:鉴权失败(致命错误,终止 Loop)"),
    ];
    let suggestion_width = suggestions
        .iter()
        .map(|(tag, text)| display_width(&format!("〔{tag}〕 {text}")))
        .max()
        .unwrap_or(0);
    for (tag, text) in suggestions {
        lines.push(suggestion_line(tag, text, theme, width, suggestion_width));
    }

    lines
}

fn centered_text_line(text: &str, style: Style, width: usize, theme: &Theme) -> Line<'static> {
    centered_spans(
        width,
        display_width(text),
        vec![Span::styled(text.to_string(), style)],
        theme,
    )
}

fn centered_spans(
    width: usize,
    content_width: usize,
    spans: Vec<Span<'static>>,
    theme: &Theme,
) -> Line<'static> {
    let left_padding = width.saturating_sub(content_width) / 2;
    let mut centered = Vec::with_capacity(spans.len() + 1);
    if left_padding > 0 {
        centered.push(Span::styled(
            " ".repeat(left_padding),
            Style::default().bg(theme.bg_base),
        ));
    }
    centered.extend(spans);
    Line::from(centered)
}

fn suggestion_line(
    tag: &str,
    text: &str,
    theme: &Theme,
    width: usize,
    block_width: usize,
) -> Line<'static> {
    centered_spans(
        width,
        block_width,
        vec![
            Span::styled(
                format!("〔{tag}〕"),
                Style::default().fg(theme.accent_primary).bg(theme.bg_base),
            ),
            Span::styled(
                format!(" {text}"),
                Style::default().fg(theme.text_primary).bg(theme.bg_base),
            ),
        ],
        theme,
    )
}

fn transcript_lines(state: &AppState, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for block in &state.transcript {
        match block {
            TranscriptBlock::User(text) => {
                lines.extend(message_lines(
                    "> ",
                    text,
                    width,
                    Style::default().fg(theme.accent_primary).bg(theme.bg_base),
                    Style::default().fg(theme.text_primary).bg(theme.bg_base),
                ));
            }
            TranscriptBlock::Assistant(text) => {
                lines.extend(message_lines(
                    "◆ ",
                    text,
                    width,
                    Style::default().fg(theme.info_fg).bg(theme.bg_base),
                    Style::default().fg(theme.text_body).bg(theme.bg_base),
                ));
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
            TranscriptBlock::Tool(card) => {
                lines.extend(tool_card_lines(card, state, theme));
            }
        }
        lines.push(Line::from(""));
    }
    lines
}

fn message_lines(
    marker: &'static str,
    text: &str,
    width: usize,
    marker_style: Style,
    text_style: Style,
) -> Vec<Line<'static>> {
    let marker_width = display_width(marker);
    let content_width = width.saturating_sub(marker_width).max(1);
    let indent = " ".repeat(marker_width);
    let mut lines = Vec::new();
    let mut first_line = true;

    for physical in text.split('\n') {
        let wrapped = wrap_text(physical, content_width);
        let wrapped = if wrapped.is_empty() {
            vec![String::new()]
        } else {
            wrapped
        };

        for chunk in wrapped {
            if first_line {
                lines.push(Line::from(vec![
                    Span::styled(marker, marker_style),
                    Span::styled(chunk, text_style),
                ]));
                first_line = false;
            } else {
                lines.push(Line::from(vec![
                    Span::styled(indent.clone(), text_style),
                    Span::styled(chunk, text_style),
                ]));
            }
        }
    }

    lines
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    let max_width = max_width.max(1);

    for ch in text.chars() {
        let ch_width = char_width(ch);
        if ch_width > 0 && current_width + ch_width > max_width && !current.is_empty() {
            lines.push(current);
            current = String::new();
            current_width = 0;
        }

        current.push(ch);
        current_width += ch_width;
    }

    if !current.is_empty() {
        lines.push(current);
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
    if is_zero_width(ch) {
        return 0;
    }

    if matches!(
        ch as u32,
        0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F000..=0x1FAFF
    ) {
        2
    } else {
        1
    }
}

fn is_zero_width(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200D | 0xFE00..=0xFE0F
    )
}

fn render_input(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    const INPUT_PROMPT: &str = "mysteries ▸ ";

    let content = if state.input.is_empty() {
        Line::from(vec![
            Span::styled(
                INPUT_PROMPT,
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
                INPUT_PROMPT,
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
    frame.set_cursor_position(input_cursor_position(area, &state.input));
}

fn input_cursor_position(area: Rect, input: &str) -> Position {
    const INPUT_PROMPT: &str = "mysteries ▸ ";

    let content_width = display_width(INPUT_PROMPT) + display_width(input);
    let max_x = area.x.saturating_add(area.width.saturating_sub(2));
    let x = area
        .x
        .saturating_add(1)
        .saturating_add(content_width as u16)
        .min(max_x);
    let y = area.y.saturating_add(1);
    Position::new(x, y)
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
    use ratatui::layout::Position;
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

    fn render_to_plain_with_size(
        state: &AppState,
        theme: &Theme,
        width: u16,
        height: u16,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, state, theme)).unwrap();
        buffer_to_plain(terminal.backend().buffer())
    }

    fn buffer_to_plain(buffer: &Buffer) -> String {
        let area = *buffer.area();
        let mut lines = Vec::new();
        for y in area.y..area.y + area.height {
            let mut line = String::new();
            let mut x = area.x;
            while x < area.x + area.width {
                let symbol = buffer[(x, y)].symbol().to_string();
                line.push_str(&symbol);
                x += super::display_width(&symbol).max(1) as u16;
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
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
                x += super::display_width(&symbol).max(1) as u16;
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

    fn status_line(text: &str) -> String {
        text.lines()
            .find(|line| line.contains("status:"))
            .unwrap()
            .to_string()
    }

    #[test]
    fn display_width_treats_common_emoji_as_wide() {
        assert_eq!(super::display_width("a"), 1);
        assert_eq!(super::display_width("你好"), 4);
        assert_eq!(super::display_width("👋"), 2);
        assert_eq!(super::display_width("😊"), 2);
    }

    #[test]
    fn assistant_transcript_uses_diamond_marker_hanging_indent_and_emoji_width() {
        let mut state = AppState::new();
        state.transcript.push(TranscriptBlock::Assistant(
            "你好！ 👋 我是在 Mysteries 中运行的 AI 编程助手。".to_string(),
        ));

        let text = render_to_plain_with_size(&state, &Theme::midnight(), 19, 12);
        let lines = text.lines().collect::<Vec<_>>();

        assert!(lines.iter().any(|line| line.starts_with("◆ 你好！ 👋")));
        assert!(lines.iter().any(|line| line.starts_with("  Mysteries")));
        assert!(!lines.iter().any(|line| line.starts_with("Mysteries")));
        assert!(!text.contains("m 你好"));
    }

    #[test]
    fn input_render_sets_cursor_at_input_end() {
        let mut state = AppState::new();
        state.input = "你好".to_string();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, &state, &Theme::midnight()))
            .unwrap();

        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(17, 22));
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
    fn interrupted_notice_snapshot() {
        let mut state = AppState::new();
        state.apply(AgentEvent::Interrupted);
        let text = render_to_styled(&state, &Theme::midnight());

        println!(
            "\n--- interrupted notice frame ---\n{text}\n--- end interrupted notice frame ---"
        );
        insta::assert_snapshot!("tui_interrupted_notice", text);
    }

    #[test]
    fn interrupted_notice_daylight_snapshot() {
        let mut state = AppState::new();
        state.apply(AgentEvent::Interrupted);
        let text = render_to_styled(&state, &Theme::daylight());

        println!(
            "\n--- interrupted notice daylight frame ---\n{text}\n--- end interrupted notice daylight frame ---"
        );
        insta::assert_snapshot!("tui_interrupted_notice_daylight", text);
    }

    #[test]
    fn tool_card_running_snapshot() {
        let mut state = AppState::new();
        state.spinner_frame = 3;
        state.transcript.push(TranscriptBlock::Tool(tool_card(
            "read_file",
            ToolCardStatus::Running,
            None,
            true,
            false,
        )));
        let theme = Theme::midnight();
        let text = render_to_styled(&state, &theme);

        println!("\n--- tool card running frame ---\n{text}\n--- end tool card running frame ---");
        insta::assert_snapshot!("tui_tool_card_running", text);
    }

    #[test]
    fn tool_card_done_snapshot() {
        let mut state = AppState::new();
        state.transcript.push(TranscriptBlock::Tool(tool_card(
            "write_file",
            ToolCardStatus::Done,
            Some("pub struct Config {\n    pub timeout_secs: u64,\n}"),
            false,
            false,
        )));
        let theme = Theme::midnight();
        let text = render_to_styled(&state, &theme);

        println!("\n--- tool card done frame ---\n{text}\n--- end tool card done frame ---");
        insta::assert_snapshot!("tui_tool_card_done", text);
    }

    #[test]
    fn tool_card_error_snapshot() {
        let mut state = AppState::new();
        state.transcript.push(TranscriptBlock::Tool(tool_card(
            "run_shell",
            ToolCardStatus::Error,
            Some("command failed: permission denied"),
            false,
            true,
        )));
        let theme = Theme::midnight();
        let text = render_to_styled(&state, &theme);

        println!("\n--- tool card error frame ---\n{text}\n--- end tool card error frame ---");
        insta::assert_snapshot!("tui_tool_card_error", text);
    }

    #[test]
    fn run_shell_exit_foot_snapshot() {
        let mut state = AppState::new();
        state.transcript.push(TranscriptBlock::Tool(ToolCard {
            id: "run-shell-1".to_string(),
            name: "run_shell".to_string(),
            args: json!({ "command": "exit 7" }),
            readonly: false,
            status: ToolCardStatus::Error,
            output: Some("exit: 7\n--- stdout ---\nfailed\n--- stderr ---\n".to_string()),
            truncated: false,
            exit: Some(7),
        }));
        let text = render_to_styled(&state, &Theme::midnight());

        println!(
            "\n--- run shell exit foot frame ---\n{text}\n--- end run shell exit foot frame ---"
        );
        insta::assert_snapshot!("tui_run_shell_exit_foot", text);
    }

    #[test]
    fn timeline_tool_then_final_answer_snapshot() {
        let mut state = AppState::new();
        state
            .transcript
            .push(TranscriptBlock::User("总结这个项目".to_string()));
        state.transcript.push(TranscriptBlock::Tool(tool_card(
            "read_file",
            ToolCardStatus::Done,
            Some("# Mysteries\nA terminal coding assistant."),
            true,
            false,
        )));
        state.transcript.push(TranscriptBlock::Assistant(
            "这是一个 Rust 编写的终端编码助手。".to_string(),
        ));
        let text = render_to_styled(&state, &Theme::midnight());

        println!("\n--- timeline final answer frame ---\n{text}\n--- end timeline final answer frame ---");
        insta::assert_snapshot!("tui_timeline_tool_then_final_answer", text);
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
        state.transcript.push(TranscriptBlock::Tool(ToolCard {
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
        }));
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
        let total_lines = transcript_line_count(&state, &theme, 80);
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
