//! Pure, deterministic taskbar layout. No Win32 dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Search,
    Window(usize),
    AllWindows,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Button {
    pub rect: Rect,
    pub action: Action,
}

pub struct Layout {
    pub buttons: Vec<Button>,
    pub clock: Rect,
    pub hidden_windows: usize,
}

pub fn scale(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * i64::from(dpi) + 48) / 96) as i32
}

pub fn layout(width: i32, height: i32, dpi: u32, window_count: usize) -> Layout {
    let gap = scale(6, dpi);
    let pad = scale(10, dpi);
    let top = scale(8, dpi);
    let bottom = (height - top).max(top);
    let clock_w = scale(75, dpi);
    let exit_w = scale(51, dpi);
    let mut x = pad;

    let clock = Rect {
        left: width - pad - exit_w - gap - clock_w,
        top,
        right: width - pad - exit_w - gap,
        bottom,
    };
    let mut buttons = Vec::with_capacity(window_count.min(12) + 4);
    let mut add = |action, button_width: i32, x: &mut i32| {
        buttons.push(Button {
            rect: Rect { left: *x, top, right: *x + button_width, bottom },
            action,
        });
        *x += button_width + gap;
    };

    add(Action::Start, scale(67, dpi), &mut x);
    add(Action::Search, scale(76, dpi), &mut x);

    let slot_w = scale(116, dpi);
    let max_windows_width = (clock.left - gap - x).max(0);
    let fit = ((max_windows_width + gap) / (slot_w + gap)).max(0) as usize;
    let visible = if window_count > fit { fit.saturating_sub(1) } else { fit }.min(window_count);
    for index in 0..visible {
        add(Action::Window(index), slot_w, &mut x);
    }
    let hidden_windows = window_count - visible;
    if hidden_windows > 0 && x + scale(48, dpi) <= clock.left {
        add(Action::AllWindows, scale(48, dpi), &mut x);
    }

    buttons.push(Button {
        rect: Rect {
            left: width - pad - exit_w,
            top,
            right: width - pad,
            bottom,
        },
        action: Action::Exit,
    });
    Layout { buttons, clock, hidden_windows }
}

pub fn hit_test(layout: &Layout, x: i32, y: i32) -> Option<Action> {
    layout
        .buttons
        .iter()
        .find(|button| button.rect.contains(x, y))
        .map(|button| button.action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_keeps_buttons_inside_window_at_96_dpi() {
        let size = (900, 56);
        let l = layout(size.0, size.1, 96, 5);
        assert!(l.buttons.iter().all(|b| b.rect.left >= 0 && b.rect.right <= size.0));
        assert!(l.buttons.iter().all(|b| b.rect.top >= 0 && b.rect.bottom <= size.1));
        assert!(l.buttons.iter().any(|b| b.action == Action::Start));
    }

    #[test]
    fn overflow_does_not_create_buttons_for_every_window() {
        let l = layout(650, 56, 96, 1_000);
        assert!(l.hidden_windows > 0);
        assert!(l.buttons.len() < 10);
        assert!(l.buttons.iter().any(|b| b.action == Action::AllWindows));
    }

    #[test]
    fn hitboxes_are_non_overlapping() {
        let l = layout(1200, 112, 192, 9);
        for (i, a) in l.buttons.iter().enumerate() {
            for b in l.buttons.iter().skip(i + 1) {
                assert!(a.rect.right <= b.rect.left || b.rect.right <= a.rect.left);
            }
        }
    }

    #[test]
    fn hit_test_matches_button_and_not_gaps() {
        let panel = layout(1200, 56, 96, 3);
        let start = panel.buttons[0];
        assert_eq!(
            hit_test(&panel, start.rect.left + 1, start.rect.top + 1),
            Some(Action::Start)
        );
        assert_eq!(hit_test(&panel, 0, 0), None);
        assert_eq!(hit_test(&panel, start.rect.right, start.rect.top + 1), None);
    }

    #[test]
    fn bounds_exclude_right_and_bottom_edge() {
        let r = Rect { left: 1, top: 2, right: 10, bottom: 20 };
        assert!(r.contains(1, 2));
        assert!(!r.contains(10, 2));
        assert!(!r.contains(1, 20));
    }
}
