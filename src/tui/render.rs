use crate::permission::{permission_mode_label, PermissionMode};
use crate::tui::app::{
    compute_diff, AppState, DiffKind, DiffLine, ModelsPickerRowKind, Phase, StatusSnapshot,
    ToolCard, ToolCardStatus, TranscriptBlock,
};
use crate::tui::input_layout::{
    input_content_height_cap, input_scroll_offset, visual_input_layout, InputVisualLayout,
};
use crate::tui::jump_to_bottom::jump_to_bottom_pill_text;
use crate::tui::selection::{col_range_for_row, Selection};
use crate::tui::theme::Theme;
use crate::tui::width::{char_width, display_width};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

const STATUS_TOP_GAP_LINES: u16 = 2;
const INPUT_PROMPT: &str = "> ";
const INPUT_MAX_CONTENT_ROWS: u16 = 10;

pub fn render(frame: &mut Frame<'_>, state: &AppState, theme: &Theme) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.bg_base)),
        area,
    );

    let rows = layout_rows(area, state);

    render_header(frame, rows[0], theme);
    render_transcript(frame, rows[1], state, theme);
    render_jump_to_bottom_pill(frame, rows[1], state, theme);
    if state.pending_permission.is_some() {
        render_permission(frame, rows[2], state, theme);
    }
    render_activity(frame, rows[4], state, theme);
    render_input(frame, rows[5], state, theme);
    render_command_completion(frame, rows[5], state, theme);
    render_status(frame, rows[6], state, theme);
    render_models_picker(frame, rows[6], state, theme);
    render_mode_line(frame, rows[7], state, theme);
    highlight_selection(frame, state, theme);
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
            Constraint::Length(input_box_height(area, state)),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area)
}

fn input_box_height(area: Rect, state: &AppState) -> u16 {
    let inner_width = area.width.saturating_sub(2) as usize;
    let layout_width = inner_width.saturating_sub(display_width(INPUT_PROMPT));
    let layout = visual_input_layout(state.input(), state.input_line.cursor, layout_width);
    let cap = input_content_height_cap(
        area.height,
        status_top_gap_height(state),
        permission_height(state),
        INPUT_MAX_CONTENT_ROWS,
    );
    (layout.lines.len() as u16).clamp(1, cap).saturating_add(2)
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
        .style(Style::default().fg(theme.text_primary).bg(theme.bg_base));
    frame.render_widget(paragraph, area);
}

fn render_jump_to_bottom_pill(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    if state.follows_bottom() || area.height == 0 || area.width == 0 {
        return;
    }

    let text = jump_to_bottom_pill_text(state.new_message_count);
    let content_width = display_width(&text);
    let pill_width = (content_width + 2).min(area.width as usize) as u16;
    if pill_width == 0 {
        return;
    }

    let pill_area = Rect {
        x: area.x + (area.width.saturating_sub(pill_width)) / 2,
        y: area.y + area.height.saturating_sub(1),
        width: pill_width,
        height: 1,
    };

    frame.render_widget(Clear, pill_area);

    let pill_style = Style::default()
        .fg(theme.accent_primary)
        .bg(theme.bg_surface);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, pill_style)))
            .style(Style::default().bg(theme.bg_surface)),
        pill_area,
    );
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
    for (index, block) in state.transcript.iter().enumerate() {
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
                lines.extend(error_block_lines(text, theme, width));
            }
            TranscriptBlock::Help => {
                lines.extend(help_block_lines(theme, width));
            }
            TranscriptBlock::Status(snapshot) => {
                lines.extend(status_block_lines(snapshot, theme, width));
            }
            TranscriptBlock::Notice(text) => {
                lines.extend(notice_block_lines(text, theme, width));
            }
            TranscriptBlock::Tool(card) => {
                let prev_is_tool =
                    index > 0 && matches!(state.transcript[index - 1], TranscriptBlock::Tool(_));
                let is_group_first = !prev_is_tool;
                lines.extend(tool_card_lines(card, state, theme, width, is_group_first));
            }
        }
        let cur_is_tool = matches!(block, TranscriptBlock::Tool(_));
        let next_is_tool = state
            .transcript
            .get(index + 1)
            .is_some_and(|next| matches!(next, TranscriptBlock::Tool(_)));
        if !(cur_is_tool && next_is_tool) {
            lines.push(Line::from(""));
        }
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

fn block_top_border(
    title: &str,
    width: usize,
    title_style: Style,
    border_style: Style,
) -> Line<'static> {
    let before_dashes_width = display_width(&format!("┌─ {title}"));
    let dash_count = width.saturating_sub(before_dashes_width + 1);
    Line::from(vec![
        Span::styled("┌─ ", border_style),
        Span::styled(title.to_string(), title_style),
        Span::styled(format!(" {}", "─".repeat(dash_count)), border_style),
    ])
}

fn block_bottom_border(width: usize, style: Style) -> Line<'static> {
    Line::from(Span::styled(
        format!("└{}", "─".repeat(width.saturating_sub(1))),
        style,
    ))
}

fn help_block_lines(theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let commands = [
        ("/help", "查看内置命令"),
        ("/clear", "清空当前 transcript"),
        ("/model", "查看当前 model"),
        ("/model <name>", "切换后续请求 model"),
        ("/models", "浏览 / 切换 provider 与模型"),
        ("/status", "当前会话快照"),
        ("/exit", "退出 TUI"),
    ];
    let border_style = Style::default().fg(theme.border_subtle).bg(theme.bg_base);
    let title_style = Style::default()
        .fg(theme.info_fg)
        .bg(theme.bg_base)
        .add_modifier(Modifier::BOLD);
    let mut lines = vec![block_top_border("帮助", width, title_style, border_style)];

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

    lines.push(block_bottom_border(width, border_style));
    lines
}

fn status_block_lines(
    snapshot: &StatusSnapshot,
    theme: &Theme,
    width: usize,
) -> Vec<Line<'static>> {
    let border_style = Style::default().fg(theme.border_subtle).bg(theme.bg_base);
    let title_style = Style::default()
        .fg(theme.info_fg)
        .bg(theme.bg_base)
        .add_modifier(Modifier::BOLD);
    vec![
        block_top_border("会话快照", width, title_style, border_style),
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
        block_bottom_border(width, border_style),
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

fn notice_block_lines(text: &str, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let marker = "◇ ";
    let marker_style = Style::default().fg(theme.info_fg).bg(theme.bg_base);
    let text_style = Style::default().fg(theme.text_body).bg(theme.bg_base);
    let content_width = width.saturating_sub(display_width(marker)).max(1);
    let wrapped = wrap_text(text, content_width);
    let wrapped = if wrapped.is_empty() {
        vec![String::new()]
    } else {
        wrapped
    };

    wrapped
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            if index == 0 {
                Line::from(vec![
                    Span::styled(marker, marker_style),
                    Span::styled(chunk, text_style),
                ])
            } else {
                let indent = " ".repeat(display_width(marker));
                Line::from(vec![
                    Span::styled(indent, text_style),
                    Span::styled(chunk, text_style),
                ])
            }
        })
        .collect()
}

fn error_block_lines(text: &str, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let border_style = Style::default().fg(theme.error_border).bg(theme.error_bg);
    let title_style = Style::default()
        .fg(theme.error_fg)
        .bg(theme.error_bg)
        .add_modifier(Modifier::BOLD);
    let body_style = Style::default().fg(theme.error_fg).bg(theme.error_bg);
    let body_prefix = "│ ";
    let content_width = width.saturating_sub(display_width(body_prefix)).max(1);
    let wrapped = wrap_text(text, content_width);
    let wrapped = if wrapped.is_empty() {
        vec![String::new()]
    } else {
        wrapped
    };

    let mut lines = vec![block_top_border(
        "致命错误",
        width,
        title_style,
        border_style,
    )];
    for chunk in wrapped {
        lines.push(Line::from(vec![
            Span::styled(body_prefix, border_style),
            Span::styled(chunk, body_style),
        ]));
    }
    lines.push(block_bottom_border(width, border_style));
    lines
}

fn tool_card_lines(
    card: &ToolCard,
    state: &AppState,
    theme: &Theme,
    width: usize,
    is_group_first: bool,
) -> Vec<Line<'static>> {
    let args = if state.tools_expanded {
        card.args.to_string()
    } else {
        tool_args_preview(&card.name, &card.args)
    };
    let collapsed = !state.tools_expanded;
    let mut head = tool_card_head(card, state, theme, args, collapsed);
    if collapsed {
        head.extend(collapsed_tool_summary(card, theme, width, is_group_first));
        return vec![Line::from(head)];
    }

    let mut lines = vec![Line::from(head)];

    match &card.output {
        Some(output) if output.is_empty() => lines.push(tool_output_line("", theme)),
        Some(output) => {
            for line in visible_tool_output_lines(card, output, width) {
                lines.push(tool_output_line(&line, theme));
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

    lines.push(block_bottom_border(
        width,
        Style::default().fg(theme.border_subtle).bg(theme.bg_base),
    ));
    lines
}

fn tool_card_head(
    card: &ToolCard,
    state: &AppState,
    theme: &Theme,
    args: String,
    collapsed: bool,
) -> Vec<Span<'static>> {
    let (glyph, glyph_color) = match card.status {
        ToolCardStatus::Running => (state.spinner_glyph(), theme.accent_primary),
        ToolCardStatus::Done => ("✓", theme.success_fg),
        ToolCardStatus::Error => ("✗", theme.error_fg),
    };
    let mut head = Vec::new();
    if !collapsed {
        head.push(Span::styled(
            "┌─ ",
            Style::default().fg(theme.border_subtle).bg(theme.bg_base),
        ));
    }
    head.extend([
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
            format!(" {args}"),
            Style::default().fg(theme.text_muted).bg(theme.bg_base),
        ),
    ]);
    if card.readonly {
        head.push(Span::styled(
            "  只读 · 自动运行",
            Style::default().fg(theme.text_secondary).bg(theme.bg_base),
        ));
    }
    head
}

fn collapsed_tool_summary(
    card: &ToolCard,
    theme: &Theme,
    width: usize,
    show_expand_hint: bool,
) -> Vec<Span<'static>> {
    let secondary = Style::default().fg(theme.text_secondary).bg(theme.bg_base);
    let muted = Style::default().fg(theme.text_muted).bg(theme.bg_base);

    let mut spans = if matches!(card.status, ToolCardStatus::Running) {
        vec![Span::styled(" · 运行中…", secondary)]
    } else if let Some(exit) = card.exit {
        let color = if exit == 0 {
            theme.success_fg
        } else {
            theme.error_fg
        };
        vec![Span::styled(
            format!(" · exit {exit}"),
            Style::default().fg(color).bg(theme.bg_base),
        )]
    } else {
        match &card.output {
            Some(output) if !output.is_empty() => {
                let line_count = visible_tool_output_lines(card, output, width).len();
                vec![Span::styled(format!(" · {line_count} 行 ⌄"), secondary)]
            }
            _ => Vec::new(),
        }
    };

    if show_expand_hint {
        spans.push(Span::styled(" · ctrl+o 展开", muted));
    }
    spans
}

fn tool_args_preview(tool_name: &str, args: &serde_json::Value) -> String {
    match tool_name {
        "read_file" | "list_dir" | "write_file" | "edit_file" => args
            .get("path")
            .and_then(serde_json::Value::as_str)
            .map(|path| format!("path={path}"))
            .unwrap_or_else(|| args.to_string()),
        "run_shell" => args
            .get("command")
            .and_then(serde_json::Value::as_str)
            .map(|command| format!("command={command}"))
            .unwrap_or_else(|| args.to_string()),
        "glob" => args
            .get("pattern")
            .and_then(serde_json::Value::as_str)
            .map(|pattern| format!("pattern={pattern}"))
            .unwrap_or_else(|| args.to_string()),
        "grep" => {
            let pattern = args.get("pattern").and_then(serde_json::Value::as_str);
            let path = args.get("path").and_then(serde_json::Value::as_str);
            match (pattern, path) {
                (Some(pattern), Some(path)) => format!("pattern={pattern} path={path}"),
                (Some(pattern), None) => format!("pattern={pattern}"),
                (None, Some(path)) => format!("path={path}"),
                (None, None) => args.to_string(),
            }
        }
        _ => args.to_string(),
    }
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

fn visible_tool_output_lines(card: &ToolCard, output: &str, width: usize) -> Vec<String> {
    let content_width = width.saturating_sub(display_width("│ ")).max(1);
    let mut lines = output.lines().collect::<Vec<_>>();
    if let Some(exit) = card.exit {
        let expected_exit = format!("exit: {exit}");
        if lines.first().is_some_and(|line| *line == expected_exit) {
            lines.remove(0);
        }
    }

    let mut wrapped = Vec::new();
    for line in lines {
        let chunks = wrap_text(line, content_width);
        if chunks.is_empty() {
            wrapped.push(String::new());
        } else {
            wrapped.extend(chunks);
        }
    }
    wrapped
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

fn render_activity(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let spans = activity_spans(state, theme);
    let paragraph = Paragraph::new(Line::from(spans))
        .style(Style::default().fg(theme.text_primary).bg(theme.bg_base));
    frame.render_widget(paragraph, area);
}

fn activity_spans(state: &AppState, theme: &Theme) -> Vec<Span<'static>> {
    let base = Style::default().bg(theme.bg_base);
    match &state.phase {
        Phase::Ready => {
            if state.idle_output_tokens() > 0 || state.idle_rate_tps().is_some() {
                idle_token_summary_spans(state, theme)
            } else {
                vec![Span::styled("◇ 就绪", base.fg(theme.text_muted))]
            }
        }
        Phase::Busy => {
            running_activity_spans(state, theme, format!("{} 处理…", state.spinner_glyph()))
        }
        Phase::CallingModel => {
            running_activity_spans(state, theme, format!("{} 调用模型…", state.spinner_glyph()))
        }
        Phase::ExecutingTool(name) => running_activity_spans(
            state,
            theme,
            format!("{} 执行 {name}…", state.spinner_glyph()),
        ),
        Phase::WaitingForPermission => {
            let mut spans = vec![Span::styled("▲ 等待授权…", base.fg(theme.warning_fg))];
            spans.extend(token_rate_spans(state, theme));
            spans
        }
    }
}

fn idle_token_summary_spans(state: &AppState, theme: &Theme) -> Vec<Span<'static>> {
    let base = Style::default().bg(theme.bg_base).fg(theme.text_muted);
    let mut text = format!("↓ {} tok", state.idle_output_tokens());
    if let Some(rate) = state.idle_rate_tps() {
        text.push_str(" · ");
        if state.idle_rate_is_approximate() {
            text.push_str(&format!("~{rate:.0} t/s"));
        } else {
            text.push_str(&format!("{rate:.0} t/s"));
        }
    }
    vec![Span::styled(text, base)]
}

fn running_activity_spans(state: &AppState, theme: &Theme, label: String) -> Vec<Span<'static>> {
    let base = Style::default().bg(theme.bg_base);
    let mut spans = vec![Span::styled(label, base.fg(theme.accent_primary))];
    spans.push(Span::styled(" · esc 中断", base.fg(theme.text_muted)));
    spans.extend(token_rate_spans(state, theme));
    spans
}

fn token_rate_spans(state: &AppState, theme: &Theme) -> Vec<Span<'static>> {
    let tokens = state.output_tokens_this_turn();
    let rate = state.last_rate_tps();
    if tokens == 0 && rate.is_none() {
        return Vec::new();
    }

    let base = Style::default().bg(theme.bg_base).fg(theme.text_muted);
    let mut text = String::new();
    if tokens > 0 {
        text.push_str(&format!(" · ↓ {tokens} tok"));
    }
    if let Some(rate) = rate {
        text.push_str(" · ");
        if state.last_rate_is_approximate() {
            text.push_str(&format!("~{rate:.0} t/s"));
        } else {
            text.push_str(&format!("{rate:.0} t/s"));
        }
    }
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Span::styled(text, base)]
    }
}

fn render_status(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let spans = status_meta_spans(state, theme);
    let paragraph = Paragraph::new(Line::from(spans))
        .style(Style::default().fg(theme.text_primary).bg(theme.bg_base));
    frame.render_widget(paragraph, area);
}

fn status_meta_spans(state: &AppState, theme: &Theme) -> Vec<Span<'static>> {
    let base = Style::default().fg(theme.text_muted).bg(theme.bg_base);
    let meta = format!(
        "{} · {} · iter {}/{} · {} msgs · {}",
        state.session.provider,
        state.session.model,
        state.iteration,
        state.session.max_iterations,
        state.dialog_message_count(),
        state.session.cwd.display()
    );
    vec![Span::styled(meta, base)]
}

fn mode_glyph_and_style(mode: PermissionMode, theme: &Theme) -> (&'static str, Style) {
    match mode {
        PermissionMode::Normal => ("▸", Style::default().fg(theme.text_muted).bg(theme.bg_base)),
        PermissionMode::AcceptEdits => (
            "▸▸",
            Style::default().fg(theme.accent_primary).bg(theme.bg_base),
        ),
        PermissionMode::Yolo => ("▲", Style::default().fg(theme.warning_fg).bg(theme.bg_base)),
    }
}

fn render_mode_line(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let mode = state.current_permission_mode();
    let label = permission_mode_label(mode);
    let (glyph, mode_style) = mode_glyph_and_style(mode, theme);
    let tail_style = Style::default().fg(theme.text_muted).bg(theme.bg_base);
    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled(glyph, mode_style),
        Span::styled(format!(" {label}"), mode_style),
        Span::styled(" · shift+tab 切换", tail_style),
    ]))
    .style(Style::default().bg(theme.bg_base));
    frame.render_widget(paragraph, area);
}
fn render_input(frame: &mut Frame<'_>, area: Rect, state: &AppState, theme: &Theme) {
    let input_text = state.input();
    let prompt_style = Style::default().fg(theme.accent_primary).bg(theme.bg_base);
    let text_style = Style::default().fg(theme.text_primary).bg(theme.bg_base);
    let inner_width = area.width.saturating_sub(2) as usize;
    let layout_width = inner_width.saturating_sub(display_width(INPUT_PROMPT));
    let layout = visual_input_layout(input_text, state.input_line.cursor, layout_width);
    let content_rows = area.height.saturating_sub(2).max(1) as usize;
    let scroll_offset = input_scroll_offset(layout.lines.len(), content_rows, layout.cursor.row);

    let content = if !input_text.is_empty() {
        input_content_lines(
            &layout,
            scroll_offset,
            content_rows,
            prompt_style,
            text_style,
        )
    } else if state.models_picker.is_some() {
        // 浮层活跃时不显示输入提示(浮层是焦点),也避免右对齐提示从浮层旁露出。
        vec![Line::from(vec![Span::styled(INPUT_PROMPT, prompt_style)])]
    } else {
        // 空态:提示右对齐,避开终端 IME 组合浮层(画在左侧光标处)。app 收不到
        // IME composition 事件,无法按组合状态隐藏提示,故靠版式让开碰撞区。
        let hint = "输入任务,或 / 执行命令…";
        let pad = inner_width.saturating_sub(display_width(INPUT_PROMPT) + display_width(hint));
        vec![Line::from(vec![
            Span::styled(INPUT_PROMPT, prompt_style),
            Span::styled(" ".repeat(pad), Style::default().bg(theme.bg_base)),
            Span::styled(
                hint,
                Style::default().fg(theme.text_muted).bg(theme.bg_base),
            ),
        ])]
    };
    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border_subtle).bg(theme.bg_base))
                .style(Style::default().fg(theme.text_primary).bg(theme.bg_base)),
        )
        .style(text_style);
    frame.render_widget(paragraph, area);
    frame.set_cursor_position(input_cursor_position(area, &layout, scroll_offset));
}

fn input_content_lines<'a>(
    layout: &InputVisualLayout,
    scroll_offset: usize,
    content_rows: usize,
    prompt_style: Style,
    text_style: Style,
) -> Vec<Line<'a>> {
    let prompt_width = display_width(INPUT_PROMPT);
    layout
        .lines
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(content_rows)
        .map(|(visual_row, text)| {
            let gutter = if visual_row == 0 {
                INPUT_PROMPT.to_string()
            } else {
                " ".repeat(prompt_width)
            };
            Line::from(vec![
                Span::styled(gutter, prompt_style),
                Span::styled(text.clone(), text_style),
            ])
        })
        .collect()
}

fn render_command_completion(
    frame: &mut Frame<'_>,
    input_area: Rect,
    state: &AppState,
    theme: &Theme,
) {
    let Some(completion) = &state.command_completion else {
        return;
    };
    if completion.candidates.is_empty() {
        return;
    }

    let list_height = completion.candidates.len() as u16 + 2;
    let width = input_area.width.min(52);
    let area = Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(list_height + 1),
        width,
        height: list_height,
    };

    let lines = completion
        .candidates
        .iter()
        .enumerate()
        .map(|(index, command)| {
            let base = Style::default().bg(theme.bg_surface);
            let style = if index == completion.selected {
                base.add_modifier(Modifier::REVERSED)
            } else {
                base
            };
            Line::from(vec![
                Span::styled(
                    format!("{:<10}", command.name),
                    style.fg(theme.accent_primary).add_modifier(Modifier::BOLD),
                ),
                Span::styled(command.description.to_string(), style.fg(theme.text_body)),
            ])
        })
        .collect::<Vec<_>>();

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(theme.border_strong)
                        .bg(theme.bg_surface),
                )
                .style(Style::default().fg(theme.text_primary).bg(theme.bg_surface)),
        )
        .style(Style::default().fg(theme.text_primary).bg(theme.bg_surface));
    frame.render_widget(paragraph, area);
}

fn render_models_picker(frame: &mut Frame<'_>, status_area: Rect, state: &AppState, theme: &Theme) {
    let Some(picker) = &state.models_picker else {
        return;
    };

    let visible = picker.visible_rows();
    let highlighted = picker
        .highlighted_row()
        .map(|row| (row.provider_id.as_str(), row.model.as_deref().unwrap_or("")));

    let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled(
        "模型",
        Style::default()
            .fg(theme.accent_primary)
            .bg(theme.bg_surface)
            .add_modifier(Modifier::BOLD),
    ))];

    if picker.shows_empty_hint() {
        lines.push(Line::from(Span::styled(
            "无匹配模型",
            Style::default()
                .fg(theme.text_secondary)
                .bg(theme.bg_surface),
        )));
    } else {
        for row in visible {
            match row.kind {
                ModelsPickerRowKind::ProviderHeader => {
                    lines.push(Line::from(Span::styled(
                        row.provider_id.clone(),
                        Style::default()
                            .fg(theme.text_secondary)
                            .bg(theme.bg_surface)
                            .add_modifier(Modifier::DIM),
                    )));
                }
                ModelsPickerRowKind::Model => {
                    let model = row.model.as_deref().unwrap_or("");
                    let is_highlighted = highlighted
                        .is_some_and(|(id, name)| id == row.provider_id && name == model);
                    let mut spans = vec![Span::styled(
                        "  ".to_string(),
                        Style::default().bg(theme.bg_surface),
                    )];
                    if row.is_current {
                        spans.push(Span::styled(
                            "● ".to_string(),
                            Style::default()
                                .fg(theme.accent_primary)
                                .bg(theme.bg_surface),
                        ));
                    } else {
                        spans.push(Span::styled(
                            "  ".to_string(),
                            Style::default().bg(theme.bg_surface),
                        ));
                    }
                    let mut style = Style::default().bg(theme.bg_surface);
                    if is_highlighted {
                        style = style
                            .fg(theme.accent_primary)
                            .add_modifier(Modifier::BOLD | Modifier::REVERSED);
                    } else {
                        style = style.fg(theme.text_body);
                    }
                    spans.push(Span::styled(model.to_string(), style));
                    lines.push(Line::from(spans));
                }
            }
        }
    }

    if !picker.filter_text().is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                "过滤: ",
                Style::default()
                    .fg(theme.text_secondary)
                    .bg(theme.bg_surface),
            ),
            Span::styled(
                picker.filter_text().to_string(),
                Style::default()
                    .fg(theme.accent_primary)
                    .bg(theme.bg_surface),
            ),
        ]));
    }

    lines.push(Line::from(Span::styled(
        "↑↓ 选 · Enter 切 · Esc 取消",
        Style::default()
            .fg(theme.text_secondary)
            .bg(theme.bg_surface),
    )));

    let list_height = lines.len() as u16 + 2;
    let width = status_area.width.min(56);
    let area = Rect {
        x: status_area.x,
        y: status_area.y.saturating_sub(list_height + 1),
        width,
        height: list_height,
    };

    frame.render_widget(Clear, area);

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(
                    Style::default()
                        .fg(theme.border_strong)
                        .bg(theme.bg_surface),
                )
                .style(Style::default().fg(theme.text_primary).bg(theme.bg_surface)),
        )
        .style(Style::default().fg(theme.text_primary).bg(theme.bg_surface));
    frame.render_widget(paragraph, area);
}

fn highlight_selection(frame: &mut Frame<'_>, state: &AppState, theme: &Theme) {
    let Some(selection) = state.selection.selection else {
        return;
    };

    let buffer = frame.buffer_mut();
    let area = *buffer.area();
    if area.width == 0 || area.height == 0 {
        return;
    }

    let normalized = selection.normalized();
    let area_bottom = area.y.saturating_add(area.height.saturating_sub(1));
    let row_start = normalized.start.row.max(area.y);
    let row_end = normalized.end.row.min(area_bottom);
    if row_start > row_end {
        return;
    }

    let area_right = area.x.saturating_add(area.width);
    for row in row_start..=row_end {
        let Some(range) = col_range_for_row(&selection, row, area_right) else {
            continue;
        };
        let start = range.start.max(area.x);
        let end = range.end.min(area_right);
        for col in start..end {
            if let Some(cell) = buffer.cell_mut(Position::new(col, row)) {
                cell.set_bg(theme.selection_bg);
            }
        }
    }
}

pub(crate) fn selection_text(buffer: &Buffer, selection: &Selection) -> String {
    let area = *buffer.area();
    if area.width == 0 || area.height == 0 {
        return String::new();
    }

    let normalized = selection.normalized();
    let area_bottom = area.y.saturating_add(area.height.saturating_sub(1));
    let row_start = normalized.start.row.max(area.y);
    let row_end = normalized.end.row.min(area_bottom);
    if row_start > row_end {
        return String::new();
    }

    let area_right = area.x.saturating_add(area.width);
    let mut lines = Vec::new();
    for row in row_start..=row_end {
        let Some(range) = col_range_for_row(selection, row, area_right) else {
            continue;
        };
        let mut line = String::new();
        let mut col = range.start.max(area.x);
        let end = range.end.min(area_right);
        while col < end {
            let Some(cell) = buffer.cell(Position::new(col, row)) else {
                col = col.saturating_add(1);
                continue;
            };
            let symbol = cell.symbol();
            line.push_str(symbol);
            col = col.saturating_add(display_width(symbol).max(1) as u16);
        }
        lines.push(line.trim_end().to_string());
    }

    lines.join("\n")
}

fn input_cursor_position(area: Rect, layout: &InputVisualLayout, scroll_offset: usize) -> Position {
    let max_x = area.x.saturating_add(area.width.saturating_sub(2));
    let max_y = area.y.saturating_add(area.height.saturating_sub(2));
    let x = area
        .x
        .saturating_add(1)
        .saturating_add(display_width(INPUT_PROMPT) as u16)
        .saturating_add(layout.cursor.col as u16)
        .min(max_x);
    let visible_row = layout.cursor.row.saturating_sub(scroll_offset) as u16;
    let y = area
        .y
        .saturating_add(1)
        .saturating_add(visible_row)
        .min(max_y);
    Position::new(x, y)
}

#[cfg(test)]
mod tests {
    use super::{render, selection_text, transcript_line_count};
    use crate::config::{AuthType, ProviderKind, ProviderProfile};
    use crate::permission::PermissionMode;
    use crate::provider::Usage;
    use crate::tui::app::{
        AppState, ModelsPicker, Phase, SessionSnapshot, ToolCard, ToolCardStatus, TranscriptBlock,
    };
    use crate::tui::channel::{AgentEvent, PermissionRequest};
    use crate::tui::selection::{Point, Selection, SelectionState};
    use crate::tui::theme::Theme;
    use crate::tui::width::display_width;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::{Position, Rect};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::Terminal;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio::sync::{mpsc, oneshot};

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct StyleKey {
        fg: Option<&'static str>,
        bg: Option<&'static str>,
        bold: bool,
        dim: bool,
        reversed: bool,
    }

    fn render_to_buffer(state: &AppState, theme: &Theme) -> Buffer {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let completed = terminal.draw(|frame| render(frame, state, theme)).unwrap();
        completed.buffer.clone()
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
                x += display_width(&symbol).max(1) as u16;
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
                x += display_width(&symbol).max(1) as u16;
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

    fn activity_line(text: &str) -> String {
        text.lines()
            .find(|line| {
                line.contains("◇ 就绪")
                    || line.contains("调用模型")
                    || line.contains("执行 ")
                    || line.contains("等待授权")
                    || line.contains("处理…")
                    || line.contains("↓ ")
                    || line.contains("t/s")
            })
            .unwrap()
            .to_string()
    }

    fn meta_line(text: &str) -> String {
        text.lines()
            .find(|line| line.contains(" · iter "))
            .unwrap()
            .to_string()
    }

    fn mode_line(text: &str) -> String {
        text.lines()
            .find(|line| line.contains("shift+tab 切换"))
            .unwrap()
            .to_string()
    }

    fn jump_to_bottom_pill_line(text: &str) -> String {
        text.lines()
            .find(|line| line.contains("ctrl+End"))
            .map(|line| line.to_string())
            .unwrap_or_default()
    }

    fn scrolled_away_state() -> AppState {
        let mut state = AppState::new();
        state.transcript.push(TranscriptBlock::Assistant(
            "first line\nsecond line\nthird line".to_string(),
        ));
        state.scroll_to_top(100, 20);
        state
    }

    fn state_with_permission_mode(mode: PermissionMode) -> AppState {
        let state = AppState::new();
        *state
            .permission_mode
            .lock()
            .expect("permission mode mutex poisoned") = mode;
        state
    }

    fn find_symbol(buffer: &Buffer, symbol: &str) -> Position {
        let area = *buffer.area();
        for row in area.y..area.y.saturating_add(area.height) {
            for col in area.x..area.x.saturating_add(area.width) {
                if buffer
                    .cell(Position::new(col, row))
                    .is_some_and(|cell| cell.symbol() == symbol)
                {
                    return Position::new(col, row);
                }
            }
        }
        panic!("symbol {symbol:?} not found in buffer");
    }

    fn selection(anchor_col: u16, anchor_row: u16, head_col: u16, head_row: u16) -> Selection {
        Selection {
            anchor: Point {
                col: anchor_col,
                row: anchor_row,
            },
            head: Point {
                col: head_col,
                row: head_row,
            },
        }
    }

    #[test]
    fn selection_highlight_snapshot() {
        let theme = Theme::midnight();
        let mut state = AppState::new();
        state
            .transcript
            .push(TranscriptBlock::Assistant("alpha\n你好 beta".to_string()));

        let before = render_to_buffer(&state, &theme);
        let start = find_symbol(&before, "你");
        state.selection = SelectionState {
            selection: Some(selection(
                start.x,
                start.y,
                start.x.saturating_add(3),
                start.y,
            )),
            dragging: false,
        };

        let after = render_to_buffer(&state, &theme);
        let leading = after.cell(start).expect("selected leading CJK cell");
        let continuation = after
            .cell(Position::new(start.x.saturating_add(1), start.y))
            .expect("selected CJK continuation cell");
        let original = before.cell(start).expect("original selected cell");

        assert_eq!(leading.bg, theme.selection_bg);
        assert_eq!(continuation.bg, theme.selection_bg);
        assert_eq!(leading.fg, original.fg);
        assert!(
            state.has_selection(),
            "released selection should remain highlighted"
        );

        let text = buffer_to_styled(&after, &theme);
        println!(
            "\n--- selection highlight frame ---\n{text}\n--- end selection highlight frame ---"
        );
        insta::assert_snapshot!("tui_selection_highlight", text);
    }

    #[test]
    fn selection_text_skips_cjk_continuation_cells() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        buffer.set_string(0, 0, "你好  ", Style::default());

        let text = selection_text(&buffer, &selection(0, 0, 3, 0));

        assert_eq!(text, "你好");
    }

    #[test]
    fn selection_text_trims_lines_and_joins_cross_line() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 2));
        buffer.set_string(0, 0, "abc   ", Style::default());
        buffer.set_string(0, 1, "de    ", Style::default());

        let text = selection_text(&buffer, &selection(1, 0, 1, 1));

        assert_eq!(text, "bc\nde");
    }

    #[test]
    fn selection_text_single_line_uses_inclusive_end_col() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        buffer.set_string(0, 0, "abcdef", Style::default());

        let text = selection_text(&buffer, &selection(1, 0, 3, 0));

        assert_eq!(text, "bcd");
    }

    #[test]
    fn selection_text_out_of_bounds_selection_does_not_panic() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        buffer.set_string(0, 0, "hi", Style::default());

        let text = selection_text(&buffer, &selection(5, 5, 9, 5));

        assert_eq!(text, "");
    }
    #[test]
    fn display_width_treats_common_emoji_as_wide() {
        assert_eq!(display_width("a"), 1);
        assert_eq!(display_width("你好"), 4);
        assert_eq!(display_width("👋"), 2);
        assert_eq!(display_width("😊"), 2);
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
        state.set_input_text("你好");
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, &state, &Theme::midnight()))
            .unwrap();

        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(7, 20));
    }
    #[test]
    fn input_render_sets_cursor_at_multiline_cursor_position() {
        let mut state = AppState::new();
        state.set_input_text("ab\ncd");
        state.input_line.cursor = 0;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, &state, &Theme::midnight()))
            .unwrap();

        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(3, 19));
    }
    #[test]
    fn multiline_input_dynamic_height_soft_wrap_snapshot() {
        let mut state = AppState::new();
        state.transcript.push(TranscriptBlock::Assistant(
            "transcript remains visible".to_string(),
        ));
        let text = "普通 multi\nabcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ\nfinal 行";
        state.set_input_text(text);
        state.input_line.cursor = "普通 multi\nabcdefghijklmnopqrstuvwxyz0123456789ABCD".len();

        let backend = TestBackend::new(42, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &state, &Theme::midnight()))
            .unwrap();

        terminal
            .backend_mut()
            .assert_cursor_position(Position::new(5, 19));
        let output = buffer_to_plain(terminal.backend().buffer());
        assert!(output.contains("transcript remains visible"));
        assert!(output.contains("> 普通 multi"));
        assert!(output.contains("abcdefghijklmnopqrstuvwxyz0123456789AB"));
        assert!(output.contains("  CDEFGHIJ"));
        assert!(output.contains("final 行"));
        insta::assert_snapshot!("tui_multiline_input_dynamic_height_soft_wrap", output);
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

    fn wps_openai_profiles() -> BTreeMap<String, ProviderProfile> {
        BTreeMap::from([
            (
                "wps".to_string(),
                ProviderProfile {
                    id: "wps".to_string(),
                    kind: ProviderKind::OpenAi,
                    base_url: None,
                    model: "zhipu/glm-5.2".to_string(),
                    auth_type: AuthType::ApiKey,
                },
            ),
            (
                "openai".to_string(),
                ProviderProfile {
                    id: "openai".to_string(),
                    kind: ProviderKind::OpenAi,
                    base_url: None,
                    model: "gpt-5.5".to_string(),
                    auth_type: AuthType::ApiKey,
                },
            ),
        ])
    }

    fn models_picker_state() -> AppState {
        let profiles = wps_openai_profiles();
        let mut state = AppState::with_session(SessionSnapshot {
            provider: "wps".to_string(),
            model: "zhipu/glm-5.2".to_string(),
            max_iterations: 8,
            cwd: PathBuf::from("workspace"),
            tools: 7,
        });
        state.provider_profiles = profiles.clone();
        state.models_picker = Some(ModelsPicker::new(&profiles, ("wps", "zhipu/glm-5.2")));
        state
    }

    fn models_picker_filtered_state() -> AppState {
        let mut state = models_picker_state();
        if let Some(picker) = state.models_picker.as_mut() {
            for ch in "glm".chars() {
                picker.push_filter_char(ch);
            }
        }
        state
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
    fn command_completion_snapshot() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), &tx);
        let text = render_to_styled(&state, &Theme::midnight());

        for command in ["/help", "/models", "/compact"] {
            assert!(
                text.contains(command),
                "completion popup should list {command}"
            );
        }
        println!(
            "\n--- command completion frame ---\n{text}\n--- end command completion frame ---"
        );
        insta::assert_snapshot!("tui_command_completion", text);
    }

    #[test]
    fn models_picker_open_snapshot() {
        let state = models_picker_state();
        let text = render_to_styled(&state, &Theme::midnight());

        assert!(text.contains("模型"));
        assert!(text.contains("wps"));
        assert!(text.contains("zhipu/glm-5.2"));
        assert!(text.contains("↑↓ 选 · Enter 切 · Esc 取消"));
        println!("\n--- models picker open ---\n{text}\n--- end models picker open ---");
        insta::assert_snapshot!("tui_models_picker_open", text);
    }

    #[test]
    fn models_picker_filtered_snapshot() {
        let state = models_picker_filtered_state();
        let text = render_to_styled(&state, &Theme::midnight());
        println!("\n--- models picker filtered ---\n{text}\n--- end models picker filtered ---");
        insta::assert_snapshot!("tui_models_picker_filtered", text);
    }

    #[test]
    fn jump_to_bottom_pill_hidden_when_following() {
        let state = AppState::new();
        let text = render_to_styled(&state, &Theme::midnight());
        assert!(!text.contains("ctrl+End"));
    }

    #[test]
    fn jump_to_bottom_pill_idle_snapshot() {
        let state = scrolled_away_state();
        let text = render_to_styled(&state, &Theme::midnight());
        insta::assert_snapshot!(
            "tui_jump_to_bottom_pill_idle",
            jump_to_bottom_pill_line(&text)
        );
    }

    #[test]
    fn jump_to_bottom_pill_with_new_messages_snapshot() {
        let mut state = scrolled_away_state();
        state.new_message_count = 2;
        let text = render_to_styled(&state, &Theme::midnight());
        insta::assert_snapshot!(
            "tui_jump_to_bottom_pill_new_messages",
            jump_to_bottom_pill_line(&text)
        );
    }

    #[test]
    fn mode_line_normal_snapshot() {
        let state = state_with_permission_mode(PermissionMode::Normal);
        let text = render_to_styled(&state, &Theme::midnight());
        insta::assert_snapshot!("tui_mode_line_normal", mode_line(&text));
    }

    #[test]
    fn mode_line_accept_edits_snapshot() {
        let state = state_with_permission_mode(PermissionMode::AcceptEdits);
        let text = render_to_styled(&state, &Theme::midnight());
        insta::assert_snapshot!("tui_mode_line_accept_edits", mode_line(&text));
    }

    #[test]
    fn mode_line_yolo_snapshot() {
        let state = state_with_permission_mode(PermissionMode::Yolo);
        let text = render_to_styled(&state, &Theme::midnight());
        insta::assert_snapshot!("tui_mode_line_yolo", mode_line(&text));
    }

    #[test]
    fn yolo_mode_shows_mode_line_without_permission_box() {
        let state = state_with_permission_mode(PermissionMode::Yolo);
        let text = render_to_styled(&state, &Theme::midnight());

        assert!(text.contains("▲ yolo"));
        assert!(text.contains("shift+tab 切换"));
        assert!(!text.contains("MODE:"));
        assert!(!text.contains("需要授权"));
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
    fn tool_card_expanded_done_snapshot() {
        let mut state = AppState::new();
        state.tools_expanded = true;
        state.transcript.push(TranscriptBlock::Tool(tool_card(
            "write_file",
            ToolCardStatus::Done,
            Some("pub struct Config {\n    pub timeout_secs: u64,\n}"),
            false,
            false,
        )));
        let text = render_to_styled(&state, &Theme::midnight());

        println!(
            "\n--- tool card expanded done frame ---\n{text}\n--- end tool card expanded done frame ---"
        );
        insta::assert_snapshot!("tui_tool_card_expanded_done", text);
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
    fn folding_only_affects_tool_blocks_snapshot() {
        let mut state = AppState::new();
        state.transcript.push(TranscriptBlock::User(
            "第一行用户消息\n第二行用户消息仍然完整显示".to_string(),
        ));
        state.transcript.push(TranscriptBlock::Tool(tool_card(
            "read_file",
            ToolCardStatus::Done,
            Some("hidden line one\nhidden line two"),
            true,
            false,
        )));
        state.transcript.push(TranscriptBlock::Assistant(
            "第一行最终回答\n第二行最终回答仍然完整显示".to_string(),
        ));
        let text = render_to_styled(&state, &Theme::midnight());

        println!(
            "\n--- folding only tool blocks frame ---\n{text}\n--- end folding only tool blocks frame ---"
        );
        insta::assert_snapshot!("tui_folding_only_affects_tool_blocks", text);
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
    fn follows_bottom_shows_latest_content_despite_wide_error_border() {
        const NEEDLE: &str = "NEEDLE_LAST_LINE";
        const WIDTH: u16 = 40;
        const HEIGHT: u16 = 24;

        let mut state = AppState::new();
        for index in 0..12 {
            state.transcript.push(TranscriptBlock::User(format!(
                "filler {index:02}: 压栈使预换行后总逻辑行数超过 transcript 视口高度"
            )));
        }
        state.transcript.push(TranscriptBlock::Error(
            "鉴权失败:未找到 API_KEY。Agent Loop 已终止。".to_string(),
        ));
        state
            .transcript
            .push(TranscriptBlock::Assistant(NEEDLE.to_string()));

        let theme = Theme::midnight();
        let area = ratatui::layout::Rect::new(0, 0, WIDTH, HEIGHT);
        let viewport_lines = super::transcript_viewport_height(area, &state);
        let total_lines = transcript_line_count(&state, &theme, WIDTH as usize);
        assert!(
            total_lines > viewport_lines,
            "复现前提:总逻辑行({total_lines})应大于视口高度({viewport_lines})"
        );

        let text = render_to_plain_with_size(&state, &theme, WIDTH, HEIGHT);
        assert!(
            text.contains(NEEDLE),
            "follows_bottom 时最新内容应在视口内可见,但渲染输出未包含针标串 {NEEDLE:?}\n--- rendered ---\n{text}\n--- end ---"
        );
    }

    fn line_plain(line: &ratatui::text::Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn assert_transcript_lines_fit_width(lines: &[ratatui::text::Line<'_>], width: usize) {
        for line in lines {
            let plain = line_plain(line);
            if plain.is_empty() {
                continue;
            }
            assert!(
                display_width(&plain) <= width,
                "逻辑行宽度 {} 超过视口宽度 {width}: {plain:?}",
                display_width(&plain)
            );
        }
    }

    #[test]
    fn error_border_adapts_to_width_and_occupies_one_screen_line() {
        const WIDTH: usize = 40;
        let mut state = AppState::new();
        state.transcript.push(TranscriptBlock::Error(
            "鉴权失败:未找到 API_KEY。".to_string(),
        ));
        let theme = Theme::midnight();
        let lines = super::transcript_content_lines(&state, &theme, WIDTH);

        let border_lines: Vec<_> = lines
            .iter()
            .map(line_plain)
            .filter(|plain| plain.starts_with('┌') || plain.starts_with('└'))
            .collect();
        assert_eq!(border_lines.len(), 2, "Error 块应有顶/底各一条边框行");
        for border in border_lines {
            assert_eq!(
                display_width(&border),
                WIDTH,
                "边框行应铺满视口宽度: {border:?}"
            );
        }
        assert_transcript_lines_fit_width(&lines, WIDTH);
    }

    #[test]
    fn expanded_tool_output_wraps_long_lines_before_viewport_slice() {
        const WIDTH: usize = 40;
        let long_line = "W".repeat(100);
        let mut state = AppState::new();
        state.tools_expanded = true;
        state.transcript.push(TranscriptBlock::Tool(ToolCard {
            id: "read-file-1".to_string(),
            name: "read_file".to_string(),
            args: json!({ "path": "note.txt" }),
            readonly: true,
            status: ToolCardStatus::Done,
            output: Some(format!("{long_line}\nsecond line")),
            truncated: false,
            exit: None,
        }));
        let theme = Theme::midnight();
        let lines = super::transcript_content_lines(&state, &theme, WIDTH);

        let output_lines: Vec<_> = lines
            .iter()
            .map(line_plain)
            .filter(|plain| plain.starts_with('│') && !plain.contains("exit "))
            .collect();
        assert!(
            output_lines.len() >= 3,
            "100 字符长行在 width={WIDTH} 下应预换行为多行,实际: {output_lines:?}"
        );
        assert!(output_lines.iter().any(|line| line.contains("second line")));
        for line in &output_lines {
            assert!(
                display_width(line) <= WIDTH,
                "工具输出行宽度 {} 超过视口: {line:?}",
                display_width(line)
            );
        }
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
    fn collapsed_group_first_tool_shows_ctrl_o_expand_hint_on_summary_only() {
        let theme = Theme::midnight();

        let mut single = AppState::new();
        single.transcript.push(TranscriptBlock::Tool(tool_card(
            "read_file",
            ToolCardStatus::Done,
            Some("line one\nline two\nline three"),
            true,
            false,
        )));
        let single_text = render_to_styled(&single, &theme);
        assert!(
            single_text.contains("ctrl+o 展开"),
            "transcript 开头 Tool 组首应含 ctrl+o 展开"
        );
        assert!(
            !meta_line(&single_text).contains("ctrl+o"),
            "底部 meta 行不应含 ctrl+o 提示"
        );

        let mut dual = AppState::new();
        dual.transcript.push(TranscriptBlock::Tool(tool_card(
            "read_file",
            ToolCardStatus::Done,
            Some("first"),
            true,
            false,
        )));
        dual.transcript.push(TranscriptBlock::Tool(tool_card(
            "write_file",
            ToolCardStatus::Done,
            Some("second\nline"),
            false,
            false,
        )));
        let dual_text = render_to_styled(&dual, &theme);
        assert_eq!(
            dual_text.matches("ctrl+o 展开").count(),
            1,
            "同组仅组首 Tool 带 ctrl+o 展开: {dual_text}"
        );

        let mut grouped = AppState::new();
        grouped
            .transcript
            .push(TranscriptBlock::User("先做 read".to_string()));
        grouped.transcript.push(TranscriptBlock::Tool(tool_card(
            "read_file",
            ToolCardStatus::Done,
            Some("chunk"),
            true,
            false,
        )));
        grouped.transcript.push(TranscriptBlock::Assistant(
            "读完了,再写两个文件".to_string(),
        ));
        grouped.transcript.push(TranscriptBlock::Tool(tool_card(
            "write_file",
            ToolCardStatus::Done,
            Some("a"),
            false,
            false,
        )));
        grouped.transcript.push(TranscriptBlock::Tool(tool_card(
            "grep",
            ToolCardStatus::Done,
            Some("match\nline"),
            false,
            false,
        )));
        let grouped_text = render_to_styled(&grouped, &theme);
        assert_eq!(
            grouped_text.matches("ctrl+o 展开").count(),
            2,
            "User→Tool 与 Assistant 后新 Tool 组各应有一个 hint: {grouped_text}"
        );
        let grouped_lines: Vec<_> = grouped_text.lines().collect();
        let read_line = grouped_lines
            .iter()
            .find(|line| line.contains("read_file"))
            .expect("read_file 行");
        let write_line = grouped_lines
            .iter()
            .find(|line| line.contains("write_file"))
            .expect("write_file 行");
        let grep_line = grouped_lines
            .iter()
            .find(|line| line.contains("grep"))
            .expect("grep 行");
        assert!(read_line.contains("ctrl+o 展开"));
        assert!(write_line.contains("ctrl+o 展开"));
        assert!(!grep_line.contains("ctrl+o"));

        let mut expanded = single;
        expanded.tools_expanded = true;
        let expanded_text = render_to_styled(&expanded, &theme);
        assert!(
            !expanded_text.contains("ctrl+o"),
            "展开态不应显示 ctrl+o 提示: {expanded_text}"
        );
    }

    #[test]
    fn tool_group_ctrl_o_hints_snapshot() {
        let mut state = AppState::new();
        state
            .transcript
            .push(TranscriptBlock::User("先做 read".to_string()));
        state.transcript.push(TranscriptBlock::Tool(tool_card(
            "read_file",
            ToolCardStatus::Done,
            Some("chunk one\nchunk two"),
            true,
            false,
        )));
        state.transcript.push(TranscriptBlock::Assistant(
            "读完了,再写两个文件".to_string(),
        ));
        state.transcript.push(TranscriptBlock::Tool(tool_card(
            "write_file",
            ToolCardStatus::Done,
            Some("written"),
            false,
            false,
        )));
        state.transcript.push(TranscriptBlock::Tool(tool_card(
            "grep",
            ToolCardStatus::Done,
            Some("match"),
            false,
            false,
        )));
        let text = render_to_styled(&state, &Theme::midnight());

        println!("\n--- tool group ctrl+o hints frame ---\n{text}\n--- end ---");
        insta::assert_snapshot!("tui_tool_group_ctrl_o_hints", text);
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
            let rendered = render_to_styled(&state, &Theme::midnight());
            text.push_str(label);
            text.push_str(": ");
            text.push_str(&activity_line(&rendered));
            text.push_str(" | meta: ");
            text.push_str(&meta_line(&rendered));
            text.push('\n');
        }

        println!("\n--- phase status lines ---\n{text}--- end phase status lines ---");
        insta::assert_snapshot!("tui_phase_status_lines", text);
    }

    #[test]
    fn activity_token_rate_snapshots() {
        let theme = Theme::midnight();
        let mut text = String::new();

        let mut streaming = AppState::new();
        streaming.phase = Phase::CallingModel;
        streaming.spinner_frame = 3;
        streaming.record_streaming_chars(400, Duration::from_secs(2));
        text.push_str("streaming_approx: ");
        text.push_str(&activity_line(&render_to_styled(&streaming, &theme)));
        text.push('\n');

        let mut real_running = AppState::new();
        real_running.phase = Phase::CallingModel;
        real_running.spinner_frame = 3;
        real_running.record_usage(
            Usage {
                input_tokens: 0,
                output_tokens: 120,
            },
            Duration::from_secs(2),
        );
        text.push_str("real_running: ");
        text.push_str(&activity_line(&render_to_styled(&real_running, &theme)));
        text.push('\n');

        let mut idle = AppState::new();
        idle.record_usage(
            Usage {
                input_tokens: 10,
                output_tokens: 120,
            },
            Duration::from_secs(2),
        );
        idle.apply(AgentEvent::TurnComplete);
        text.push_str("idle_after_turn: ");
        text.push_str(&activity_line(&render_to_styled(&idle, &theme)));
        text.push('\n');

        println!("\n--- activity token rates ---\n{text}--- end activity token rates ---");
        insta::assert_snapshot!("tui_activity_token_rates", text);
    }
}
