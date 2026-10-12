//! Dirty tracking for component replication
//!
//! Tracks which components have changed since the last network sync,
//! enabling delta updates instead of full state transmission.

use crate::core::ecs::Component;
use std::collections::{HashMap, HashSet, VecDeque};

use renet::ClientId;

use super::component::ComponentTypeId;
use super::types::NetworkId;

/// Most unacknowledged change batches remembered per client. An ack for an
/// older batch is ignored; its components stay pending and are sent again.
pub const MAX_IN_FLIGHT_BATCHES: usize = 128;

/// A component change a client hasn't acknowledged yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingComponent {
  /// The entity
  pub network_id: NetworkId,
  /// The component type
  pub component_id: ComponentTypeId,
  /// Generation of the component's newest change
  pub change: u64,
}

/// One component's delivery state for one client.
#[derive(Debug, Default, Clone, Copy)]
struct ComponentDelivery {
  /// Newest change generation the client acknowledged
  acked: Option<u64>,
  /// Generation of the broadcast that last sent it
  last_sent: Option<u64>,
}

/// A change batch sent to a client and not acknowledged yet.
#[derive(Debug)]
struct SentBatch {
  generation: u64,
  components: Vec<PendingComponent>,
}

/// What one client has been sent and has acknowledged.
#[derive(Debug, Default)]
struct ClientDelivery {
  /// Every change at or before this generation already reached the client
  /// (its full-state sync, or `acknowledge_client`)
  baseline: Option<u64>,
  components: HashMap<NetworkId, HashMap<ComponentTypeId, ComponentDelivery>>,
  in_flight: VecDeque<SentBatch>,
  newest_ack: Option<u64>,
}

/// Tracks which components have changed since last sync.
///
/// The dirty tracker maintains per-entity, per-component dirty flags
/// and supports generation-based tracking for multiple clients.
///
/// # Example
///
/// ```
/// use ryo2d::prelude::*;
///
/// let mut tracker = DirtyTracker::new();
/// let network_id = NetworkId::new();
/// let client_id = 1;
///
/// // Mark a component as dirty
/// tracker.mark_dirty(network_id, ComponentTypeId::from_name("Transform"));
///
/// // Components the client hasn't acknowledged yet
/// let pending = tracker.pending_for_client(client_id);
/// // ... send them in a change batch, then remember what it carried
/// tracker.record_sent(client_id, pending);
///
/// // Later, the client acknowledges the batch by its generation
/// tracker.record_ack(client_id, tracker.generation());
/// ```
#[derive(Component, Debug)]
pub struct DirtyTracker {
  /// Current generation counter (incremented each frame/sync cycle)
  generation: u64,

  /// Maps NetworkId -> (ComponentTypeId -> generation when marked dirty)
  dirty_components: HashMap<NetworkId, HashMap<ComponentTypeId, u64>>,

  /// What each client has been sent and has acknowledged
  clients: HashMap<ClientId, ClientDelivery>,

  /// Entities that were spawned (need full sync)
  spawned_entities: HashSet<NetworkId>,

  /// Entities that were despawned
  despawned_entities: HashSet<NetworkId>,
}

impl DirtyTracker {
  /// Create a new dirty tracker.
  pub fn new() -> Self {
    Self {
      generation: 0,
      dirty_components: HashMap::new(),
      clients: HashMap::new(),
      spawned_entities: HashSet::new(),
      despawned_entities: HashSet::new(),
    }
  }

  /// Get the current generation.
  pub fn generation(&self) -> u64 {
    self.generation
  }

  /// Advance to the next generation.
  ///
  /// Call this at the end of each sync cycle.
  pub fn next_generation(&mut self) {
    self.generation += 1;
    // Clear spawn/despawn sets after they've been processed
    self.spawned_entities.clear();
    self.despawned_entities.clear();
  }

  /// Mark a component as dirty for an entity.
  pub fn mark_dirty(&mut self, network_id: NetworkId, component_id: ComponentTypeId) {
    self
      .dirty_components
      .entry(network_id)
      .or_default()
      .insert(component_id, self.generation);
  }

  /// Mark all components of an entity as dirty.
  pub fn mark_all_dirty(&mut self, network_id: NetworkId, component_ids: &[ComponentTypeId]) {
    let entry = self.dirty_components.entry(network_id).or_default();
    for &id in component_ids {
      entry.insert(id, self.generation);
    }
  }

  /// Mark an entity as newly spawned.
  ///
  /// Spawned entities need full component sync to all clients.
  pub fn mark_spawned(&mut self, network_id: NetworkId) {
    self.spawned_entities.insert(network_id);
  }

  /// Mark an entity as despawned.
  pub fn mark_despawned(&mut self, network_id: NetworkId) {
    self.despawned_entities.insert(network_id);
    self.dirty_components.remove(&network_id);
    self.forget_delivery(network_id);
  }

  /// Clear dirty flags for a specific entity.
  pub fn clear_entity(&mut self, network_id: NetworkId) {
    self.dirty_components.remove(&network_id);
    self.forget_delivery(network_id);
  }

  /// Drop every client's delivery state for an entity, so a reused id and
  /// generation can't look delivered.
  fn forget_delivery(&mut self, network_id: NetworkId) {
    for client in self.clients.values_mut() {
      client.components.remove(&network_id);
    }
  }

  /// Register a new client. Until its full-state sync is recorded it is sent
  /// every tracked change. Registering an existing client keeps its state.
  pub fn register_client(&mut self, client_id: ClientId) {
    self.clients.entry(client_id).or_default();
  }

  /// Remove a disconnected client.
  pub fn remove_client(&mut self, client_id: ClientId) {
    self.clients.remove(&client_id);
  }

  /// Forget every client for which `keep` returns false.
  pub fn retain_clients(&mut self, keep: impl Fn(ClientId) -> bool) {
    self.clients.retain(|&id, _| keep(id));
  }

  /// The newest change batch generation the client acknowledged (or the
  /// generation [`acknowledge_client`](Self::acknowledge_client) marked it at).
  pub fn client_generation(&self, client_id: ClientId) -> Option<u64> {
    self
      .clients
      .get(&client_id)
      .and_then(|client| client.newest_ack)
  }

  /// Treat a client as having every change marked so far, for state sent by
  /// other means. Replication tracks delivery itself from acknowledgements;
  /// calling this stops changes the client may not have from being re-sent.
  pub fn acknowledge_client(&mut self, client_id: ClientId) {
    let newest_ack = self.client_generation(client_id).max(Some(self.generation));
    self.clients.insert(
      client_id,
      ClientDelivery {
        baseline: Some(self.generation),
        newest_ack,
        ..Default::default()
      },
    );
  }

  /// Record that a client was sent a full-state sync built in the current
  /// generation: changes from earlier generations are covered; changes in
  /// this one may have been marked after the sync was built, so stay pending.
  pub fn record_full_sync(&mut self, client_id: ClientId) {
    let newest_ack = self.client_generation(client_id);
    self.clients.insert(
      client_id,
      ClientDelivery {
        baseline: self.generation.checked_sub(1),
        newest_ack,
        ..Default::default()
      },
    );
  }

  /// Components whose newest change the client hasn't acknowledged: never
  /// sent first, then least recently sent, then oldest change - so a backlog
  /// sent in budgeted batches reaches every component before re-sending any.
  pub fn pending_for_client(&self, client_id: ClientId) -> Vec<PendingComponent> {
    let client = self.clients.get(&client_id);
    let baseline = client.and_then(|client| client.baseline);
    let mut pending = Vec::new();
    for (&network_id, components) in &self.dirty_components {
      let delivered = client.and_then(|client| client.components.get(&network_id));
      for (&component_id, &change) in components {
        let state = delivered
          .and_then(|delivered| delivered.get(&component_id))
          .copied()
          .unwrap_or_default();
        if baseline
          .max(state.acked)
          .is_none_or(|covered| change > covered)
        {
          pending.push((
            state.last_sent,
            PendingComponent {
              network_id,
              component_id,
              change,
            },
          ));
        }
      }
    }
    pending.sort_unstable_by_key(|(last_sent, p)| {
      (*last_sent, p.change, p.network_id.0, p.component_id.0)
    });
    pending.into_iter().map(|(_, p)| p).collect()
  }

  /// Record a change batch sent to a client in the current generation.
  pub fn record_sent(&mut self, client_id: ClientId, components: Vec<PendingComponent>) {
    let generation = self.generation;
    let client = self.clients.entry(client_id).or_default();
    for p in &components {
      client
        .components
        .entry(p.network_id)
        .or_default()
        .entry(p.component_id)
        .or_default()
        .last_sent = Some(generation);
    }
    client.in_flight.push_back(SentBatch {
      generation,
      components,
    });
    while client.in_flight.len() > MAX_IN_FLIGHT_BATCHES {
      client.in_flight.pop_front();
    }
  }

  /// Record that a client applied the change batch sent in `generation`: the
  /// changes it carried are delivered. Acks for a batch that isn't in flight
  /// (never sent, already acknowledged, evicted, or from the future) are
  /// ignored.
  pub fn record_ack(&mut self, client_id: ClientId, generation: u64) {
    let Some(client) = self.clients.get_mut(&client_id) else {
      return;
    };
    let Some(index) = client
      .in_flight
      .iter()
      .position(|batch| batch.generation == generation)
    else {
      return;
    };
    let Some(batch) = client.in_flight.remove(index) else {
      return;
    };
    for p in batch.components {
      // Skip components despawned since the batch was sent
      let tracked = self
        .dirty_components
        .get(&p.network_id)
        .is_some_and(|components| components.contains_key(&p.component_id));
      if !tracked {
        continue;
      }
      let state = client
        .components
        .entry(p.network_id)
        .or_default()
        .entry(p.component_id)
        .or_default();
      state.acked = state.acked.max(Some(p.change));
    }
    client.newest_ack = client.newest_ack.max(Some(generation));
  }

  /// Get dirty components for a specific client.
  ///
  /// Returns entities and their components whose newest change the client
  /// hasn't acknowledged yet. Unregistered clients receive ALL dirty
  /// components.
  pub fn get_dirty_for_client(
    &self,
    client_id: ClientId,
  ) -> Vec<(NetworkId, Vec<ComponentTypeId>)> {
    let mut grouped: HashMap<NetworkId, Vec<ComponentTypeId>> = HashMap::new();
    for p in self.pending_for_client(client_id) {
      grouped
        .entry(p.network_id)
        .or_default()
        .push(p.component_id);
    }
    grouped.into_iter().collect()
  }

  /// Get entities spawned this generation.
  pub fn spawned_entities(&self) -> impl Iterator<Item = NetworkId> + '_ {
    self.spawned_entities.iter().copied()
  }

  /// Get entities despawned this generation.
  pub fn despawned_entities(&self) -> impl Iterator<Item = NetworkId> + '_ {
    self.despawned_entities.iter().copied()
  }

  /// Check if an entity is newly spawned.
  pub fn is_spawned(&self, network_id: NetworkId) -> bool {
    self.spawned_entities.contains(&network_id)
  }

  /// Check if an entity is despawned.
  pub fn is_despawned(&self, network_id: NetworkId) -> bool {
    self.despawned_entities.contains(&network_id)
  }

  /// Check if any entity has dirty components for a client.
  pub fn has_dirty_for_client(&self, client_id: ClientId) -> bool {
    !self.pending_for_client(client_id).is_empty()
  }

  /// Get all network IDs being tracked.
  pub fn tracked_entities(&self) -> impl Iterator<Item = NetworkId> + '_ {
    self.dirty_components.keys().copied()
  }

  /// Check if a specific component is currently marked dirty for an entity.
  ///
  /// This checks if the component has been marked dirty since the last
  /// generation clear, regardless of client acknowledgments.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let is_dirty = tracker.is_component_dirty(network_id, ComponentTypeId::from_name("Transform"));
  /// ```
  pub fn is_component_dirty(&self, network_id: NetworkId, component_id: ComponentTypeId) -> bool {
    self
      .dirty_components
      .get(&network_id)
      .is_some_and(|components| components.contains_key(&component_id))
  }
}

impl Default for DirtyTracker {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn make_component_id(name: &str) -> ComponentTypeId {
    ComponentTypeId::from_name(name)
  }

  #[test]
  fn dirty_tracker_basic() {
    let mut tracker = DirtyTracker::new();
    let net_id = NetworkId::from_raw(1);
    let comp_id = make_component_id("Transform");

    tracker.mark_dirty(net_id, comp_id);

    let dirty = tracker.get_dirty_for_client(0);
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].0, net_id);
    assert!(dirty[0].1.contains(&comp_id));
  }

  #[test]
  fn dirty_tracker_generation() {
    let mut tracker = DirtyTracker::new();
    let net_id = NetworkId::from_raw(1);
    let comp_id = make_component_id("Transform");

    // Mark dirty in gen 0
    tracker.mark_dirty(net_id, comp_id);

    // Register client at gen 0
    tracker.register_client(1);

    // Acknowledge - client is now at gen 0
    tracker.acknowledge_client(1);

    // Move to gen 1
    tracker.next_generation();

    // Client shouldn't see old dirty flags
    let dirty = tracker.get_dirty_for_client(1);
    assert!(dirty.is_empty());

    // Mark dirty again in gen 1
    tracker.mark_dirty(net_id, comp_id);

    // Now client should see it
    let dirty = tracker.get_dirty_for_client(1);
    assert_eq!(dirty.len(), 1);
  }

  #[test]
  fn dirty_tracker_spawn_despawn() {
    let mut tracker = DirtyTracker::new();
    let net_id = NetworkId::from_raw(1);

    tracker.mark_spawned(net_id);
    assert!(tracker.is_spawned(net_id));
    assert!(!tracker.is_despawned(net_id));

    tracker.next_generation();
    assert!(!tracker.is_spawned(net_id));

    tracker.mark_despawned(net_id);
    assert!(tracker.is_despawned(net_id));
  }

  #[test]
  fn dirty_tracker_multiple_clients() {
    let mut tracker = DirtyTracker::new();
    let net_id = NetworkId::from_raw(1);
    let comp_id = make_component_id("Transform");

    tracker.register_client(1);
    tracker.register_client(2);

    tracker.mark_dirty(net_id, comp_id);

    // Both clients see dirty
    assert!(tracker.has_dirty_for_client(1));
    assert!(tracker.has_dirty_for_client(2));

    // Acknowledge client 1
    tracker.acknowledge_client(1);

    // Only client 2 still sees dirty (gen hasn't advanced)
    // Actually both still see it until next_generation()
    tracker.next_generation();

    // Mark dirty again
    tracker.mark_dirty(net_id, comp_id);

    // Now client 1 (at gen 0 relative to new dirty) sees it
    // Client 2 (at gen 0) also sees it
    assert!(tracker.has_dirty_for_client(1));
    assert!(tracker.has_dirty_for_client(2));
  }

  #[test]
  fn is_component_dirty() {
    let mut tracker = DirtyTracker::new();
    let net_id = NetworkId::from_raw(1);
    let transform_id = make_component_id("Transform");
    let velocity_id = make_component_id("Velocity");

    // Initially nothing is dirty
    assert!(!tracker.is_component_dirty(net_id, transform_id));
    assert!(!tracker.is_component_dirty(net_id, velocity_id));

    // Mark transform dirty
    tracker.mark_dirty(net_id, transform_id);

    // Transform is dirty, velocity is not
    assert!(tracker.is_component_dirty(net_id, transform_id));
    assert!(!tracker.is_component_dirty(net_id, velocity_id));

    // Check non-existent entity
    let other_id = NetworkId::from_raw(999);
    assert!(!tracker.is_component_dirty(other_id, transform_id));
  }

  fn pending_ids(tracker: &DirtyTracker, client: ClientId) -> Vec<(NetworkId, ComponentTypeId)> {
    tracker
      .pending_for_client(client)
      .iter()
      .map(|p| (p.network_id, p.component_id))
      .collect()
  }

  #[test]
  fn an_ack_delivers_exactly_the_batch_it_names() {
    let mut tracker = DirtyTracker::new();
    let (a, b) = (NetworkId::from_raw(1), NetworkId::from_raw(2));
    let health = make_component_id("Health");
    tracker.register_client(1);
    tracker.mark_dirty(a, health);
    let sent = tracker.pending_for_client(1);
    tracker.record_sent(1, sent); // batch 0 carries a
    tracker.next_generation();
    tracker.mark_dirty(b, health);
    let sent = tracker.pending_for_client(1);
    assert_eq!(sent.first().map(|p| p.network_id), Some(b)); // never sent goes first
    tracker.record_sent(1, sent); // batch 1 carries b and a
    tracker.next_generation();

    tracker.record_ack(1, 0);

    assert_eq!(pending_ids(&tracker, 1), vec![(b, health)]);
    assert_eq!(tracker.client_generation(1), Some(0));
  }

  #[test]
  fn acks_for_batches_never_sent_are_ignored() {
    let mut tracker = DirtyTracker::new();
    let a = NetworkId::from_raw(1);
    let health = make_component_id("Health");
    tracker.register_client(1);
    tracker.mark_dirty(a, health);

    tracker.record_ack(1, 0);
    tracker.record_ack(1, u64::MAX);

    assert_eq!(pending_ids(&tracker, 1), vec![(a, health)]);
    assert_eq!(tracker.client_generation(1), None);
  }

  #[test]
  fn a_newer_change_is_pending_after_an_older_one_was_acknowledged() {
    let mut tracker = DirtyTracker::new();
    let a = NetworkId::from_raw(1);
    let health = make_component_id("Health");
    tracker.register_client(1);
    tracker.mark_dirty(a, health);
    let sent = tracker.pending_for_client(1);
    tracker.record_sent(1, sent);
    tracker.next_generation();
    tracker.record_ack(1, 0);
    assert!(pending_ids(&tracker, 1).is_empty());

    tracker.mark_dirty(a, health);

    assert_eq!(pending_ids(&tracker, 1), vec![(a, health)]);
  }

  #[test]
  fn an_ack_marks_the_change_the_batch_carried() {
    let mut tracker = DirtyTracker::new();
    let a = NetworkId::from_raw(1);
    let health = make_component_id("Health");
    tracker.register_client(1);
    tracker.mark_dirty(a, health);
    let sent = tracker.pending_for_client(1);
    tracker.record_sent(1, sent); // batch 0 carries the first change
    tracker.next_generation();
    tracker.mark_dirty(a, health); // a newer change the batch did not carry

    tracker.record_ack(1, 0);

    assert_eq!(pending_ids(&tracker, 1), vec![(a, health)]);
  }

  #[test]
  fn a_full_sync_covers_changes_before_it_but_not_during_it() {
    let mut tracker = DirtyTracker::new();
    let (a, b) = (NetworkId::from_raw(1), NetworkId::from_raw(2));
    let health = make_component_id("Health");
    tracker.mark_dirty(a, health); // generation 0
    tracker.next_generation();
    tracker.register_client(1);
    tracker.record_full_sync(1); // built during generation 1
    tracker.mark_dirty(b, health); // generation 1, possibly after the sync was built

    assert_eq!(pending_ids(&tracker, 1), vec![(b, health)]);
  }

  #[test]
  fn registering_again_keeps_a_clients_sync() {
    let mut tracker = DirtyTracker::new();
    let a = NetworkId::from_raw(1);
    let health = make_component_id("Health");
    tracker.mark_dirty(a, health);
    tracker.next_generation();
    tracker.register_client(1);
    tracker.record_full_sync(1);

    tracker.register_client(1);

    assert!(pending_ids(&tracker, 1).is_empty());
  }

  #[test]
  fn never_sent_components_go_before_resends() {
    let mut tracker = DirtyTracker::new();
    let (a, b) = (NetworkId::from_raw(1), NetworkId::from_raw(2));
    let health = make_component_id("Health");
    tracker.register_client(1);
    tracker.mark_dirty(a, health);
    tracker.mark_dirty(b, health);
    let only_a: Vec<PendingComponent> = tracker
      .pending_for_client(1)
      .into_iter()
      .filter(|p| p.network_id == a)
      .collect();
    tracker.record_sent(1, only_a);
    tracker.next_generation();

    assert_eq!(pending_ids(&tracker, 1), vec![(b, health), (a, health)]);
  }

  #[test]
  fn despawning_forgets_delivery_state() {
    let mut tracker = DirtyTracker::new();
    let a = NetworkId::from_raw(1);
    let health = make_component_id("Health");
    tracker.register_client(1);
    tracker.mark_dirty(a, health);
    let sent = tracker.pending_for_client(1);
    tracker.record_sent(1, sent);
    tracker.record_ack(1, 0);
    tracker.mark_despawned(a);

    // The same id and generation again must not look delivered
    tracker.mark_dirty(a, health);

    assert_eq!(pending_ids(&tracker, 1), vec![(a, health)]);
  }

  #[test]
  fn only_the_newest_batches_are_remembered() {
    let mut tracker = DirtyTracker::new();
    let a = NetworkId::from_raw(1);
    let health = make_component_id("Health");
    tracker.register_client(1);
    for _ in 0..=MAX_IN_FLIGHT_BATCHES {
      tracker.mark_dirty(a, health);
      let sent = tracker.pending_for_client(1);
      tracker.record_sent(1, sent);
      tracker.next_generation();
    }

    tracker.record_ack(1, 0); // evicted
    assert_eq!(tracker.client_generation(1), None);
    tracker.record_ack(1, MAX_IN_FLIGHT_BATCHES as u64);
    assert_eq!(
      tracker.client_generation(1),
      Some(MAX_IN_FLIGHT_BATCHES as u64)
    );
  }
}
