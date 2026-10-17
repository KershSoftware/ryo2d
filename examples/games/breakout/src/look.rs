//! The game's look: Press Start 2P, an arcade font on an 8 px grid, at one
//! small size, and panels roomy enough for it

use ryo2d::prelude::*;

/// Press Start 2P (SIL OFL 1.1, see `assets/fonts/`), embedded so the web
/// build has it too
const PRESS_START_2P: &[u8] = include_bytes!("../assets/fonts/PressStart2P-Regular.ttf");

/// The game's text size in UI units. Pixel text snaps to whole multiples of
/// the font's 8 px grid on screen: at a UI scale of 2 this is 24 pixels.
pub const TEXT_SIZE: f32 = 12.0;

/// Dialog titles, a step above the text
const TITLE_SIZE: f32 = 16.0;

/// The game's text in `color`
pub fn text_style(color: Color) -> TextStyle {
  TextStyle::default().size(TEXT_SIZE).color(color)
}

/// Make Press Start 2P the UI font, so the menus, the HUD and the dialogs
/// all use it, drawn as a pixel font with an 8 px design size; size the
/// theme's text to the game's; and pad panels and dialogs 16 px, since the
/// font has almost no side bearing. Calling this again changes nothing, so
/// every menu entry may.
pub fn use_game_look(ctx: &mut SceneContext) {
  match ctx.fonts().add_bytes(PRESS_START_2P) {
    Ok(font) => {
      ctx.fonts().set_render(font, FontRender::pixel(8.0));
      ctx.fonts().set_role(FontRole::Ui, font);
    }
    Err(e) => eprintln!("Press Start 2P could not be loaded: {e}"),
  }
  ctx.ui_theme().modify(|theme| {
    theme.metrics.sizes.panel_padding = 16.0;
    theme.text.body.size = TEXT_SIZE;
    theme.text.button.size = TEXT_SIZE;
    theme.text.heading.size = TITLE_SIZE;
  });
}
