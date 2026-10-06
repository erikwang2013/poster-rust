//! 海报：链式 Builder + 模板 + 14 种元素，对应 PHP `src/Poster/`。
//!
//! ```no_run
//! use poster::poster::{PosterBuilder, elements::text::TextElement};
//!
//! let mut b = PosterBuilder::new()?;
//! b.background("#FFFFFF").add_text("你好，海报", TextElement::default());
//! let img = b.render()?;
//! # Ok::<(), poster::PosterError>(())
//! ```

pub mod builder;
pub mod elements;
pub mod template;

pub use builder::{Direction, PosterBuilder};
pub use elements::Element;
pub use template::PosterTemplate;
