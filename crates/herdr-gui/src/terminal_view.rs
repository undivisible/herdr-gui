use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{
    cached_view, canvas, div, fill, point, rgb, size, Bounds, ContentMask, Entity, Font,
    IntoElement, Render, TextRun, Window,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use crate::ghostty::{TerminalCellKind, TerminalFrame, TerminalLine};

#[path = "terminal_geometry.rs"]
pub mod geometry;

/// The geometry is updated during prepaint, in window-relative logical pixels.
pub struct TerminalPane {
    frame: Arc<TerminalFrame>,
    bg: u32,
    geometry: Rc<Cell<Option<geometry::TerminalGeometry>>>,
    painted_frame: Rc<RefCell<Option<Arc<TerminalFrame>>>>,
}

impl TerminalPane {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            frame: Arc::new(TerminalFrame::default()),
            bg: 0x0a0a0a,
            geometry: Rc::new(Cell::new(None)),
            painted_frame: Rc::new(RefCell::new(None)),
        }
    }

    /// Last prepainted grid, or None before layout or if font metrics are unavailable.
    pub fn geometry(&self) -> Option<geometry::TerminalGeometry> {
        self.geometry.get()
    }

    pub fn has_painted(&self, frame: &Arc<TerminalFrame>) -> bool {
        self.painted_frame
            .borrow()
            .as_ref()
            .is_some_and(|painted| Arc::ptr_eq(painted, frame))
    }

    pub fn set_frame(&mut self, frame: Arc<TerminalFrame>, cx: &mut Context<Self>) {
        if Arc::ptr_eq(&self.frame, &frame) || self.frame.as_ref() == frame.as_ref() {
            return;
        }
        self.frame = frame;
        cx.notify();
    }

    pub fn set_bg(&mut self, bg: u32, cx: &mut Context<Self>) {
        if self.bg == bg {
            return;
        }
        self.bg = bg;
        cx.notify();
    }
}

impl Render for TerminalPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self.frame.clone();
        let geometry = self.geometry.clone();
        let painted_frame = self.painted_frame.clone();
        let bg = self.bg;
        div().size_full().child(
            canvas(
                move |bounds, window, _cx| {
                    let style = window.text_style();
                    let font = style.font();
                    let font_size = style.font_size.to_pixels(window.rem_size());
                    let cell_width = window
                        .text_system()
                        .ch_advance(window.text_system().resolve_font(&font), font_size)
                        .ok();
                    let cell_height = window.line_height();
                    let columns = frame
                        .lines
                        .iter()
                        .map(|line| line.cells.len())
                        .max()
                        .unwrap_or(0);
                    let grid = cell_width.and_then(|width| {
                        geometry::TerminalGeometry::new(
                            geometry::Rect {
                                origin: geometry::Point {
                                    x: f32::from(bounds.origin.x) as f64,
                                    y: f32::from(bounds.origin.y) as f64,
                                },
                                width: f32::from(bounds.size.width) as f64,
                                height: f32::from(bounds.size.height) as f64,
                            },
                            f32::from(width) as f64,
                            f32::from(cell_height) as f64,
                            u32::try_from(columns).ok()?,
                            u32::try_from(frame.lines.len()).ok()?,
                        )
                    });
                    geometry.set(grid);
                    (frame, font, font_size, cell_width, cell_height)
                },
                move |bounds, (frame, font, font_size, cell_width, cell_height), window, cx| {
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        window.paint_quad(fill(bounds, rgb(bg)));
                        for (row, line) in frame.lines.iter().enumerate() {
                            let y = bounds.origin.y + cell_height * row as f32;
                            if y >= bounds.bottom() {
                                break;
                            }
                            if let Some((text, runs)) = fallback_line(line, &font) {
                                let shaped = window.text_system().shape_line(
                                    text.into(),
                                    font_size,
                                    &runs,
                                    None,
                                );
                                let origin = point(bounds.origin.x, y);
                                let _ = shaped.paint_background(origin, cell_height, window, cx);
                                let _ = shaped.paint(origin, cell_height, window, cx);
                                continue;
                            }
                            let Some(cell_width) = cell_width else {
                                continue;
                            };
                            for cell in &line.cells {
                                let x = bounds.origin.x + cell_width * f32::from(cell.column);
                                let cell_bounds =
                                    Bounds::new(point(x, y), size(cell_width, cell_height));
                                if let Some(color) = cell.bg {
                                    window.paint_quad(fill(cell_bounds, rgb(color)));
                                }
                                if line.selection.is_some_and(|(start, end)| {
                                    cell.column >= start && cell.column <= end
                                }) {
                                    window.paint_quad(fill(cell_bounds, rgb(0x454b59)));
                                }
                            }
                            for cell in &line.cells {
                                if cell.text.is_empty()
                                    || matches!(
                                        cell.kind,
                                        TerminalCellKind::SpacerHead | TerminalCellKind::SpacerTail
                                    )
                                {
                                    continue;
                                }
                                let x = bounds.origin.x + cell_width * f32::from(cell.column);
                                let glyph_bounds = Bounds::new(
                                    point(x, y),
                                    size(cell_width * f32::from(cell.span.max(1)), cell_height),
                                );
                                let text = cell.text.clone();
                                let run = TextRun {
                                    len: text.len(),
                                    font: font.clone(),
                                    color: rgb(cell.fg).into(),
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                };
                                let shaped = window.text_system().shape_line(
                                    text.into(),
                                    font_size,
                                    &[run],
                                    None,
                                );
                                window.with_content_mask(
                                    Some(ContentMask {
                                        bounds: glyph_bounds,
                                    }),
                                    |window| {
                                        let _ = shaped.paint(point(x, y), cell_height, window, cx);
                                    },
                                );
                            }
                        }
                    });
                    *painted_frame.borrow_mut() = Some(frame);
                },
            )
            .size_full(),
        )
    }
}

fn fallback_line(line: &TerminalLine, font: &Font) -> Option<(String, Vec<TextRun>)> {
    if !line.cells.is_empty() || line.runs.is_empty() {
        return None;
    }
    let mut text = String::new();
    let mut runs = Vec::new();
    for run in &line.runs {
        let value = run.text.replace(['\n', '\r'], " ");
        if value.is_empty() {
            continue;
        }
        runs.push(TextRun {
            len: value.len(),
            font: font.clone(),
            color: rgb(run.fg).into(),
            background_color: run.bg.map(|bg| rgb(bg).into()),
            underline: None,
            strikethrough: None,
        });
        text.push_str(&value);
    }
    (!text.is_empty()).then_some((text, runs))
}

pub fn cached_terminal(entity: Entity<TerminalPane>) -> impl IntoElement {
    cached_view(entity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ghostty::{TerminalCell, TerminalRun};
    use crepuscularity_gpui::{font, Hsla};

    #[test]
    fn fallback_preserves_run_text_and_colors_but_never_replaces_cells() {
        let font = font("Menlo");
        let mut line = TerminalLine {
            runs: vec![
                TerminalRun {
                    text: "attach ".into(),
                    fg: 0xfca5a5,
                    bg: None,
                },
                TerminalRun {
                    text: "é\nerror".into(),
                    fg: 0xffffff,
                    bg: Some(0x123456),
                },
            ],
            ..TerminalLine::default()
        };
        let Some((text, runs)) = fallback_line(&line, &font) else {
            panic!("run-only lines must be rendered");
        };
        assert_eq!(text, "attach é error");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len, "attach ".len());
        assert_eq!(runs[1].len, "é error".len());
        assert_eq!(runs[0].color, Hsla::from(rgb(0xfca5a5)));
        assert_eq!(runs[1].background_color, Some(Hsla::from(rgb(0x123456))));

        line.cells.push(TerminalCell {
            column: 0,
            span: 1,
            kind: TerminalCellKind::Narrow,
            text: "x".into(),
            fg: 0xffffff,
            bg: None,
        });
        assert!(fallback_line(&line, &font).is_none());
    }
}
