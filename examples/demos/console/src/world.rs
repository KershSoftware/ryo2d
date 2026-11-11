//! The World page: `game:spawn <kind> [count]` and `game:clear` add and
//! remove entities in the world, with typed arguments and an error for a bad
//! one, and `game:say "<phrase>"` shows a quoted phrase on screen

use demo_kit::prelude::*;

use crate::shared::{DIM, GOOD, HOT, MAX_SPAWN, OPEN_LINE, Spawned, draw_footer};

/// What `game:spawn` knows: name, side in pixels, colour
const KINDS: [(&str, u32, [u8; 3]); 3] = [
  ("slime", 22, [90, 200, 110]),
  ("bat", 14, [170, 110, 220]),
  ("rock", 30, [140, 140, 150]),
];

/// How long a phrase stays, in seconds
const SAY_SECONDS: f32 = 5.0;

pub(crate) struct WorldPage {
  /// Spawned so far, for the positions
  counter: u32,
  /// The phrase `game:say` set, and the seconds it has left
  phrase: String,
  phrase_left: f32,
  /// How many spawned entities are alive, as the page counted
  count: usize,
}

impl WorldPage {
  pub(crate) fn new() -> Self {
    Self {
      counter: 0,
      phrase: String::new(),
      phrase_left: 0.0,
      count: 0,
    }
  }

  fn spawn(&mut self, ctx: &mut SceneContext, kind: &str, count: usize) -> Result<(), String> {
    let Some(&(_, side, [r, g, b])) = KINDS.iter().find(|(name, ..)| *name == kind) else {
      let names: Vec<&str> = KINDS.iter().map(|k| k.0).collect();
      return Err(format!("unknown kind {kind:?}; try {}", names.join(", ")));
    };
    let image = ctx
      .assets()
      .add(Image::solid_rect(side, side, r, g, b, 255));
    for _ in 0..count {
      self.counter += 1;
      // A golden-angle spread over the field under the text
      let turn = self.counter as f32 * 0.618_034;
      let x = 60.0 + turn.fract() * 760.0;
      let y = 330.0 + ((self.counter as f32 * 0.381_966).fract()) * 150.0;
      ctx.world.spawn((
        Transform::from_xy(x, y),
        Renderable::image(image.clone()).with_z_order(5),
        Spawned,
      ));
    }
    self.count += count;
    Ok(())
  }

  fn clear(&mut self, ctx: &mut SceneContext) -> usize {
    let world = &mut *ctx.world;
    let found: Vec<Entity> = world
      .query_filtered::<Entity, With<Spawned>>()
      .iter(world)
      .collect();
    for entity in &found {
      world.despawn(*entity);
    }
    self.count = 0;
    found.len()
  }
}

impl DemoPage for WorldPage {
  fn name(&self) -> &str {
    "World"
  }

  fn description(&self) -> &str {
    concat!(
      "Console commands that act on the game: game:spawn, game:clear and game:say",
      "\nOpen the console with backquote and try game:spawn slime 20, game:spawn rock 5, game:spawn dragon (an error), game:spawn slime many (the usage), game:clear and game:say \"hello there\".",
      "\nA handler gets typed arguments with cmd.arg::<T>(i); an Err(text) it returns prints in red.",
      "\nThe console is a development tool and is absent from a shipped build (ryo2d build --dev-tools keeps it).\nRun `help` in it to list the commands; Tab completes a name."
    )
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(
      KeyCode::Space,
      "Spawn a slime (as game:spawn slime)",
    )]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // The page's own scenery: game:clear leaves it alone
    let ground = ctx
      .assets()
      .add(Image::solid_rect(800, 4, 70, 76, 100, 255));
    ctx.world.spawn((
      Transform::from_xy(480.0, 500.0),
      Renderable::image(ground).with_z_order(0),
    ));

    page_command(
      ctx,
      CommandSpec::new("game:spawn", "Add entities to the world")
        .usage("game:spawn <kind> [count]")
        .help(format!(
          "Adds count (default 1, at most {MAX_SPAWN}) of a kind: slime, bat or rock."
        )),
      |page: &mut WorldPage, ctx, cmd| {
        let kind: String = cmd.arg(0)?;
        let count: usize = cmd.arg_or(1, 1)?;
        if count == 0 || count > MAX_SPAWN {
          return Err(format!("count must be 1 to {MAX_SPAWN}, not {count}"));
        }
        page.spawn(ctx, &kind.to_ascii_lowercase(), count)?;
        cmd.print(format!("Spawned {count} {kind}"));
        Ok(())
      },
    );

    page_command(
      ctx,
      CommandSpec::new("game:clear", "Remove what game:spawn added")
        .usage("game:clear")
        .help("Removes the entities game:spawn added; the page's own stay."),
      |page: &mut WorldPage, ctx, cmd| {
        let removed = page.clear(ctx);
        cmd.print(format!("Removed {removed}"));
        Ok(())
      },
    );

    page_command(
      ctx,
      CommandSpec::new("game:say", "Show a phrase on screen")
        .usage("game:say \"<phrase>\"")
        .help("Double quotes keep a phrase with spaces together; it shows for a few seconds."),
      |page: &mut WorldPage, _ctx, cmd| {
        let phrase = cmd.rest(0);
        if phrase.trim().is_empty() {
          return Err("nothing to say; usage: game:say \"<phrase>\"".into());
        }
        cmd.print(format!("Saying: {phrase}"));
        page.phrase = phrase;
        page.phrase_left = SAY_SECONDS;
        Ok(())
      },
    );
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.phrase_left = (self.phrase_left - dt).max(0.0);
    if ctx.input.key_pressed(KeyCode::Space) {
      // A bounded count, so this cannot fail
      let _ = self.spawn(ctx, "slime", 1);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let y = area.y + 8.0;
    ui.draw_text(OPEN_LINE, x, y, HOT);
    ui.draw_text(
      "game:spawn <kind> [count]   kinds: slime, bat, rock; count 1 to 500",
      x,
      y + 26.0,
      Color::WHITE,
    );
    ui.draw_text(
      "game:clear                  removes what spawn added",
      x,
      y + 46.0,
      Color::WHITE,
    );
    ui.draw_text(
      "game:say \"<phrase>\"         shows the phrase",
      x,
      y + 66.0,
      Color::WHITE,
    );
    ui.draw_text(&format!("Spawned now: {}", self.count), x, y + 96.0, GOOD);
    if self.phrase_left > 0.0 {
      ui.draw_text(
        &format!("\"{}\"", self.phrase),
        x,
        y + 120.0,
        Color::rgb(255, 230, 150),
      );
    } else {
      ui.draw_text("(nothing said)", x, y + 120.0, DIM);
    }
    draw_footer(ui, x, area.y + area.height - 24.0);
  }
}
