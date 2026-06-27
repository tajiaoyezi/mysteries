use crate::tui::app::{AppState, Phase, TranscriptBlock};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn render(frame: &mut Frame<'_>, state: &AppState) {
    let area = frame.area();
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

    render_header(frame, rows[0]);
    render_transcript(frame, rows[1], state);
    if state.pending_permission.is_some() {
        render_permission(frame, rows[2], state);
    }
    render_status(frame, rows[3], state);
    render_input(frame, rows[4], state);
}

fn permission_height(state: &AppState) -> u16 {
    if state.pending_permission.is_some() {
        7
    } else {
        0
    }
}

fn render_header(frame: &mut Frame<'_>, area: Rect) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray));
    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled(
            "✦ mysteries",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  agent · v1.0"),
    ]))
    .block(block);
    frame.render_widget(paragraph, area);
}

fn render_transcript(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let lines = if state.transcript.is_empty() {
        welcome_lines()
    } else {
        transcript_lines(state)
    };
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn welcome_lines() -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "✦ MYSTERIES",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("AGENT · v1.0 · 终端编码助手"),
        Line::from(
            "读只读,写必询 —— 每一次文件改动与命令执行,都先把 diff 摊给你,等你按下 y 才动手。",
        ),
        Line::from("试试 ↓"),
        Line::from("〔任务〕 给 Config 加 timeout_secs 字段"),
        Line::from("〔/help〕 查看内置命令"),
        Line::from("〔/status〕 当前会话快照"),
        Line::from("〔错误〕 演示:鉴权失败(致命错误,终止 Loop)"),
    ]
}

fn transcript_lines(state: &AppState) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for block in &state.transcript {
        match block {
            TranscriptBlock::User(text) => {
                lines.push(Line::from(vec![
                    Span::styled("> ", Style::default().fg(Color::Yellow)),
                    Span::raw(text.clone()),
                ]));
            }
            TranscriptBlock::Assistant(text) => {
                lines.push(Line::from(vec![
                    Span::styled("m ", Style::default().fg(Color::Cyan)),
                    Span::raw(text.clone()),
                ]));
            }
            TranscriptBlock::Error(text) => {
                lines.push(Line::from(vec![
                    Span::styled("! ", Style::default().fg(Color::Red)),
                    Span::raw(text.clone()),
                ]));
            }
        }
        lines.push(Line::from(""));
    }
    lines
}

fn render_permission(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let Some(request) = &state.pending_permission else {
        return;
    };
    let paragraph = Paragraph::new(vec![
        Line::from(Span::styled(
            "▲ 需要授权",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("tool: {}", request.tool_name)),
        Line::from(format!("args: {}", request.args)),
        Line::from("[y · 允许]   [n · 拒绝]"),
        Line::from("提示:Enter = 允许 · Esc = 拒绝"),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    frame.render_widget(paragraph, area);
}

fn render_status(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let label = match state.phase {
        Phase::Ready => "就绪",
        Phase::Busy => "忙",
        Phase::WaitingForPermission => "等待授权",
    };
    let paragraph = Paragraph::new(format!("status: {label}"));
    frame.render_widget(paragraph, area);
}

fn render_input(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let content = if state.input.is_empty() {
        "mysteries ▸ 输入任务,或 / 执行命令…".to_string()
    } else {
        format!("mysteries ▸ {}", state.input)
    };
    let paragraph = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::tui::app::AppState;
    use crate::tui::channel::{AgentEvent, PermissionRequest};
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;
    use serde_json::json;
    use tokio::sync::oneshot;

    fn render_to_text(state: &AppState) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, state)).unwrap();
        buffer_to_text(terminal.backend().buffer())
    }

    fn buffer_to_text(buffer: &Buffer) -> String {
        let area = *buffer.area();
        let mut lines = Vec::new();
        for y in area.y..area.y + area.height {
            let mut line = String::new();
            let mut x = area.x;
            while x < area.x + area.width {
                let symbol = buffer[(x, y)].symbol();
                line.push_str(symbol);
                x += if is_cjk_wide(symbol) { 2 } else { 1 };
            }
            lines.push(line.trim_end_matches(' ').to_string());
        }
        lines.join("\n")
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

    #[test]
    fn welcome_state_snapshot() {
        let state = AppState::new();
        let text = render_to_text(&state);

        println!("\n--- welcome frame ---\n{text}\n--- end welcome frame ---");
        insta::assert_snapshot!("tui_welcome_state", text);
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
        let text = render_to_text(&state);

        println!("\n--- permission frame ---\n{text}\n--- end permission frame ---");
        insta::assert_snapshot!("tui_permission_state", text);
    }
}
