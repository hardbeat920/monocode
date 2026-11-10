//! A bounded native component tree. This is data, never HTML, CSS, or a host API.
use serde::{Deserialize, Serialize};

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_NODES: usize = 512;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VisualDocument {
    pub id: String,
    pub title: String,
    pub source: String,
}

impl VisualDocument {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 96
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        {
            return Err(
                "Visual id must contain 1–96 ASCII letters, digits, underscores or hyphens".into(),
            );
        }
        if self.title.trim().is_empty() || self.title.chars().count() > 160 {
            return Err("Visual title must contain 1–160 characters".into());
        }
        validate_source(&self.source)
    }
}

pub fn validate_source(source: &str) -> Result<(), String> {
    if source.trim().is_empty() || source.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "Visual source must contain 1–{MAX_SOURCE_BYTES} bytes"
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThemeColor {
    #[default]
    Ink,
    Muted,
    Faint,
    Accent,
    Positive,
    Negative,
    Warning,
    Series1,
    Series2,
    Series3,
    Series4,
    Series5,
    Series6,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextSize {
    Caption,
    Small,
    #[default]
    Body,
    Subtitle,
    Title,
    Hero,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    Bar,
    Line,
    Area,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Series {
    pub label: String,
    pub values: Vec<f64>,
    #[serde(default)]
    pub color: ThemeColor,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slice {
    pub label: String,
    pub value: f64,
    #[serde(default)]
    pub color: ThemeColor,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Node {
    Column {
        children: Vec<Node>,
        gap: f32,
    },
    Row {
        children: Vec<Node>,
        gap: f32,
    },
    Grid {
        children: Vec<Node>,
        columns: usize,
        gap: f32,
    },
    /// A raised panel around its children, laid out as a column.
    Card {
        children: Vec<Node>,
        gap: f32,
    },
    Text {
        text: String,
        size: TextSize,
        color: ThemeColor,
    },
    Stat {
        label: String,
        value: String,
        detail: Option<String>,
        color: ThemeColor,
        /// A signed change such as "+12.4%", drawn with an arrow.
        #[serde(default)]
        delta: Option<String>,
        /// A sparkline under the value.
        #[serde(default)]
        trend: Vec<f64>,
        /// A falling value is good news, as for latency or error rates.
        #[serde(default)]
        lower_is_better: bool,
    },
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Chart {
        chart_type: ChartKind,
        series: Vec<Series>,
        labels: Vec<String>,
        height: f32,
    },
    Gauge {
        label: String,
        value: f64,
        min: f64,
        max: f64,
        unit: Option<String>,
        color: ThemeColor,
    },
    Donut {
        slices: Vec<Slice>,
        label: Option<String>,
        value: Option<String>,
    },
    Heatmap {
        values: Vec<Vec<f64>>,
        rows: Vec<String>,
        columns: Vec<String>,
        color: ThemeColor,
    },
    Progress {
        label: String,
        value: f64,
        max: f64,
        detail: Option<String>,
        color: ThemeColor,
    },
    Button {
        label: String,
        handler: String,
        disabled: bool,
    },
    Toggle {
        label: String,
        value: bool,
        handler: String,
    },
    Input {
        label: String,
        value: String,
        placeholder: Option<String>,
        handler: String,
    },
    Select {
        label: String,
        options: Vec<String>,
        selected: usize,
        handler: String,
    },
    Slider {
        label: String,
        value: f64,
        min: f64,
        max: f64,
        step: f64,
        handler: String,
    },
    Tabs {
        labels: Vec<String>,
        selected: usize,
        handler: String,
    },
    Canvas {
        width: f32,
        height: f32,
        shapes: Vec<Shape>,
    },
    Scene {
        width: f32,
        height: f32,
        objects: Vec<SceneObject>,
        camera: Camera,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: ThemeColor,
    },
    Circle {
        x: f32,
        y: f32,
        radius: f32,
        color: ThemeColor,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        color: ThemeColor,
    },
    Polyline {
        points: Vec<[f32; 2]>,
        width: f32,
        color: ThemeColor,
    },
    Label {
        x: f32,
        y: f32,
        text: String,
        color: ThemeColor,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneObject {
    pub geometry: Geometry,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub color: ThemeColor,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Geometry {
    Box,
    Sphere,
    Cylinder,
    Cone,
    Plane,
    Mesh {
        vertices: Vec<[f32; 3]>,
        indices: Vec<[usize; 3]>,
    },
    Line {
        points: Vec<[f32; 3]>,
        width: f32,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub fov: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: [5., 4., 6.],
            target: [0.; 3],
            fov: 45.,
        }
    }
}

impl Node {
    pub fn validate(&self) -> Result<(), String> {
        let mut budget = Budget::default();
        self.validate_with(0, &mut budget)
    }

    fn validate_with(&self, depth: usize, budget: &mut Budget) -> Result<(), String> {
        budget.nodes += 1;
        if budget.nodes > MAX_NODES || depth > 16 {
            return Err("Visual tree exceeds 512 nodes or 16 levels".into());
        }
        match self {
            Self::Column { children, gap }
            | Self::Row { children, gap }
            | Self::Card { children, gap } => {
                bounded(*gap as f64, 0., 48., "layout gap")?;
                children
                    .iter()
                    .try_for_each(|n| n.validate_with(depth + 1, budget))?;
            }
            Self::Grid {
                children,
                columns,
                gap,
            } => {
                if !(1..=12).contains(columns) {
                    return Err("Grid columns must be 1–12".into());
                }
                bounded(*gap as f64, 0., 48., "layout gap")?;
                children
                    .iter()
                    .try_for_each(|n| n.validate_with(depth + 1, budget))?;
            }
            Self::Text { text, .. } => budget.text(text, 16_384)?,
            Self::Stat {
                label,
                value,
                detail,
                delta,
                trend,
                ..
            } => {
                budget.text(label, 256)?;
                budget.text(value, 512)?;
                if let Some(detail) = detail {
                    budget.text(detail, 2048)?;
                }
                if let Some(delta) = delta {
                    budget.text(delta, 64)?;
                }
                points(trend, budget)?;
            }
            Self::Gauge {
                label,
                value,
                min,
                max,
                unit,
                ..
            } => {
                budget.text(label, 256)?;
                if let Some(unit) = unit {
                    budget.text(unit, 32)?;
                }
                bounded(*min, -1e12, 1e12, "gauge min")?;
                bounded(*max, -1e12, 1e12, "gauge max")?;
                if min >= max {
                    return Err("Gauge max must exceed min".into());
                }
                bounded(*value, -1e12, 1e12, "gauge value")?;
            }
            Self::Donut {
                slices,
                label,
                value,
            } => {
                if slices.is_empty() || slices.len() > 12 {
                    return Err("Donuts need 1–12 slices".into());
                }
                for slice in slices {
                    budget.text(&slice.label, 256)?;
                    bounded(slice.value, 0., 1e12, "slice value")?;
                }
                if slices.iter().all(|s| s.value <= 0.) {
                    return Err("A donut needs at least one positive slice".into());
                }
                for text in [label, value].into_iter().flatten() {
                    budget.text(text, 128)?;
                }
            }
            Self::Heatmap {
                values,
                rows,
                columns,
                ..
            } => {
                let width = values.first().map_or(0, Vec::len);
                if values.is_empty() || values.len() > 32 || width == 0 || width > 128 {
                    return Err("Heatmaps need 1–32 rows of 1–128 values".into());
                }
                if values.iter().any(|row| row.len() != width) {
                    return Err("Heatmap rows must have equal lengths".into());
                }
                if (!rows.is_empty() && rows.len() != values.len())
                    || (!columns.is_empty() && columns.len() != width)
                {
                    return Err("Heatmap labels must match its rows and columns".into());
                }
                for label in rows.iter().chain(columns) {
                    budget.text(label, 64)?;
                }
                for row in values {
                    points(row, budget)?;
                }
            }
            Self::Progress {
                label,
                value,
                max,
                detail,
                ..
            } => {
                budget.text(label, 256)?;
                if let Some(detail) = detail {
                    budget.text(detail, 256)?;
                }
                bounded(*max, 1e-9, 1e12, "progress max")?;
                bounded(*value, 0., *max, "progress value")?;
            }
            Self::Table { columns, rows } => {
                if columns.is_empty() || columns.len() > 24 || rows.len() > 250 {
                    return Err("Table needs 1–24 columns and at most 250 rows".into());
                }
                for column in columns {
                    budget.text(column, 256)?;
                }
                for row in rows {
                    if row.len() != columns.len() {
                        return Err("Every table row must match its columns".into());
                    }
                    for cell in row {
                        budget.text(cell, 2048)?;
                    }
                }
            }
            Self::Chart {
                series,
                labels,
                height,
                ..
            } => {
                bounded(*height as f64, 120., 900., "chart height")?;
                if series.is_empty() || series.len() > 8 || labels.len() > 512 {
                    return Err("Charts need 1–8 series and at most 512 labels".into());
                }
                let count = series[0].values.len();
                if count == 0 || count > 512 || (!labels.is_empty() && labels.len() != count) {
                    return Err("Chart series need 1–512 values and matching labels".into());
                }
                for series in series {
                    budget.text(&series.label, 256)?;
                    if series.values.len() != count {
                        return Err("Chart series must have equal lengths".into());
                    }
                    budget.vertices += series.values.len();
                    if budget.vertices > 32_768 {
                        return Err("Visual data point budget exceeded".into());
                    }
                    for value in &series.values {
                        bounded(*value, -1e12, 1e12, "chart value")?;
                    }
                }
                for label in labels {
                    budget.text(label, 256)?;
                }
            }
            Self::Button { label, handler, .. } | Self::Toggle { label, handler, .. } => {
                budget.text(label, 256)?;
                validate_handler(handler)?;
            }
            Self::Slider {
                label,
                value,
                min,
                max,
                step,
                handler,
            } => {
                budget.text(label, 256)?;
                validate_handler(handler)?;
                bounded(*min, -1e9, 1e9, "slider min")?;
                bounded(*max, -1e9, 1e9, "slider max")?;
                if min >= max {
                    return Err("Slider max must exceed min".into());
                }
                bounded(*value, *min, *max, "slider value")?;
                bounded(*step, 1e-9, max - min, "slider step")?;
            }
            Self::Input {
                label,
                value,
                placeholder,
                handler,
            } => {
                budget.text(label, 256)?;
                budget.text(value, 4096)?;
                if let Some(placeholder) = placeholder {
                    budget.text(placeholder, 256)?;
                }
                validate_handler(handler)?;
            }
            Self::Select {
                label,
                options,
                selected,
                handler,
            } => {
                budget.text(label, 256)?;
                if options.is_empty() || options.len() > 24 || *selected >= options.len() {
                    return Err("Select needs 1–24 options and a valid selected index".into());
                }
                for option in options {
                    budget.text(option, 256)?;
                }
                validate_handler(handler)?;
            }
            Self::Tabs {
                labels,
                selected,
                handler,
            } => {
                if labels.is_empty() || labels.len() > 12 || *selected >= labels.len() {
                    return Err("Tabs need 1–12 labels and a valid selected index".into());
                }
                for label in labels {
                    budget.text(label, 128)?;
                }
                validate_handler(handler)?;
            }
            Self::Canvas {
                width,
                height,
                shapes,
            } => {
                dimensions(*width, *height)?;
                budget.primitives += shapes.len();
                if budget.primitives > 4096 || shapes.len() > 512 {
                    return Err("Canvas shape budget exceeded".into());
                }
                for shape in shapes {
                    shape.validate(budget)?;
                }
            }
            Self::Scene {
                width,
                height,
                objects,
                camera,
            } => {
                dimensions(*width, *height)?;
                budget.primitives += objects.len();
                if budget.primitives > 4096 || objects.len() > 96 {
                    return Err("Scene object budget exceeded".into());
                }
                vector(camera.eye)?;
                vector(camera.target)?;
                bounded(camera.fov as f64, 10., 120., "camera fov")?;
                let delta = [
                    camera.eye[0] - camera.target[0],
                    camera.eye[1] - camera.target[1],
                    camera.eye[2] - camera.target[2],
                ];
                if delta.iter().map(|v| v * v).sum::<f32>() < 0.0001 {
                    return Err("Camera eye and target must differ".into());
                }
                // The renderer uses world-up [0, 1, 0]; reject a singular view basis.
                if delta[0] * delta[0] + delta[2] * delta[2] < 0.0001 {
                    return Err("Camera must not point exactly along the vertical axis".into());
                }
                for object in objects {
                    object.validate(budget)?;
                }
            }
        }
        Ok(())
    }
}

/// Data points count against the shared vertex budget.
fn points(values: &[f64], budget: &mut Budget) -> Result<(), String> {
    if values.len() > 512 {
        return Err("A data row may hold at most 512 values".into());
    }
    budget.vertices += values.len();
    if budget.vertices > 32_768 {
        return Err("Visual data point budget exceeded".into());
    }
    for value in values {
        bounded(*value, -1e12, 1e12, "data value")?;
    }
    Ok(())
}

#[derive(Default)]
struct Budget {
    nodes: usize,
    text_bytes: usize,
    primitives: usize,
    vertices: usize,
    triangles: usize,
}
impl Budget {
    fn text(&mut self, text: &str, max: usize) -> Result<(), String> {
        self.text_bytes += text.len();
        if text.len() > max || self.text_bytes > 128 * 1024 {
            return Err("Visual text budget exceeded".into());
        }
        if text.chars().any(|c| c == '\0') {
            return Err("Visual text may not contain NUL".into());
        }
        Ok(())
    }
}

impl Shape {
    fn validate(&self, budget: &mut Budget) -> Result<(), String> {
        match self {
            Self::Rect {
                x,
                y,
                width,
                height,
                ..
            } => {
                coordinate(*x)?;
                coordinate(*y)?;
                bounded(*width as f64, 0., 10_000., "rectangle width")?;
                bounded(*height as f64, 0., 10_000., "rectangle height")?;
            }
            Self::Circle { x, y, radius, .. } => {
                coordinate(*x)?;
                coordinate(*y)?;
                bounded(*radius as f64, 0., 10_000., "circle radius")?;
            }
            Self::Line {
                x1,
                y1,
                x2,
                y2,
                width,
                ..
            } => {
                for x in [x1, y1, x2, y2] {
                    coordinate(*x)?;
                }
                bounded(*width as f64, 0.1, 12., "line width")?;
            }
            Self::Polyline { points, width, .. } => {
                if points.len() < 2 || points.len() > 1024 {
                    return Err("Polyline needs 2–1024 points".into());
                }
                budget.vertices += points.len();
                for point in points {
                    coordinate(point[0])?;
                    coordinate(point[1])?;
                }
                bounded(*width as f64, 0.1, 12., "line width")?;
            }
            Self::Label { x, y, text, .. } => {
                coordinate(*x)?;
                coordinate(*y)?;
                budget.text(text, 1024)?;
            }
        }
        if budget.vertices > 32_768 {
            return Err("Visual geometry vertex budget exceeded".into());
        }
        Ok(())
    }
}

impl SceneObject {
    fn validate(&self, budget: &mut Budget) -> Result<(), String> {
        vector(self.position)?;
        vector(self.rotation)?;
        for scale in self.scale {
            bounded(scale as f64, 0.0001, 1000., "object scale")?;
        }
        match &self.geometry {
            Geometry::Mesh { vertices, indices } => {
                if vertices.len() < 3
                    || vertices.len() > 2048
                    || indices.is_empty()
                    || indices.len() > 4096
                {
                    return Err("Meshes need 3–2048 vertices and 1–4096 triangles".into());
                }
                budget.vertices += vertices.len();
                budget.triangles += indices.len();
                for vertex in vertices {
                    vector(*vertex)?;
                }
                if indices
                    .iter()
                    .flatten()
                    .any(|index| *index >= vertices.len())
                {
                    return Err("Mesh triangle references a missing vertex".into());
                }
            }
            Geometry::Line { points, width } => {
                if points.len() < 2 || points.len() > 1024 {
                    return Err("Scene line needs 2–1024 points".into());
                }
                budget.vertices += points.len();
                for point in points {
                    vector(*point)?;
                }
                bounded(*width as f64, 0.1, 12., "scene line width")?;
            }
            _ => {
                budget.vertices += 512;
                budget.triangles += 1024;
            }
        }
        if budget.vertices > 32_768 || budget.triangles > 24_000 {
            return Err("Visual geometry budget exceeded".into());
        }
        Ok(())
    }
}

fn validate_handler(handler: &str) -> Result<(), String> {
    if handler.is_empty()
        || handler.len() > 64
        || !handler.bytes().all(|b| b.is_ascii_digit() || b == b':')
    {
        return Err("Invalid visual event handler".into());
    }
    Ok(())
}
fn dimensions(width: f32, height: f32) -> Result<(), String> {
    bounded(width as f64, 160., 1600., "visual width")?;
    bounded(height as f64, 120., 900., "visual height")
}
fn coordinate(value: f32) -> Result<(), String> {
    bounded(value as f64, -10_000., 10_000., "coordinate")
}
fn vector(value: [f32; 3]) -> Result<(), String> {
    value.into_iter().try_for_each(coordinate)
}
fn bounded(value: f64, min: f64, max: f64, name: &str) -> Result<(), String> {
    if value.is_finite() && value >= min && value <= max {
        Ok(())
    } else {
        Err(format!("{name} must be finite and between {min} and {max}"))
    }
}
