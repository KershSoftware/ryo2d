//! The engine's loaders, registered in every world's registry

mod image;
mod shader;

pub use self::image::{ImageLoader, ImageSettings, SvgSize};
pub use self::shader::ShaderLoader;
