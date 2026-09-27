//! tradoor-render — the chart, independent of how it reaches the screen.
//!
//! `Chart` owns the data, the view (time and price scales, panes), the drawing
//! tools and every pointer interaction, and produces a `Frame`: triangles in CSS
//! pixels, drawn in order by a GPU layer, and text drawn over them by a 2D
//! canvas. Nothing here touches a browser API, so all of it is unit-tested; the
//! WASM crate is only the backend that uploads a frame.
#![forbid(unsafe_code)]

pub mod chart;
pub mod color;
pub mod drawings;
pub mod footprint;
pub mod geometry;
pub mod paint;
pub mod scale;
pub mod theme;
mod ticks;
pub mod tpo;

pub use chart::{Chart, Cursor, Event, Legend, Magnet, StudyMeta};
pub use color::Color;
pub use drawings::{Anchor, Dash, DrawingKind, DrawingShape, DrawingStyle, Level};
pub use paint::{Align, Baseline, Batch, Frame, IconCmd, Rect, TextCmd};
pub use theme::Theme;
