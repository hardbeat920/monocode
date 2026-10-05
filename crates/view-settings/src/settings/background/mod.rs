//! Chat background images and their effects: dither, ASCII, halftone,
//! scanlines, and Haze. Ports of newThreadBackgroundEffects.ts, its web
//! worker, useProjectBackgroundEffect.ts, and GradientBlurBackground.tsx.

pub mod chat_background;
pub mod effects;
pub mod gradient_blur;
pub mod project_effect;
pub mod service;

#[cfg(test)]
mod tests;

pub use chat_background::{BackgroundImage, ChatBackground};
pub use effects::HazeVariant;
pub use gradient_blur::{GradientBlurBackground, gradient_blur_background};
pub use project_effect::ProjectBackgroundEffect;
pub use service::{BackgroundEffects, EffectTask, SourceReader};
