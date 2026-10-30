//! The LDtk project, `ldtk/dungeon.ldtk`, written as LDtk 1.5.3 writes one
//! (its keys and their order follow LDtk's own sample projects). Two levels
//! side by side: "Platformer", the character demo's platformer level, and
//! "Vault", a room of entities with fields. Each level's IntGrid layer
//! ("Collisions": `solid`, `one_way`, `slope_bottom_right`,
//! `slope_bottom_left`) carries auto-layer rules that draw the dungeon
//! terrain sheet over it; the tiles saved here are the ones those rules
//! give, worked out the way LDtk works them out, until LDtk saves the
//! project again.

use super::json::{J, obj};

/// The character demo's `PLATFORMER_LEVEL` (examples/demos/character): 30 x
/// 30 cells of 16 px, `#` solid, `=` one-way, `/` and `\` slopes
const PLATFORMER: [&str; 30] = [
  "##############################",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#...................######...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#...#####................#...#",
  "#........................#...#",
  "#..........======........#...#",
  "#........................#...#",
  "#........................#...#",
  "#.................======.#...#",
  "#........................#...#",
  "#............................#",
  "#........====................#",
  "#..../\\......................#",
  "#.../##\\.....................#",
  "#############........#########",
  "#############........#########",
];

/// The vault, to the platformer's right: 20 x 15 cells
const VAULT: [&str; 15] = [
  "####################",
  "#..................#",
  "#..................#",
  "#..................#",
  "#.......====.......#",
  "#..................#",
  "#..................#",
  "#...###......###...#",
  "#..................#",
  "#..................#",
  "#.....======.......#",
  "#..................#",
  "#......./\\.....##..#",
  "####################",
  "####################",
];

const GRID: i64 = 16;
/// The dungeon terrain sheet's columns (images/dungeon-terrain.png)
const TERRAIN_COLUMNS: i64 = 8;
/// Tiny Dungeon's columns
const DUNGEON_COLUMNS: i64 = 12;

// Unique ids: LDtk counts one sequence for every definition
const ENTITIES_LAYER: i64 = 1;
const DECOR_LAYER: i64 = 2;
const COLLISIONS_LAYER: i64 = 3;
const TERRAIN: i64 = 4;
const DUNGEON: i64 = 5;
const FACING: i64 = 6;
const LOOT: i64 = 7;
const FIRST_FREE: i64 = 8;

/// IntGrid values and their names; 1, 3 and 4 are in the `walls` group
const SOLID: i32 = 1;
const ONE_WAY: i32 = 2;
const SLOPE_BR: i32 = 3;
const SLOPE_BL: i32 = 4;
/// The walls group (uid 1) in a rule's pattern, as LDtk writes a group:
/// (group uid + 1) x 1000
const IN: i32 = 2000;
const OUT: i32 = -2000;

fn value_of(c: char) -> i32 {
  match c {
    '#' => SOLID,
    '=' => ONE_WAY,
    '/' => SLOPE_BR,
    '\\' => SLOPE_BL,
    _ => 0,
  }
}

fn group_of(value: i32) -> Option<i32> {
  match value {
    SOLID | SLOPE_BR | SLOPE_BL => Some(1),
    _ => None,
  }
}

/// A stable id in LDtk's form (a version 1 UUID) for `name`
fn iid(name: &str) -> String {
  let hash = |seed: u64| {
    let mut h = seed;
    for byte in name.bytes() {
      h ^= u64::from(byte);
      h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
  };
  let (a, b) = (hash(0xcbf2_9ce4_8422_2325), hash(0x6c62_272e_07bb_0142));
  format!(
    "{:08x}-{:04x}-1{:03x}-{:04x}-{:012x}",
    a >> 32,
    (a >> 16) & 0xffff,
    a & 0xfff,
    0x8000 | (b >> 48) & 0x3fff,
    b & 0xffff_ffff_ffff
  )
}

/// One auto-layer rule: a 3 x 3 pattern around a cell (0 anything, a value,
/// minus a value for "not it", or the walls group) and the terrain tile it
/// draws there
struct Rule {
  uid: i64,
  pattern: [i32; 9],
  tile: i64,
}

impl Rule {
  /// Whether the rule matches at (x, y); cells outside the level read as
  /// solid (the rules' out-of-bounds value)
  fn matches(&self, cells: &[i32], width: usize, x: usize, y: usize) -> bool {
    let height = cells.len() / width;
    self.pattern.iter().enumerate().all(|(i, &wanted)| {
      if wanted == 0 {
        return true;
      }
      let (dx, dy) = (i as i64 % 3 - 1, i as i64 / 3 - 1);
      let (cx, cy) = (x as i64 + dx, y as i64 + dy);
      let value = if (0..width as i64).contains(&cx) && (0..height as i64).contains(&cy) {
        cells[cy as usize * width + cx as usize]
      } else {
        SOLID
      };
      match wanted {
        1_000_001 => value != 0,
        -1_000_001 => value == 0,
        w if w >= 1000 => group_of(value) == Some(w / 1000 - 1),
        w if w <= -1000 => group_of(value) != Some(-w / 1000 - 1),
        w if w > 0 => value == w,
        w => value != -w,
      }
    })
  }
}

/// The pattern with `center` at the middle and the walls group in or out at
/// the eight neighbours named by `cells` (index 0 top-left, row by row)
fn pattern(center: i32, cells: &[(usize, i32)]) -> [i32; 9] {
  let mut out = [0; 9];
  out[4] = center;
  for &(index, value) in cells {
    out[index] = value;
  }
  out
}

// Neighbours in a pattern
const NW: usize = 0;
const N: usize = 1;
const NE: usize = 2;
const W: usize = 3;
const E: usize = 5;
const SW: usize = 6;
const S: usize = 7;
const SE: usize = 8;

/// The rule groups, in LDtk's order (the first match in a cell wins, as
/// every rule breaks on a match): name and rules
fn rule_groups(next: &mut i64) -> Vec<(i64, &'static str, Vec<Rule>)> {
  let mut uid = || {
    *next += 1;
    *next - 1
  };
  let mut group = |name: &'static str, rules: Vec<([i32; 9], i64)>| {
    let group_uid = uid();
    let rules = rules
      .into_iter()
      .map(|(pattern, tile)| Rule {
        uid: uid(),
        pattern,
        tile,
      })
      .collect();
    (group_uid, name, rules)
  };
  let ortho = |n: i32, e: i32, s: i32, w: i32| vec![(N, n), (E, e), (S, s), (W, w)];
  let all_in = ortho(IN, IN, IN, IN);
  let with = |base: &Vec<(usize, i32)>, extra: &[(usize, i32)]| {
    let mut cells = base.clone();
    cells.extend_from_slice(extra);
    cells
  };
  vec![
    group(
      "Slopes",
      vec![(pattern(SLOPE_BR, &[]), 31), (pattern(SLOPE_BL, &[]), 32)],
    ),
    group(
      "Slope feet",
      vec![
        (pattern(SOLID, &[(N, SLOPE_BR)]), 33),
        (pattern(SOLID, &[(N, SLOPE_BL)]), 34),
      ],
    ),
    group(
      "One-way ledges",
      vec![
        (pattern(ONE_WAY, &[(W, -ONE_WAY), (E, -ONE_WAY)]), 30),
        (pattern(ONE_WAY, &[(W, -ONE_WAY)]), 27),
        (pattern(ONE_WAY, &[(E, -ONE_WAY)]), 29),
        (pattern(ONE_WAY, &[]), 28),
      ],
    ),
    group(
      "Walls",
      vec![
        // rock, and its inner corners where a diagonal neighbour is open
        (pattern(SOLID, &with(&all_in, &[(NE, OUT), (SW, OUT)])), 15),
        (pattern(SOLID, &with(&all_in, &[(NW, OUT), (SE, OUT)])), 19),
        (pattern(SOLID, &with(&all_in, &[(NW, OUT)])), 18),
        (pattern(SOLID, &with(&all_in, &[(NE, OUT)])), 16),
        (pattern(SOLID, &with(&all_in, &[(SE, OUT)])), 0),
        (pattern(SOLID, &with(&all_in, &[(SW, OUT)])), 2),
        (pattern(SOLID, &all_in), 5),
        // a rim on each open side
        (pattern(SOLID, &ortho(OUT, IN, IN, IN)), 35),
        (pattern(SOLID, &ortho(IN, IN, OUT, IN)), 1),
        (pattern(SOLID, &ortho(IN, IN, IN, OUT)), 10),
        (pattern(SOLID, &ortho(IN, OUT, IN, IN)), 8),
        (pattern(SOLID, &ortho(OUT, IN, IN, OUT)), 36),
        (pattern(SOLID, &ortho(OUT, OUT, IN, IN)), 37),
        (pattern(SOLID, &ortho(IN, IN, OUT, OUT)), 11),
        (pattern(SOLID, &ortho(IN, OUT, OUT, IN)), 12),
        // walls one cell thick
        (pattern(SOLID, &ortho(OUT, IN, OUT, IN)), 20),
        (pattern(SOLID, &ortho(IN, OUT, IN, OUT)), 21),
        (pattern(SOLID, &ortho(OUT, IN, OUT, OUT)), 23),
        (pattern(SOLID, &ortho(OUT, OUT, OUT, IN)), 24),
        (pattern(SOLID, &ortho(OUT, OUT, IN, OUT)), 25),
        (pattern(SOLID, &ortho(IN, OUT, OUT, OUT)), 26),
        (pattern(SOLID, &ortho(OUT, OUT, OUT, OUT)), 22),
      ],
    ),
  ]
}

/// The auto-layer's tiles for `cells`, as LDtk lists them: by rule, the last
/// rule's first, each rule's cells in order
fn auto_tiles(groups: &[(i64, &str, Vec<Rule>)], cells: &[i32], width: usize) -> Vec<J> {
  let rules: Vec<&Rule> = groups.iter().flat_map(|(_, _, rules)| rules).collect();
  let mut matched: Vec<Option<usize>> = vec![None; cells.len()];
  for (index, slot) in matched.iter_mut().enumerate() {
    let (x, y) = (index % width, index / width);
    *slot = rules
      .iter()
      .position(|rule| rule.matches(cells, width, x, y));
  }
  let mut out = Vec::new();
  for (rule_index, rule) in rules.iter().enumerate().rev() {
    for (index, _) in matched
      .iter()
      .enumerate()
      .filter(|(_, m)| **m == Some(rule_index))
    {
      let (x, y) = ((index % width) as i64, (index / width) as i64);
      out.push(obj([
        ("px", vec![x * GRID, y * GRID].into()),
        (
          "src",
          vec![
            (rule.tile % TERRAIN_COLUMNS) * GRID,
            (rule.tile / TERRAIN_COLUMNS) * GRID,
          ]
          .into(),
        ),
        ("f", 0.into()),
        ("t", rule.tile.into()),
        ("d", vec![rule.uid, index as i64].into()),
        ("a", 1.into()),
      ]));
    }
  }
  out
}

fn rect(tileset: i64, columns: i64, tile: i64) -> J {
  obj([
    ("tilesetUid", tileset.into()),
    ("x", ((tile % columns) * GRID).into()),
    ("y", ((tile / columns) * GRID).into()),
    ("w", GRID.into()),
    ("h", GRID.into()),
  ])
}

/// A field definition's type and how LDtk names it on an instance
#[derive(Clone, Copy)]
enum Kind {
  Int,
  Float,
  Bool,
  Str,
  Text,
  Color,
  Enum(i64, &'static str),
  Points,
  EntityRef,
}

impl Kind {
  fn type_name(self) -> String {
    match self {
      Kind::Int => "Int".into(),
      Kind::Float => "Float".into(),
      Kind::Bool => "Bool".into(),
      Kind::Str => "String".into(),
      Kind::Text => "Multilines".into(),
      Kind::Color => "Color".into(),
      Kind::Enum(_, name) => format!("LocalEnum.{name}"),
      Kind::Points => "Array<Point>".into(),
      Kind::EntityRef => "EntityRef".into(),
    }
  }

  fn def_type(self) -> String {
    match self {
      Kind::Int => "F_Int".into(),
      Kind::Float => "F_Float".into(),
      Kind::Bool => "F_Bool".into(),
      Kind::Str => "F_String".into(),
      Kind::Text => "F_Text".into(),
      Kind::Color => "F_Color".into(),
      Kind::Enum(uid, _) => format!("F_Enum({uid})"),
      Kind::Points => "F_Point".into(),
      Kind::EntityRef => "F_EntityRef".into(),
    }
  }
}

/// A value of a field instance
#[derive(Clone)]
enum Value {
  Int(i64),
  Float(f64),
  Bool(bool),
  Str(String),
  /// `#RRGGBB`
  Color(u32),
  Points(Vec<(i64, i64)>),
  /// The entity, layer and level iids
  Ref(String, String, String),
}

struct Field {
  uid: i64,
  name: &'static str,
  kind: Kind,
  default: Option<Value>,
  /// For an entity reference: the entity it may point at
  refers_to: Option<i64>,
}

fn editor_value(value: &Value) -> J {
  match value {
    Value::Int(i) => obj([("id", "V_Int".into()), ("params", vec![*i].into())]),
    Value::Float(f) => obj([("id", "V_Float".into()), ("params", vec![*f].into())]),
    Value::Bool(b) => obj([("id", "V_Bool".into()), ("params", vec![*b].into())]),
    Value::Str(s) => obj([
      ("id", "V_String".into()),
      ("params", vec![s.clone()].into()),
    ]),
    Value::Color(c) => obj([
      ("id", "V_Int".into()),
      ("params", vec![i64::from(*c)].into()),
    ]),
    Value::Points(_) | Value::Ref(..) => J::Null,
  }
}

fn field_def(field: &Field) -> J {
  let display = match field.kind {
    Kind::Points => "PointPath",
    Kind::EntityRef => "RefLinkBetweenCenters",
    _ => "NameAndValue",
  };
  let (allowed, allowed_uid) = match field.refers_to {
    Some(uid) => ("OnlySpecificEntity", J::Int(uid)),
    None => ("OnlySame", J::Null),
  };
  obj([
    ("identifier", field.name.into()),
    ("doc", J::Null),
    ("__type", field.kind.type_name().into()),
    ("uid", field.uid.into()),
    ("type", field.kind.def_type().into()),
    ("isArray", matches!(field.kind, Kind::Points).into()),
    ("canBeNull", matches!(field.kind, Kind::EntityRef).into()),
    ("arrayMinLength", J::Null),
    ("arrayMaxLength", J::Null),
    ("editorDisplayMode", display.into()),
    ("editorDisplayScale", 1.into()),
    ("editorDisplayPos", "Above".into()),
    ("editorLinkStyle", "StraightArrow".into()),
    ("editorDisplayColor", J::Null),
    ("editorAlwaysShow", false.into()),
    ("editorShowInWorld", true.into()),
    ("editorCutLongValues", true.into()),
    ("editorTextSuffix", J::Null),
    ("editorTextPrefix", J::Null),
    ("useForSmartColor", false.into()),
    ("exportToToc", false.into()),
    ("searchable", false.into()),
    ("min", J::Null),
    ("max", J::Null),
    ("regex", J::Null),
    ("acceptFileTypes", J::Null),
    (
      "defaultOverride",
      field.default.as_ref().map_or(J::Null, editor_value),
    ),
    ("textLanguageMode", J::Null),
    ("symmetricalRef", false.into()),
    ("autoChainRef", true.into()),
    ("allowOutOfLevelRef", true.into()),
    ("allowedRefs", allowed.into()),
    ("allowedRefsEntityUid", allowed_uid),
    ("allowedRefTags", J::Arr(vec![])),
    ("tilesetUid", J::Null),
  ])
}

fn field_instance(field: &Field, value: &Value) -> J {
  let (shown, editor) = match value {
    Value::Int(i) => (J::Int(*i), vec![editor_value(value)]),
    Value::Float(f) => (J::Num(*f), vec![editor_value(value)]),
    // LDtk keeps no editor value for a bool left false
    Value::Bool(b) => (
      J::Bool(*b),
      vec![if *b { editor_value(value) } else { J::Null }],
    ),
    Value::Str(s) => (J::Str(s.clone()), vec![editor_value(value)]),
    Value::Color(c) => (J::Str(format!("#{c:06X}")), vec![editor_value(value)]),
    Value::Points(points) => (
      J::Arr(
        points
          .iter()
          .map(|&(cx, cy)| obj([("cx", cx.into()), ("cy", cy.into())]))
          .collect(),
      ),
      points
        .iter()
        .map(|(cx, cy)| editor_value(&Value::Str(format!("{cx},{cy}"))))
        .collect(),
    ),
    Value::Ref(entity, layer, level) => (
      obj([
        ("entityIid", entity.clone().into()),
        ("layerIid", layer.clone().into()),
        ("levelIid", level.clone().into()),
        ("worldIid", iid("world").into()),
      ]),
      vec![editor_value(&Value::Str(entity.clone()))],
    ),
  };
  obj([
    ("__identifier", field.name.into()),
    ("__type", field.kind.type_name().into()),
    ("__value", shown),
    ("__tile", J::Null),
    ("defUid", field.uid.into()),
    ("realEditorValues", J::Arr(editor)),
  ])
}

struct Entity {
  uid: i64,
  name: &'static str,
  size: (i64, i64),
  pivot: (f64, f64),
  color: &'static str,
  /// The Tiny Dungeon tile it shows, if any
  tile: Option<i64>,
  fields: Vec<Field>,
}

fn entity_def(entity: &Entity) -> J {
  let tile = entity.tile.map(|t| rect(DUNGEON, DUNGEON_COLUMNS, t));
  obj([
    ("identifier", entity.name.into()),
    ("uid", entity.uid.into()),
    ("tags", J::Arr(vec![])),
    ("exportToToc", false.into()),
    ("allowOutOfBounds", false.into()),
    ("doc", J::Null),
    ("width", entity.size.0.into()),
    ("height", entity.size.1.into()),
    ("resizableX", false.into()),
    ("resizableY", false.into()),
    ("minWidth", J::Null),
    ("maxWidth", J::Null),
    ("minHeight", J::Null),
    ("maxHeight", J::Null),
    ("keepAspectRatio", false.into()),
    ("tileOpacity", 1.into()),
    ("fillOpacity", 0.08.into()),
    ("lineOpacity", 1.into()),
    ("hollow", false.into()),
    ("color", entity.color.into()),
    (
      "renderMode",
      if entity.tile.is_some() {
        "Tile"
      } else {
        "Rectangle"
      }
      .into(),
    ),
    ("showName", true.into()),
    ("tilesetId", entity.tile.map(|_| DUNGEON).into()),
    ("tileRenderMode", "FitInside".into()),
    ("tileRect", tile.unwrap_or(J::Null)),
    ("uiTileRect", J::Null),
    ("nineSliceBorders", J::Arr(vec![])),
    ("maxCount", 0.into()),
    ("limitScope", "PerLevel".into()),
    ("limitBehavior", "MoveLastOne".into()),
    ("pivotX", entity.pivot.0.into()),
    ("pivotY", entity.pivot.1.into()),
    (
      "fieldDefs",
      J::Arr(entity.fields.iter().map(field_def).collect()),
    ),
  ])
}

/// The project's entity definitions, their uids taken from `next` on
fn entities(next: &mut i64) -> Vec<Entity> {
  let mut uid = || {
    *next += 1;
    *next - 1
  };
  let mut defs = Vec::new();
  let mut entity = |name: &'static str,
                    size: (i64, i64),
                    pivot: (f64, f64),
                    color: &'static str,
                    tile: Option<i64>,
                    fields: Vec<(&'static str, Kind, Option<Value>)>,
                    uid: &mut dyn FnMut() -> i64| {
    let own = uid();
    let fields = fields
      .into_iter()
      .map(|(name, kind, default)| Field {
        uid: uid(),
        name,
        kind,
        default,
        refers_to: None,
      })
      .collect();
    defs.push(Entity {
      uid: own,
      name,
      size,
      pivot,
      color,
      tile,
      fields,
    });
  };
  let bottom = (0.5, 1.0);
  let middle = (0.5, 0.5);
  entity(
    "Player",
    (14, 26),
    bottom,
    "#63C74D",
    None,
    vec![(
      "facing",
      Kind::Enum(FACING, "Facing"),
      Some(Value::Str("Right".into())),
    )],
    &mut uid,
  );
  entity(
    "Pickup",
    (10, 10),
    middle,
    "#FEE761",
    None,
    vec![("respawn_after", Kind::Float, Some(Value::Float(3.0)))],
    &mut uid,
  );
  entity(
    "Checkpoint",
    (12, 32),
    middle,
    "#2CE8F5",
    None,
    vec![],
    &mut uid,
  );
  entity(
    "Chest",
    (16, 16),
    bottom,
    "#E4A672",
    Some(89),
    vec![
      (
        "loot",
        Kind::Enum(LOOT, "Loot"),
        Some(Value::Str("Gold".into())),
      ),
      ("count", Kind::Int, Some(Value::Int(1))),
      ("locked", Kind::Bool, None),
    ],
    &mut uid,
  );
  entity(
    "Door",
    (16, 16),
    bottom,
    "#B86F50",
    Some(45),
    vec![
      ("switch", Kind::EntityRef, None),
      ("open", Kind::Bool, None),
    ],
    &mut uid,
  );
  entity(
    "Switch",
    (16, 16),
    bottom,
    "#C0CBDC",
    Some(101),
    vec![("on", Kind::Bool, None)],
    &mut uid,
  );
  entity(
    "Sign",
    (16, 16),
    bottom,
    "#EAD4AA",
    Some(66),
    vec![("text", Kind::Text, None)],
    &mut uid,
  );
  entity(
    "Patrol",
    (16, 16),
    bottom,
    "#5DE073",
    Some(108),
    vec![
      ("path", Kind::Points, None),
      ("speed", Kind::Float, Some(Value::Float(30.0))),
    ],
    &mut uid,
  );
  entity(
    "Torch",
    (16, 16),
    middle,
    "#F77622",
    Some(29),
    vec![
      ("color", Kind::Color, Some(Value::Color(0xFF_B3_47))),
      ("radius", Kind::Float, Some(Value::Float(48.0))),
    ],
    &mut uid,
  );
  // the door's switch may only be a switch
  let switch = defs.iter().find(|e| e.name == "Switch").map(|e| e.uid);
  if let Some(door) = defs.iter_mut().find(|e| e.name == "Door") {
    door.fields[0].refers_to = switch;
  }
  defs
}

/// An entity placed in a level: its definition, iid, pixel position (where
/// its pivot lies) and field values by name
struct Placed {
  def: &'static str,
  iid: String,
  px: (i64, i64),
  values: Vec<(&'static str, Value)>,
}

fn entity_instance(defs: &[Entity], placed: &Placed, level_pos: (i64, i64)) -> J {
  let def = defs
    .iter()
    .find(|e| e.name == placed.def)
    .expect("every placed entity is defined");
  let fields: Vec<J> = def
    .fields
    .iter()
    .map(|field| {
      let value = placed
        .values
        .iter()
        .find(|(name, _)| *name == field.name)
        .map(|(_, v)| v.clone())
        .or_else(|| field.default.clone());
      match value {
        Some(value) => field_instance(field, &value),
        // an unset field: null, as LDtk saves one
        None => {
          let shown = match field.kind {
            Kind::Bool => J::Bool(false),
            Kind::Points => J::Arr(vec![]),
            _ => J::Null,
          };
          let editor = match field.kind {
            Kind::Points => J::Arr(vec![]),
            _ => J::Arr(vec![J::Null]),
          };
          obj([
            ("__identifier", field.name.into()),
            ("__type", field.kind.type_name().into()),
            ("__value", shown),
            ("__tile", J::Null),
            ("defUid", field.uid.into()),
            ("realEditorValues", editor),
          ])
        }
      }
    })
    .collect();
  obj([
    ("__identifier", def.name.into()),
    (
      "__grid",
      vec![placed.px.0 / GRID, placed.px.1 / GRID].into(),
    ),
    ("__pivot", vec![def.pivot.0, def.pivot.1].into()),
    ("__tags", J::Arr(vec![])),
    (
      "__tile",
      def
        .tile
        .map_or(J::Null, |t| rect(DUNGEON, DUNGEON_COLUMNS, t)),
    ),
    ("__smartColor", def.color.into()),
    ("iid", placed.iid.clone().into()),
    ("width", def.size.0.into()),
    ("height", def.size.1.into()),
    ("defUid", def.uid.into()),
    ("px", vec![placed.px.0, placed.px.1].into()),
    ("fieldInstances", J::Arr(fields)),
    ("__worldX", (level_pos.0 + placed.px.0).into()),
    ("__worldY", (level_pos.1 + placed.px.1).into()),
  ])
}

fn layer_def(uid: i64, kind: &str, name: &str, tileset: Option<i64>, extra: Vec<(&str, J)>) -> J {
  let mut pairs = vec![
    ("__type", J::from(kind)),
    ("identifier", name.into()),
    ("type", kind.into()),
    ("uid", uid.into()),
    ("doc", J::Null),
    ("uiColor", J::Null),
    ("gridSize", GRID.into()),
    ("guideGridWid", 0.into()),
    ("guideGridHei", 0.into()),
    ("displayOpacity", 1.into()),
    ("inactiveOpacity", 1.into()),
    ("hideInList", false.into()),
    ("hideFieldsWhenInactive", false.into()),
    ("canSelectWhenInactive", true.into()),
    ("renderInWorldView", true.into()),
    ("pxOffsetX", 0.into()),
    ("pxOffsetY", 0.into()),
    ("parallaxFactorX", 0.into()),
    ("parallaxFactorY", 0.into()),
    ("parallaxScaling", true.into()),
    ("requiredTags", J::Arr(vec![])),
    ("excludedTags", J::Arr(vec![])),
    ("autoTilesKilledByOtherLayerUid", J::Null),
    ("uiFilterTags", J::Arr(vec![])),
    ("useAsyncRender", false.into()),
  ];
  let mut values = J::Arr(vec![]);
  let mut groups_of_values = J::Arr(vec![]);
  let mut rules = J::Arr(vec![]);
  for (key, value) in extra {
    match key {
      "intGridValues" => values = value,
      "intGridValuesGroups" => groups_of_values = value,
      "autoRuleGroups" => rules = value,
      _ => {}
    }
  }
  pairs.extend([
    ("intGridValues", values),
    ("intGridValuesGroups", groups_of_values),
    ("autoRuleGroups", rules),
    ("autoSourceLayerDefUid", J::Null),
    ("tilesetDefUid", tileset.into()),
    ("tilePivotX", 0.into()),
    ("tilePivotY", 0.into()),
    ("biomeFieldUid", J::Null),
  ]);
  J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn rule_json(rule: &Rule) -> J {
  obj([
    ("uid", rule.uid.into()),
    ("active", true.into()),
    ("size", 3.into()),
    ("tileRectsIds", J::Arr(vec![vec![rule.tile].into()])),
    ("alpha", 1.into()),
    ("chance", 1.into()),
    ("breakOnMatch", true.into()),
    ("pattern", rule.pattern.to_vec().into()),
    ("flipX", false.into()),
    ("flipY", false.into()),
    ("xModulo", 1.into()),
    ("yModulo", 1.into()),
    ("xOffset", 0.into()),
    ("yOffset", 0.into()),
    ("tileXOffset", 0.into()),
    ("tileYOffset", 0.into()),
    ("tileRandomXMin", 0.into()),
    ("tileRandomXMax", 0.into()),
    ("tileRandomYMin", 0.into()),
    ("tileRandomYMax", 0.into()),
    ("checker", "None".into()),
    ("tileMode", "Single".into()),
    ("pivotX", 0.into()),
    ("pivotY", 0.into()),
    ("outOfBoundsValue", SOLID.into()),
    ("invalidated", false.into()),
    ("perlinActive", false.into()),
    ("perlinSeed", 2_004_011.into()),
    ("perlinScale", 0.2.into()),
    ("perlinOctaves", 2.into()),
  ])
}

fn tileset_def(uid: i64, name: &str, path: &str, size: (i64, i64)) -> J {
  obj([
    ("__cWid", (size.0 / GRID).into()),
    ("__cHei", (size.1 / GRID).into()),
    ("identifier", name.into()),
    ("uid", uid.into()),
    ("relPath", path.into()),
    ("embedAtlas", J::Null),
    ("pxWid", size.0.into()),
    ("pxHei", size.1.into()),
    ("tileGridSize", GRID.into()),
    ("spacing", 0.into()),
    ("padding", 0.into()),
    ("tags", J::Arr(vec![])),
    ("tagsSourceEnumUid", J::Null),
    ("enumTags", J::Arr(vec![])),
    ("customData", J::Arr(vec![])),
    ("savedSelections", J::Arr(vec![])),
    ("cachedPixelData", J::Null),
  ])
}

fn enum_def(uid: i64, name: &str, values: &[(&str, Option<i64>, u32)]) -> J {
  obj([
    ("identifier", name.into()),
    ("uid", uid.into()),
    (
      "values",
      J::Arr(
        values
          .iter()
          .map(|(id, tile, color)| {
            obj([
              ("id", (*id).into()),
              (
                "tileRect",
                tile.map_or(J::Null, |t| rect(DUNGEON, DUNGEON_COLUMNS, t)),
              ),
              ("color", (*color).into()),
            ])
          })
          .collect(),
      ),
    ),
    ("iconTilesetUid", DUNGEON.into()),
    ("externalRelPath", J::Null),
    ("externalFileChecksum", J::Null),
    ("tags", J::Arr(vec![])),
  ])
}

/// A level: its name, rows, place in the world, title, entities and decor
/// tiles (Tiny Dungeon tile and cell)
struct Level {
  name: &'static str,
  uid: i64,
  rows: &'static [&'static str],
  at: (i64, i64),
  title: &'static str,
  entities: Vec<Placed>,
  decor: Vec<(i64, (i64, i64))>,
}

impl Level {
  fn size(&self) -> (i64, i64) {
    (self.rows[0].len() as i64, self.rows.len() as i64)
  }

  fn cells(&self) -> Vec<i32> {
    self
      .rows
      .iter()
      .flat_map(|row| row.chars().map(value_of))
      .collect()
  }
}

/// The side of `level` that `other` touches along an edge, as LDtk names it
/// (`n`, `s`, `e`, `w`), if they touch along one
fn side(level: &Level, other: &Level) -> Option<&'static str> {
  let (w, h) = level.size();
  let (ow, oh) = other.size();
  let (x0, y0, x1, y1) = (
    level.at.0,
    level.at.1,
    level.at.0 + w * GRID,
    level.at.1 + h * GRID,
  );
  let (ox0, oy0, ox1, oy1) = (
    other.at.0,
    other.at.1,
    other.at.0 + ow * GRID,
    other.at.1 + oh * GRID,
  );
  let overlap_y = oy0 < y1 && y0 < oy1;
  let overlap_x = ox0 < x1 && x0 < ox1;
  if ox0 == x1 && overlap_y {
    Some("e")
  } else if ox1 == x0 && overlap_y {
    Some("w")
  } else if oy0 == y1 && overlap_x {
    Some("s")
  } else if oy1 == y0 && overlap_x {
    Some("n")
  } else {
    None
  }
}

fn level_json(
  level: &Level,
  levels: &[Level],
  defs: &[Entity],
  groups: &[(i64, &str, Vec<Rule>)],
  title_field: &Field,
) -> J {
  let (w, h) = level.size();
  let level_iid = iid(&format!("level/{}", level.name));
  let layer_iid = |name: &str| iid(&format!("level/{}/{name}", level.name));
  let cells = level.cells();
  let instance =
    |name: &str, kind: &str, def: i64, tileset: Option<(i64, &str)>, rest: Vec<(&str, J)>| {
      let mut pairs = vec![
        ("__identifier", J::from(name)),
        ("__type", kind.into()),
        ("__cWid", w.into()),
        ("__cHei", h.into()),
        ("__gridSize", GRID.into()),
        ("__opacity", 1.into()),
        ("__pxTotalOffsetX", 0.into()),
        ("__pxTotalOffsetY", 0.into()),
        ("__tilesetDefUid", tileset.map(|t| t.0).into()),
        ("__tilesetRelPath", tileset.map(|t| t.1).into()),
        ("iid", layer_iid(name).into()),
        ("levelId", level.uid.into()),
        ("layerDefUid", def.into()),
        ("pxOffsetX", 0.into()),
        ("pxOffsetY", 0.into()),
        ("visible", true.into()),
        ("optionalRules", J::Arr(vec![])),
      ];
      let mut csv = J::Arr(vec![]);
      let mut auto = J::Arr(vec![]);
      let mut grid = J::Arr(vec![]);
      let mut placed = J::Arr(vec![]);
      for (key, value) in rest {
        match key {
          "intGridCsv" => csv = value,
          "autoLayerTiles" => auto = value,
          "gridTiles" => grid = value,
          "entityInstances" => placed = value,
          _ => {}
        }
      }
      // a fixed seed per layer, as LDtk keeps one
      let seed = i64::from_str_radix(&layer_iid(name)[..6], 16).unwrap_or(0);
      pairs.extend([
        ("intGridCsv", csv),
        ("autoLayerTiles", auto),
        ("seed", seed.into()),
        ("overrideTilesetUid", J::Null),
        ("gridTiles", grid),
        ("entityInstances", placed),
      ]);
      J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    };
  let entities = instance(
    "Entities",
    "Entities",
    ENTITIES_LAYER,
    None,
    vec![(
      "entityInstances",
      J::Arr(
        level
          .entities
          .iter()
          .map(|placed| entity_instance(defs, placed, level.at))
          .collect(),
      ),
    )],
  );
  let decor_tiles: Vec<J> = level
    .decor
    .iter()
    .map(|&(tile, (cx, cy))| {
      obj([
        ("px", vec![cx * GRID, cy * GRID].into()),
        (
          "src",
          vec![
            (tile % DUNGEON_COLUMNS) * GRID,
            (tile / DUNGEON_COLUMNS) * GRID,
          ]
          .into(),
        ),
        ("f", 0.into()),
        ("t", tile.into()),
        ("d", vec![cx + cy * w].into()),
        ("a", 1.into()),
      ])
    })
    .collect();
  let decor = instance(
    "Decor",
    "Tiles",
    DECOR_LAYER,
    Some((DUNGEON, "../kenney-tiny-dungeon/tilemap_packed.png")),
    vec![("gridTiles", J::Arr(decor_tiles))],
  );
  let collisions = instance(
    "Collisions",
    "IntGrid",
    COLLISIONS_LAYER,
    Some((TERRAIN, "../images/dungeon-terrain.png")),
    vec![
      ("intGridCsv", cells.clone().into()),
      (
        "autoLayerTiles",
        J::Arr(auto_tiles(groups, &cells, w as usize)),
      ),
    ],
  );
  let neighbours: Vec<J> = levels
    .iter()
    .filter(|other| other.name != level.name)
    .filter_map(|other| {
      side(level, other).map(|dir| {
        obj([
          ("levelIid", iid(&format!("level/{}", other.name)).into()),
          ("dir", dir.into()),
        ])
      })
    })
    .collect();
  obj([
    ("identifier", level.name.into()),
    ("iid", level_iid.into()),
    ("uid", level.uid.into()),
    ("worldX", level.at.0.into()),
    ("worldY", level.at.1.into()),
    ("worldDepth", 0.into()),
    ("pxWid", (w * GRID).into()),
    ("pxHei", (h * GRID).into()),
    ("__bgColor", BACKGROUND.into()),
    ("bgColor", J::Null),
    ("useAutoIdentifier", false.into()),
    ("bgRelPath", J::Null),
    ("bgPos", J::Null),
    ("bgPivotX", 0.5.into()),
    ("bgPivotY", 0.5.into()),
    ("__smartColor", "#6E6D7B".into()),
    ("__bgPos", J::Null),
    ("externalRelPath", J::Null),
    (
      "fieldInstances",
      J::Arr(vec![field_instance(
        title_field,
        &Value::Str(level.title.into()),
      )]),
    ),
    ("layerInstances", J::Arr(vec![entities, decor, collisions])),
    ("__neighbours", J::Arr(neighbours)),
  ])
}

const BACKGROUND: &str = "#25131A";

fn placed(
  level: &str,
  def: &'static str,
  n: usize,
  px: (i64, i64),
  values: Vec<(&'static str, Value)>,
) -> Placed {
  Placed {
    def,
    iid: iid(&format!("level/{level}/{def}/{n}")),
    px,
    values,
  }
}

/// The levels and what is placed in them
fn levels(level_uids: (i64, i64)) -> Vec<Level> {
  // The character demo's pickups and checkpoints, by (column, row) as its
  // `tile_top` takes them: a pickup's centre 12 px above the cell's top
  // edge, a checkpoint's 16 px
  let pickups = [
    (5.5, 26.0),
    (10.5, 25.0),
    (15.5, 27.0),
    (20.5, 22.0),
    (13.5, 19.0),
    (5.0, 17.0),
    (27.0, 18.0),
    (27.0, 13.0),
    (21.0, 10.0),
  ];
  let checkpoints = [(7.0, 17.0), (23.5, 10.0)];
  let at = |column: f64, row: f64, up: i64| {
    (
      ((column + 0.5) * GRID as f64) as i64,
      (row * GRID as f64) as i64 - up,
    )
  };
  let mut platformer = vec![placed("Platformer", "Player", 0, at(9.5, 28.0, 0), vec![])];
  for (n, &(column, row)) in pickups.iter().enumerate() {
    platformer.push(placed(
      "Platformer",
      "Pickup",
      n,
      at(column, row, 12),
      vec![],
    ));
  }
  for (n, &(column, row)) in checkpoints.iter().enumerate() {
    platformer.push(placed(
      "Platformer",
      "Checkpoint",
      n,
      at(column, row, 16),
      vec![],
    ));
  }

  let cell = |cx: i64, cy: i64| (cx * GRID + GRID / 2, (cy + 1) * GRID);
  let switch_iid = iid("level/Vault/Switch/0");
  let entities_layer = iid("level/Vault/Entities");
  let vault_iid = iid("level/Vault");
  let vault = vec![
    placed(
      "Vault",
      "Chest",
      0,
      cell(3, 12),
      vec![
        ("loot", Value::Str("Gold".into())),
        ("count", Value::Int(25)),
      ],
    ),
    placed(
      "Vault",
      "Chest",
      1,
      cell(5, 6),
      vec![
        ("loot", Value::Str("Potion".into())),
        ("locked", Value::Bool(true)),
      ],
    ),
    placed(
      "Vault",
      "Door",
      0,
      cell(18, 12),
      vec![("switch", Value::Ref(switch_iid, entities_layer, vault_iid))],
    ),
    placed("Vault", "Switch", 0, cell(10, 3), vec![]),
    placed(
      "Vault",
      "Sign",
      0,
      cell(1, 12),
      vec![(
        "text",
        Value::Str("The vault keeps its gold\nbehind the door.".into()),
      )],
    ),
    placed(
      "Vault",
      "Patrol",
      0,
      cell(2, 12),
      vec![
        (
          "path",
          Value::Points(vec![(2, 12), (6, 12), (10, 12), (14, 12)]),
        ),
        ("speed", Value::Float(40.0)),
      ],
    ),
    placed("Vault", "Torch", 0, (24, 88), vec![]),
    placed(
      "Vault",
      "Torch",
      1,
      (296, 88),
      vec![("radius", Value::Float(64.0))],
    ),
  ];
  vec![
    Level {
      name: "Platformer",
      uid: level_uids.0,
      rows: &PLATFORMER,
      at: (0, 0),
      title: "Hills and a shaft",
      entities: platformer,
      decor: vec![],
    },
    Level {
      name: "Vault",
      uid: level_uids.1,
      rows: &VAULT,
      at: (480, 240),
      title: "The Vault",
      entities: vault,
      // a barrel and a bookcase on the floor
      decor: vec![(82, (12, 12)), (63, (13, 12))],
    },
  ]
}

/// The project, as LDtk prints it
pub fn project() -> String {
  let mut next = FIRST_FREE;
  let defs = entities(&mut next);
  let title = Field {
    uid: next,
    name: "title",
    kind: Kind::Str,
    default: None,
    refers_to: None,
  };
  next += 1;
  let groups = rule_groups(&mut next);
  let level_uids = (next, next + 1);
  next += 2;
  let levels = levels(level_uids);

  let int_grid_values = J::Arr(
    [
      (SOLID, "solid", "#8B9BB4", Some(5)),
      (ONE_WAY, "one_way", "#C0CBDC", Some(28)),
      (SLOPE_BR, "slope_bottom_right", "#E4A672", Some(31)),
      (SLOPE_BL, "slope_bottom_left", "#B86F50", Some(32)),
    ]
    .into_iter()
    .map(|(value, name, color, tile)| {
      obj([
        ("value", value.into()),
        ("identifier", name.into()),
        ("color", color.into()),
        (
          "tile",
          tile.map_or(J::Null, |t| rect(TERRAIN, TERRAIN_COLUMNS, t)),
        ),
        ("groupUid", group_of(value).unwrap_or(0).into()),
      ])
    })
    .collect(),
  );
  let rule_groups_json = J::Arr(
    groups
      .iter()
      .map(|(uid, name, rules)| {
        obj([
          ("uid", (*uid).into()),
          ("name", (*name).into()),
          ("color", J::Null),
          ("icon", J::Null),
          ("active", true.into()),
          ("isOptional", false.into()),
          ("usesWizard", false.into()),
          ("requiredBiomeValues", J::Arr(vec![])),
          ("biomeRequirementMode", 0.into()),
          ("rules", J::Arr(rules.iter().map(rule_json).collect())),
        ])
      })
      .collect(),
  );
  let layers = J::Arr(vec![
    layer_def(ENTITIES_LAYER, "Entities", "Entities", None, vec![]),
    layer_def(DECOR_LAYER, "Tiles", "Decor", Some(DUNGEON), vec![]),
    layer_def(
      COLLISIONS_LAYER,
      "IntGrid",
      "Collisions",
      Some(TERRAIN),
      vec![
        ("intGridValues", int_grid_values),
        (
          "intGridValuesGroups",
          J::Arr(vec![obj([
            ("uid", 1.into()),
            ("identifier", "walls".into()),
            ("color", J::Null),
          ])]),
        ),
        ("autoRuleGroups", rule_groups_json),
      ],
    ),
  ]);
  let defs_json = obj([
    ("layers", layers),
    ("entities", J::Arr(defs.iter().map(entity_def).collect())),
    (
      "tilesets",
      J::Arr(vec![
        tileset_def(
          TERRAIN,
          "Terrain",
          "../images/dungeon-terrain.png",
          (128, 80),
        ),
        tileset_def(
          DUNGEON,
          "Tiny_dungeon",
          "../kenney-tiny-dungeon/tilemap_packed.png",
          (192, 176),
        ),
      ]),
    ),
    (
      "enums",
      J::Arr(vec![
        enum_def(
          FACING,
          "Facing",
          &[("Left", None, 0x63C74D), ("Right", None, 0x3E8948)],
        ),
        enum_def(
          LOOT,
          "Loot",
          &[
            ("Gold", Some(89), 0xFEE761),
            ("Potion", Some(115), 0xE43B44),
            ("Key", None, 0xC0CBDC),
          ],
        ),
      ]),
    ),
    ("externalEnums", J::Arr(vec![])),
    ("levelFields", J::Arr(vec![field_def(&title)])),
  ]);
  let levels_json = J::Arr(
    levels
      .iter()
      .map(|level| level_json(level, &levels, &defs, &groups, &title))
      .collect(),
  );
  let root = obj([
    (
      "__header__",
      obj([
        ("fileType", "LDtk Project JSON".into()),
        ("app", "LDtk".into()),
        ("doc", "https://ldtk.io/json".into()),
        ("schema", "https://ldtk.io/files/JSON_SCHEMA.json".into()),
        ("appAuthor", "Sebastien 'deepnight' Benard".into()),
        ("appVersion", "1.5.3".into()),
        ("url", "https://ldtk.io".into()),
      ]),
    ),
    ("iid", iid("project").into()),
    ("jsonVersion", "1.5.3".into()),
    ("appBuildId", 473_702.into()),
    ("nextUid", next.into()),
    ("identifierStyle", "Free".into()),
    ("toc", J::Arr(vec![])),
    ("worldLayout", "Free".into()),
    ("worldGridWidth", 256.into()),
    ("worldGridHeight", 256.into()),
    ("defaultLevelWidth", 320.into()),
    ("defaultLevelHeight", 240.into()),
    ("defaultPivotX", 0.into()),
    ("defaultPivotY", 0.into()),
    ("defaultGridSize", GRID.into()),
    ("defaultEntityWidth", GRID.into()),
    ("defaultEntityHeight", GRID.into()),
    ("bgColor", "#40465B".into()),
    ("defaultLevelBgColor", BACKGROUND.into()),
    ("minifyJson", false.into()),
    ("externalLevels", false.into()),
    ("exportTiled", false.into()),
    ("simplifiedExport", false.into()),
    ("imageExportMode", "None".into()),
    ("exportLevelBg", true.into()),
    ("pngFilePattern", J::Null),
    ("backupOnSave", false.into()),
    ("backupLimit", 10.into()),
    ("backupRelPath", J::Null),
    ("levelNamePattern", "Level_%idx".into()),
    ("tutorialDesc", J::Null),
    ("customCommands", J::Arr(vec![])),
    (
      "flags",
      vec!["ExportOldTableOfContentData", "UseMultilinesType"].into(),
    ),
    ("defs", defs_json),
    ("levels", levels_json),
    ("worlds", J::Arr(vec![])),
    ("dummyWorldIid", iid("world").into()),
  ]);
  super::json::ldtk(&root)
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The Walls rules give every solid cell exactly one tile, and a cell
  /// whose eight neighbours are all solid is plain rock
  #[test]
  fn every_filled_cell_takes_a_tile() {
    let mut next = 100;
    let groups = rule_groups(&mut next);
    let cells: Vec<i32> = PLATFORMER
      .iter()
      .flat_map(|row| row.chars().map(value_of))
      .collect();
    let tiles = auto_tiles(&groups, &cells, 30);
    let filled = cells.iter().filter(|v| **v != 0).count();
    assert_eq!(tiles.len(), filled);
  }

  #[test]
  fn iids_are_stable_version_one_uuids() {
    let id = iid("level/Vault");
    assert_eq!(id, iid("level/Vault"));
    assert_ne!(id, iid("level/Platformer"));
    assert_eq!(id.len(), 36);
    assert_eq!(&id[14..15], "1");
  }
}
