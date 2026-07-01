use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub col: u16,
    pub row: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Point,
    pub head: Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NormalizedSelection {
    pub start: Point,
    pub end: Point,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionState {
    pub selection: Option<Selection>,
    pub dragging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionAction {
    Press(Point),
    Drag(Point),
    Release(Point),
    Clear,
}

impl Selection {
    pub fn normalized(&self) -> NormalizedSelection {
        let anchor_first = self.anchor.row < self.head.row
            || (self.anchor.row == self.head.row && self.anchor.col <= self.head.col);
        if anchor_first {
            NormalizedSelection {
                start: self.anchor,
                end: self.head,
            }
        } else {
            NormalizedSelection {
                start: self.head,
                end: self.anchor,
            }
        }
    }
}

pub fn reduce_selection(state: &SelectionState, action: SelectionAction) -> SelectionState {
    let mut next = state.clone();
    match action {
        SelectionAction::Press(point) => {
            next.selection = Some(Selection {
                anchor: point,
                head: point,
            });
            next.dragging = true;
        }
        SelectionAction::Drag(point) => {
            if next.dragging {
                if let Some(selection) = next.selection.as_mut() {
                    selection.head = point;
                }
            }
        }
        SelectionAction::Release(point) => {
            if next.dragging {
                next.dragging = false;
                if let Some(selection) = next.selection.as_mut() {
                    selection.head = point;
                    if selection.anchor == selection.head {
                        next.selection = None;
                    }
                }
            }
        }
        SelectionAction::Clear => {
            next.selection = None;
            next.dragging = false;
        }
    }
    next
}

pub fn col_range_for_row(selection: &Selection, row: u16, width: u16) -> Option<Range<u16>> {
    let normalized = selection.normalized();
    if row < normalized.start.row || row > normalized.end.row || width == 0 {
        return None;
    }

    let (start, end_exclusive) = if normalized.start.row == normalized.end.row {
        (
            normalized.start.col.min(width),
            normalized.end.col.saturating_add(1).min(width),
        )
    } else if row == normalized.start.row {
        (normalized.start.col.min(width), width)
    } else if row == normalized.end.row {
        (0, normalized.end.col.saturating_add(1).min(width))
    } else {
        (0, width)
    };

    (start < end_exclusive).then_some(start..end_exclusive)
}

#[cfg(test)]
mod tests {
    use super::{
        col_range_for_row, reduce_selection, Point, Selection, SelectionAction, SelectionState,
    };

    fn point(col: u16, row: u16) -> Point {
        Point { col, row }
    }

    fn selection(anchor: Point, head: Point) -> Selection {
        Selection { anchor, head }
    }

    #[test]
    fn reduce_selection_starts_drags_releases_and_click_clears() {
        let state = reduce_selection(
            &SelectionState::default(),
            SelectionAction::Press(point(2, 1)),
        );
        assert_eq!(state.selection, Some(selection(point(2, 1), point(2, 1))));
        assert!(state.dragging);

        let state = reduce_selection(&state, SelectionAction::Drag(point(6, 1)));
        assert_eq!(state.selection, Some(selection(point(2, 1), point(6, 1))));
        assert!(state.dragging);

        let state = reduce_selection(&state, SelectionAction::Release(point(6, 1)));
        assert_eq!(state.selection, Some(selection(point(2, 1), point(6, 1))));
        assert!(!state.dragging);

        let clicked = reduce_selection(
            &SelectionState::default(),
            SelectionAction::Press(point(3, 0)),
        );
        let clicked = reduce_selection(&clicked, SelectionAction::Release(point(3, 0)));
        assert_eq!(clicked.selection, None);
        assert!(!clicked.dragging);
    }

    #[test]
    fn press_replaces_existing_selection() {
        let state = SelectionState {
            selection: Some(selection(point(0, 0), point(4, 0))),
            dragging: false,
        };

        let state = reduce_selection(&state, SelectionAction::Press(point(8, 2)));

        assert_eq!(state.selection, Some(selection(point(8, 2), point(8, 2))));
        assert!(state.dragging);
    }

    #[test]
    fn normalized_orders_points_in_reading_order() {
        let normalized = selection(point(6, 2), point(2, 0)).normalized();

        assert_eq!(normalized.start, point(2, 0));
        assert_eq!(normalized.end, point(6, 2));

        let same_row = selection(point(9, 4), point(3, 4)).normalized();
        assert_eq!(same_row.start, point(3, 4));
        assert_eq!(same_row.end, point(9, 4));
    }

    #[test]
    fn col_range_for_row_handles_single_and_multiline_selection() {
        let multiline = selection(point(6, 2), point(2, 0));

        assert_eq!(col_range_for_row(&multiline, 0, 10), Some(2..10));
        assert_eq!(col_range_for_row(&multiline, 1, 10), Some(0..10));
        assert_eq!(col_range_for_row(&multiline, 2, 10), Some(0..7));
        assert_eq!(col_range_for_row(&multiline, 3, 10), None);

        let single_line = selection(point(2, 1), point(6, 1));
        assert_eq!(col_range_for_row(&single_line, 1, 10), Some(2..7));
        assert_eq!(col_range_for_row(&single_line, 0, 10), None);
    }

    #[test]
    fn clear_action_removes_selection_and_dragging() {
        let state = SelectionState {
            selection: Some(selection(point(1, 1), point(5, 1))),
            dragging: true,
        };

        let state = reduce_selection(&state, SelectionAction::Clear);

        assert_eq!(state.selection, None);
        assert!(!state.dragging);
    }

    #[test]
    fn drag_without_active_drag_is_no_op() {
        let state = SelectionState::default();

        let state = reduce_selection(&state, SelectionAction::Drag(point(4, 2)));

        assert_eq!(state, SelectionState::default());
    }

    #[test]
    fn release_without_active_drag_is_no_op() {
        let state = SelectionState {
            selection: Some(selection(point(1, 1), point(5, 1))),
            dragging: false,
        };

        let next = reduce_selection(&state, SelectionAction::Release(point(8, 3)));

        assert_eq!(next, state);
    }
}
