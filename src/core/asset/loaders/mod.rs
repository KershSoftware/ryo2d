//! The engine's loaders, registered in every world's registry

mod audio_clip;
mod bitmap_font;
mod font;
mod image;
mod shader;
mod theme;
mod ui_file;

pub use self::audio_clip::{AudioClipLoader, AudioClipSettings, ClipMode};
pub use self::bitmap_font::BitmapFontLoader;
pub use self::font::{FontLoader, FontSettings};
pub use self::image::{ImageLoader, ImageSettings, SvgSize};
pub use self::shader::ShaderLoader;
pub use self::theme::ThemeLoader;
pub use self::ui_file::UiFileLoader;
