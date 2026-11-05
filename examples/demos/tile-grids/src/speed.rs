//! The page "Speed on every grid": the map the engine's speed guards hold
//! to their targets (`benches/tilemap.rs`, recorded in
//! `docs/backlog/tilemap.md`), built in code on the grid the Grid setting
//! picks and flown over. It is 1024 x 1024 cells in four layers: a floor on
//! every cell, cave rock where value noise is high (colliding and casting
//! shadows), props on about one open cell in 16 and small things on one in
//! 64. Each grid's cells are as big as the guards': orthogonal 16 px,
//! isometric and staggered diamonds of 32 x 16, hexagons of 16 x 16 with
//! sides of 8, and oblique 16 px squares skewed 4 px a row.
//!
//! The tiles are Tiny Dungeon's, cut to each grid's cells in code (on the
//! isometric and staggered grids its squares laid flat on the diamonds).
//! Only the chosen grid's map is built, as the page opens and when the
//! Grid setting changes: its million cells are made on the main thread in
//! that step (a hitch), then the engine works out its chunks, colliders and
//! shadows off the main thread, the first build of a map this size taking
//! about 55 to 100 ms before it lands. The HUD shows what the frames cost
//! on the CPU (`FrameCost`); compare it with the record in a release build

use demo_kit::prelude::*;

use crate::shared::{DIM, GOOD, HOT, View, beside_panel, hud, load};

/// Tiny Dungeon's sheet: 16 px tiles, 12 a row
const SHEET: &str = "kenney-tiny-dungeon/tilemap_packed.png";
const SHEET_COLUMNS: u32 = 12;
const SIDE: u32 = 16;
/// The map's side in cells, as the guards'
const BIG: u32 = 1024;
/// The sheet's tiles each layer draws from: sand and flagstone floors, grey
/// brick rock, chests, tombstones and furniture, and potions and blades
const BACK: [u32; 3] = [48, 49, 42];
const ROCK: [u32; 2] = [40, 28];
const PROPS: [u32; 6] = [89, 64, 65, 74, 72, 73];
const FRONT: [u32; 6] = [113, 114, 115, 116, 103, 104];
/// The layers, bottom first
const LAYERS: [&str; 4] = ["background", "ground", "props", "front"];
/// How fast the camera flies, in cells a second along its loop
const FLY: f32 = 561.0 / SIDE as f32;
const DARK: Color = Color::rgb(25, 17, 22);
/// The settings rows, for the notes under the panel
const ROWS: usize = 2;

/// The grids, as the Grid setting names them, and as the HUD tells them
const GRIDS: [&str; 5] = [
  "Orthogonal",
  "Isometric",
  "Staggered",
  "Hexagonal",
  "Oblique",
];
const CELLS: [&str; 5] = [
  "16 px squares",
  "32 x 16 diamonds",
  "32 x 16 staggered diamonds",
  "16 x 16 hexagons of side 8",
  "16 px squares skewed (4, 0)",
];

fn shape(grid: usize) -> GridShape {
  match grid {
    1 => GridShape::Isometric,
    2 => GridShape::Staggered {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
    },
    3 => GridShape::Hexagonal {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
      side: 8,
    },
    4 => GridShape::Oblique {
      skew: IVec2::new(4, 0),
    },
    _ => GridShape::Orthogonal,
  }
}

fn cell_size(grid: usize) -> UVec2 {
  match grid {
    1 | 2 => UVec2::new(32, 16),
    _ => UVec2::splat(SIDE),
  }
}

/// A number in 0..1 from a cell, the same every time
fn hash(x: i32, y: i32, salt: u32) -> f32 {
  let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
    ^ (y as u32).wrapping_mul(0xD816_3841)
    ^ salt.wrapping_mul(0xCB1A_B31F);
  h ^= h >> 13;
  h = h.wrapping_mul(0x5BD1_E995);
  h ^= h >> 15;
  (h & 0xFFFF) as f32 / 65536.0
}

/// Smooth value noise in 0..1, on a lattice one unit apart
fn noise(x: f32, y: f32, salt: u32) -> f32 {
  let (x0, y0) = (x.floor(), y.floor());
  let (fx, fy) = (x - x0, y - y0);
  let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
  let (ix, iy) = (x0 as i32, y0 as i32);
  let at = |dx: i32, dy: i32| hash(ix + dx, iy + dy, salt);
  let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
  let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
  top + (bottom - top) * sy
}

/// Whether the cell is cave rock: where noise is high, as in the guards
fn rocky(x: i32, y: i32) -> bool {
  let (fx, fy) = (x as f32, y as f32);
  noise(fx / 22.0, fy / 22.0, 1) * 0.7 + noise(fx / 7.0, fy / 7.0, 2) * 0.3 > 0.6
}

/// One of `count` tiles from `first`, picked by `roll` (0..1)
fn pick(first: u32, count: usize, roll: f32) -> u32 {
  first + ((roll * count as f32) as u32).min(count as u32 - 1)
}

/// The tileset's tiles, in order: the atlas holds the sheet's tiles each
/// layer draws from, one after the other
fn atlas_ids() -> Vec<u32> {
  [&BACK[..], &ROCK, &PROPS, &FRONT].concat()
}

/// An atlas of the sheet's `ids` cut to `grid`'s cells, one a column: a
/// square tile as it is (an oblique map moves its pictures unskewed), the
/// hexagonal grid's masked to its hexagon, the diamonds' laid flat on them
/// (each pixel of the diamond takes the sheet's texel under it, the square
/// turned and squashed onto the diamond)
fn atlas(sheet: &Image, grid: &MapGrid, ids: &[u32]) -> Image {
  let cell = grid.cell;
  let outline = grid.cell_outline(IVec2::ZERO);
  let corner = grid.cell_bounds(IVec2::ZERO);
  let (width, height) = (cell.x * ids.len() as u32, cell.y);
  let mut data = vec![0u8; (width * height * 4) as usize];
  let diamond = matches!(
    grid.shape,
    GridShape::Isometric | GridShape::Staggered { .. }
  );
  let hexagon = matches!(grid.shape, GridShape::Hexagonal { .. });
  let inside = |p: Vec2| {
    let points = outline.points();
    (0..points.len()).all(|i| {
      let (a, b) = (points[i], points[(i + 1) % points.len()]);
      (b - a).perp_dot(p - a) >= 0.0
    })
  };
  let texel = |id: u32, u: u32, v: u32| -> [u8; 4] {
    let (x, y) = (
      (id % SHEET_COLUMNS) * SIDE + u,
      (id / SHEET_COLUMNS) * SIDE + v,
    );
    let i = ((y * sheet.width() + x) * 4) as usize;
    let mut out = [0u8; 4];
    if let Some(src) = sheet.data().get(i..i + 4) {
      out.copy_from_slice(src);
    }
    out
  };
  for (n, &id) in ids.iter().enumerate() {
    for v in 0..cell.y {
      for u in 0..cell.x {
        let p = Vec2::new(u as f32 + 0.5, v as f32 + 0.5);
        let source = if diamond {
          // Across the diamond from its top vertex: a down-right, b down-left
          let (w, h) = (cell.x as f32, cell.y as f32);
          let a = (p.x - w / 2.0) / w + p.y / h;
          let b = p.y / h - (p.x - w / 2.0) / w;
          ((0.0..1.0).contains(&a) && (0.0..1.0).contains(&b))
            .then_some(((a * SIDE as f32) as u32, (b * SIDE as f32) as u32))
        } else {
          let world = Vec2::new(corner.x, corner.y) + p;
          (!hexagon || inside(world)).then_some((u.min(SIDE - 1), v.min(SIDE - 1)))
        };
        let Some((su, sv)) = source else {
          continue;
        };
        let i = ((v * width + n as u32 * cell.x + u) * 4) as usize;
        data[i..i + 4].copy_from_slice(&texel(id, su, sv));
      }
    }
  }
  Image::from_rgba(data, width, height)
}

/// The tileset over `grid`'s atlas: the rock tiles collide as their whole
/// cell, the cell's outline in the tile's pixels (its diamond, its hexagon,
/// else the tile's square), as the speed guards' do
fn tileset(image: Handle<Image>, grid: &MapGrid, count: u32) -> Tileset {
  let cell = grid.cell;
  let mut set = Tileset::new(
    "speed",
    cell,
    TilesetImage::Atlas {
      image,
      size: UVec2::new(cell.x * count, cell.y),
    },
  );
  set.columns = count;
  set.tile_count = count;
  let (w, h) = (cell.x as f32, cell.y as f32);
  let points = match grid.shape {
    GridShape::Isometric | GridShape::Staggered { .. } => vec![
      Vec2::new(w / 2.0, 0.0),
      Vec2::new(w, h / 2.0),
      Vec2::new(w / 2.0, h),
      Vec2::new(0.0, h / 2.0),
    ],
    // A tile the size of its cell stands on the cell's box
    GridShape::Hexagonal { .. } => {
      let corner = grid.cell_bounds(IVec2::ZERO);
      let corner = Vec2::new(corner.x, corner.y);
      let outline = grid.cell_outline(IVec2::ZERO);
      outline.points().iter().map(|&p| p - corner).collect()
    }
    _ => vec![
      Vec2::ZERO,
      Vec2::new(w, 0.0),
      Vec2::new(w, h),
      Vec2::new(0.0, h),
    ],
  };
  for i in 0..ROCK.len() {
    set.insert_tile(
      (BACK.len() + i) as u32,
      TileData {
        shapes: vec![TileShape {
          points: points.clone(),
          ..TileShape::default()
        }],
        ..TileData::default()
      },
    );
  }
  set
}

/// The guards' cave map on `grid`'s cells, its tiles from tileset `set`
fn cave(grid: usize, set: TilesetSource) -> TileMap {
  let mut map = TileMap::new(UVec2::splat(BIG), cell_size(grid)).with_shape(shape(grid));
  let set = map.add_tileset(set);
  let layers: Vec<usize> = LAYERS.iter().map(|name| map.add_tile_layer(name)).collect();
  // Each layer's tiles' first id in the atlas
  let (back, rock, props) = (0, BACK.len() as u32, (BACK.len() + ROCK.len()) as u32);
  let front = props + PROPS.len() as u32;
  for y in 0..BIG as i32 {
    for x in 0..BIG as i32 {
      let cell = IVec2::new(x, y);
      let roll = hash(x, y, 3);
      let mut put = |layer: usize, id: u32| {
        if let Some(tiles) = map.tiles_mut(layers[layer]) {
          tiles.set(cell, Some(Tile::new(set, id)));
        }
      };
      put(0, pick(back, BACK.len(), roll));
      if rocky(x, y) {
        put(1, pick(rock, ROCK.len(), roll));
        continue;
      }
      let scatter = hash(x, y, 4);
      if scatter < 1.0 / 16.0 {
        put(2, pick(props, PROPS.len(), scatter * 16.0));
      } else if scatter < 1.0 / 16.0 + 1.0 / 64.0 {
        put(3, pick(front, FRONT.len(), (scatter - 1.0 / 16.0) * 64.0));
      }
    }
  }
  map
}

/// The camera's loop over the map at `path` (radians along it), in cells
/// counted as on an orthogonal grid, as the guards fly it
fn flown(path: f32) -> Vec2 {
  let side = BIG as f32;
  let reach = side * 0.42;
  Vec2::new(
    side / 2.0 + reach * path.sin(),
    side / 2.0 + reach * (path * 1.37 + 1.0).sin(),
  )
}

/// The point `at` cells into `grid` (as on an orthogonal grid, from cell
/// (0, 0)'s corner): the four nearest cells' centres blended
fn grid_point(grid: &MapGrid, at: Vec2) -> Vec2 {
  let at = at - 0.5;
  let base = at.floor();
  let (f, cell) = (at - base, base.as_ivec2());
  let centre = |x: i32, y: i32| grid.cell_center(cell + IVec2::new(x, y));
  let top = centre(0, 0).lerp(centre(1, 0), f.x);
  let bottom = centre(0, 1).lerp(centre(1, 1), f.x);
  top.lerp(bottom, f.y)
}

pub(crate) struct SpeedPage {
  view: View,
  root: Option<Entity>,
  /// The grid the root's map is on, by the Grid setting's index
  built: usize,
  grid: usize,
  fly: bool,
  /// Radians along the camera's loop
  path: f32,
  /// Frames since the map was spawned, until it landed
  waited: u32,
  landed_after: Option<u32>,
}

impl SpeedPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      root: None,
      built: 0,
      grid: 0,
      fly: true,
      path: 0.0,
      waited: 0,
      landed_after: None,
    }
  }

  /// The chosen grid's map, in place of the one there
  fn build(&mut self, ctx: &mut SceneContext) {
    if let Some(root) = self.root.take() {
      ctx.world.despawn(root);
    }
    self.built = self.grid;
    self.waited = 0;
    self.landed_after = None;
    let Some(sheet) = load::<Image>(ctx, SHEET) else {
      return;
    };
    let grid = MapGrid::new(shape(self.grid), cell_size(self.grid));
    let ids = atlas_ids();
    let image = {
      let assets = ctx.assets();
      let Some(sheet) = assets.get(&sheet) else {
        return;
      };
      atlas(sheet, &grid, &ids)
    };
    let image = ctx.assets().add(image);
    let set = ctx.assets().add(tileset(image, &grid, ids.len() as u32));
    let map = ctx.assets().add(cave(self.grid, TilesetSource::Asset(set)));
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
  }

  /// Where the camera is on its loop, on the map's grid
  fn eye(&self) -> Vec2 {
    let grid = MapGrid::for_map(
      shape(self.built),
      cell_size(self.built),
      UVec2::splat(BIG),
      false,
    );
    grid_point(&grid, flown(self.path))
  }
}

impl DemoPage for SpeedPage {
  fn name(&self) -> &str {
    "Speed on every grid"
  }

  fn description(&self) -> &str {
    "The speed guards' 1024 x 1024 four-layer map on the grid you pick, flown over"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Grid", &mut self.grid, &GRIDS);
    s.toggle("Fly", &mut self.fly);
  }

  fn controls(&self) -> Vec<Control> {
    Vec::new()
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    crate::shared::clear_to(ctx, DARK);
    self.build(ctx);
    let centre = self.view.centre_for(ctx, self.eye(), 1.0);
    self.view.place(ctx, centre, 1.0, true);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    if self.grid != self.built {
      self.build(ctx);
    }
    let Some(root) = self.root else {
      return;
    };
    if self.landed_after.is_none() {
      if ctx.maps().is_ready(root) {
        self.landed_after = Some(self.waited);
      } else {
        self.waited += 1;
      }
    }
    if self.fly {
      self.path += dt * FLY / (BIG as f32 * 0.5);
    }
    let centre = self.view.centre_for(ctx, self.eye(), 1.0);
    self.view.place(ctx, centre, 1.0, false);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    let cost = ctx.resource::<FrameCost>().copied();
    let frame = cost.map_or_else(
      || "Frame: measuring".to_string(),
      |cost| {
        format!(
          "Frame: {:.2} ms on the CPU (update {:.2}, render {:.2})",
          cost.total_ms(),
          cost.update_ms(),
          cost.render_ms()
        )
      },
    );
    let landing = match self.landed_after {
      Some(steps) => format!("Landed {steps} steps after it was built"),
      None => "Landing: its chunks are worked out off the main thread".to_string(),
    };
    let lines = vec![
      (
        format!("{}: 1024 x 1024 of them, four layers", CELLS[self.built]),
        Color::WHITE,
      ),
      (frame, HOT),
      (landing, GOOD),
      ("The rock collides and casts shadows".to_string(), DIM),
      (
        "Compare in a release build with docs/backlog/tilemap.md".to_string(),
        DIM,
      ),
    ];
    let column = beside_panel(ui, ROWS);
    hud(ui, column.x, column.y, column.width, &lines);
  }
}
