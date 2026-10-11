//! Game Scenes
//!
//! Scenes represent distinct states of the game (menu, gameplay, pause, etc.).
//! Each scene handles its own input, update logic, and rendering.

mod game;
mod menu;
mod pause;
mod quit_confirm;

pub use game::GameScene;
pub use menu::MenuScene;
pub use pause::PauseScene;
pub use quit_confirm::QuitConfirmScene;
