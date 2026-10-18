//! The Big list page: a leaderboard of 10,000 players twice over, as a
//! closure `ui.list` beside a node `List` built from a UI file. Each builds
//! only the rows in view. The pad walks either list end to end, the shoulder
//! buttons switch between them, and Go jumps both to a rank.

use demo_kit::prelude::*;

/// The node list's file, relative to the working directory
const BIG_LIST: &str = "assets/ui/big_list.ui.ron";

/// Players on the board
const PLAYERS: usize = 10_000;
/// A row's height and the gap under it, as the file's list has them, so the
/// two lists' rows line up
const ROW_HEIGHT: f32 = 28.0;
const ROW_GAP: f32 = 2.0;
/// The lists' side padding, as the file's list has it: room for the focus
/// ring
const LIST_PADDING: f32 = 4.0;
/// The room the page's other lines take above and below the lists
const AROUND_LISTS: f32 = 132.0;
const MIN_LIST_HEIGHT: f32 = 120.0;
/// Between the two lists
const LIST_GAP: f32 = 24.0;
/// The jump field's rank when the page opens
const START_RANK: &str = "5000";

const FIRST_NAMES: [&str; 17] = [
  "Ada", "Bram", "Cleo", "Dara", "Emil", "Fern", "Gus", "Hana", "Ivo", "Juno", "Kai", "Lena",
  "Milo", "Nia", "Otto", "Pia", "Rex",
];
const LAST_NAMES: [&str; 13] = [
  "Marsh", "Vale", "Stone", "Reed", "Frost", "Hale", "Wren", "Moss", "Pike", "Lark", "Thorn",
  "Ash", "Brook",
];

/// One player as the rows show them
struct Entry {
  rank: String,
  name: String,
  score: String,
}

/// The board both lists show: rank `i + 1` is entry `i`, scores falling
fn leaderboard() -> Vec<Entry> {
  let mut score: usize = 2_500_000;
  (0..PLAYERS)
    .map(|i| {
      let first = FIRST_NAMES[i % FIRST_NAMES.len()];
      let last = LAST_NAMES[(i / FIRST_NAMES.len()) % LAST_NAMES.len()];
      let entry = Entry {
        rank: grouped(i + 1),
        name: format!("{first} {last}"),
        score: grouped(score),
      };
      score = score.saturating_sub(40 + (i * 37) % 160);
      entry
    })
    .collect()
}

/// `n` with a comma between groups of three digits: 12,345
fn grouped(n: usize) -> String {
  let digits = n.to_string();
  let mut out = String::with_capacity(digits.len() + digits.len() / 3);
  for (i, digit) in digits.chars().enumerate() {
    if i > 0 && (digits.len() - i).is_multiple_of(3) {
      out.push(',');
    }
    out.push(digit);
  }
  out
}

/// The rank a jump field's text asks for, clamped to the board
fn rank_of(text: &str) -> Option<usize> {
  let digits: String = text.chars().filter(|c| *c != ',').collect();
  let rank = digits.trim().parse::<u64>().ok()?;
  Some(rank.clamp(1, PLAYERS as u64) as usize)
}

/// What a list shows: the rank whose row has the focus, if one of its rows
/// has it, and the first and last ranks in view
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListShown {
  pub focused: Option<usize>,
  pub first: usize,
  pub last: usize,
}

/// What the page's two lists show, as of its last UI frame
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BigListShown {
  pub closure: ListShown,
  pub node: ListShown,
}

/// The ranks a list of the page shows from scroll offset `offset` in a view
/// `view` tall
fn ranks_in_view(offset: f32, view: f32) -> (usize, usize) {
  let stride = f64::from(ROW_HEIGHT + ROW_GAP);
  let (offset, view) = (f64::from(offset.max(0.0)), f64::from(view.max(0.0)));
  // Float-to-integer casts saturate
  let first = ((offset - f64::from(ROW_HEIGHT)) / stride)
    .floor()
    .max(-1.0) as i64
    + 1;
  let last = ((offset + view) / stride).ceil() as i64 - 1;
  let clamp = |item: i64| item.clamp(0, PLAYERS as i64 - 1) as usize + 1;
  (clamp(first), clamp(last))
}

/// The page's two lists
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
  Closure,
  Node,
}

/// The item of the list node `list` whose row holds the focus
fn focused_item(world: &World, list: Entity) -> Option<usize> {
  let rows = world.get::<Children>(list)?;
  let row = rows.iter().find(|row| holds_focus(world, **row))?;
  world.get::<UiListRow>(*row).map(UiListRow::index)
}

/// Whether `node` or a node under it has the focus
fn holds_focus(world: &World, node: Entity) -> bool {
  world
    .get::<UiInteraction>(node)
    .is_some_and(UiInteraction::focused)
    || world
      .get::<Children>(node)
      .is_some_and(|kids| kids.iter().any(|kid| holds_focus(world, *kid)))
}

/// One row of the closure list: a button across it, under the rank, the
/// name and the score, as the file's `LeaderRow` is
fn leader_row(ui: &mut UiContext, entry: &Entry, first: bool, muted: Color) -> Response {
  let mut response = Response::default();
  ui.stack().fill().show(|ui| {
    let mut pick = ui
      .button("")
      .width(Length::Pct(1.0))
      .height(Length::Pct(1.0));
    if first {
      pick = pick.default_focus();
    }
    response = pick.show();
    ui.row()
      .fill()
      .padding((12.0, 0.0))
      .gap(12.0)
      .align(Align::Center)
      .show(|ui| {
        ui.column().width(64.0).show(|ui| {
          ui.text(entry.rank.as_str()).color(muted);
        });
        ui.column().fill_width().show(|ui| {
          ui.text(entry.name.as_str());
        });
        ui.column().width(96.0).align(Align::End).show(|ui| {
          ui.text(entry.score.as_str());
        });
      });
  });
  response
}

pub struct BigListPage {
  board: Vec<Entry>,
  /// The jump field's text
  jump: String,
  status: String,
  /// The closure list's id, from its last draw
  closure: Option<UiId>,
  /// The file's tree, drawn inside the page, and its list node
  root: Option<Entity>,
  list: Option<Entity>,
  /// The item each list's focus is on, or was on last: switching to a list
  /// goes back there
  closure_at: usize,
  node_at: usize,
  /// The list that had the focus last: a jump gives it the focus
  active: Side,
  shown: BigListShown,
  /// Why the file did not load, drawn in place of the node list
  error: Option<String>,
}

impl BigListPage {
  pub fn new() -> Self {
    Self {
      board: leaderboard(),
      jump: START_RANK.to_string(),
      status: "Type a rank, then Go.".to_string(),
      closure: None,
      root: None,
      list: None,
      closure_at: 0,
      node_at: 0,
      active: Side::Closure,
      shown: BigListShown::default(),
      error: None,
    }
  }

  /// What the two lists showed on the last UI frame
  pub fn shown(&self) -> BigListShown {
    self.shown
  }

  fn fail(&mut self, error: UiFileError) {
    let text = error.to_string();
    if self.error.as_deref() != Some(text.as_str()) {
      game_error!("{error}");
      self.error = Some(text);
    }
  }

  /// Build the file's tree, then spawn it
  fn spawn_board(&mut self, ctx: &mut SceneContext) -> Result<(), UiFileError> {
    let tree = ctx.assets.load_ui(BIG_LIST)?.root(ctx.assets)?;
    let root = ctx.spawn_ui(tree);
    let Some(list) = ctx.find_ui_in(root, "board") else {
      ctx.world.despawn(root);
      return Err(UiFileError {
        issues: vec![UiFileIssue {
          path: BIG_LIST.to_string(),
          line: 0,
          column: 0,
          message: "the root needs a List named board".to_string(),
        }],
      });
    };
    self.root = Some(root);
    self.list = Some(list);
    Ok(())
  }

  /// A saved file is brought in line before the UI pass: follow a replaced
  /// root, and look the list up again
  fn follow_reloads(&mut self, ctx: &mut SceneContext) {
    for reloaded in ctx.messages::<UiReloaded>() {
      let ours = |root| root == reloaded.root || Some(root) == reloaded.replaced;
      if self.root.is_some_and(ours) {
        self.root = Some(reloaded.root);
        self.list = ctx.find_ui_in(reloaded.root, "board");
      }
    }
  }

  /// Fill the node list's rows given an item this frame
  fn bind_rows(&self, ctx: &mut SceneContext) {
    for bind in ctx.messages::<UiListBind>() {
      // Every list sends binds: answer only the page's
      if Some(bind.list) != self.list {
        continue;
      }
      let Some(entry) = self.board.get(bind.index) else {
        continue;
      };
      ctx.ui_set(bind.row, "rank", entry.rank.as_str());
      ctx.ui_set(bind.row, "name", entry.name.as_str());
      ctx.ui_set(bind.row, "score", entry.score.as_str());
    }
  }

  /// Report the player of a row that was picked
  fn picked(&mut self, item: usize) {
    if let Some(entry) = self.board.get(item) {
      self.status = format!(
        "Rank {}: {}, {} points.",
        entry.rank, entry.name, entry.score
      );
    }
  }

  /// Give list `side` the focus on its last item
  fn switch_to(&self, side: Side, ui: &mut UiContext, ctx: &mut SceneContext) {
    match side {
      Side::Closure => {
        if let Some(list) = self.closure {
          ui.list_focus(list, self.closure_at);
        }
      }
      Side::Node => {
        if let Some(list) = self.list {
          ctx.ui_list_focus(list, self.node_at);
        }
      }
    }
  }

  /// Jump both lists to the field's rank: the list that had the focus last
  /// takes it there, and the other scrolls there
  fn jump(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let Some(rank) = rank_of(&self.jump) else {
      self.status = format!("Type a rank from 1 to {}.", grouped(PLAYERS));
      return;
    };
    let item = rank - 1;
    self.closure_at = item;
    self.node_at = item;
    match (self.active, self.closure, self.list) {
      (Side::Closure, Some(closure), list) => {
        ui.list_focus(closure, item);
        if let Some(list) = list {
          ctx.ui_list_scroll_to(list, item);
        }
      }
      (Side::Node, closure, Some(list)) => {
        ctx.ui_list_focus(list, item);
        if let Some(closure) = closure {
          ui.list_scroll_to(closure, item);
        }
      }
      _ => {}
    }
    self.status = format!("Jumped to rank {}.", grouped(rank));
  }
}

impl Default for BigListPage {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for BigListPage {
  fn name(&self) -> &str {
    "Big list"
  }

  fn description(&self) -> &str {
    "10,000 rows twice: a closure ui.list (left) and a node List from a UI file (right) build only the rows in view.\n\
     The D-pad walks a list end to end (Left and Right cross to the other), the shoulder buttons (Q and E) switch lists, \
     and Go jumps both to the rank in the field: the list last used takes the focus there."
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys("Q / E", &[KeyCode::KeyQ, KeyCode::KeyE], "Switch list"),
      Control::key(KeyCode::Enter, "Go (in the field)"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    if let Err(error) = self.spawn_board(ctx) {
      self.fail(error);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.follow_reloads(ctx);
    self.bind_rows(ctx);
    for event in ctx.ui_events() {
      if event.kind == UiEventKind::Clicked
        && event.action.as_deref() == Some("pick")
        && let Some(item) = ctx.ui_list_index(event.node)
      {
        self.picked(item);
      }
    }
    // The shoulder buttons, unless they are typing into the field
    let switch = if ui.wants_keyboard() {
      None
    } else if ctx.input.action_pressed("ui.tab_prev") {
      Some(Side::Closure)
    } else if ctx.input.action_pressed("ui.tab_next") {
      Some(Side::Node)
    } else {
      None
    };

    let area = content_area(ui);
    let list_height = (area.height - AROUND_LISTS).floor().max(MIN_LIST_HEIGHT);
    let heading = ui.theme().text.heading.style();
    let muted = ui.theme().palette.muted;
    let mut go = false;
    let mut picked = None;
    let mut closure_focus = None;
    let mut closure = None;
    let board = &self.board;
    ui.column()
      .focus_scope()
      .at(area)
      .gap(10.0)
      .show(|ui| {
        ui.row().gap(8.0).align(Align::Center).show(|ui| {
          ui.text("Jump to rank");
          go = ui
            .text_input(&mut self.jump)
            .width(96.0)
            .max_len(6)
            .placeholder("1-10,000")
            .show()
            .submitted();
          go |= ui.button("Go").clicked();
          ui.text(self.status.as_str()).color(muted);
        });
        ui.row().fill_width().gap(LIST_GAP).show(|ui| {
          for (title, what) in [
            ("Closure list", "ui.list(10,000): a closure builds each row in view"),
            ("Node list", "List from big_list.ui.ron: pooled rows filled by UiListBind"),
          ] {
            ui.column().fill_width().gap(2.0).show(|ui| {
              ui.text(title).style(&heading);
              ui.text(what).color(muted);
            });
          }
        });
        ui.row()
          .fill_width()
          .height(list_height)
          .gap(LIST_GAP)
          .show(|ui| {
            ui.column().focus_scope().fill().show(|ui| {
              let list = ui
                .list(PLAYERS)
                .row_height(ROW_HEIGHT)
                .gap(ROW_GAP)
                .padding((LIST_PADDING, 0.0))
                .fill()
                .show(|ui, i| {
                  let Some(entry) = board.get(i) else {
                    return;
                  };
                  let row = leader_row(ui, entry, i == 0, muted);
                  if row.focused() {
                    closure_focus = Some(i);
                  }
                  if row.clicked() {
                    picked = Some(i);
                  }
                });
              closure = Some(list.id);
            });
            match self.root {
              Some(root) => {
                ui.node(root, ctx.world);
              }
              None => {
                ui.column().fill().show(|ui| {
                  if let Some(error) = &self.error {
                    ui.text(error.as_str());
                  }
                });
              }
            }
          });
        ui.text(RichText::markup(
          "[prompt=ui.tab_prev] [prompt=ui.tab_next] Switch list    [prompt=ui.confirm] Pick a row or Go",
        ))
        .color(muted);
      });
    self.closure = closure;
    if let Some(item) = picked {
      self.picked(item);
    }

    // Where each list's focus is, and which had it last
    let node_focus = self.list.and_then(|list| focused_item(ctx.world, list));
    if let Some(item) = closure_focus {
      (self.closure_at, self.active) = (item, Side::Closure);
    }
    if let Some(item) = node_focus {
      (self.node_at, self.active) = (item, Side::Node);
    }
    if go {
      self.jump(ui, ctx);
    } else if let Some(side) = switch {
      self.switch_to(side, ui, ctx);
    }

    let view = list_height;
    let in_view =
      |id: Option<UiId>| id.map_or((1, 1), |id| ranks_in_view(ui.scroll_offset(id).y, view));
    let node_id = self
      .list
      .and_then(|list| ctx.world.get::<UiInteraction>(list))
      .and_then(UiInteraction::id);
    let (first, last) = in_view(self.closure);
    let closure = ListShown {
      focused: closure_focus.map(|i| i + 1),
      first,
      last,
    };
    let (first, last) = in_view(node_id);
    let node = ListShown {
      focused: node_focus.map(|i| i + 1),
      first,
      last,
    };
    self.shown = BigListShown { closure, node };
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn ranks_are_grouped_in_threes() {
    assert_eq!(grouped(7), "7");
    assert_eq!(grouped(999), "999");
    assert_eq!(grouped(1000), "1,000");
    assert_eq!(grouped(10_000), "10,000");
    assert_eq!(grouped(2_500_000), "2,500,000");
  }

  #[test]
  fn a_jump_rank_is_clamped_to_the_board() {
    assert_eq!(rank_of("9990"), Some(9990));
    assert_eq!(rank_of(" 9,990 "), Some(9990));
    assert_eq!(rank_of("0"), Some(1));
    assert_eq!(rank_of("999999"), Some(PLAYERS));
    assert_eq!(rank_of(""), None);
    assert_eq!(rank_of("top"), None);
  }

  #[test]
  fn scores_fall_down_the_board() {
    let board = leaderboard();
    assert_eq!(board.len(), PLAYERS);
    assert_eq!(board[0].rank, "1");
    assert_eq!(board[PLAYERS - 1].rank, "10,000");
    let score = |e: &Entry| e.score.replace(',', "").parse::<usize>().unwrap();
    assert!(board.windows(2).all(|w| score(&w[0]) > score(&w[1])));
  }

  #[test]
  fn the_ranks_in_view_follow_the_offset() {
    // Rows of 28 every 30: a 100-tall view at the top shows ranks 1 to 4
    assert_eq!(ranks_in_view(0.0, 100.0), (1, 4));
    // Rank 1's row ends at 28, so from 28 the view starts at rank 2
    assert_eq!(ranks_in_view(28.0, 100.0), (2, 5));
    assert_eq!(ranks_in_view(27.0, 100.0), (1, 5));
    assert_eq!(ranks_in_view(1e9, 100.0), (PLAYERS, PLAYERS));
  }
}
