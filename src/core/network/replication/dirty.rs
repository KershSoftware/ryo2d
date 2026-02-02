//! Dirty tracking for component replication
//!
//! Tracks which components have changed since the last network sync,
//! enabling delta updates instead of full state transmission.

use std::collections::{HashMap, HashSet};

use renet::ClientId;

use super::component::ComponentTypeId;
use super::types::NetworkId;

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
/// // Check what's dirty for a client
/// for (id, components) in tracker.get_dirty_for_client(client_id) {
///     // Send updates for these components
/// }
///
/// // Acknowledge that client received updates
/// tracker.acknowledge_client(client_id);
/// ```
#[derive(Debug)]
pub struct DirtyTracker {
    /// Current generation counter (incremented each frame/sync cycle)
    generation: u64,

    /// Maps NetworkId -> (ComponentTypeId -> generation when marked dirty)
    dirty_components: HashMap<NetworkId, HashMap<ComponentTypeId, u64>>,

    /// Last acknowledged generation per client
    client_generations: HashMap<ClientId, u64>,

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
            client_generations: HashMap::new(),
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
    }

    /// Clear dirty flags for a specific entity.
    pub fn clear_entity(&mut self, network_id: NetworkId) {
        self.dirty_components.remove(&network_id);
    }

    /// Register a new client.
    ///
    /// New clients are NOT added to `client_generations` yet, which means
    /// `client_generation()` returns `None` and they see ALL dirty components.
    /// Call `acknowledge_client()` after sending a full state sync.
    pub fn register_client(&mut self, _client_id: ClientId) {
        // Don't insert yet - client will receive everything until acknowledged
    }

    /// Remove a disconnected client.
    pub fn remove_client(&mut self, client_id: ClientId) {
        self.client_generations.remove(&client_id);
    }

    /// Get the last acknowledged generation for a client.
    ///
    /// Returns `None` for clients that have never been registered,
    /// indicating they should receive all dirty updates.
    pub fn client_generation(&self, client_id: ClientId) -> Option<u64> {
        self.client_generations.get(&client_id).copied()
    }

    /// Acknowledge that a client has received updates up to current generation.
    pub fn acknowledge_client(&mut self, client_id: ClientId) {
        self.client_generations.insert(client_id, self.generation);
    }

    /// Get dirty components for a specific client.
    ///
    /// Returns entities and their dirty components that have changed
    /// since the client's last acknowledged generation.
    /// Unregistered clients receive ALL dirty components.
    pub fn get_dirty_for_client(
        &self,
        client_id: ClientId,
    ) -> Vec<(NetworkId, Vec<ComponentTypeId>)> {
        let client_gen = self.client_generation(client_id);

        self
            .dirty_components
            .iter()
            .filter_map(|(&network_id, components)| {
                let dirty: Vec<ComponentTypeId> = match client_gen {
                    // Unregistered client: return all dirty components
                    None => components.keys().copied().collect(),
                    // Registered client: only return components dirty since their last ack
                    Some(acked_gen) => components
                        .iter()
                        .filter(|&(_, component_gen)| *component_gen > acked_gen)
                        .map(|(&id, _)| id)
                        .collect(),
                };

                if dirty.is_empty() {
                    None
                } else {
                    Some((network_id, dirty))
                }
            })
            .collect()
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
        match self.client_generation(client_id) {
            // Unregistered client: has dirty if any components are tracked
            None => !self.dirty_components.is_empty(),
            // Registered client: check if any component is newer than their ack
            Some(client_gen) => self.dirty_components.values().any(|components| {
                components.values().any(|generation| *generation > client_gen)
            }),
        }
    }

    /// Get all network IDs being tracked.
    pub fn tracked_entities(&self) -> impl Iterator<Item = NetworkId> + '_ {
        self.dirty_components.keys().copied()
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
}
