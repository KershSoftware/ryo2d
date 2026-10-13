//! Shadow demo pages
//!
//! Each module contains a different shadow type demonstration.

mod ambient_occlusion;
mod blob_shadows;
mod colored_shadows;
mod contact_shadows;
mod drop_shadows;
mod hard_shadows;
mod placed_occluders;
mod soft_shadows;

pub use ambient_occlusion::AmbientOcclusionPage;
pub use blob_shadows::BlobShadowsPage;
pub use colored_shadows::ColoredShadowsPage;
pub use contact_shadows::ContactShadowsPage;
pub use drop_shadows::DropShadowsPage;
pub use hard_shadows::HardShadowsPage;
pub use placed_occluders::PlacedOccludersPage;
pub use soft_shadows::SoftShadowsPage;
