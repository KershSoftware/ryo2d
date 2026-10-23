//! The engine's loaders, registered in every world's registry

mod bitmap_font;
mod font;
mod image;
mod shader;

pub use self::bitmap_font::BitmapFontLoader;
pub use self::font::{FontLoader, FontSettings};
pub use self::image::{ImageLoader, ImageSettings, SvgSize};
pub use self::shader::ShaderLoader;
