use crate::tui::theme::Theme;
use crate::tui::width::{char_width, display_width};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SyntectColor, Theme as SyntectTheme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

pub(crate) fn render_markdown(text: &str, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut renderer = MarkdownRenderer::new(theme, width);
    let parser = Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH);

    for event in parser {
        renderer.handle_event(event);
    }

    renderer.finish()
}

pub(crate) fn wrap_spans(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut current_width = 0usize;

    for span in spans {
        for ch in span.content.as_ref().chars() {
            let ch_width = char_width(ch);
            if ch_width > 0 && current_width + ch_width > width && !current.is_empty() {
                lines.push(current);
                current = Vec::new();
                current_width = 0;
            }

            push_styled_text(&mut current, ch.to_string(), span.style);
            current_width += ch_width;
        }
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }

    lines
}

#[derive(Default)]
struct InlineState {
    emphasis: usize,
    strong: usize,
    strikethrough: usize,
    link: usize,
}

#[derive(Clone, Copy)]
enum BlockKind {
    Paragraph,
    Heading(HeadingLevel),
}

#[derive(Clone, Default)]
struct LinePrefix {
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
}

struct ListState {
    ordered: bool,
    next: u64,
}

struct CodeBlock {
    language: String,
    text: String,
}

#[derive(Default)]
struct TableState {
    rows: Vec<Vec<Vec<Span<'static>>>>,
    current_row: Vec<Vec<Span<'static>>>,
    current_cell: Vec<Span<'static>>,
}

struct MarkdownRenderer<'a> {
    theme: &'a Theme,
    width: usize,
    inline: InlineState,
    block: Option<BlockKind>,
    quote_depth: usize,
    list_stack: Vec<ListState>,
    item_prefix_stack: Vec<LinePrefix>,
    code_block: Option<CodeBlock>,
    table: Option<TableState>,
    block_prefix: LinePrefix,
    current: Vec<Span<'static>>,
    logical_lines: Vec<Vec<Span<'static>>>,
    output: Vec<Line<'static>>,
}

impl<'a> MarkdownRenderer<'a> {
    fn new(theme: &'a Theme, width: usize) -> Self {
        Self {
            theme,
            width: width.max(1),
            inline: InlineState::default(),
            block: None,
            quote_depth: 0,
            list_stack: Vec::new(),
            item_prefix_stack: Vec::new(),
            code_block: None,
            table: None,
            block_prefix: LinePrefix::default(),
            current: Vec::new(),
            logical_lines: Vec::new(),
            output: Vec::new(),
        }
    }

    fn handle_event(&mut self, event: Event<'_>) {
        if self.code_block.is_some() {
            self.handle_code_event(event);
            return;
        }
        if self.table.is_some() {
            self.handle_table_event(event);
            return;
        }

        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                self.push_text(text.into_string(), self.current_text_style());
            }
            Event::Code(code) => {
                self.push_text(code.into_string(), self.inline_code_style());
            }
            Event::SoftBreak | Event::HardBreak => self.push_logical_line(),
            Event::Rule => self.emit_horizontal_rule(),
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        if self.code_block.is_some() {
            self.finish_code_block();
        }
        if self.table.is_some() {
            self.finish_table();
        }
        if self.block.is_some() {
            self.finish_block();
        }
        if self.output.is_empty() {
            self.output.push(Line::from(""));
        }
        self.output
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.start_block(BlockKind::Paragraph),
            Tag::Heading { level, .. } => self.start_block(BlockKind::Heading(level)),
            Tag::BlockQuote(_) => {
                if self.quote_depth == 0 && self.list_stack.is_empty() {
                    self.ensure_block_separator();
                }
                self.quote_depth += 1;
            }
            Tag::List(first) => self.start_list(first),
            Tag::Item => self.start_item(),
            Tag::CodeBlock(kind) => self.start_code_block(kind),
            Tag::Table(_) => self.start_table(),
            Tag::Emphasis => self.inline.emphasis += 1,
            Tag::Strong => self.inline.strong += 1,
            Tag::Strikethrough => self.inline.strikethrough += 1,
            Tag::Link { .. } => self.inline.link += 1,
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.finish_block(),
            TagEnd::Heading(_) => self.finish_block(),
            TagEnd::BlockQuote(_) => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::Item => {
                if self.block.is_some() {
                    self.finish_block();
                }
                self.item_prefix_stack.pop();
            }
            TagEnd::CodeBlock => self.finish_code_block(),
            TagEnd::Emphasis => self.inline.emphasis = self.inline.emphasis.saturating_sub(1),
            TagEnd::Strong => self.inline.strong = self.inline.strong.saturating_sub(1),
            TagEnd::Strikethrough => {
                self.inline.strikethrough = self.inline.strikethrough.saturating_sub(1);
            }
            TagEnd::Link => self.inline.link = self.inline.link.saturating_sub(1),
            _ => {}
        }
    }

    fn start_block(&mut self, kind: BlockKind) {
        self.block = Some(kind);
        self.block_prefix = self.current_prefix();
        self.current.clear();
        self.logical_lines.clear();
    }

    fn finish_block(&mut self) {
        self.push_logical_line();
        let mut rendered = Vec::new();
        let first_prefix_width = spans_width(&self.block_prefix.first);
        let rest_prefix_width = spans_width(&self.block_prefix.rest);
        let content_width = self
            .width
            .saturating_sub(first_prefix_width.max(rest_prefix_width))
            .max(1);
        let mut first_line = true;
        for logical in &self.logical_lines {
            for wrapped in wrap_spans(logical, content_width) {
                let mut spans = if first_line {
                    self.block_prefix.first.clone()
                } else {
                    self.block_prefix.rest.clone()
                };
                spans.extend(wrapped);
                rendered.push(Line::from(spans));
                first_line = false;
            }
        }
        self.emit_block(rendered, self.current_block_needs_separator());
        self.block = None;
        self.block_prefix = LinePrefix::default();
        self.current.clear();
        self.logical_lines.clear();
    }

    fn emit_block(&mut self, lines: Vec<Line<'static>>, with_separator: bool) {
        if with_separator {
            self.ensure_block_separator();
        }
        self.output.extend(lines);
    }

    fn ensure_block_separator(&mut self) {
        if !self.output.is_empty() && !last_line_is_blank(&self.output) {
            self.output.push(Line::from(""));
        }
    }

    fn push_logical_line(&mut self) {
        if self.block.is_some() {
            self.logical_lines.push(std::mem::take(&mut self.current));
        }
    }

    fn push_text(&mut self, text: String, style: Style) {
        if self.block.is_none() {
            self.start_block(BlockKind::Paragraph);
        }
        push_styled_text(&mut self.current, text, style);
    }

    fn current_text_style(&self) -> Style {
        let mut style = self.current_block_style();
        if self.inline.link > 0 {
            style = style
                .fg(self.theme.accent_primary)
                .add_modifier(Modifier::UNDERLINED);
        }
        if self.inline.strikethrough > 0 {
            style = style
                .fg(self.theme.text_muted)
                .add_modifier(Modifier::CROSSED_OUT);
        }
        if self.inline.strong > 0 {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.inline.emphasis > 0 {
            style = style.add_modifier(Modifier::ITALIC);
        }
        style
    }

    fn current_block_style(&self) -> Style {
        let style = Style::default().bg(self.theme.bg_base);
        match self.block {
            Some(BlockKind::Heading(HeadingLevel::H1)) => style
                .fg(self.theme.accent_primary)
                .add_modifier(Modifier::BOLD),
            Some(BlockKind::Heading(_)) => {
                style.fg(self.theme.text_title).add_modifier(Modifier::BOLD)
            }
            _ if self.quote_depth > 0 => style.fg(self.theme.text_secondary),
            _ => style.fg(self.theme.text_body),
        }
    }

    fn inline_code_style(&self) -> Style {
        Style::default()
            .fg(self.theme.text_body)
            .bg(self.theme.bg_surface_alt)
    }

    fn start_list(&mut self, first: Option<u64>) {
        if self.block.is_some() {
            self.finish_block();
        }
        if self.list_stack.is_empty() && self.quote_depth == 0 {
            self.ensure_block_separator();
        }
        self.list_stack.push(ListState {
            ordered: first.is_some(),
            next: first.unwrap_or(1),
        });
    }

    fn start_item(&mut self) {
        let depth = self.list_stack.len().saturating_sub(1);
        let Some(list) = self.list_stack.last_mut() else {
            return;
        };
        let marker = if list.ordered {
            let marker = format!("{}. ", list.next);
            list.next += 1;
            marker
        } else {
            "• ".to_string()
        };
        let first = format!("{}{}", "  ".repeat(depth), marker);
        let rest = " ".repeat(display_width(&first));
        let style = Style::default()
            .fg(self.theme.text_secondary)
            .bg(self.theme.bg_base);
        self.item_prefix_stack.push(LinePrefix {
            first: vec![Span::styled(first, style)],
            rest: vec![Span::styled(rest, style)],
        });
    }

    fn current_prefix(&self) -> LinePrefix {
        let mut prefix = LinePrefix::default();
        if self.quote_depth > 0 {
            let style = Style::default()
                .fg(self.theme.border_strong)
                .bg(self.theme.bg_base);
            prefix.first.push(Span::styled("▎ ".to_string(), style));
            prefix.rest.push(Span::styled("▎ ".to_string(), style));
        }
        if let Some(item) = self.item_prefix_stack.last() {
            prefix.first.extend(item.first.clone());
            prefix.rest.extend(item.rest.clone());
        }
        prefix
    }

    fn current_block_needs_separator(&self) -> bool {
        self.quote_depth == 0 && self.list_stack.is_empty()
    }

    fn emit_horizontal_rule(&mut self) {
        let line = Line::from(Span::styled(
            "─".repeat(self.width),
            Style::default()
                .fg(self.theme.border_subtle)
                .bg(self.theme.bg_base),
        ));
        self.emit_block(
            vec![line],
            self.quote_depth == 0 && self.list_stack.is_empty(),
        );
    }

    fn start_code_block(&mut self, kind: CodeBlockKind<'_>) {
        if self.block.is_some() {
            self.finish_block();
        }
        let language = match kind {
            CodeBlockKind::Fenced(info) => info
                .split_whitespace()
                .next()
                .map(str::to_string)
                .unwrap_or_default(),
            CodeBlockKind::Indented => String::new(),
        };
        self.code_block = Some(CodeBlock {
            language,
            text: String::new(),
        });
    }

    fn handle_code_event(&mut self, event: Event<'_>) {
        match event {
            Event::End(TagEnd::CodeBlock) => self.finish_code_block(),
            Event::Text(text) | Event::Code(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if let Some(block) = self.code_block.as_mut() {
                    block.text.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(block) = self.code_block.as_mut() {
                    block.text.push('\n');
                }
            }
            _ => {}
        }
    }

    fn finish_code_block(&mut self) {
        let Some(block) = self.code_block.take() else {
            return;
        };
        let lines = render_code_block(&block, self.theme, self.width);
        self.emit_block(lines, self.current_block_needs_separator());
    }

    fn start_table(&mut self) {
        if self.block.is_some() {
            self.finish_block();
        }
        self.table = Some(TableState::default());
    }

    fn handle_table_event(&mut self, event: Event<'_>) {
        match event {
            Event::End(TagEnd::Table) => self.finish_table(),
            Event::Start(Tag::TableRow) => {
                if let Some(table) = self.table.as_mut() {
                    table.current_row.clear();
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(table) = self.table.as_mut() {
                    table.rows.push(std::mem::take(&mut table.current_row));
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(table) = self.table.as_mut() {
                    if !table.current_row.is_empty() {
                        table.rows.push(std::mem::take(&mut table.current_row));
                    }
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(table) = self.table.as_mut() {
                    table.current_cell.clear();
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(table) = self.table.as_mut() {
                    table
                        .current_row
                        .push(std::mem::take(&mut table.current_cell));
                }
            }
            Event::Start(tag @ (Tag::Emphasis | Tag::Strong | Tag::Strikethrough)) => {
                self.start_tag(tag);
            }
            Event::Start(tag @ Tag::Link { .. }) => self.start_tag(tag),
            Event::End(
                tag @ (TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link),
            ) => self.end_tag(tag),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                self.push_table_text(text.into_string(), self.current_text_style());
            }
            Event::Code(code) => self.push_table_text(code.into_string(), self.inline_code_style()),
            Event::SoftBreak | Event::HardBreak => {
                self.push_table_text(" ".to_string(), self.current_text_style());
            }
            _ => {}
        }
    }

    fn push_table_text(&mut self, text: String, style: Style) {
        if let Some(table) = self.table.as_mut() {
            push_styled_text(&mut table.current_cell, text, style);
        }
    }

    fn finish_table(&mut self) {
        let Some(table) = self.table.take() else {
            return;
        };
        let lines = render_table(&table, self.theme, self.width);
        self.emit_block(lines, self.current_block_needs_separator());
    }
}

fn push_styled_text(spans: &mut Vec<Span<'static>>, text: String, style: Style) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut() {
        if last.style == style {
            last.content.to_mut().push_str(&text);
            return;
        }
    }
    spans.push(Span::styled(text, style));
}

fn last_line_is_blank(lines: &[Line<'static>]) -> bool {
    lines
        .last()
        .is_some_and(|line| line.spans.iter().all(|span| span.content.is_empty()))
}

fn spans_width(spans: &[Span<'static>]) -> usize {
    spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum()
}

fn render_code_block(block: &CodeBlock, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let bg_style = Style::default().bg(theme.bg_sunken);

    if !block.language.is_empty() {
        let label = truncate_text_to_width(&format!(" {} ", block.language), width);
        let mut spans = vec![Span::styled(
            label,
            Style::default().fg(theme.text_muted).bg(theme.bg_sunken),
        )];
        pad_spans_to_width(&mut spans, width, bg_style);
        lines.push(Line::from(spans));
    }

    let code_lines = code_lines(&block.text);
    let (syntax, plain) = syntax_for_language(&block.language);
    if plain {
        for line in code_lines {
            lines.push(plain_code_line(line, theme, width));
        }
        return lines;
    }

    let Some(syntect_theme) = select_syntect_theme(theme) else {
        for line in code_lines {
            lines.push(plain_code_line(line, theme, width));
        }
        return lines;
    };

    let mut highlighter = HighlightLines::new(syntax, syntect_theme);
    for line in code_lines {
        // 先对完整行(含 \n,newlines 语法集所需)高亮,再按显示宽截断 span——
        // 截断喂 highlighter 会污染其跨行状态(如截在字符串中间,后续行全被当字符串上色)。
        let with_newline = format!("{line}\n");
        let raw_spans = match highlighter.highlight_line(&with_newline, &SYNTAX_SET) {
            Ok(ranges) => ranges
                .into_iter()
                .filter_map(|(style, text)| {
                    let text = text.trim_end_matches('\n');
                    (!text.is_empty()).then(|| {
                        Span::styled(
                            text.to_string(),
                            Style::default()
                                .fg(syntect_color_to_ratatui(style.foreground))
                                .bg(theme.bg_sunken),
                        )
                    })
                })
                .collect(),
            Err(_) => vec![Span::styled(
                line.to_string(),
                Style::default().fg(theme.text_body).bg(theme.bg_sunken),
            )],
        };
        let spans = fit_spans_to_width(&raw_spans, width, bg_style);
        lines.push(Line::from(spans));
    }

    lines
}

fn code_lines(text: &str) -> Vec<&str> {
    let mut lines = text.split('\n').collect::<Vec<_>>();
    if text.ends_with('\n') {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push("");
    }
    lines
}

fn plain_code_line(line: &str, theme: &Theme, width: usize) -> Line<'static> {
    let mut spans = vec![Span::styled(
        truncate_text_to_width(line, width),
        Style::default().fg(theme.text_body).bg(theme.bg_sunken),
    )];
    pad_spans_to_width(&mut spans, width, Style::default().bg(theme.bg_sunken));
    Line::from(spans)
}

fn syntax_for_language(language: &str) -> (&'static SyntaxReference, bool) {
    if language.is_empty() {
        return (SYNTAX_SET.find_syntax_plain_text(), true);
    }
    SYNTAX_SET
        .find_syntax_by_token(language)
        .map(|syntax| (syntax, false))
        .unwrap_or_else(|| (SYNTAX_SET.find_syntax_plain_text(), true))
}

fn select_syntect_theme(theme: &Theme) -> Option<&'static SyntectTheme> {
    let preferred = if theme.is_dark {
        &["base16-ocean.dark"][..]
    } else {
        &["InspiredGitHub", "Solarized (light)"][..]
    };
    preferred
        .iter()
        .find_map(|name| THEME_SET.themes.get(*name))
        .or_else(|| THEME_SET.themes.values().next())
}

fn syntect_color_to_ratatui(color: SyntectColor) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

fn truncate_text_to_width(text: &str, max_width: usize) -> String {
    let max_width = max_width.max(1);
    if display_width(text) <= max_width {
        return text.to_string();
    }

    let ellipsis = '…';
    let ellipsis_width = char_width(ellipsis).max(1);
    let content_width = max_width.saturating_sub(ellipsis_width);
    let mut output = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let ch_width = char_width(ch);
        if ch_width > 0 && width + ch_width > content_width {
            break;
        }
        output.push(ch);
        width += ch_width;
    }
    output.push(ellipsis);
    output
}

fn pad_spans_to_width(spans: &mut Vec<Span<'static>>, width: usize, style: Style) {
    let current_width = spans_width(spans);
    if current_width < width {
        push_styled_text(spans, " ".repeat(width - current_width), style);
    }
}

fn render_table(table: &TableState, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let col_count = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    if col_count == 0 {
        return Vec::new();
    }

    let col_widths = table_column_widths(&table.rows, col_count, width);
    let separator_style = Style::default().fg(theme.border_subtle).bg(theme.bg_base);
    let mut lines = Vec::new();

    for (row_index, row) in table.rows.iter().enumerate() {
        lines.push(render_table_row(row, &col_widths, row_index == 0, theme));
        if row_index == 0 {
            lines.push(render_table_separator(&col_widths, separator_style));
        }
    }

    lines
}

fn table_column_widths(
    rows: &[Vec<Vec<Span<'static>>>],
    col_count: usize,
    width: usize,
) -> Vec<usize> {
    let separator_width = table_separator_width(col_count);
    let available_for_columns = width.saturating_sub(separator_width).max(col_count);
    let mut widths = vec![1usize; col_count];
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(spans_width(cell));
        }
    }

    while widths.iter().sum::<usize>() > available_for_columns {
        let Some((index, _)) = widths
            .iter()
            .enumerate()
            .filter(|(_, width)| **width > 1)
            .max_by_key(|(_, width)| **width)
        else {
            break;
        };
        widths[index] -= 1;
    }

    widths
}

fn table_separator_width(col_count: usize) -> usize {
    if col_count > 0 {
        (col_count - 1) * 3
    } else {
        0
    }
}

fn render_table_row(
    row: &[Vec<Span<'static>>],
    col_widths: &[usize],
    header: bool,
    theme: &Theme,
) -> Line<'static> {
    let mut spans = Vec::new();
    let separator_style = Style::default().fg(theme.border_subtle).bg(theme.bg_base);

    for (index, width) in col_widths.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" │ ".to_string(), separator_style));
        }
        let cell = row.get(index).cloned().unwrap_or_default();
        let cell = if header {
            table_header_spans(&cell, theme)
        } else {
            cell
        };
        spans.extend(fit_spans_to_width(
            &cell,
            *width,
            Style::default().fg(theme.text_body).bg(theme.bg_base),
        ));
    }

    Line::from(spans)
}

fn render_table_separator(col_widths: &[usize], style: Style) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, width) in col_widths.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("─┼─".to_string(), style));
        }
        spans.push(Span::styled("─".repeat(*width), style));
    }
    Line::from(spans)
}

fn table_header_spans(spans: &[Span<'static>], theme: &Theme) -> Vec<Span<'static>> {
    spans
        .iter()
        .map(|span| {
            let mut style = span.style.fg(theme.text_title).add_modifier(Modifier::BOLD);
            if style.bg.is_none() {
                style = style.bg(theme.bg_base);
            }
            Span::styled(span.content.to_string(), style)
        })
        .collect()
}

fn fit_spans_to_width(
    spans: &[Span<'static>],
    width: usize,
    default_style: Style,
) -> Vec<Span<'static>> {
    let width = width.max(1);
    if spans_width(spans) <= width {
        let mut fitted = spans.to_vec();
        pad_spans_to_width(&mut fitted, width, default_style);
        return fitted;
    }

    let ellipsis = '…';
    let ellipsis_width = char_width(ellipsis).max(1);
    let content_width = width.saturating_sub(ellipsis_width);
    let mut fitted = Vec::new();
    let mut used_width = 0usize;
    let mut ellipsis_style = default_style;

    'outer: for span in spans {
        for ch in span.content.as_ref().chars() {
            let ch_width = char_width(ch);
            if ch_width > 0 && used_width + ch_width > content_width {
                ellipsis_style = span.style;
                break 'outer;
            }
            push_styled_text(&mut fitted, ch.to_string(), span.style);
            used_width += ch_width;
            ellipsis_style = span.style;
        }
    }

    if used_width < content_width {
        push_styled_text(
            &mut fitted,
            " ".repeat(content_width - used_width),
            default_style,
        );
    }
    push_styled_text(&mut fitted, ellipsis.to_string(), ellipsis_style);
    fitted
}

#[cfg(test)]
mod tests {
    use super::{render_markdown, wrap_spans};
    use crate::tui::theme::Theme;
    use crate::tui::width::display_width;
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn find_span<'a>(line: &'a Line<'static>, needle: &str) -> &'a Span<'static> {
        line.spans
            .iter()
            .find(|span| span.content.as_ref() == needle)
            .unwrap_or_else(|| panic!("span {needle:?} not found in {line:?}"))
    }

    fn find_span_containing<'a>(line: &'a Line<'static>, needle: &str) -> &'a Span<'static> {
        line.spans
            .iter()
            .find(|span| span.content.contains(needle))
            .unwrap_or_else(|| panic!("span containing {needle:?} not found in {line:?}"))
    }

    #[test]
    fn inline_markdown_maps_to_expected_span_styles() {
        let theme = Theme::midnight();
        let lines = render_markdown("普通 **b** *i* ***bi*** `code` [t](u) ~~d~~", &theme, 80);

        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "普通 b i bi code t d");

        let bold = find_span(&lines[0], "b");
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(bold.style.fg, Some(theme.text_body));

        let italic = find_span(&lines[0], "i");
        assert!(italic.style.add_modifier.contains(Modifier::ITALIC));

        let both = find_span(&lines[0], "bi");
        assert!(both.style.add_modifier.contains(Modifier::BOLD));
        assert!(both.style.add_modifier.contains(Modifier::ITALIC));

        let code = find_span(&lines[0], "code");
        assert_eq!(code.style.fg, Some(theme.text_body));
        assert_eq!(code.style.bg, Some(theme.bg_surface_alt));

        let link = find_span(&lines[0], "t");
        assert_eq!(link.style.fg, Some(theme.accent_primary));
        assert!(link.style.add_modifier.contains(Modifier::UNDERLINED));

        let deleted = find_span(&lines[0], "d");
        assert_eq!(deleted.style.fg, Some(theme.text_muted));
        assert!(deleted.style.add_modifier.contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn softbreak_and_hardbreak_keep_physical_line_structure() {
        let theme = Theme::midnight();
        let lines = render_markdown("alpha\n你好 beta  \ngamma", &theme, 80);
        let plain = lines.iter().map(line_text).collect::<Vec<_>>();

        assert_eq!(plain, ["alpha", "你好 beta", "gamma"]);
    }

    #[test]
    fn wrap_spans_preserves_styles_across_cjk_width_breaks() {
        let theme = Theme::midnight();
        let red = Style::default().fg(theme.error_fg);
        let blue = Style::default().fg(theme.info_fg);
        let spans = vec![
            Span::styled("ab你好".to_string(), red),
            Span::styled("cd".to_string(), blue),
        ];

        let wrapped = wrap_spans(&spans, 4);

        assert_eq!(wrapped.len(), 2);
        assert_eq!(wrapped[0][0].content.as_ref(), "ab你");
        assert_eq!(wrapped[0][0].style.fg, Some(theme.error_fg));
        assert_eq!(wrapped[1][0].content.as_ref(), "好");
        assert_eq!(wrapped[1][0].style.fg, Some(theme.error_fg));
        assert_eq!(wrapped[1][1].content.as_ref(), "cd");
        assert_eq!(wrapped[1][1].style.fg, Some(theme.info_fg));
    }

    #[test]
    fn headings_hr_lists_and_quotes_use_block_styles_and_prefixes() {
        let theme = Theme::midnight();
        let lines = render_markdown("# 标题\n\n---\n\n- a\n  - b\n\n> 引用", &theme, 12);
        let plain = lines.iter().map(line_text).collect::<Vec<_>>();

        assert_eq!(
            plain,
            ["标题", "", "────────────", "", "• a", "  • b", "", "▎ 引用"]
        );

        let heading = find_span(&lines[0], "标题");
        assert_eq!(heading.style.fg, Some(theme.accent_primary));
        assert!(heading.style.add_modifier.contains(Modifier::BOLD));

        assert_eq!(lines[2].spans[0].style.fg, Some(theme.border_subtle));
        assert_eq!(lines[4].spans[0].style.fg, Some(theme.text_secondary));
        assert_eq!(lines[5].spans[0].content.as_ref(), "  • ");
        assert_eq!(lines[7].spans[0].content.as_ref(), "▎ ");
        assert_eq!(lines[7].spans[0].style.fg, Some(theme.border_strong));
        assert_eq!(lines[7].spans[1].style.fg, Some(theme.text_secondary));
    }

    #[test]
    fn rust_code_block_uses_syntect_highlight_spans_and_sunken_background() {
        let theme = Theme::midnight();
        let lines = render_markdown("```rust\nfn main() {}\n```", &theme, 80);

        assert!(line_text(&lines[0]).trim_end().contains("rust"));
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.text_muted));
        assert_eq!(lines[0].spans[0].style.bg, Some(theme.bg_sunken));

        let code_line = &lines[1];
        let mut foregrounds = Vec::new();
        for span in &code_line.spans {
            assert_eq!(span.style.bg, Some(theme.bg_sunken));
            if !span.content.trim().is_empty() && !foregrounds.contains(&span.style.fg) {
                foregrounds.push(span.style.fg);
            }
        }
        assert!(
            foregrounds.len() >= 2,
            "rust code should have at least two token foregrounds: {code_line:?}"
        );
    }

    #[test]
    fn unknown_code_language_falls_back_to_plain_sunken_block() {
        let theme = Theme::midnight();
        let lines = render_markdown("```zzz\nfn main() {}\n```", &theme, 80);

        assert!(line_text(&lines[0]).trim_end().contains("zzz"));
        for span in &lines[1].spans {
            assert_eq!(span.style.bg, Some(theme.bg_sunken));
            if !span.content.trim().is_empty() {
                assert_eq!(span.style.fg, Some(theme.text_body));
            }
        }
    }

    #[test]
    fn code_block_lines_truncate_by_display_width_without_splitting_cjk() {
        let theme = Theme::midnight();
        let lines = render_markdown("```zzz\n一二三四五\n```", &theme, 6);
        let code = line_text(&lines[1]);
        let trimmed = code.trim_end();

        assert!(trimmed.ends_with('…'));
        assert!(display_width(trimmed) <= 6);
        assert!(trimmed.is_char_boundary(trimmed.len()));
    }

    #[test]
    fn highlighted_code_line_truncates_render_but_feeds_full_line_to_highlighter() {
        let theme = Theme::midnight();
        let lines = render_markdown(
            "```rust\nlet s = \"一二三四五六七八九十\";\nlet y = 2;\n```",
            &theme,
            12,
        );

        // lines[0]=语言标签,lines[1]=超宽行(渲染截断),lines[2]=后续行
        let truncated = line_text(&lines[1]);
        assert!(truncated.trim_end().ends_with('…'));
        assert!(display_width(&truncated) <= 12);
        for span in &lines[1].spans {
            assert_eq!(span.style.bg, Some(theme.bg_sunken));
        }
        // 截断只作用于渲染;highlighter 吃完整行,跨行状态不被污染,后续行文本完整产出
        assert_eq!(line_text(&lines[2]).trim_end(), "let y = 2;");
    }

    #[test]
    fn gfm_table_aligns_cjk_columns_and_styles_header_separator() {
        let theme = Theme::midnight();
        let lines = render_markdown(
            "| 名称 | 值 |\n| --- | --- |\n| 苹果 | 10 |\n| 梨 | 200 |",
            &theme,
            30,
        );
        let plain = lines.iter().map(line_text).collect::<Vec<_>>();

        assert_eq!(lines.len(), 4);
        assert!(plain[0].contains("名称"));
        assert!(plain[2].contains("苹果"));
        assert!(plain[3].contains("梨"));
        assert_eq!(display_width(&plain[0]), display_width(&plain[2]));
        assert_eq!(display_width(&plain[2]), display_width(&plain[3]));

        let header = find_span_containing(&lines[0], "名称");
        assert_eq!(header.style.fg, Some(theme.text_title));
        assert!(header.style.add_modifier.contains(Modifier::BOLD));
        assert!(lines[1]
            .spans
            .iter()
            .all(|span| span.style.fg == Some(theme.border_subtle)));
    }

    #[test]
    fn gfm_table_truncates_cjk_cell_to_column_width_without_half_char() {
        let theme = Theme::midnight();
        let lines = render_markdown("| 列名AA |\n| --- |\n| 一二三四五 |\n| abc |", &theme, 6);
        let plain = lines.iter().map(line_text).collect::<Vec<_>>();

        assert_eq!(plain.len(), 4);
        for row in &plain {
            assert_eq!(display_width(row), 6, "row must align to width 6: {row:?}");
            assert!(row.is_char_boundary(row.len()));
        }
        assert_eq!(plain[2], "一二 …");
        assert!(plain[2].ends_with('…'));
        assert!(!plain[2].contains('三'));
    }
}
