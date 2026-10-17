//! The game's look: Press Start 2P, an arcade font on an 8 px grid, and
//! panels roomy enough for it

use ryo2d::prelude::*;

/// Press Start 2P (SIL OFL 1.1, see `assets/fonts/`), embedded so the web
/// build has it too
const PRESS_START_2P: &[u8] = include_bytes!("../assets/fonts/PressStart2P-Regular.ttf");

/// Make Press Start 2P the UI font, so the menus, the HUD and the dialogs
/// all use it, drawn as a pixel font with an 8 px design size; and pad
/// panels and dialogs 16 px, since the font has almost no side bearing.
/// Calling this again changes nothing, so every menu entry may.
pub fn use_game_look(ctx: &mut SceneContext) {
  match ctx.fonts().add_bytes(PRESS_START_2P) {
    Ok(font) => {
      ctx.fonts().set_render(font, FontRender::pixel(8.0));
      ctx.fonts().set_role(FontRole::Ui, font);
    }
    Err(e) => eprintln!("Press Start 2P could not be loaded: {e}"),
  }
  ctx
    .ui_theme()
    .modify(|theme| theme.metrics.sizes.panel_padding = 16.0);
}
