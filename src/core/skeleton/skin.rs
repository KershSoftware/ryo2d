//! Skins: which attachment each (slot, attachment name) means.

use std::collections::HashMap;
use std::sync::Arc;

use super::attachment::Attachment;
use super::data::SlotId;

/// A set of attachments keyed by slot and attachment name. Changing a
/// skeleton's skin swaps what its slots' attachment names mean
#[derive(Debug, Clone, PartialEq)]
pub struct Skin {
  pub name: Arc<str>,
  /// One map per slot, by slot index
  pub(super) slots: Vec<HashMap<Arc<str>, Attachment>>,
}

impl Skin {
  pub(super) fn new(name: Arc<str>, slot_count: usize) -> Self {
    Self {
      name,
      slots: vec![HashMap::new(); slot_count],
    }
  }

  /// The attachment called `name` in `slot`, if this skin has one
  pub fn attachment(&self, slot: SlotId, name: &str) -> Option<&Attachment> {
    self.slots.get(slot.index())?.get(name)
  }

  /// Every attachment in the skin, as (slot, name, attachment), in no
  /// particular order within a slot
  pub fn attachments(&self) -> impl Iterator<Item = (SlotId, &Arc<str>, &Attachment)> {
    self.slots.iter().enumerate().flat_map(|(index, map)| {
      // A skin has at most 255 slots, so the index fits
      let slot = SlotId(index as u8);
      map
        .iter()
        .map(move |(name, attachment)| (slot, name, attachment))
    })
  }
}
