//! Shadow demo scenes
//!
//! Each module contains a different shadow type demonstration.

mod ambient_occlusion;
mod blob_shadows;
mod colored_shadows;
mod contact_shadows;
mod drop_shadows;
mod hard_shadows;
mod soft_shadows;

pub use ambient_occlusion::AmbientOcclusionScene;
pub use blob_shadows::BlobShadowsScene;
pub use colored_shadows::ColoredShadowsScene;
pub use contact_shadows::ContactShadowsScene;
pub use drop_shadows::DropShadowsScene;
pub use hard_shadows::HardShadowsScene;
pub use soft_shadows::SoftShadowsScene;
