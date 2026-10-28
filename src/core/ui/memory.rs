//! What the UI remembers between frames: last frame's laid-out records (for
//! hit testing and navigation), widgets' own state and per-scope focus

use std::any::{Any, TypeId};
use std::ops::{BitOr, BitOrAssign};
use std::sync::Arc;

use glam::Vec2;

use super::fragment::{CaptureLog, Fragment, FragmentStats, Fragments, Role};
use super::id::{IdScopes, TapeLog, UiId};
use super::layout::engine::NodeDesc;
use super::layout::{Measure, UiStyle};
use super::tape::Place;
#[cfg(debug_assertions)]
use super::tape::Tape;
use super::tree::{Keeping, Node, Placing, Subtree};
use super::types::Rect;
use super::widgets::drag::DragState;
#[cfg(not(target_arch = "wasm32"))]
use super::widgets::state_debug;
#[cfg(debug_assertions)]
use crate::core::ecs::kernel::Entity;
use crate::core::hash::FastMap;
use crate::core::input::Direction;

/// What a widget senses and how navigation treats it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Flags(u32);

impl Flags {
  pub(crate) const CLICK: Flags = Flags(1 << 0);
  pub(crate) const DRAG: Flags = Flags(1 << 1);
  pub(crate) const FOCUSABLE: Flags = Flags(1 << 2);
  pub(crate) const BLOCKING: Flags = Flags(1 << 3);
  pub(crate) const SCROLLABLE: Flags = Flags(1 << 4);
  pub(crate) const DROP_TARGET: Flags = Flags(1 << 5);
  pub(crate) const DISABLED: Flags = Flags(1 << 6);
  pub(crate) const TAKES_HORIZONTAL: Flags = Flags(1 << 7);
  pub(crate) const TAKES_VERTICAL: Flags = Flags(1 << 8);
  pub(crate) const FOCUS_SCOPE: Flags = Flags(1 << 9);
  pub(crate) const TRAP: Flags = Flags(1 << 10);
  pub(crate) const WRAP: Flags = Flags(1 << 11);
  pub(crate) const MODAL: Flags = Flags(1 << 12);
  pub(crate) const TEXT_FIELD: Flags = Flags(1 << 13);
  pub(crate) const DEFAULT_FOCUS: Flags = Flags(1 << 14);
  pub(crate) const USES_BACK: Flags = Flags(1 << 15);
  /// Carries a drag payload: Confirm picks it up rather than clicking it
  pub(crate) const DRAG_SOURCE: Flags = Flags(1 << 16);
  pub(crate) const NAV_SKIP: Flags = Flags(1 << 17);
  /// Reports hovering only: never hot, blocks nothing
  pub(crate) const HOVER: Flags = Flags(1 << 18);
  /// The root of a popup that is not a modal (a dropdown's list, a
  /// `ui.popup`): while it traps the focus, the UI wants the keyboard
  pub(crate) const POPUP: Flags = Flags(1 << 19);
  /// A rebinding row waiting for a press: navigation stands aside while it
  /// has the focus, so the arrows, Confirm and Back can be captured
  pub(crate) const CAPTURING: Flags = Flags(1 << 20);
  /// Declines Confirm (`Custom::takes_confirm(false)`): while it has the
  /// focus, Confirm activates nothing and its keys stay gameplay's
  pub(crate) const NO_CONFIRM: Flags = Flags(1 << 21);

  /// Every flag in `other` is set
  pub(crate) fn contains(self, other: Flags) -> bool {
    self.0 & other.0 == other.0
  }

  /// At least one flag in `other` is set
  pub(crate) fn intersects(self, other: Flags) -> bool {
    self.0 & other.0 != 0
  }
}

impl BitOr for Flags {
  type Output = Flags;

  fn bitor(self, rhs: Flags) -> Flags {
    Flags(self.0 | rhs.0)
  }
}

impl BitOrAssign for Flags {
  fn bitor_assign(&mut self, rhs: Flags) {
    self.0 |= rhs.0;
  }
}

/// One widget as laid out in a frame
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Record {
  pub(crate) rect: Rect,
  pub(crate) clip: Option<Rect>,
  pub(crate) layer: u8,
  pub(crate) order: u32,
  pub(crate) depth: u16,
  pub(crate) scene: u64,
  pub(crate) flags: Flags,
  /// Nearest focus-scope ancestor (`NONE` outside scopes)
  pub(crate) scope: UiId,
  /// Nearest scrollable ancestor (`NONE`)
  pub(crate) scroll: UiId,
  /// Scrollables: content size
  pub(crate) content: Vec2,
  /// Explicit Up, Down, Left, Right neighbours (`NONE`)
  pub(crate) nav: [UiId; 4],
  /// A drop target: the payload type it takes (`None` otherwise)
  pub(crate) accepts: Option<TypeId>,
}

/// A sound the UI raises; the engine plays the theme's sound of that kind
/// after the UI pass
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum UiSound {
  /// Focus moved by navigation
  Focus,
  /// Confirm activated a widget, or a button was clicked
  Confirm,
  /// Back closed or cancelled something
  Back,
  /// A slider stepped
  Tick,
  /// A drop was refused, or a rebind cancelled
  Error,
}

impl UiSound {
  /// Every kind
  pub(crate) const ALL: [UiSound; 5] = [
    UiSound::Focus,
    UiSound::Confirm,
    UiSound::Back,
    UiSound::Tick,
    UiSound::Error,
  ];
}

/// The least time between two slider ticks, in seconds
const TICK_INTERVAL: f64 = 0.05;

/// The pointer's interaction, resolved once per engine frame against last
/// frame's records (`interact::resolve_pointer`)
#[derive(Debug, Clone, Default)]
pub(crate) struct Interaction {
  /// The topmost sensing widget under the pointer (`NONE`)
  pub(crate) hot: UiId,
  /// The topmost record under the pointer, hover-only widgets included:
  /// what reports `hovered()` (`NONE`)
  pub(crate) hovered: UiId,
  /// The widget a press started on, until that button's release (`NONE`)
  pub(crate) active: UiId,
  /// The button that made it active: 0 left, 1 right
  pub(crate) active_button: u8,
  /// Where that press was, in UI units
  pub(crate) press_pos: Vec2,
  /// The pointer this frame, in UI units
  pub(crate) pointer: Vec2,
  /// The pointer last frame
  pub(crate) prev_pointer: Vec2,
  /// The real time of this frame's resolve, in seconds: what widgets time
  /// their own state by (a rebinding row's capture)
  pub(crate) now: f64,
  /// The active widget's press moved past the drag threshold
  pub(crate) dragging: bool,
  /// This frame's left click (`NONE`)
  pub(crate) clicked: UiId,
  /// This frame's right click
  pub(crate) secondary: UiId,
  /// This frame's double click
  pub(crate) double: UiId,
  /// The last left click's widget and time, for double clicks
  pub(crate) last_click: (UiId, f64),
  /// The widget whose drag started this frame
  pub(crate) drag_started: UiId,
  /// The widget whose drag ended this frame
  pub(crate) drag_released: UiId,
  /// The pointer is over a widget or panel of the top scene
  pub(crate) over_ui: bool,
  /// A left or right press went down this frame (wherever it was)
  pub(crate) pressed: bool,
  /// What this frame's press landed on: the topmost record under it that
  /// takes the pointer, hot or not (`NONE` without a press, or over nothing)
  pub(crate) press_target: UiId,

  // Focus and navigation (`nav::resolve_focus`)
  /// The widget with the keyboard and gamepad focus (`NONE`)
  pub(crate) focused: UiId,
  /// The widget that took the focus this frame (`NONE`)
  pub(crate) gained: UiId,
  /// The widget that lost the focus this frame (`NONE`)
  pub(crate) lost: UiId,
  /// The widget Confirm activated this frame: it reports `clicked()`
  pub(crate) activated: UiId,
  /// A direction the focused widget takes this frame (a slider, an open list)
  pub(crate) nav_step: Option<(UiId, Direction)>,
  /// Back was pressed this frame
  pub(crate) back_pressed: bool,
  /// The focused widget used this frame's Back (`USES_BACK`)
  pub(crate) back_used: bool,
  /// A scene took this frame's Back with `ui.back()`
  pub(crate) back_taken: bool,
  /// The focus ring shows: the last device is the keyboard or a gamepad
  pub(crate) focus_visible: bool,
  /// -1, 0 or 1 from `ui.tab_prev` / `ui.tab_next` while a scope is shown
  pub(crate) tab_step: i8,
  /// A tabs widget took `tab_step` this frame
  pub(crate) tab_taken: bool,
  /// Per direction (Up, Down, Left, Right), the seconds until a held
  /// direction fires again; `Some` while the UI has it captured (its press
  /// was used and it is still held), `None` otherwise
  pub(crate) repeat: [Option<f32>; 4],
  /// The top scene the last focus resolve ran for
  pub(crate) nav_scene: Option<u64>,

  // Drag and drop (`widgets/drag.rs`)
  /// The payload being dragged, from its source's pick-up until it is
  /// dropped or cancelled
  pub(crate) drag: Option<DragState>,
  /// This frame's drop: the target and the payload (cleared by the next
  /// pointer resolve)
  pub(crate) dropped: Option<(UiId, Arc<dyn Any + Send + Sync>)>,
  /// The drop target a mouse drag would land on if released now (`NONE`)
  pub(crate) drop_hover: UiId,

  // Sounds (`UiSounds`)
  /// This frame's sound events, at most one of each kind
  pub(crate) sounds: Vec<UiSound>,
  /// When the last slider tick sounded (`now`)
  pub(crate) last_tick: Option<f64>,
}

impl Interaction {
  /// Raise `sound` for this frame (once per kind)
  pub(crate) fn play(&mut self, sound: UiSound) {
    if !self.sounds.contains(&sound) {
      self.sounds.push(sound);
    }
  }

  /// Raise a slider tick, unless one sounded in the last 0.05 s
  pub(crate) fn tick(&mut self) {
    if self
      .last_tick
      .is_none_or(|last| self.now - last >= TICK_INTERVAL || self.now < last)
    {
      self.last_tick = Some(self.now);
      self.play(UiSound::Tick);
    }
  }

  /// Every id that holds an interaction role this frame: hot, hovered,
  /// pressed, clicked (left, right, double), dragged, pressed on, focused
  /// (held, gained, lost), activated, stepped, dropped on or over, and the
  /// held payload's source. A widget draws each, and its exit from each.
  pub(crate) fn roles(&self) -> Roles {
    // One slot per role: a role added here without growing `Roles` does
    // not compile
    Roles([
      self.hot,
      self.hovered,
      self.active,
      self.clicked,
      self.secondary,
      self.double,
      self.drag_started,
      self.drag_released,
      self.press_target,
      self.focused,
      self.gained,
      self.lost,
      self.activated,
      self.nav_step.map_or(UiId::NONE, |(id, _)| id),
      self.drop_hover,
      self.dropped.as_ref().map_or(UiId::NONE, |(id, _)| *id),
      self.drag.as_ref().map_or(UiId::NONE, |drag| drag.source),
    ])
  }
}

/// The ids holding an interaction role in one frame
/// ([`Interaction::roles`]), one slot per role (`NONE` when no widget holds
/// it): a fixed array, so taking them allocates nothing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Roles([UiId; 17]);

impl Roles {
  /// Whether `id` holds a role (never `NONE`)
  #[cfg(test)]
  pub(crate) fn contains(&self, id: UiId) -> bool {
    id != UiId::NONE && self.0.contains(&id)
  }

  /// The ids holding a role, a repeated one as often as it holds one
  pub(crate) fn iter(&self) -> impl Iterator<Item = UiId> + '_ {
    self.0.iter().copied().filter(|&id| id != UiId::NONE)
  }
}

struct Slot {
  scene: u64,
  seen: u64,
  value: Box<dyn Any>,
}

/// Buffers kept from one subtree, and one frame, to the next: the subtrees'
/// node and draw lists and tree resolve's vectors, cleared at each use with
/// their capacity kept
#[derive(Default)]
pub(crate) struct Scratch {
  /// Subtrees given back, empty
  subtrees: Vec<Subtree>,
  /// The layout's node descriptions; empty between uses, since they borrow
  /// the subtree being resolved
  pub(crate) descs: Vec<NodeDesc<'static>>,
  /// The style out of its parent's flow of each node placed at a rect, by
  /// its index, in node order
  pub(crate) floating: Vec<(usize, UiStyle)>,
  /// Every node's children, run by run in node order
  pub(crate) children: Vec<usize>,
  /// Where each node's run of `children` ends
  pub(crate) child_ends: Vec<usize>,
  /// Where resolve put each node: shifts, rects, clips and slots
  pub(crate) placing: Placing,
  /// What resolve works in to copy, resolve again and capture fragments
  pub(crate) keeping: Keeping,
  /// The subtree index of each node a replay pushed, by its place in the
  /// tape; a replay inside another stacks its own after the outer's
  pub(crate) replayed: Vec<usize>,
  /// Where each node the open tapes pushed went; a tape started inside
  /// another stacks its own after the outer's
  pub(crate) places: Vec<Place>,
  /// The capture logs: the first `captures_used` are this pass's, in the
  /// order they began; the rest are kept for their buffers
  captures: Vec<CaptureLog>,
  captures_used: usize,
  /// The last of this pass's capture logs is open
  capturing: bool,
}

impl Scratch {
  /// An empty subtree started under `clip` in focus scope `scope`, placed at
  /// `at`: one given back if there is one
  pub(crate) fn take_subtree(
    &mut self,
    at: Option<Rect>,
    clip: Option<Rect>,
    scope: UiId,
  ) -> Subtree {
    match self.subtrees.pop() {
      Some(mut subtree) => {
        subtree.at = at;
        subtree.clip = clip;
        subtree.scope = scope;
        subtree.below = None;
        subtree
      }
      None => Subtree::new(at, clip, scope),
    }
  }

  /// Keep `subtree`'s lists for the next one, emptied
  pub(crate) fn give_subtree(&mut self, mut subtree: Subtree) {
    subtree.nodes.clear();
    subtree.draws.clear();
    subtree.leaves.clear();
    self.subtrees.push(subtree);
  }

  /// Open a capture log for the clean subtree rooted at `root`, whose
  /// record was built in frame `built`; `popup` when it begins in a popup
  pub(crate) fn begin_capture(&mut self, root: UiId, built: u64, popup: bool) {
    if self.captures_used == self.captures.len() {
      self.captures.push(CaptureLog::default());
    }
    if let Some(log) = self.captures.get_mut(self.captures_used) {
      log.reset(root, built);
      log.popup = popup;
    }
    self.captures_used += 1;
    self.capturing = true;
  }

  /// Close the capture log opened last: it stays for resolve
  pub(crate) fn end_capture(&mut self) {
    self.capturing = false;
  }

  /// Whether a capture log is open
  pub(crate) fn capturing(&self) -> bool {
    self.capturing
  }

  /// Where the open capture log's ids, states and nodes end, if one is open
  #[cfg(debug_assertions)]
  pub(crate) fn capture_mark(&self) -> Option<[usize; 3]> {
    if !self.capturing {
      return None;
    }
    let log = self.captures[..self.captures_used].last()?;
    Some([log.ids.len(), log.states.len(), log.entities.len()])
  }

  /// The node `entity` was built in the open capture from `mark` on (what
  /// is drawn in it was logged since), making the ids and touching the
  /// states of `tape`, its own build: they go where it began, before what
  /// is drawn in it, as a replay of it logs them
  #[cfg(debug_assertions)]
  pub(crate) fn capture_built(&mut self, mark: [usize; 3], entity: Entity, tape: &Tape) {
    let Some(log) = self.open_capture() else {
      return;
    };
    let [ids, states, entities] = mark;
    log.ids.splice(ids..ids, tape.ids.iter().copied());
    log
      .states
      .splice(states..states, tape.states.iter().copied());
    log.entities.insert(entities, entity);
  }

  /// The capture log open now, if one is
  pub(crate) fn open_capture(&mut self) -> Option<&mut CaptureLog> {
    if !self.capturing {
      return None;
    }
    self.captures[..self.captures_used].last_mut()
  }

  /// This pass's capture log of the subtree rooted at `root`
  pub(crate) fn capture_of(&mut self, root: UiId) -> Option<&mut CaptureLog> {
    self.captures[..self.captures_used]
      .iter_mut()
      .rev()
      .find(|log| log.root == root)
  }

  /// A new pass: no capture logs, their buffers kept
  fn clear_captures(&mut self) {
    self.captures_used = 0;
    self.capturing = false;
  }
}

/// An empty vector in `v`'s allocation, for values of the same type with
/// another lifetime: a vector of borrowing values kept between uses. The
/// standard library collects in place when the layouts match.
pub(crate) fn recycle<T, U>(mut v: Vec<T>) -> Vec<U> {
  v.clear();
  v.into_iter().filter_map(|_| None).collect()
}

/// What the UI remembers between frames: last frame's laid-out records,
/// widgets' own state (scroll offsets, carets, open lists) and focus
pub(crate) struct UiMemory {
  prev: FastMap<UiId, Record>,
  next: FastMap<UiId, Record>,
  states: FastMap<UiId, Slot>,
  focus_memory: FastMap<UiId, (u64, UiId)>,
  /// Widgets `scroll_to` asked to bring into view, applied by the next resolve
  scroll_requests: Vec<UiId>,
  /// The widget `request_focus` asked for, applied by the next resolve
  focus_request: Option<UiId>,
  /// The focus scopes the last focus resolve saw, to tell which appeared
  pub(crate) known_scopes: Vec<UiId>,
  /// Open modal and trapping scopes, innermost last, each with the focus
  /// to restore when it closes
  pub(crate) modal_stack: Vec<(UiId, UiId)>,
  frame: u64,
  /// A frame is being drawn: between `begin_frame` and `end_frame`
  drawing: bool,
  pub(crate) scopes: IdScopes,
  /// The widget states touched while a tape records
  pub(crate) touched: TapeLog,
  /// The pointer's hot, active, click and drag state, and the focus
  pub(crate) interact: Interaction,
  /// Buffers the subtrees and their resolution reuse
  pub(crate) scratch: Scratch,
  /// The kept fragments: last frame's, for this frame's resolves to take
  /// (one not taken is gone the frame after), and this frame's
  pub(crate) fragments: Fragments,
  /// The last fragment version given out before this frame: the fragments
  /// carried into it have one up to it, and a rerun pass keeps only those
  carried: u64,
  /// The last fragment version given out
  fragment_version: u64,
  /// What resolve did with fragments this pass
  pub(crate) fragment_stats: FragmentStats,
  /// Whether fragments are kept (the walk's switch)
  fragments_on: bool,
  /// The kept fragment the debug spot check set aside this frame, by its
  /// root's id, and its slot: its subtree is replayed and captured in place
  /// of its copy, and resolve compares the capture with it
  #[cfg(debug_assertions)]
  pub(crate) spot_held: Option<(UiId, u32)>,
}

impl UiMemory {
  pub(crate) fn new() -> UiMemory {
    UiMemory {
      prev: FastMap::default(),
      next: FastMap::default(),
      states: FastMap::default(),
      focus_memory: FastMap::default(),
      scroll_requests: Vec::new(),
      focus_request: None,
      known_scopes: Vec::new(),
      modal_stack: Vec::new(),
      frame: 0,
      drawing: false,
      scopes: IdScopes::default(),
      touched: TapeLog::default(),
      interact: Interaction::default(),
      scratch: Scratch::default(),
      fragments: Fragments::default(),
      carried: 0,
      fragment_version: 0,
      fragment_stats: FragmentStats::default(),
      fragments_on: true,
      #[cfg(debug_assertions)]
      spot_held: None,
    }
  }

  /// Last frame's records become readable; this frame starts empty
  pub(crate) fn begin_frame(&mut self) {
    std::mem::swap(&mut self.prev, &mut self.next);
    self.next.clear();
    self.frame += 1;
    self.drawing = true;
    self.scopes.begin_frame();
    self.touched.clear();
    self.interact.sounds.clear();
    // Last frame's fragments are this frame's to take; those it did not
    // take go
    self.fragments.begin_frame(self.frame, &mut self.scopes);
    #[cfg(debug_assertions)]
    {
      self.spot_held = None;
    }
    self.carried = self.fragment_version;
    self.fragment_stats = FragmentStats::default();
    self.scratch.clear_captures();
  }

  /// This frame wrote no records (the UI harness)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn drew_nothing(&self) -> bool {
    self.next.is_empty()
  }

  /// The widget states kept, sorted by id: each one's id, scene and value
  /// in `Debug` form (the UI snapshots of the test harnesses)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn kept_states(&self) -> Vec<(UiId, u64, String)> {
    let mut states: Vec<(UiId, u64, String)> = self
      .states
      .iter()
      .map(|(&id, slot)| (id, slot.scene, state_debug(&*slot.value)))
      .collect();
    states.sort_unstable_by_key(|&(id, _, _)| id);
    states
  }

  /// Each scope's remembered focus, sorted (the UI snapshots of the test
  /// harnesses)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn kept_focus(&self) -> Vec<(UiId, UiId)> {
    let mut focus: Vec<(UiId, UiId)> = self
      .focus_memory
      .iter()
      .map(|(&scope, &(_, focused))| (scope, focused))
      .collect();
    focus.sort_unstable();
    focus
  }

  /// Make the frame that began never have been: last frame's records stay
  /// last frame's (the UI harness)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn rewind_frame(&mut self) {
    std::mem::swap(&mut self.prev, &mut self.next);
    self.unhold_spot();
    self.fragments.rewind(self.frame, &mut self.scopes);
    self.drawing = false;
  }

  /// The UI pass runs again this frame (a scene switched from `ui`): the
  /// records it made so far are discarded and ids start over. Of the
  /// fragments it took, those it only copied are last frame's again; those
  /// it captured or resolved again go with it.
  pub(crate) fn restart_pass(&mut self) {
    self.next.clear();
    self.scopes.begin_frame();
    self.touched.clear();
    self.unhold_spot();
    let (frame, carried, latest) = (self.frame, self.carried, self.fragment_version);
    self
      .fragments
      .restart(frame, carried, latest, &mut self.scopes);
    self.fragment_stats = FragmentStats::default();
    self.scratch.clear_captures();
  }

  /// Keep fragments, or not: turned off, every fragment kept is dropped
  pub(crate) fn set_fragments(&mut self, on: bool) {
    if self.fragments_on == on {
      return;
    }
    self.fragments_on = on;
    if !on {
      self.fragments.clear(&mut self.scopes);
      #[cfg(debug_assertions)]
      {
        self.spot_held = None;
      }
    }
  }

  /// The fragment the spot check set aside, not yet compared, is last
  /// frame's again: it never left its slot
  fn unhold_spot(&mut self) {
    #[cfg(debug_assertions)]
    {
      self.spot_held = None;
    }
  }

  /// The slot of the fragment rooted at `id` the spot check set aside this
  /// frame, if it did
  #[cfg(debug_assertions)]
  pub(crate) fn take_spot_held(&mut self, id: UiId) -> Option<u32> {
    self
      .spot_held
      .take_if(|(held, _)| *held == id)
      .map(|(_, slot)| slot)
  }

  /// `fragment`, taken from `slot` this pass, cannot be used (its nodes are
  /// no longer laid out): it goes when the frame ends (`Fragments::lose`)
  pub(crate) fn lose_fragment(&mut self, slot: u32, fragment: Box<Fragment>) {
    self.fragments.lose(slot, fragment);
  }

  /// Drop the fragment in `slot`
  #[cfg(debug_assertions)]
  pub(crate) fn drop_fragment(&mut self, slot: u32) {
    self.fragments.drop_slot(slot, &mut self.scopes);
  }

  /// A new fragment version: a fragment captured or resolved again takes one
  pub(crate) fn next_fragment_version(&mut self) -> u64 {
    self.fragment_version += 1;
    self.fragment_version
  }

  /// Make the ids and keep the widget states of the fragment in `slot`,
  /// rooted at `id`, as a replay of its subtree in `scene` would, mark it
  /// pushed this frame, and push its root to `nodes` as a kept node
  /// (`Role::Kept`), its place in the tree not yet stamped. Its ids are
  /// made at once when its slot indexes them. Without such a fragment (the
  /// caller found it), a kept node with nothing kept, which resolve draws
  /// empty.
  pub(crate) fn replay_fragment(&mut self, slot: u32, id: UiId, scene: u64, nodes: &mut Vec<Node>) {
    let fragment = self.fragments.get(slot);
    debug_assert!(
      fragment.is_some(),
      "a kept fragment is pushed only once `keeps` found it"
    );
    match fragment {
      Some(fragment) => {
        if fragment.indexed {
          self.scopes.make_kept(slot, &fragment.ids);
        } else {
          for &made in &fragment.ids {
            self.scopes.replay_id(made);
          }
        }
        for &state in &fragment.states {
          touch(&mut self.states, state, scene, self.frame);
        }
        nodes.extend_from_slice(std::slice::from_ref(&fragment.root));
      }
      None => nodes.push(Node::new(id, UiStyle::default(), Measure::None)),
    }
    self.fragments.pushed(slot, self.frame);
    if let Some(root) = nodes.last_mut() {
      root.role = Role::Kept(slot);
      root.parent = None;
      root.depth = 0;
      root.scroll = UiId::NONE;
    }
  }

  /// The ids of every fragment kept for the next frame
  #[cfg(test)]
  pub(crate) fn kept_ids(&self) -> Vec<UiId> {
    let fragments = self.fragments.kept_in(self.frame);
    fragments
      .flat_map(|(_, fragment)| fragment.ids.iter().copied())
      .collect()
  }

  /// Widget state not touched this frame is dropped
  pub(crate) fn end_frame(&mut self) {
    self.drawing = false;
    let frame = self.frame;
    self.states.retain(|_, slot| slot.seen == frame);
  }

  pub(crate) fn record(&mut self, id: UiId, record: Record) {
    self.next.insert(id, record);
  }

  /// Ask for `id` to be scrolled into view by the next resolve
  pub(crate) fn request_scroll(&mut self, id: UiId) {
    if !self.scroll_requests.contains(&id) {
      self.scroll_requests.push(id);
    }
  }

  /// The pending `scroll_to` requests, cleared
  pub(crate) fn take_scroll_requests(&mut self) -> Vec<UiId> {
    std::mem::take(&mut self.scroll_requests)
  }

  /// Ask for `id` to be focused by the next resolve (the last request wins)
  pub(crate) fn request_focus(&mut self, id: UiId) {
    self.focus_request = Some(id);
  }

  /// The pending `request_focus`, cleared
  pub(crate) fn take_focus_request(&mut self) -> Option<UiId> {
    self.focus_request.take()
  }

  /// A widget's record from last frame
  pub(crate) fn prev(&self, id: UiId) -> Option<&Record> {
    self.prev.get(&id)
  }

  /// Every record from last frame
  pub(crate) fn prev_records(&self) -> impl Iterator<Item = (UiId, &Record)> {
    self.prev.iter().map(|(id, record)| (*id, record))
  }

  /// A widget's record from the last frame drawn to its end: last frame's
  /// while a frame is being drawn (what the resolves read), this frame's
  /// once it has ended (what the headless engine reads between frames)
  pub(crate) fn drawn(&self, id: UiId) -> Option<&Record> {
    if self.drawing {
      self.prev.get(&id)
    } else {
      self.next.get(&id)
    }
  }

  /// Every record from the last frame drawn to its end (see [`drawn`](Self::drawn))
  pub(crate) fn drawn_records(&self) -> impl Iterator<Item = (UiId, &Record)> {
    let records = if self.drawing { &self.prev } else { &self.next };
    records.iter().map(|(id, record)| (*id, record))
  }

  /// Every record written this frame so far
  pub(crate) fn current_records(&self) -> impl Iterator<Item = (UiId, &Record)> {
    self.next.iter().map(|(id, record)| (*id, record))
  }

  /// A widget's record from this frame so far
  pub(crate) fn current(&self, id: UiId) -> Option<&Record> {
    self.next.get(&id)
  }

  /// A widget's own state, created with `T::default()` when absent or of
  /// another type. Always `Some`; the `Option` keeps library code panic-free
  pub(crate) fn state<T: Default + 'static>(&mut self, id: UiId, scene: u64) -> Option<&mut T> {
    let frame = self.frame;
    let slot = self.states.entry(id).or_insert_with(|| Slot {
      scene,
      seen: frame,
      value: Box::new(T::default()),
    });
    if !slot.value.is::<T>() {
      slot.value = Box::new(T::default());
    }
    slot.scene = scene;
    slot.seen = frame;
    self.touched.note(id);
    slot.value.downcast_mut::<T>()
  }

  /// Keep widget `id`'s state this frame, as [`state`](Self::state) would,
  /// for `scene`: a replay of a tape that touched it. A state that is gone
  /// stays gone.
  pub(crate) fn touch(&mut self, id: UiId, scene: u64) {
    touch(&mut self.states, id, scene, self.frame);
  }

  pub(crate) fn peek_state<T: 'static>(&self, id: UiId) -> Option<&T> {
    self.states.get(&id)?.value.downcast_ref::<T>()
  }

  /// A widget's own state of type `T`, to change, if it has one; it is not
  /// marked seen, so it is still dropped at the end of a frame it is not
  /// drawn in
  pub(crate) fn peek_state_mut<T: 'static>(&mut self, id: UiId) -> Option<&mut T> {
    self.states.get_mut(&id)?.value.downcast_mut::<T>()
  }

  /// The frame being drawn, counted from 1 by `begin_frame`
  pub(crate) fn frame(&self) -> u64 {
    self.frame
  }

  pub(crate) fn remember_focus(&mut self, scope: UiId, scene: u64, focused: UiId) {
    self.focus_memory.insert(scope, (scene, focused));
  }

  pub(crate) fn recall_focus(&self, scope: UiId) -> Option<UiId> {
    self.focus_memory.get(&scope).map(|(_, focused)| *focused)
  }

  /// A scene exited: drop everything it left behind
  pub(crate) fn forget_scene(&mut self, scene: u64) {
    self.states.retain(|_, slot| slot.scene != scene);
    self.focus_memory.retain(|_, (owner, _)| *owner != scene);
    self.prev.retain(|_, record| record.scene != scene);
    self.next.retain(|_, record| record.scene != scene);
    self.fragments.forget_scene(scene, &mut self.scopes);
  }
}

/// Keep widget `id`'s state in `states` in `frame`, for `scene`, if it has
/// one: a state that is gone stays gone
fn touch(states: &mut FastMap<UiId, Slot>, id: UiId, scene: u64, frame: u64) {
  if let Some(slot) = states.get_mut(&id) {
    slot.scene = scene;
    slot.seen = frame;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::layout::Measure;
  use crate::core::ui::tree::Node;

  fn record(scene: u64) -> Record {
    Record {
      rect: Rect::new(0.0, 0.0, 10.0, 10.0),
      clip: None,
      layer: 0,
      order: 0,
      depth: 0,
      scene,
      flags: Flags::CLICK,
      scope: UiId::NONE,
      scroll: UiId::NONE,
      content: glam::Vec2::ZERO,
      nav: [UiId::NONE; 4],
      accepts: None,
    }
  }

  #[test]
  fn records_from_last_frame_are_read_this_frame() {
    let mut memory = UiMemory::new();
    memory.begin_frame();
    memory.record(UiId(5), record(1));
    assert!(memory.prev(UiId(5)).is_none(), "not until the next frame");
    memory.end_frame();
    memory.begin_frame();
    assert!(memory.prev(UiId(5)).is_some());
    memory.end_frame();
    memory.begin_frame();
    assert!(
      memory.prev(UiId(5)).is_none(),
      "gone once a frame did not record it"
    );
  }

  #[test]
  fn widget_state_is_dropped_after_a_frame_unseen() {
    let mut memory = UiMemory::new();
    memory.begin_frame();
    *memory.state::<u32>(UiId(9), 1).unwrap() = 42;
    memory.end_frame();
    memory.begin_frame();
    assert_eq!(
      *memory.state::<u32>(UiId(9), 1).unwrap(),
      42,
      "seen again: kept"
    );
    memory.end_frame();
    memory.begin_frame();
    memory.end_frame();
    memory.begin_frame();
    assert_eq!(
      *memory.state::<u32>(UiId(9), 1).unwrap(),
      0,
      "a frame unseen: dropped"
    );
  }

  #[test]
  fn focus_memory_lasts_until_the_scene_is_forgotten() {
    let mut memory = UiMemory::new();
    memory.remember_focus(UiId(100), 3, UiId(7));
    for _ in 0..5 {
      memory.begin_frame();
      memory.end_frame();
    }
    assert_eq!(memory.recall_focus(UiId(100)), Some(UiId(7)));
    memory.forget_scene(3);
    assert_eq!(memory.recall_focus(UiId(100)), None);
  }

  #[test]
  fn a_recycled_vector_keeps_its_allocation() {
    let words = [String::from("a"), String::from("b")];
    let mut borrowed: Vec<&str> = Vec::with_capacity(16);
    borrowed.extend(words.iter().map(String::as_str));
    let at = borrowed.as_ptr() as usize;
    let kept: Vec<&'static str> = recycle(borrowed);
    assert!(kept.is_empty());
    assert_eq!((kept.capacity(), kept.as_ptr() as usize), (16, at));
  }

  #[test]
  fn a_given_back_subtree_comes_back_empty_with_its_room() {
    let mut scratch = Scratch::default();
    let mut subtree = scratch.take_subtree(None, None, UiId(1));
    subtree.below = Some(Rect::new(0.0, 0.0, 1.0, 1.0));
    for i in 0..10 {
      let node = Node::new(UiId(i), UiStyle::default(), Measure::None);
      subtree.nodes.push(node);
    }
    let room = subtree.nodes.capacity();
    scratch.give_subtree(subtree);
    let at = Some(Rect::new(1.0, 2.0, 3.0, 4.0));
    let again = scratch.take_subtree(at, None, UiId(2));
    assert!(again.nodes.is_empty() && again.draws.is_empty());
    assert_eq!(again.nodes.capacity(), room);
    assert_eq!((again.at, again.scope, again.below), (at, UiId(2), None));
  }

  #[test]
  fn a_restarted_pass_drops_its_records_and_makes_the_same_ids() {
    let site = std::panic::Location::caller();
    let mut memory = UiMemory::new();
    memory.begin_frame();
    memory.record(UiId(5), record(1));
    memory.end_frame();
    memory.begin_frame();
    memory.scopes.enter_root(UiId::root(1));
    let first = memory.scopes.make(site, None);
    memory.record(UiId(6), record(1));
    memory.restart_pass();
    memory.scopes.enter_root(UiId::root(1));
    assert!(memory.current(UiId(6)).is_none(), "the first pass's record");
    assert!(memory.prev(UiId(5)).is_some(), "last frame's stay");
    assert_eq!(
      memory.scopes.make(site, None),
      first,
      "ids start over, so the rerun's widgets keep theirs"
    );
  }

  /// A fragment of `scene`, rooted at node `root`, at `version`
  fn fragment(root: u64, scene: u64, version: u64) -> Box<Fragment> {
    use crate::core::ui::fragment::Inputs;
    Box::new(Fragment {
      built: 1,
      scene,
      inputs: Inputs {
        origin: [0; 2],
        rect: [0; 4],
        shift: [0; 2],
        base: [0; 2],
        scale: 1.0f32.to_bits(),
        slot: 0,
        offset: [0; 2],
        clip: None,
        layer: 0,
      },
      count: 1,
      root: Node::new(UiId(root), UiStyle::default(), Measure::None),
      at: None,
      ids: Vec::new(),
      states: Vec::new(),
      entities: Vec::new(),
      records: Vec::new(),
      draws: Vec::new(),
      nodes: Vec::new(),
      node_draws: Vec::new(),
      sprites: Vec::new(),
      interest: None,
      version,
      bounds: None,
      indexed: false,
      holding: false,
    })
  }

  /// The roots of the fragments last frame left this one to take, sorted
  fn to_take(memory: &UiMemory) -> Vec<UiId> {
    let frame = memory.frame();
    let mut roots: Vec<UiId> = memory
      .fragments
      .of_last_frame(frame)
      .map(|(root, _)| root)
      .collect();
    roots.sort_unstable();
    roots
  }

  /// How many fragments this frame keeps for the next
  fn kept(memory: &UiMemory) -> usize {
    memory.fragments.kept_in(memory.frame()).count()
  }

  /// `fragment` captured this frame: its slot
  fn capture(memory: &mut UiMemory, fragment: Box<Fragment>) -> u32 {
    let frame = memory.frame();
    memory.fragments.insert(fragment, frame, &mut memory.scopes)
  }

  /// Last frame's fragment of `root` taken and kept again this frame:
  /// copied, or (`again`) resolved again
  fn copy(memory: &mut UiMemory, root: u64, again: bool) {
    let frame = memory.frame();
    let slot = memory.fragments.last_of(UiId(root), frame).unwrap();
    let mut fragment = memory.fragments.take(slot).unwrap();
    if again {
      fragment.version = memory.next_fragment_version();
    }
    memory.fragments.keep(slot, fragment, frame);
  }

  #[test]
  fn a_fragment_lasts_a_frame_undrawn_and_a_rerun_keeps_only_last_frames() {
    let mut memory = UiMemory::new();
    memory.begin_frame();
    for root in [1, 2] {
      let version = memory.next_fragment_version();
      capture(&mut memory, fragment(root, 7, version));
    }
    memory.end_frame();
    memory.begin_frame();
    assert_eq!(to_take(&memory), [UiId(1), UiId(2)], "last frame's");
    // A pass copies one, resolves the other again and captures a third
    copy(&mut memory, 1, false);
    copy(&mut memory, 2, true);
    let version = memory.next_fragment_version();
    capture(&mut memory, fragment(3, 7, version));
    assert!(to_take(&memory).is_empty(), "all taken");
    // The pass is discarded: what it only copied is last frame's again
    memory.restart_pass();
    assert_eq!(to_take(&memory), [UiId(1)]);
    assert_eq!(kept(&memory), 0);
    let version = memory.next_fragment_version();
    capture(&mut memory, fragment(4, 8, version));
    memory.end_frame();
    // Not drawn in a frame: gone the frame after
    memory.begin_frame();
    assert_eq!(to_take(&memory), [UiId(4)]);
    memory.forget_scene(8);
    assert!(to_take(&memory).is_empty(), "its scene's go with it");
  }

  #[test]
  fn fragments_turned_off_are_dropped() {
    let mut memory = UiMemory::new();
    memory.begin_frame();
    capture(&mut memory, fragment(1, 7, 1));
    memory.set_fragments(true);
    assert_eq!(kept(&memory), 1, "on: kept");
    memory.set_fragments(false);
    assert_eq!(kept(&memory), 0);
  }

  #[test]
  fn a_fragment_lost_mid_pass_keeps_its_ids_and_its_hold_until_the_frame_ends() {
    let site = std::panic::Location::caller();
    let mut memory = UiMemory::new();
    memory.begin_frame();
    memory.scopes.enter_root(UiId::root(1));
    let a = memory.scopes.make(site, Some(5));
    let mut kept = fragment(1, 7, memory.next_fragment_version());
    kept.ids = vec![a];
    let slot = capture(&mut memory, kept);
    memory.end_frame();
    memory.begin_frame();
    memory.scopes.enter_root(UiId::root(1));
    // Pushed, then lost by resolve (its nodes no longer laid out)
    let mut nodes = Vec::new();
    memory.replay_fragment(slot, UiId(1), 7, &mut nodes);
    let frame = memory.frame();
    let hold = memory.fragments.first_hold(slot).map(|(_, hold)| hold);
    let taken = memory.fragments.take(slot).unwrap();
    memory.lose_fragment(slot, taken);
    // For the rest of the pass its id is made, and a build making it again
    // is given another; its nodes' records are drawn this frame
    assert!(memory.scopes.made(a), "made for the rest of the pass");
    assert_ne!(memory.scopes.make(site, Some(5)), a, "a duplicate");
    let hold = hold.unwrap();
    assert!(memory.fragments.holds(hold, frame), "its records drawn");
    // Never taken again; gone the next frame
    assert_eq!(memory.fragments.last_of(UiId(1), frame), None);
    memory.end_frame();
    memory.begin_frame();
    memory.scopes.enter_root(UiId::root(1));
    assert!(!memory.scopes.made(a));
    assert_eq!(memory.scopes.make(site, Some(5)), a, "its id is free");
    assert!(!memory.fragments.holds(hold, memory.frame()));
  }

  #[test]
  fn a_pushed_fragment_makes_its_ids_at_once_and_a_newer_one_of_its_root_takes_them() {
    let site = std::panic::Location::caller();
    let mut memory = UiMemory::new();
    let begin = |memory: &mut UiMemory| {
      memory.begin_frame();
      memory.scopes.enter_root(UiId::root(1));
    };
    begin(&mut memory);
    let a = memory.scopes.make(site, Some(5));
    let mut kept_a = fragment(1, 7, memory.next_fragment_version());
    kept_a.ids = vec![a];
    let first = capture(&mut memory, kept_a);
    memory.end_frame();
    // Pushed: its id is made, as one registered would be, and a build
    // making it again is given another
    begin(&mut memory);
    let mut nodes = Vec::new();
    memory.replay_fragment(first, UiId(1), 7, &mut nodes);
    assert_eq!(nodes[0].role, Role::Kept(first));
    assert!(memory.scopes.made(a));
    assert_eq!(memory.scopes.replayed, 0, "made as a block");
    assert_ne!(memory.scopes.make(site, Some(5)), a, "a duplicate");
    copy(&mut memory, 1, false);
    memory.end_frame();
    // Not pushed: not made
    begin(&mut memory);
    assert!(!memory.scopes.made(a));
    // Its root captured anew, holding `a` and `b`: pushed, both are made
    let b = UiId(77);
    let mut newer = fragment(1, 7, memory.next_fragment_version());
    newer.ids = vec![a, b];
    let second = capture(&mut memory, newer);
    memory.restart_pass();
    memory.scopes.enter_root(UiId::root(1));
    // The capture went with the discarded pass: the older one takes `a`
    // back
    memory.replay_fragment(first, UiId(1), 7, &mut nodes);
    assert!(memory.scopes.made(a) && !memory.scopes.made(b));
    assert_eq!(memory.scopes.replayed, 0);
    let _ = second;
    // Another root's fragment holding `a` registers its ids one at a time
    let mut other = fragment(2, 7, memory.next_fragment_version());
    other.ids = vec![a, b];
    let third = capture(&mut memory, other);
    memory.end_frame();
    begin(&mut memory);
    memory.replay_fragment(third, UiId(2), 7, &mut nodes);
    assert_eq!(memory.scopes.replayed, 2, "registered one at a time");
    assert!(memory.scopes.made(a) && memory.scopes.made(b));
  }
}
