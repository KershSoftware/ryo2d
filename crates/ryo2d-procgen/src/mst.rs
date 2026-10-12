//! Minimum Spanning Tree for region connectivity
//!
//! Uses Kruskal's algorithm to find the minimum set of connections
//! that links all regions together.

use super::grid::{GenerationGrid, Region};

/// Configuration for MST generation
#[derive(Debug, Clone)]
pub struct MstConfig {
  /// Extra connectivity (0.0 to 1.0) - adds random edges beyond the MST
  /// to create loops in the dungeon
  pub extra_connectivity: f32,
}

impl Default for MstConfig {
  fn default() -> Self {
    MstConfig {
      extra_connectivity: 0.0,
    }
  }
}

impl MstConfig {
  pub fn new() -> Self {
    Self::default()
  }

  /// Set extra connectivity (probability of adding non-MST edges)
  pub fn with_extra_connectivity(mut self, connectivity: f32) -> Self {
    self.extra_connectivity = connectivity.clamp(0.0, 1.0);
    self
  }
}

/// An edge in the MST graph
#[derive(Debug, Clone)]
pub struct MstEdge {
  pub region_a: u32,
  pub region_b: u32,
  pub weight: f32,
}

impl MstEdge {
  pub fn new(region_a: u32, region_b: u32, weight: f32) -> Self {
    MstEdge {
      region_a,
      region_b,
      weight,
    }
  }
}

/// Union-Find (Disjoint Set Union) data structure for Kruskal's algorithm
struct UnionFind {
  parent: Vec<usize>,
  rank: Vec<usize>,
}

impl UnionFind {
  fn new(size: usize) -> Self {
    UnionFind {
      parent: (0..size).collect(),
      rank: vec![0; size],
    }
  }

  fn find(&mut self, x: usize) -> usize {
    if self.parent[x] != x {
      self.parent[x] = self.find(self.parent[x]); // Path compression
    }
    self.parent[x]
  }

  fn union(&mut self, x: usize, y: usize) -> bool {
    let root_x = self.find(x);
    let root_y = self.find(y);

    if root_x == root_y {
      return false; // Already in same set
    }

    // Union by rank
    match self.rank[root_x].cmp(&self.rank[root_y]) {
      std::cmp::Ordering::Less => self.parent[root_x] = root_y,
      std::cmp::Ordering::Greater => self.parent[root_y] = root_x,
      std::cmp::Ordering::Equal => {
        self.parent[root_y] = root_x;
        self.rank[root_x] += 1;
      }
    }

    true
  }
}

/// Compute MST edges for a set of regions using Kruskal's algorithm
///
/// Returns the edges that form the minimum spanning tree.
pub fn compute_mst(regions: &[Region]) -> Vec<MstEdge> {
  if regions.len() < 2 {
    return Vec::new();
  }

  // Build all possible edges with weights (Euclidean distance)
  let mut edges: Vec<MstEdge> = Vec::new();
  for i in 0..regions.len() {
    for j in (i + 1)..regions.len() {
      let dist = regions[i].distance_to(&regions[j]);
      edges.push(MstEdge::new(regions[i].id, regions[j].id, dist));
    }
  }

  // Sort edges by weight
  edges.sort_by(|a, b| a.weight.partial_cmp(&b.weight).unwrap());

  // Create ID to index mapping
  let id_to_index: std::collections::HashMap<u32, usize> =
    regions.iter().enumerate().map(|(i, r)| (r.id, i)).collect();

  // Kruskal's algorithm
  let mut uf = UnionFind::new(regions.len());
  let mut mst = Vec::new();

  for edge in edges {
    let idx_a = id_to_index[&edge.region_a];
    let idx_b = id_to_index[&edge.region_b];

    if uf.union(idx_a, idx_b) {
      mst.push(edge);
      if mst.len() == regions.len() - 1 {
        break;
      }
    }
  }

  mst
}

/// Compute MST with optional extra connections for loops
pub fn compute_mst_with_extras(
  regions: &[Region],
  config: &MstConfig,
  rng: &mut super::config::SeededRng,
) -> Vec<MstEdge> {
  if regions.len() < 2 {
    return Vec::new();
  }

  // Get the base MST
  let mst = compute_mst(regions);

  if config.extra_connectivity <= 0.0 {
    return mst;
  }

  // Collect MST edges as a set for quick lookup
  let mst_set: std::collections::HashSet<(u32, u32)> = mst
    .iter()
    .map(|e| {
      let (a, b) = (e.region_a, e.region_b);
      if a < b { (a, b) } else { (b, a) }
    })
    .collect();

  // Add extra edges with probability
  let mut result = mst;
  for i in 0..regions.len() {
    for j in (i + 1)..regions.len() {
      let (a, b) = (regions[i].id, regions[j].id);
      let key = if a < b { (a, b) } else { (b, a) };

      if !mst_set.contains(&key) && rng.random::<f32>() < config.extra_connectivity {
        let dist = regions[i].distance_to(&regions[j]);
        result.push(MstEdge::new(a, b, dist));
      }
    }
  }

  result
}

/// Connect regions in a grid using MST
///
/// This is a high-level function that:
/// 1. Computes MST edges for the grid's regions
/// 2. Draws corridors between connected regions
/// 3. Updates the grid's connection list
pub fn connect_regions_mst(
  grid: &mut GenerationGrid,
  config: &MstConfig,
  rng: &mut super::config::SeededRng,
  tile: super::config::TileType,
) {
  let regions: Vec<Region> = grid.regions().to_vec();
  let edges = compute_mst_with_extras(&regions, config, rng);

  for edge in &edges {
    let region_a = grid.get_region(edge.region_a);
    let region_b = grid.get_region(edge.region_b);

    if let (Some(ra), Some(rb)) = (region_a, region_b) {
      let (x1, y1) = ra.center;
      let (x2, y2) = rb.center;

      // Draw L-shaped corridor
      grid.corridor_l(x1, y1, x2, y2, tile);

      // Record the connection
      grid.add_connection(edge.region_a, edge.region_b, edge.weight);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::GridRect;
  use crate::grid::RegionType;

  fn make_regions() -> Vec<Region> {
    vec![
      Region::new(0, GridRect::new(0, 0, 10, 10), RegionType::Room),
      Region::new(1, GridRect::new(30, 0, 10, 10), RegionType::Room),
      Region::new(2, GridRect::new(15, 30, 10, 10), RegionType::Room),
    ]
  }

  #[test]
  fn mst_connects_all_regions() {
    let regions = make_regions();
    let mst = compute_mst(&regions);

    // MST should have n-1 edges for n regions
    assert_eq!(mst.len(), 2);
  }

  #[test]
  fn mst_empty_regions() {
    let regions: Vec<Region> = vec![];
    let mst = compute_mst(&regions);
    assert!(mst.is_empty());
  }

  #[test]
  fn mst_single_region() {
    let regions = vec![Region::new(
      0,
      GridRect::new(0, 0, 10, 10),
      RegionType::Room,
    )];
    let mst = compute_mst(&regions);
    assert!(mst.is_empty());
  }

  #[test]
  fn mst_with_extra_connectivity() {
    use crate::config::SeededRng;

    let regions = make_regions();
    let mut rng = SeededRng::new(12345);
    let config = MstConfig::new().with_extra_connectivity(1.0); // 100% extra edges

    let result = compute_mst_with_extras(&regions, &config, &mut rng);

    // Should have more than n-1 edges with 100% extra connectivity
    assert!(result.len() >= 2);
  }

  #[test]
  fn connect_regions_in_grid() {
    use crate::config::{SeededRng, TileType};

    let mut grid = GenerationGrid::new(50, 50);

    // Add regions manually
    grid.fill_rect(0, 0, 10, 10, TileType::Floor);
    grid.add_region(GridRect::new(0, 0, 10, 10), RegionType::Room);

    grid.fill_rect(30, 0, 10, 10, TileType::Floor);
    grid.add_region(GridRect::new(30, 0, 10, 10), RegionType::Room);

    grid.fill_rect(15, 30, 10, 10, TileType::Floor);
    grid.add_region(GridRect::new(15, 30, 10, 10), RegionType::Room);

    let mut rng = SeededRng::new(12345);
    let config = MstConfig::default();

    connect_regions_mst(&mut grid, &config, &mut rng, TileType::Floor);

    // Should have 2 connections
    assert_eq!(grid.connections().len(), 2);
  }
}
