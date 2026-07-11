use crate::tool::NetworkPermissionPreview;
use crate::tui::width::char_width;
use ratatui::layout::Rect;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkPermissionLayout {
    pub lines: Vec<String>,
    pub visible_range: Range<usize>,
    pub scroll: usize,
    pub total_lines: usize,
    pub position_hint: Option<String>,
    pub can_allow: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionBarrierState {
    pub generation: u64,
    pub rendered_generation: Option<u64>,
    pub armed_generation: Option<u64>,
    pub scroll: usize,
}

impl PermissionBarrierState {
    pub fn begin_request(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.rendered_generation = None;
        self.armed_generation = None;
        self.scroll = 0;
        self.generation
    }

    pub fn mark_rendered(&mut self, generation: u64) {
        if generation == self.generation {
            self.rendered_generation = Some(generation);
        }
    }

    pub fn needs_input_barrier(&self, generation: u64) -> bool {
        generation == self.generation
            && self.rendered_generation == Some(generation)
            && self.armed_generation != Some(generation)
    }

    pub fn complete_input_barrier(&mut self, generation: u64) {
        if self.needs_input_barrier(generation) {
            self.armed_generation = Some(generation);
        }
    }

    pub fn resize(&mut self) {
        self.rendered_generation = None;
        self.armed_generation = None;
    }

    pub fn can_allow(&self, generation: u64, layout_can_allow: bool) -> bool {
        layout_can_allow
            && self.armed_generation == Some(generation)
            && generation == self.generation
    }
}

pub fn network_permission_area(terminal_area: Rect) -> Rect {
    Rect::new(
        terminal_area.x,
        terminal_area.y,
        terminal_area.width,
        terminal_area.height.min(12),
    )
}

pub fn network_permission_layout(
    area: Rect,
    preview: &NetworkPermissionPreview,
    scroll: usize,
) -> NetworkPermissionLayout {
    let content_width = area.width.saturating_sub(4) as usize;
    let lines = if content_width == 0 {
        Vec::new()
    } else {
        crate::permission::preview::format_network_permission_preview(preview)
            .lines()
            .flat_map(|line| wrap_line(line, content_width))
            .collect()
    };
    let total_lines = lines.len();
    // border(2) + title/tool(2) + position(1) + actions/help(2)
    let visible_capacity = area.height.saturating_sub(7) as usize;
    let visible_count = total_lines.min(visible_capacity);
    let max_scroll = total_lines.saturating_sub(visible_count);
    let scroll = scroll.min(max_scroll);
    let visible_range = scroll..scroll.saturating_add(visible_count);
    let position_hint = (visible_capacity > 0).then(|| {
        format!(
            "第 {}-{} / {} 行 · ↑↓/PgUp/PgDn 查看完整参数",
            visible_range.start.saturating_add(1).min(total_lines),
            visible_range.end,
            total_lines
        )
    });
    let can_allow = preview.authorizable
        && preview.canonical_initial_target.is_some()
        && preview.scope.is_some()
        && preview.denial_reason.is_none()
        && visible_capacity > 0
        && area.width >= 8
        && area.height >= 12;

    NetworkPermissionLayout {
        lines,
        visible_range,
        scroll,
        total_lines,
        position_hint,
        can_allow,
    }
}

fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }

    let mut result = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;
    for character in line.chars() {
        let width_of_character = char_width(character);
        if current_width > 0 && current_width.saturating_add(width_of_character) > width {
            result.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push(character);
        current_width = current_width.saturating_add(width_of_character);
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::network_permission_layout;
    use crate::tool::{NetworkPermissionPreview, NetworkPermissionScope};
    use ratatui::layout::Rect;
    use serde_json::json;

    fn preview() -> NetworkPermissionPreview {
        NetworkPermissionPreview {
            authorizable: true,
            full_args: json!({ "query": "中文 query with a sufficiently long value to wrap" }),
            canonical_initial_target: Some("https://example.com/search".to_string()),
            scope: Some(NetworkPermissionScope {
                max_redirects: 3,
                may_cross_origin: true,
                ssrf_each_hop: true,
            }),
            denial_reason: None,
        }
    }

    #[test]
    fn layout_wraps_content_clamps_scroll_and_reports_position() {
        let layout = network_permission_layout(Rect::new(0, 0, 20, 12), &preview(), 999);

        assert!(layout.total_lines > 1);
        assert_eq!(
            layout.scroll,
            layout.total_lines - layout.visible_range.len()
        );
        assert!(layout.position_hint.is_some());
        assert!(layout.can_allow);
    }

    #[test]
    fn layout_fails_closed_when_terminal_cannot_show_fixed_scope_and_actions() {
        let narrow = network_permission_layout(Rect::new(0, 0, 3, 20), &preview(), 0);
        let short = network_permission_layout(Rect::new(0, 0, 80, 5), &preview(), 0);

        assert!(!narrow.can_allow);
        assert!(!short.can_allow);
    }

    #[test]
    fn layout_reject_only_preview_never_allows_and_scrolls_to_bounds() {
        let mut preview = preview();
        preview.authorizable = false;
        preview.canonical_initial_target = None;
        preview.scope = None;
        preview.denial_reason = Some("invalid target".to_string());

        let first = network_permission_layout(Rect::new(0, 0, 24, 8), &preview, 0);
        let last = network_permission_layout(Rect::new(0, 0, 24, 8), &preview, usize::MAX);

        assert!(!first.can_allow);
        assert!(!last.can_allow);
        assert!(last.scroll >= first.scroll);
        assert_eq!(last.visible_range.end, last.total_lines);
    }

    #[test]
    fn barrier_requires_matching_render_after_each_request_and_resize() {
        let mut barrier = super::PermissionBarrierState::default();
        let first = barrier.begin_request();
        assert_eq!(barrier.scroll, 0);
        assert!(!barrier.can_allow(first, true));

        barrier.mark_rendered(first);
        assert!(!barrier.can_allow(first, true));
        assert!(barrier.needs_input_barrier(first));
        barrier.complete_input_barrier(first);
        assert!(barrier.can_allow(first, true));

        barrier.resize();
        assert!(!barrier.can_allow(first, true));
        barrier.mark_rendered(first);
        assert!(!barrier.can_allow(first, true));
        barrier.complete_input_barrier(first);
        assert!(barrier.can_allow(first, true));

        let second = barrier.begin_request();
        assert!(!barrier.can_allow(first, true));
        assert!(!barrier.can_allow(second, true));
        barrier.mark_rendered(second);
        assert!(!barrier.can_allow(second, true));
        barrier.complete_input_barrier(second);
        assert!(barrier.can_allow(second, true));
        assert!(!barrier.can_allow(second, false));
    }
}
