//! Grid geometry independent of GPUI. Run its tests without linking the app:
//! `rustc --edition=2021 --test crates/herdr-gui/src/terminal_geometry.rs -o /tmp/terminal-geometry-tests && /tmp/terminal-geometry-tests`
//!
//! Coordinates and lengths must all use the same units. GPUI logical pixels are
//! appropriate; call `scaled` when the caller instead needs device pixels.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub column: u32,
    pub row: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalGeometry {
    viewport: Rect,
    cell_width: f64,
    cell_height: f64,
    columns: u32,
    rows: u32,
}

impl TerminalGeometry {
    /// The viewport starts at the painted grid origin and clips its right and
    /// bottom edges. Grid dimensions describe the terminal frame, not sizing
    /// estimates derived from the window.
    pub fn new(
        viewport: Rect,
        cell_width: f64,
        cell_height: f64,
        columns: u32,
        rows: u32,
    ) -> Option<Self> {
        let valid_length = |n: f64| n.is_finite() && n > 0.0;
        if !viewport.origin.x.is_finite()
            || !viewport.origin.y.is_finite()
            || !valid_length(viewport.width)
            || !valid_length(viewport.height)
            || !valid_length(cell_width)
            || !valid_length(cell_height)
            || columns == 0
            || rows == 0
            || !(viewport.origin.x + viewport.width).is_finite()
            || !(viewport.origin.y + viewport.height).is_finite()
            || !(cell_width * f64::from(columns)).is_finite()
            || !(cell_height * f64::from(rows)).is_finite()
        {
            return None;
        }
        Some(Self {
            viewport,
            cell_width,
            cell_height,
            columns,
            rows,
        })
    }

    /// Size the PTY from the laid-out viewport, not the previous frame's grid.
    pub fn pty_size(self) -> (u16, u16, u16, u16) {
        let dimension = |value: f64| value.clamp(1.0, f64::from(u16::MAX)) as u16;
        (
            dimension((self.viewport.width / self.cell_width).floor()),
            dimension((self.viewport.height / self.cell_height).floor()),
            dimension(self.viewport.width.round()),
            dimension(self.viewport.height.round()),
        )
    }

    /// Scale origin, viewport and cell metrics together, without rounding.
    pub fn scaled(self, scale: f64) -> Option<Self> {
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        Self::new(
            Rect {
                origin: Point {
                    x: self.viewport.origin.x * scale,
                    y: self.viewport.origin.y * scale,
                },
                width: self.viewport.width * scale,
                height: self.viewport.height * scale,
            },
            self.cell_width * scale,
            self.cell_height * scale,
            self.columns,
            self.rows,
        )
    }

    /// Hit-test half-open cells; the viewport and grid's far edges are outside.
    pub fn hit_test(self, point: Point) -> Option<Cell> {
        let x = point.x - self.viewport.origin.x;
        let y = point.y - self.viewport.origin.y;
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= self.viewport.width
            || y >= self.viewport.height
        {
            return None;
        }
        let column = (x / self.cell_width).floor();
        let row = (y / self.cell_height).floor();
        if column >= f64::from(self.columns) || row >= f64::from(self.rows) {
            return None;
        }
        Some(Cell {
            column: column as u32,
            row: row as u32,
        })
    }

    /// Clamp a drag position to the nearest visible cell. Non-finite positions
    /// are rejected; use `hit_test` instead when starting a drag.
    pub fn hit_test_clamped(self, point: Point) -> Option<Cell> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        let last_column = ((self.viewport.width / self.cell_width).ceil() - 1.0)
            .max(0.0)
            .min(f64::from(self.columns - 1));
        let last_row = ((self.viewport.height / self.cell_height).ceil() - 1.0)
            .max(0.0)
            .min(f64::from(self.rows - 1));
        Some(Cell {
            column: ((point.x - self.viewport.origin.x) / self.cell_width)
                .floor()
                .clamp(0.0, last_column) as u32,
            row: ((point.y - self.viewport.origin.y) / self.cell_height)
                .floor()
                .clamp(0.0, last_row) as u32,
        })
    }

    /// Visible highlight rectangle for a cell, clipped to the viewport.
    pub fn cell_rect(self, cell: Cell) -> Option<Rect> {
        if cell.column >= self.columns || cell.row >= self.rows {
            return None;
        }
        let x = f64::from(cell.column) * self.cell_width;
        let y = f64::from(cell.row) * self.cell_height;
        let width = self.cell_width.min(self.viewport.width - x);
        let height = self.cell_height.min(self.viewport.height - y);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        Some(Rect {
            origin: Point {
                x: self.viewport.origin.x + x,
                y: self.viewport.origin.y + y,
            },
            width,
            height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> TerminalGeometry {
        let Some(geometry) = TerminalGeometry::new(
            Rect {
                origin: Point { x: 10.0, y: 20.0 },
                width: 25.0,
                height: 35.0,
            },
            10.0,
            18.0,
            3,
            2,
        ) else {
            panic!("valid test geometry");
        };
        geometry
    }

    #[test]
    fn pty_size_uses_viewport_not_existing_grid() {
        let geometry = TerminalGeometry::new(
            Rect {
                origin: Point { x: 17.0, y: 9.0 },
                width: 803.0,
                height: 421.0,
            },
            8.0,
            20.0,
            5,
            4,
        );
        assert_eq!(
            geometry.map(TerminalGeometry::pty_size),
            Some((100, 21, 803, 421))
        );
    }

    #[test]
    fn half_open_boundaries_and_offsets() {
        let g = geometry();
        assert_eq!(
            g.hit_test(Point { x: 10.0, y: 20.0 }),
            Some(Cell { column: 0, row: 0 })
        );
        assert_eq!(
            g.hit_test(Point {
                x: 19.999,
                y: 37.999
            }),
            Some(Cell { column: 0, row: 0 })
        );
        assert_eq!(
            g.hit_test(Point { x: 20.0, y: 38.0 }),
            Some(Cell { column: 1, row: 1 })
        );
        assert_eq!(
            g.hit_test(Point {
                x: 34.999,
                y: 54.999
            }),
            Some(Cell { column: 2, row: 1 })
        );
        for point in [
            Point { x: 9.999, y: 20.0 },
            Point { x: 10.0, y: 19.999 },
            Point { x: 35.0, y: 20.0 },
            Point { x: 10.0, y: 55.0 },
            Point {
                x: f64::NAN,
                y: 20.0,
            },
        ] {
            assert_eq!(g.hit_test(point), None);
        }
    }

    #[test]
    fn drag_clamps_each_boundary_and_corner_at_offset_origin() {
        let g = geometry();
        for (point, cell) in [
            (Point { x: 0.0, y: 40.0 }, Cell { column: 0, row: 1 }),
            (Point { x: 50.0, y: 40.0 }, Cell { column: 2, row: 1 }),
            (Point { x: 25.0, y: 0.0 }, Cell { column: 1, row: 0 }),
            (Point { x: 25.0, y: 60.0 }, Cell { column: 1, row: 1 }),
            (Point { x: 0.0, y: 0.0 }, Cell { column: 0, row: 0 }),
            (Point { x: 50.0, y: 0.0 }, Cell { column: 2, row: 0 }),
            (Point { x: 0.0, y: 60.0 }, Cell { column: 0, row: 1 }),
            (Point { x: 50.0, y: 60.0 }, Cell { column: 2, row: 1 }),
        ] {
            assert_eq!(g.hit_test(point), None);
            assert_eq!(g.hit_test_clamped(point), Some(cell));
        }
        assert_eq!(
            g.hit_test_clamped(Point { x: 20.0, y: 38.0 }),
            g.hit_test(Point { x: 20.0, y: 38.0 })
        );
        assert_eq!(
            g.hit_test_clamped(Point {
                x: f64::NAN,
                y: 20.0
            }),
            None
        );
        assert_eq!(
            g.hit_test_clamped(Point {
                x: 10.0,
                y: f64::INFINITY
            }),
            None
        );
    }

    #[test]
    fn drag_clamps_to_visible_cells_not_hidden_grid_or_empty_viewport() {
        let Some(clipped) = TerminalGeometry::new(
            Rect {
                origin: Point { x: 10.0, y: 20.0 },
                width: 20.0,
                height: 19.0,
            },
            10.0,
            18.0,
            8,
            8,
        ) else {
            panic!("valid test geometry");
        };
        assert_eq!(
            clipped.hit_test_clamped(Point { x: 100.0, y: 100.0 }),
            Some(Cell { column: 1, row: 1 })
        );
        let Some(larger) = TerminalGeometry::new(
            Rect {
                origin: Point { x: 10.0, y: 20.0 },
                width: 100.0,
                height: 100.0,
            },
            10.0,
            18.0,
            2,
            2,
        ) else {
            panic!("valid test geometry");
        };
        assert_eq!(
            larger.hit_test_clamped(Point { x: 90.0, y: 90.0 }),
            Some(Cell { column: 1, row: 1 })
        );
    }

    #[test]
    fn highlight_is_clipped_and_grid_limits_apply() {
        let g = geometry();
        assert_eq!(
            g.cell_rect(Cell { column: 2, row: 1 }),
            Some(Rect {
                origin: Point { x: 30.0, y: 38.0 },
                width: 5.0,
                height: 17.0
            })
        );
        assert_eq!(g.cell_rect(Cell { column: 3, row: 0 }), None);
        let Some(larger) = TerminalGeometry::new(
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                width: 100.0,
                height: 100.0,
            },
            10.0,
            18.0,
            2,
            1,
        ) else {
            panic!("valid test geometry");
        };
        assert_eq!(larger.hit_test(Point { x: 20.0, y: 0.0 }), None);
        assert_eq!(larger.hit_test(Point { x: 0.0, y: 18.0 }), None);
    }

    #[test]
    fn scale_preserves_hits_and_rectangles() {
        let Some(scaled) = geometry().scaled(2.0) else {
            panic!("valid scale");
        };
        assert_eq!(
            scaled.hit_test(Point { x: 60.0, y: 76.0 }),
            Some(Cell { column: 2, row: 1 })
        );
        assert_eq!(
            scaled.cell_rect(Cell { column: 2, row: 1 }),
            Some(Rect {
                origin: Point { x: 60.0, y: 76.0 },
                width: 10.0,
                height: 34.0
            })
        );
        assert!(geometry().scaled(0.0).is_none());
        assert!(geometry().scaled(f64::INFINITY).is_none());
    }

    #[test]
    fn rejects_invalid_geometry() {
        let rect = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            width: 20.0,
            height: 18.0,
        };
        assert!(TerminalGeometry::new(rect, 0.0, 18.0, 2, 1).is_none());
        assert!(TerminalGeometry::new(rect, 10.0, f64::NAN, 2, 1).is_none());
        assert!(TerminalGeometry::new(rect, 10.0, 18.0, 0, 1).is_none());
        assert!(TerminalGeometry::new(
            Rect {
                width: f64::INFINITY,
                ..rect
            },
            10.0,
            18.0,
            2,
            1
        )
        .is_none());
    }
}
