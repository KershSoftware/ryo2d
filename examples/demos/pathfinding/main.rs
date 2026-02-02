//! Pathfinding Demo - Ryo2D Example
//!
//! This example demonstrates the A* pathfinding system:
//! - Grid-based navigation with obstacles
//! - Multiple agents navigating simultaneously
//! - Path smoothing options
//! - Real-time path visualization
//! - Time-sliced pathfinding for many agents
//!
//! ## Controls
//!
//! - Left Click: Set destination for all agents
//! - Right Click/Drag: Add obstacles (drag to paint)
//! - Shift + Right Click: Remove obstacle
//! - Space: Spawn 10 more agents
//! - 1: No smoothing
//! - 2: Line-of-sight smoothing
//! - 3: Spline smoothing
//! - R: Reset (clear obstacles and respawn agents)
//! - G: Toggle grid visualization
//! - T: Toggle movement trails
//! - Escape: Exit

use ryo2d::prelude::*;

const CELL_SIZE: f32 = 16.0;
const AGENT_SPEED: f32 = 120.0;
const INITIAL_AGENTS: usize = 5;

/// Marker component for agents
#[derive(Debug, Clone)]
struct Agent {
    #[allow(dead_code)]
    color: (u8, u8, u8),
}

/// Marker component for obstacles
#[derive(Debug, Clone)]
struct Obstacle;

/// Marker component for destination indicator
#[derive(Debug, Clone)]
struct DestinationMarker;

/// Main demo scene
struct PathfindingDemoScene {
    nav_grid: NavigationGrid,
    pathfinder: Pathfinder,
    agents: Vec<Entity>,
    obstacles: Vec<Entity>,
    smoothing: PathSmoothing,
    screen_w: f32,
    screen_h: f32,
    show_grid: bool,
    agent_sprite: Option<SpriteHandle>,
    obstacle_sprite: Option<SpriteHandle>,
    marker_sprite: Option<SpriteHandle>,
    /// Track last cell modified during drag to avoid repeated toggles
    last_drag_cell: Option<GridCoord>,
    /// Destination marker entity
    destination_marker: Option<Entity>,
    /// Whether trails are enabled
    show_trails: bool,
}

impl PathfindingDemoScene {
    fn new() -> Self {
        Self {
            nav_grid: NavigationGrid::new(1.0, 1.0, CELL_SIZE),
            pathfinder: Pathfinder::with_config(
                PathfinderConfig::default()
                    .with_time_slicing(100) // 100 nodes per frame for smooth performance
                    .with_smoothing(PathSmoothing::LineOfSight),
            ),
            agents: Vec::new(),
            obstacles: Vec::new(),
            smoothing: PathSmoothing::LineOfSight,
            screen_w: 800.0,
            screen_h: 600.0,
            show_grid: false,
            agent_sprite: None,
            obstacle_sprite: None,
            marker_sprite: None,
            last_drag_cell: None,
            destination_marker: None,
            show_trails: true,
        }
    }

    fn create_sprites(&mut self) {
        // Agent sprite (circle-ish)
        let agent = Sprite::solid_rect(12, 12, 100, 200, 255, 255);
        self.agent_sprite = Some(SpriteHandle::from_sprite(agent, "agent"));

        // Obstacle sprite (dark gray block)
        let obstacle = Sprite::solid_rect(
            CELL_SIZE as u32,
            CELL_SIZE as u32,
            60,
            60,
            70,
            255,
        );
        self.obstacle_sprite = Some(SpriteHandle::from_sprite(obstacle, "obstacle"));

        // Destination marker sprite (bright target indicator)
        let marker = Sprite::solid_rect(16, 16, 255, 220, 100, 200);
        self.marker_sprite = Some(SpriteHandle::from_sprite(marker, "marker"));
    }

    fn spawn_agent(&mut self, ctx: &mut SceneContext, x: f32, y: f32) {
        let colors = [
            (100, 200, 255), // Blue
            (255, 150, 100), // Orange
            (100, 255, 150), // Green
            (255, 100, 200), // Pink
            (200, 200, 100), // Yellow
            (200, 100, 255), // Purple
        ];

        let color_idx = self.agents.len() % colors.len();
        let (r, g, b) = colors[color_idx];

        let sprite = Sprite::solid_rect(12, 12, r, g, b, 255);
        let handle = SpriteHandle::from_sprite(sprite, format!("agent_{}", self.agents.len()));

        // Create a trail with the agent's color (slightly transparent)
        let trail = Trail2D::new(Color::new(r, g, b, 180), 3.0)
            .with_length(50)
            .with_taper(0.5)
            .with_fade(true)
            .with_min_distance(4.0);

        let entity = ctx.world.spawn((
            Agent { color: (r, g, b) },
            Transform::new(x, y).with_pivot_center(),
            Renderable::sprite(handle).with_z_order(10),
            NavAgent::new(AGENT_SPEED)
                .with_smoothing(self.smoothing)
                .with_arrival_threshold(4.0),
            trail,
        ));

        self.agents.push(entity);
    }

    fn spawn_initial_agents(&mut self, ctx: &mut SceneContext) {
        let spacing = self.screen_w / (INITIAL_AGENTS + 1) as f32;

        for i in 0..INITIAL_AGENTS {
            let x = spacing * (i + 1) as f32;
            let y = self.screen_h - 50.0;
            self.spawn_agent(ctx, x, y);
        }
    }

    /// Check if there's an obstacle at the given grid coordinate
    fn has_obstacle_at(&self, ctx: &SceneContext, coord: GridCoord) -> bool {
        self.obstacles.iter().any(|&e| {
            if let Some(t) = ctx.world.get::<Transform>(e) {
                self.nav_grid.world_to_grid(t.x, t.y) == coord
            } else {
                false
            }
        })
    }

    /// Add an obstacle at the given world position (does nothing if already exists)
    fn add_obstacle(&mut self, ctx: &mut SceneContext, world_x: f32, world_y: f32) -> bool {
        let coord = self.nav_grid.world_to_grid(world_x, world_y);

        // Skip if already has obstacle
        if self.has_obstacle_at(ctx, coord) {
            return false;
        }

        // Add obstacle
        let (cx, cy) = self.nav_grid.grid_to_world(coord);

        if let Some(sprite) = &self.obstacle_sprite {
            let entity = ctx.world.spawn((
                Obstacle,
                Transform::new(cx, cy).with_pivot_center(),
                Renderable::sprite(sprite.clone()).with_z_order(1),
            ));
            self.obstacles.push(entity);
            return true;
        }
        false
    }

    /// Remove an obstacle at the given world position (does nothing if none exists)
    fn remove_obstacle(&mut self, ctx: &mut SceneContext, world_x: f32, world_y: f32) -> bool {
        let coord = self.nav_grid.world_to_grid(world_x, world_y);

        // Find existing obstacle
        let existing_idx = self.obstacles.iter().position(|&e| {
            if let Some(t) = ctx.world.get::<Transform>(e) {
                self.nav_grid.world_to_grid(t.x, t.y) == coord
            } else {
                false
            }
        });

        if let Some(idx) = existing_idx {
            let entity = self.obstacles.remove(idx);
            let _ = ctx.world.despawn(entity);
            return true;
        }
        false
    }

    fn rebuild_nav_grid_from_world(&mut self, world: &World) {
        self.nav_grid = NavigationGrid::new(self.screen_w, self.screen_h, CELL_SIZE);

        for &entity in &self.obstacles {
            if let Some(t) = world.get::<Transform>(entity) {
                let coord = self.nav_grid.world_to_grid(t.x, t.y);
                self.nav_grid.set(coord, CellState::Blocked);
            }
        }
    }

    fn set_destination_for_all(&mut self, ctx: &mut SceneContext, dest_x: f32, dest_y: f32) {
        // Rebuild grid before pathfinding
        self.rebuild_nav_grid_from_world(ctx.world);

        // Clear trails when setting a new destination
        for &entity in &self.agents {
            if let Some(mut trail) = ctx.world.get_mut::<Trail2D>(entity) {
                trail.clear();
            }
        }

        for &entity in &self.agents {
            // Remove existing NavTarget if any
            let _ = ctx.world.remove_one::<NavTarget>(entity);

            // Add new destination
            ctx.world.add_component(entity, NavTarget::position(dest_x, dest_y));
        }

        // Create or move destination marker
        if let Some(marker_entity) = self.destination_marker {
            // Move existing marker
            if let Some(mut transform) = ctx.world.get_mut::<Transform>(marker_entity) {
                transform.x = dest_x;
                transform.y = dest_y;
            }
        } else if let Some(sprite) = &self.marker_sprite {
            // Create new marker
            let entity = ctx.world.spawn((
                DestinationMarker,
                Transform::new(dest_x, dest_y).with_pivot_center(),
                Renderable::sprite(sprite.clone()).with_z_order(5),
            ));
            self.destination_marker = Some(entity);
        }
    }

    fn update_agents(&mut self, ctx: &mut SceneContext, dt: f32) {
        // Process each agent
        for &entity in &self.agents {
            // Get agent state
            let (agent_state, has_target, target_pos) = {
                let agent = ctx.world.get::<NavAgent>(entity);
                let target = ctx.world.get::<NavTarget>(entity);
                (
                    agent.map(|a| a.state),
                    target.is_some(),
                    target.map(|t| (t.x, t.y)),
                )
            };

            let Some(state) = agent_state else {
                continue;
            };

            match state {
                NavAgentState::Idle if has_target => {
                    // Need to compute path
                    if let Some((tx, ty)) = target_pos {
                        if let Some(transform) = ctx.world.get::<Transform>(entity) {
                            let result = self.pathfinder.find_path(
                                &self.nav_grid,
                                transform.x,
                                transform.y,
                                tx,
                                ty,
                            );

                            if let Some(mut agent) = ctx.world.get_mut::<NavAgent>(entity) {
                                match result {
                                    PathResult::Found(mut path) => {
                                        // Apply smoothing
                                        match self.smoothing {
                                            PathSmoothing::None => {}
                                            PathSmoothing::LineOfSight => {
                                                path.smooth_line_of_sight(&self.nav_grid);
                                            }
                                            PathSmoothing::Spline => {
                                                path.smooth_spline(4);
                                            }
                                        }
                                        agent.set_path(path);
                                    }
                                    PathResult::NoPath | PathResult::InvalidEndpoints => {
                                        agent.fail();
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                NavAgentState::Moving => {
                    // Follow path
                    self.move_agent_along_path(ctx.world, entity, dt);
                }
                NavAgentState::Arrived | NavAgentState::Failed => {
                    // Remove target, go idle
                    let _ = ctx.world.remove_one::<NavTarget>(entity);
                    if let Some(mut agent) = ctx.world.get_mut::<NavAgent>(entity) {
                        agent.stop();
                    }
                }
                _ => {}
            }
        }
    }

    fn move_agent_along_path(&self, world: &mut World, entity: Entity, dt: f32) {
        // Get current position and path info
        let (current_pos, waypoint, speed, threshold) = {
            let transform = world.get::<Transform>(entity);
            let agent = world.get::<NavAgent>(entity);

            match (transform, agent) {
                (Some(t), Some(a)) => {
                    let wp = a.current_path.as_ref().and_then(|p| p.current_waypoint());
                    (Some((t.x, t.y)), wp.map(|w| (w.x, w.y)), a.speed, a.arrival_threshold)
                }
                _ => (None, None, 0.0, 0.0),
            }
        };

        let Some((x, y)) = current_pos else {
            return;
        };

        let Some((wx, wy)) = waypoint else {
            // No more waypoints - arrived
            if let Some(mut agent) = world.get_mut::<NavAgent>(entity) {
                agent.arrive();
            }
            return;
        };

        // Calculate movement
        let dx = wx - x;
        let dy = wy - y;
        let dist = (dx * dx + dy * dy).sqrt();

        if dist <= threshold {
            // Reached waypoint, advance to next
            if let Some(mut agent) = world.get_mut::<NavAgent>(entity) {
                if let Some(ref mut path) = agent.current_path {
                    path.advance();
                    if path.is_complete() {
                        agent.arrive();
                    }
                }
            }
        } else {
            // Move towards waypoint
            let move_dist = speed * dt;
            let ratio = (move_dist / dist).min(1.0);

            if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                transform.x += dx * ratio;
                transform.y += dy * ratio;
            }
        }
    }

    fn update_smoothing(&mut self, ctx: &mut SceneContext, smoothing: PathSmoothing) {
        self.smoothing = smoothing;

        // Update pathfinder config
        self.pathfinder = Pathfinder::with_config(
            PathfinderConfig::default()
                .with_time_slicing(100)
                .with_smoothing(smoothing),
        );

        // Update existing agents
        for &entity in &self.agents {
            if let Some(mut agent) = ctx.world.get_mut::<NavAgent>(entity) {
                agent.path_smoothing = smoothing;
            }
        }
    }

    fn reset(&mut self, ctx: &mut SceneContext) {
        // Despawn all agents
        for entity in self.agents.drain(..) {
            let _ = ctx.world.despawn(entity);
        }

        // Despawn all obstacles
        for entity in self.obstacles.drain(..) {
            let _ = ctx.world.despawn(entity);
        }

        // Despawn destination marker
        if let Some(marker) = self.destination_marker.take() {
            let _ = ctx.world.despawn(marker);
        }

        // Rebuild empty grid
        self.nav_grid = NavigationGrid::new(self.screen_w, self.screen_h, CELL_SIZE);

        // Respawn initial agents
        self.spawn_initial_agents(ctx);
    }

    fn smoothing_name(&self) -> &'static str {
        match self.smoothing {
            PathSmoothing::None => "None",
            PathSmoothing::LineOfSight => "LineOfSight",
            PathSmoothing::Spline => "Spline",
        }
    }

    fn update_trails(&self, ctx: &mut SceneContext, dt: f32) {
        for &entity in &self.agents {
            let pos = ctx.world.get::<Transform>(entity).map(|t| (t.x, t.y));

            if let Some((x, y)) = pos {
                if let Some(mut trail) = ctx.world.get_mut::<Trail2D>(entity) {
                    trail.set_enabled(self.show_trails);
                    if self.show_trails {
                        trail.update(x, y, dt);
                    }
                }
            }
        }
    }
}

impl Scene for PathfindingDemoScene {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        println!("Pathfinding Demo initialized!");
        println!("  Left Click: Set destination");
        println!("  Right Click/Drag: Add obstacles");
        println!("  Shift + Right Click: Remove obstacles");
        println!("  Space: Spawn 10 more agents");
        println!("  1/2/3: Change smoothing mode");
        println!("  G: Toggle grid | T: Toggle trails");
        println!("  R: Reset | ESC: Exit");

        let (screen_w, screen_h) = ctx.screen_size();
        self.screen_w = screen_w;
        self.screen_h = screen_h;

        self.create_sprites();
        self.nav_grid = NavigationGrid::new(screen_w, screen_h, CELL_SIZE);
        self.spawn_initial_agents(ctx);
    }

    fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
        // Exit on escape
        if ctx.input.key_pressed(KeyCode::Escape) {
            return SceneAction::Quit;
        }

        // Reset
        if ctx.input.key_pressed(KeyCode::KeyR) {
            self.reset(ctx);
        }

        // Toggle visualizations
        if ctx.input.key_pressed(KeyCode::KeyG) {
            self.show_grid = !self.show_grid;
        }
        if ctx.input.key_pressed(KeyCode::KeyT) {
            self.show_trails = !self.show_trails;
        }

        // Smoothing modes
        if ctx.input.key_pressed(KeyCode::Digit1) {
            self.update_smoothing(ctx, PathSmoothing::None);
        }
        if ctx.input.key_pressed(KeyCode::Digit2) {
            self.update_smoothing(ctx, PathSmoothing::LineOfSight);
        }
        if ctx.input.key_pressed(KeyCode::Digit3) {
            self.update_smoothing(ctx, PathSmoothing::Spline);
        }

        // Spawn more agents
        if ctx.input.key_pressed(KeyCode::Space) {
            for _ in 0..10 {
                let x = 50.0 + (self.agents.len() as f32 * 7.0) % (self.screen_w - 100.0);
                let y = self.screen_h - 50.0;
                self.spawn_agent(ctx, x, y);
            }
            println!("Total agents: {}", self.agents.len());
        }

        // Mouse input
        let (mx, my) = ctx.input.mouse_position();
        let (mx, my) = ctx.camera.screen_to_world(mx, my);

        // Left click - set destination
        if ctx.input.mouse_pressed(MouseButton::Left) {
            self.set_destination_for_all(ctx, mx, my);
        }

        // Right click/drag - add or remove obstacles
        let right_down = ctx.input.mouse_down(MouseButton::Right);
        let shift_held = ctx.input.key_down(KeyCode::ShiftLeft) || ctx.input.key_down(KeyCode::ShiftRight);

        if right_down {
            let current_cell = self.nav_grid.world_to_grid(mx, my);

            // Only process if we moved to a new cell (or just started clicking)
            if self.last_drag_cell != Some(current_cell) {
                self.last_drag_cell = Some(current_cell);

                let changed = if shift_held {
                    // Shift + Right click = remove obstacle
                    self.remove_obstacle(ctx, mx, my)
                } else {
                    // Right click = add obstacle
                    self.add_obstacle(ctx, mx, my)
                };

                if changed {
                    self.rebuild_nav_grid_from_world(ctx.world);
                }
            }
        } else {
            // Mouse released - reset drag tracking
            self.last_drag_cell = None;
        }

        // Update agent movement
        self.update_agents(ctx, dt);

        // Update trails with current agent positions
        self.update_trails(ctx, dt);

        SceneAction::Continue
    }

    fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, _camera: &Camera) {
        // Dark background
        for pixel in frame.chunks_exact_mut(4) {
            pixel[0] = 30; // R
            pixel[1] = 35; // G
            pixel[2] = 40; // B
            pixel[3] = 255; // A
        }

        // Draw grid if enabled
        if self.show_grid {
            let grid_color = [50, 55, 60, 255];

            // Vertical lines
            let mut x = 0.0;
            while x < width as f32 {
                let px = x as u32;
                if px < width {
                    for y in 0..height {
                        let idx = ((y * width + px) * 4) as usize;
                        if idx + 3 < frame.len() {
                            frame[idx] = grid_color[0];
                            frame[idx + 1] = grid_color[1];
                            frame[idx + 2] = grid_color[2];
                            frame[idx + 3] = grid_color[3];
                        }
                    }
                }
                x += CELL_SIZE;
            }

            // Horizontal lines
            let mut y = 0.0;
            while y < height as f32 {
                let py = y as u32;
                if py < height {
                    for px in 0..width {
                        let idx = ((py * width + px) * 4) as usize;
                        if idx + 3 < frame.len() {
                            frame[idx] = grid_color[0];
                            frame[idx + 1] = grid_color[1];
                            frame[idx + 2] = grid_color[2];
                            frame[idx + 3] = grid_color[3];
                        }
                    }
                }
                y += CELL_SIZE;
            }
        }
    }

    fn render_ui(&mut self, ui: &mut UiContext) {
        let text_light = Color::rgb(220, 220, 220);
        let text_accent = Color::rgb(130, 180, 255);
        let text_muted = Color::rgb(160, 160, 160);

        // Title
        ui.draw_text("Pathfinding Demo", 10.0, 10.0, text_light);

        // Stats
        let stats = format!(
            "Agents: {} | Obstacles: {} | Smoothing: {}",
            self.agents.len(),
            self.obstacles.len(),
            self.smoothing_name()
        );
        ui.draw_text(&stats, 10.0, 30.0, text_accent);

        // Controls
        ui.draw_text(
            "LMB: Destination | RMB/Drag: Add Obstacles | Shift+RMB: Remove",
            10.0,
            50.0,
            text_muted,
        );
        ui.draw_text(
            "Space: +10 Agents | 1/2/3: Smoothing | G: Grid | T: Trails | R: Reset",
            10.0,
            66.0,
            text_muted,
        );
    }
}

fn main() {
    let engine = EngineBuilder::new()
        .title("Pathfinding Demo - Ryo2D")
        .resolution(1024, 768)
        .window_mode(WindowMode::ResizableAspectRatio)
        .fps(60)
        .physics_hz(60)
        .with_scene(PathfindingDemoScene::new())
        .build();

    if let Err(e) = engine.run() {
        eprintln!("Engine error: {}", e);
    }
}
