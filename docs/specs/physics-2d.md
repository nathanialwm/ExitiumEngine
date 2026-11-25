# Spec: 2D physics on rapier2d

- Issue: #108, amended by #143 (SP7: the command bus (AM5 of `editor-mcp-mvp.md`), debug draw surfaces, the character controller, tile-collider merging and the joints decision)
- Crates: `exitium_physics` (Stage 1 standalone wrapper, Stage 2 `plugin` module, Stage 3 command surface, Stage 4 tooling). Depends on (Stage 2): `exitium_ecs`, `exitium_app` (#106), `exitium_reflect`, `exitium_command` (#110, `editor-mcp-mvp.md`; also a dev-dependency, see "Intake wiring"). No edge to `exitium_render`, `exitium_mcp`, `exitium_editor` or `exitium_tilemap`: runtime crates have no edges between them (C1 of `mvp-roadmap.md`), and `exitium_command` builds the MCP tools and editor panels from reflection and the command registry. The facade (`crates/exitium/`, a hotspot) holds any cross-crate glue.
- Status: draft

## Goal

`exitium_physics` gives a game rigid-body physics: falling and colliding bodies, platforms, triggers, ray queries, a kinematic character controller and tile-map colliders. It is a thin, safe layer over `rapier2d` 0.36.1 (INTERVIEW #20). The roadmap's sidescroller, top-down and isometric example games are all buildable on it (see "Game types"). It is built in four stages that match the crate graph and the lanes in `mvp-roadmap.md`:

- **Stage 1: a standalone wrapper** (`PhysicsWorld`, P1 to P7). No ECS, no app. It depends only on `exitium_core` plus `rapier2d`. A hand-written loop can drive it with `exitium_core::time::FixedClock`, so the crate is built and tested in parallel with the ECS.
- **Stage 2: a `plugin` module** (P8 to P14). It adds the `RigidBody` and `Collider` components, the fixed-step system, `Transform2d` sync, interpolation for rendering, event delivery, queries and debug draw data.
- **Stage 3: the command surface** (P15, P16). The plugin registers one command (`physics_apply_impulse`) and five queries with `exitium_command`. Every edit of a component or a resource is the generic ECS command, so physics adds no other command.
- **Stage 4: tooling** (P17 to P20). The character controller (a wrapper method and a component, with slopes, steps and one-way platforms), tile-collider merging, and the debug draw surfaces (the facade glue contract and an MCP query).

The spec decides these things so that the implementation issues need no further design: **physics runs in SI-scaled meters internally and the game sees world units** (one conversion at the boundary); **one rapier step per `FixedUpdate` tick, with render interpolation on by default**; **no rapier type appears in the public API**; **`Transform2d` is the visual transform, the physics world owns simulation state**; **repeatable on one build and platform, not cross-platform deterministic**; **no physics-specific edit commands, only the generic undoable ones**; **joints are a post-MVP non-goal** (owner decision, 2026-11-25).

## Non-goals

- **Joints, motors, soft bodies, multibody.** rapier 0.36 has them. **A post-MVP non-goal, by owner decision (2026-11-25)**: not built in the MVP and not an open question, see "Joints". A joint is one more component and one more `PhysicsWorld` method, and nothing here blocks it (design rule 5).
- **Concave shapes** (polylines, trimeshes, decomposition). Authors build levels from boxes and convex polygons. Tilemaps use merged boxes (see "Tile colliders"); a sloped tile is authored as a `ConvexPolygon` collider.
- **One-way platforms for dynamic bodies.** One-way platforms for the character controller are built (see "Character controller"): a predicate inside `move_character`, no solver change and no `Collider` field. A dynamic crate that should also pass through a jump-through platform would need a `Collider` field and a contact-pair hook in the solver, which is a hot-path change; not built, and additive (design rule 5).
- **Position-based kinematic bodies.** Kinematic bodies are velocity-based (a moving platform carries the bodies on it). The character controller moves its body by setting that velocity.
- **Cross-platform determinism, lockstep and rollback.** INTERVIEW #10: determinism is not required.
- **Multithreaded stepping and `exitium_tasks` integration.** Not built: one thread meets every budget below with 3 to 4 times of headroom, rapier's `parallel` feature stays off, and the web build has no threads (web threads need nightly Rust, `mvp-roadmap.md` owner decision 1). The path stays open, see "Threading". A budget miss on min-spec is what would reopen it.
- **Editor gizmos** that drag collider handles in the viewport. The editor edits colliders through the generated inspector.
- Re-exporting through the `exitium` facade. That is a separate PR (the facade is a high-risk area, ADR-0005).

## Design

### Decisions

| Decision | Choice | Why |
|---|---|---|
| Engine | `rapier2d` 0.36.1 (crates.io, Apache-2.0), `default-features = false`, features `dim2`, `f32`, `std`, `block-solver`, `debug-render` | Chosen in INTERVIEW #20. `f32` because the engine's math is f32 (`Vec2`); `block-solver` is what rapier enables by default in 2D, for stable stacks. `debug-render` has no extra dependencies and gives us the line generator for debug draw. `parallel`, `serde-serialize`, `simd8` and `enhanced-determinism` are off. |
| wasm | Supported, verified for this spec | A scratch crate with exactly these features built for `wasm32-unknown-unknown` on the pinned 1.100.0 toolchain (rapier2d 0.36.1, parry2d 0.31.1, nalgebra 0.35.0, no `wasm-bindgen` or other web dependency). rapier is the engine behind the official `@dimforge/rapier2d` wasm package. Its SIMD solver is backed by `wide`, which falls back to scalar code on wasm32 without `simd128`. The `profiler` feature (it pulls in `web-time`) stays off. The Stage 1 intake PR repeats the build as its acceptance test. |
| Units | The physics world uses **meters**. The game uses **world units**. `PhysicsConfig::units_per_meter` (default `32.0`) converts, in one place (`convert.rs` and the Stage 2 sync). | rapier is tuned for dynamic bodies of roughly 0.1 to 10 units and its contact tolerances are absolute. A 16 px body in a pixel world is 160 times too small. The conversion is a multiply at the boundary. See "Units". |
| Public API | No rapier type is public. `BodyId`, `ColliderId`, `RayHit`, `CollisionEvent`, `DebugDraw` and the settings types are ours. | rapier's API changes every minor release (0.35 to 0.36 changed `PhysicsPipeline::step`, `EventHandler` and the debug renderer). Our API absorbs that. It also keeps reflection and wasm free of rapier generics. |
| Math types | Our API uses `exitium_core::math::Vec2` (glam 0.34.3). rapier 0.36.1 uses glam **0.33** through `glamx`, a different crate version, so the types are distinct. `convert.rs` copies component by component (`Vec2::new(v.x, v.y)`). | Two glam versions in the build is accepted (`deny.toml` `multiple-versions = "warn"`). There is no `transmute`, no `unsafe`, and no `glamx` dependency of our own. |
| Stepping | One `PhysicsWorld::step(dt)` per `FixedUpdate` tick with `dt = Time::fixed_step`. No sub-stepping by default. | See "Fixed step and interpolation". |
| Interpolation | **On by default** for dynamic and kinematic bodies (`RigidBody::interpolate`). | See "Fixed step and interpolation". |
| Ownership of state | `Transform2d` is the visual transform. The simulation state lives in the physics world. `Velocity` is exchanged both ways. | See "Stage 2: components and sync". |
| Determinism | Repeatable for the same build, platform and inputs. No promise across platforms. | See "Determinism". |
| Command surface | One command, `physics_apply_impulse` (`UndoKind::NotUndoable`), and five queries, registered by the plugin. Config and debug settings are edited with the generic `ecs_set_resource_field`. | AM5 of `editor-mcp-mvp.md`. A physics-only wrapper over a generic undoable command would be a second schema for the same edit (design rule 2). See "MCP / editor surface". |
| Character controller | rapier's `KinematicCharacterController` behind `PhysicsWorld::move_character`, driven by a `CharacterController` component on a kinematic body. Stage 4. | A platformer feels wrong with a dynamic body, and the sidescroller and top-down examples need slopes, steps and wall sliding. The wrapper keeps every rapier type private. See "Character controller". |
| Tile colliders | A pure function merges a grid of solid cells into boxes. The facade glue turns the boxes into child entities with static `Collider`s. No tilemap type in this crate and no edge to `exitium_tilemap`. | C1 forbids the edge. Static boxes need no new collider kind. See "Tile colliders". |
| Joints | A post-MVP non-goal (owner decision, 2026-11-25). | No example game needs one; the additive design is recorded in "Joints". |
| Debug draw | Data in the plugin (`PhysicsDebugDraw`), copied by facade glue; plus an on-demand MCP query, `physics_debug_draw`. | The glue draws for humans in the viewport; the query lets an agent read the same shapes as numbers without toggling anything. See "Debug draw". |

### Crate layout

```
crates/exitium_physics/src/
  lib.rs        module docs, re-exports
  types.rs      BodyKind, RigidBody, Collider, Shape, Material, CollisionLayers, Velocity, BodyPose, BodyId, ColliderId
  error.rs      PhysicsError
  convert.rs    the only file that names rapier math types: glam 0.34 <-> 0.33, our settings -> rapier builders
  world.rs      PhysicsWorld: lifecycle, step, state getters, interpolation snapshots
  events.rs     CollisionEvent, the rapier event collector
  query.rs      QueryFilter, RayHit, ShapeHit, ContactInfo and the query methods
  debug.rs      DebugDraw, DebugLine, DebugDrawOptions, the rapier debug-render backend
  controller.rs CharacterSettings, CharacterResult and move_character (Stage 4)
  tiles.rs      CellGrid, CellRect, CellMerger: merge solid cells into boxes (Stage 4)
  plugin/       (feature `plugin`, on by default; Stage 2)
    mod.rs        PhysicsPlugin
    components.rs BodyLink, ColliderLink (internal) and Component impls for the types above
    config.rs     PhysicsConfig, PhysicsDebugSettings, PhysicsStats (reflected resources)
    reconcile.rs  create, update and remove bodies and colliders; apply PhysicsConfig changes
    step.rs       FixedFirst/FixedLast sync-in, step, write-back; PostUpdate interpolation
    events.rs     PhysicsEvents resource
    queries.rs    entity-level query facade
    debug.rs      PhysicsDebugDraw and PhysicsDebugView resources, the debug_draw system
    commands.rs   the physics_apply_impulse command (Stage 3; needs exitium_command)
    tools.rs      the five Stage 3 queries and physics_debug_draw (Stage 4) with their reflected input and output types
    controller.rs CharacterController and CharacterState components and the move system (Stage 4)
benches/step.rs  benches/query.rs  benches/debug.rs  benches/sync.rs  benches/controller.rs  benches/tiles.rs
```

Everything is safe code. rapier does its own `unsafe` internally; this crate adds none (no `#[allow(unsafe_code)]` module).

Features: `reflect` (derives `Reflect` on the settings types through `cfg_attr`, so Stage 1 builds without `exitium_reflect`) and `plugin` (implies `reflect`; adds the `exitium_ecs`, `exitium_app`, `exitium_reflect` and `exitium_command` dependencies). `default = ["plugin"]`; standalone use is `default-features = false`. `exitium_command` is also a dev-dependency (the P15 and P16 tests round-trip every command and query through its schema API).

### Units

| Quantity | Stage 1 `PhysicsWorld` | Stage 2 components and resources |
|---|---|---|
| Length (positions, sizes, offsets, radii, query distances) | physics units (meters, by convention; the crate does not enforce it) | world units, divided by `units_per_meter` on the way in and multiplied on the way out |
| Velocity, acceleration, gravity | physics units per second | world units per second (`gravity` too) |
| Mass | kilograms, from `density` (kg per m2 of shape area) | same, `density` is kg per m2 |
| Impulse | kg times physics units per second | kg times world units per second |
| Angle | radians, counter-clockwise | same |
| Time | seconds, `f32` | seconds |

The default gravity is `(0.0, -9.81 * 32.0)` world units per second squared. The physics core is orientation-agnostic: gravity is a plain vector. The default assumes **y-up**, which `app-platform.md` settled (open question 1); a game with another convention sets `gravity` itself. `Transform2d::scale` is **ignored** for physics. Shape sizes live only in `Collider`, and the plugin records an issue (see "Diagnostics") for a body whose scale is not 1.

### Data layout (Stage 1)

```rust
pub struct PhysicsWorld {
    // rapier state, all private: PhysicsPipeline, IslandManager, broad phase, narrow phase,
    // RigidBodySet, ColliderSet, CCDSolver, IntegrationParameters, gravity.
    // The implementer may use rapier's `PhysicsWorld` bundle or the individual sets;
    // the public API does not change.
    records: Vec<Option<BodyRecord>>,   // indexed by the rapier handle index; see below
    events: Vec<CollisionEvent>,        // filled during step, drained by the caller; capacity reused
    collector: EventCollector,          // rapier EventHandler: a Mutex<Vec<RawEvent>> (the trait needs Sync)
    tick: u64,                          // steps taken
    stats: StepStats,
}

// 32 bytes per body. `prev` and `pose` are the last two step results, in physics units.
struct BodyRecord { generation: u32, prev: BodyPose, pose: BodyPose }
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyPose { pub position: Vec2, pub angle: f32 }   // 12 bytes
```

`BodyId` and `ColliderId` are `Copy + Eq + Hash` newtypes over rapier's generational handles (index plus generation), so a stale id after `remove_body` is a clean `false` or `None`, never another body. Each has `to_bits() -> u64` for logs. Both rapier bodies and colliders carry a caller-supplied `user_data: u64`. Stage 2 stores `Entity::to_bits()` there (the ECS spec defines it as a `u64`), so an event or a hit maps back to an entity with one `Entity::from_bits`.

Settings types, one definition used by both stages (reflected when `reflect` is on; all derive `Clone`, `Debug`, `PartialEq`, `Default`):

```rust
pub enum BodyKind { Static, Dynamic, Kinematic }          // Kinematic = velocity-based
pub struct RigidBody {                                    // Stage 2 component
    pub kind: BodyKind,           // default Dynamic
    pub gravity_scale: f32,       // 1.0
    pub linear_damping: f32,      // 0.0, >= 0
    pub angular_damping: f32,     // 0.0, >= 0
    pub lock_rotation: bool,      // false
    pub can_sleep: bool,          // true
    pub ccd: bool,                // false: continuous collision detection for fast "bullets"
    pub interpolate: bool,        // true: render interpolation (Stage 2 only; Stage 1 always records poses)
}
pub struct Velocity { pub linear: Vec2, pub angular: f32 } // Stage 2 component, also the Stage 1 argument
pub enum Shape {
    Box { size: Vec2 },                                   // full width and height, both > 0
    Circle { radius: f32 },                               // > 0
    Capsule { length: f32, radius: f32 },                 // vertical; length = segment between the cap centers, >= 0; total height = length + 2 * radius
    ConvexPolygon { points: Vec<Vec2> },                  // 3 to 64 points; the convex hull is used (rapier convex_hull); collinear or duplicate-only input is an error
}
pub struct Material {
    pub friction: f32,            // 0.5, >= 0
    pub restitution: f32,         // 0.0, 0..=1 recommended, >= 0 required
    pub friction_combine: CombineRule,     // Average (default), Min, Max, Multiply
    pub restitution_combine: CombineRule,  // Average
}
pub struct CollisionLayers { pub memberships: u32, pub filter: u32 }  // default memberships 1, filter u32::MAX
pub struct Collider {                                     // Stage 2 component
    pub shape: Shape,
    pub offset: Vec2,             // local, from the body (or entity) origin; default zero
    pub angle: f32,               // local rotation; default 0
    pub sensor: bool,             // false: a sensor reports overlaps and never pushes
    pub density: f32,             // 1.0, >= 0; a body's mass is the sum over its colliders
    pub material: Material,
    pub layers: CollisionLayers,  // A and B collide iff (a.memberships & b.filter) != 0 and (b.memberships & a.filter) != 0
    pub report_events: bool,      // true: emit CollisionEvent. Set false for bulk debris.
}
```

A `Collider` with no body is a **static collider**: rapier allows a collider with no parent body, so no fixed body is created for it. Mass comes only from `density`; there is no manual mass or center-of-mass override in the MVP.

Colliders use `ActiveCollisionTypes` `DYNAMIC_DYNAMIC | DYNAMIC_KINEMATIC | DYNAMIC_FIXED | KINEMATIC_KINEMATIC | KINEMATIC_FIXED`, so a kinematic player or platform still triggers sensors. (rapier's default omits the two kinematic pairs; the implementer checks the constant names against 0.36.1.) Colliders with `report_events` (and every sensor) set `ActiveEvents::COLLISION_EVENTS`.

### Public API sketch (Stage 1)

```rust
impl PhysicsWorld {
    pub fn new(config: WorldConfig) -> Self;                 // WorldConfig { gravity: Vec2, max_events: usize }
    pub fn set_gravity(&mut self, gravity: Vec2);

    // Lifecycle. Bad input (NaN, negative size, empty hull, stale id) is a PhysicsError, never a panic.
    pub fn add_body(&mut self, body: &RigidBody, pose: BodyPose, velocity: Velocity, user_data: u64)
        -> Result<BodyId, PhysicsError>;
    pub fn add_collider(&mut self, parent: Option<BodyId>, collider: &Collider, local: BodyPose, user_data: u64)
        -> Result<ColliderId, PhysicsError>;                // parent None: `local` is a world pose (static collider)
    pub fn remove_body(&mut self, id: BodyId) -> bool;      // also removes its colliders
    pub fn remove_collider(&mut self, id: ColliderId) -> bool;
    pub fn update_body(&mut self, id: BodyId, body: &RigidBody) -> Result<(), PhysicsError>;
    pub fn update_collider(&mut self, id: ColliderId, collider: &Collider) -> Result<(), PhysicsError>;

    // Runtime control
    pub fn set_pose(&mut self, id: BodyId, pose: BodyPose);       // teleport: also sets prev = pose, so no interpolation across the jump
    pub fn set_velocity(&mut self, id: BodyId, velocity: Velocity);
    pub fn apply_impulse(&mut self, id: BodyId, impulse: Vec2, at_world_point: Option<Vec2>);

    // Stepping
    pub fn step(&mut self, dt: f32) -> Result<(), PhysicsError>;  // dt must be finite and >= 0; dt == 0 only applies pending changes
    pub fn drain_events(&mut self, out: &mut Vec<CollisionEvent>);
    pub fn tick(&self) -> u64;

    // Reading state
    pub fn body_state(&self, id: BodyId) -> Option<BodyState>;   // pose, Velocity, sleeping, mass
    pub fn interpolated_pose(&self, id: BodyId, alpha: f32) -> Option<BodyPose>;  // lerp(prev, pose, alpha.clamp(0,1)), shortest-arc angle
    pub fn active_bodies(&self) -> impl Iterator<Item = BodyId> + '_;             // awake non-static only
    pub fn body_user_data(&self, id: BodyId) -> Option<u64>;
    pub fn collider_user_data(&self, id: ColliderId) -> Option<u64>;
    pub fn contacts_of(&self, id: ColliderId, out: &mut Vec<ContactInfo>);        // ContactInfo { other: ColliderId, normal: Vec2 }, world normal pointing at `id`

    // Queries (see "Events and queries") and debug draw (see "Debug draw")
    pub fn cast_ray(&self, ray: Ray, max_distance: f32, filter: &QueryFilter) -> Option<RayHit>;
    pub fn cast_ray_all(&self, ray: Ray, max_distance: f32, filter: &QueryFilter, out: &mut Vec<RayHit>); // sorted by distance
    pub fn colliders_at_point(&self, point: Vec2, filter: &QueryFilter, out: &mut Vec<ColliderId>);
    pub fn overlap_shape(&self, shape: &Shape, pose: BodyPose, filter: &QueryFilter, out: &mut Vec<ColliderId>);
    pub fn cast_shape(&self, shape: &Shape, pose: BodyPose, velocity: Vec2, max_distance: f32, filter: &QueryFilter) -> Option<ShapeHit>;
    pub fn update_queries(&mut self);                       // refresh the query structure after edits and before the first step
    pub fn debug_draw(&self, options: &DebugDrawOptions, out: &mut DebugDraw);
    pub fn stats(&self) -> PhysicsWorldStats;

    // Character controller (Stage 4, see "Character controller")
    pub fn move_character(&mut self, body: BodyId, settings: &CharacterSettings, desired: Vec2, dt: f32, filter: &QueryFilter)
        -> Result<CharacterResult, PhysicsError>;
}
// Tile colliders (Stage 4, see "Tile colliders"): a free-standing type, not a method of `PhysicsWorld`.
// impl CellMerger { pub fn merge(&mut self, grid: &CellGrid<'_>, out: &mut Vec<CellRect>) -> Result<(), PhysicsError>; }
```

`PhysicsError` is `NonFinite { what }`, `InvalidShape { reason }`, `OutOfRange { field, value }`, `UnknownId`. It implements `Display` and `std::error::Error` (via `exitium_core::error` conventions). Every method that takes an id returns `bool`, `Option` or `Result` for a stale one.

`step` order (one call): (1) for each awake non-static body, copy `pose` into `prev` (O(active)); (2) apply `dt` to rapier's integration parameters and run the pipeline with our event collector; (3) for each awake non-static body, read the new pose into `pose`; (4) move the collector's events into `events` (capped at `max_events`, default 16,384; the overflow count is in `stats`); (5) `tick += 1`. Bodies that sleep keep `prev == pose`. No allocation happens in `step` after warm-up: buffers are reused.

A hand-written Stage 1 loop:

```rust
let adv = clock.advance(frame_seconds);                       // exitium_core::time::FixedClock
for _ in 0..adv.ticks { world.step(clock.step() as f32); }
let alpha = (clock.remainder() / clock.step()) as f32;        // 0.0 up to just under 1.0
let draw_at = world.interpolated_pose(id, alpha);
```

### Fixed step and interpolation

**Stepping.** Physics steps once per `FixedUpdate` tick, 60 Hz by default (the contract), with `dt = Time::fixed_step as f32`. The stepping system runs **after** the game's `FixedUpdate` systems in the tick, so a system that sets a `Velocity` this tick sees it take effect this tick. `FixedClock` already caps the ticks per frame (`max_ticks`) and drops whole steps on a stall, so physics cannot spiral. There is no sub-stepping: `IntegrationParameters` keeps rapier's solver iteration defaults. A game that needs more stability for stacks raises `PhysicsConfig::solver_iterations` (a field of the reflected config, mapped to rapier's `num_solver_iterations`). Fast small bodies use `RigidBody::ccd`.

**Interpolation: yes, by default.** At 60 Hz fixed with a 144 Hz display, or any frame rate that is not a multiple of 60, a frame runs 0, 1 or 2 ticks, and an uninterpolated sprite moves in visible hitches. Interpolation draws a body at `lerp(prev, pose, alpha)` where `alpha = FixedClock::remainder() / FixedClock::step()`. It costs one `BodyPose` pair per body and the render is up to one tick (16.7 ms) behind the simulation, which is imperceptible and is the standard trade. A body can opt out (`interpolate = false`) and then shows the latest simulation pose. **Extrapolation is not used**: it overshoots on collisions. Rotation is interpolated along the shortest arc (the angle difference is wrapped to `[-pi, pi]`).

The plugin needs the fraction of the next tick that has accumulated, in `[0, 1)`, at the point where interpolation runs. `exitium_app` exposes it as `Time::fixed_alpha` (`app-platform.md`), valid in `Update` and later, which includes `PostUpdate`. Without it the plugin cannot interpolate.

### Determinism

INTERVIEW #10: determinism is not required. The stance:

- **Same build, same platform, same inputs, same order of calls: the same result.** rapier is repeatable in that situation. The test plan checks it: two worlds fed the same scripted inputs for 600 steps have bit-identical body poses (compared with `to_bits`).
- **No promise across platforms or builds.** Native Windows and wasm may differ in the last bits, because float code paths differ (SIMD vs scalar). Do not build lockstep or rollback on this.
- **Our own code adds no nondeterminism**: no `HashMap` iteration order in any observable output (events, query results and debug lines are in a defined order: events in rapier's order, query results by distance then `ColliderId`), no wall-clock reads, no thread use.
- The path to more is kept open and costs nothing now: rapier 0.36 has an `enhanced-determinism` feature (libm-pinned transcendentals; it can combine with `parallel`; cross-platform reproducibility additionally needs matching numeric features). A later `deterministic` feature on `exitium_physics` would forward to it. Not enabled in the MVP (open question 4).

### Events and queries

**Events.** `CollisionEvent { kind: CollisionKind (Started | Stopped), a: ColliderId, b: ColliderId, a_user: u64, b_user: u64, sensor: bool, tick: u64 }`. `sensor` is true when either collider is a sensor (a trigger enter or exit). `a`/`b` are ordered by `ColliderId::to_bits()` for a stable order. Events fire only for colliders with `report_events` or `sensor`. No contact-force events and no impulse data in the MVP (additive: a `ContactForce` event kind). A body removed in the same tick as its collision `Stopped` event still produces that event (the user data is copied into the event, so the receiver does not need the id to be alive).

**Queries** return the **first** or **all** hits, sorted by distance. `QueryFilter { layers: CollisionLayers, include_sensors: bool (false), exclude_body: Option<BodyId> }` and a `Ray { origin: Vec2, direction: Vec2 }` (the call normalizes the direction; a zero or non-finite direction or origin returns `None`). `RayHit { collider: ColliderId, point: Vec2, normal: Vec2, distance: f32, user_data: u64 }`. `ShapeHit { collider, distance, point, normal, user_data }`, where `point` and `normal` are at first contact. Queries use rapier's query pipeline over the same BVH as the broad phase. They see the world as of the last `step` or `update_queries` call. `update_queries` exists because Stage 2 creates colliders in `Update` (so debug draw and picking work while the game is paused) and wants a raycast in the same frame to see them.

### Stage 2: components and sync

The plugin registers into the schedules that `app-platform.md` ("Schedules and the frame loop") defines. **The exact `App`, `Plugin`, system-set and ordering signatures are that spec's and #22's**; this spec lists what the plugin needs from them and does not invent a second API.

| Schedule | What the plugin registers |
|---|---|
| `Startup` (resources are inserted in `Plugin::build`) | Insert the `PhysicsConfig`, `PhysicsDebugSettings`, `PhysicsStats`, `PhysicsEvents`, `PhysicsDebugView` and `PhysicsDebugDraw` resources and the `PhysicsWorld`. Register every public type with the reflection registry (components `RigidBody`, `Collider`, `Velocity`, `CharacterController`, `CharacterState`; the reflected resources `PhysicsConfig`, `PhysicsDebugSettings`, `PhysicsStats`). `PhysicsDebugView`, `PhysicsDebugDraw`, `PhysicsEvents`, `ImpulseQueue` and the `PhysicsWorld` are not reflected. |
| `FixedFirst` (before the game's `FixedUpdate` systems) | `reconcile` (below). |
| `FixedLast` (after the game's `FixedUpdate` systems) | `sync_in`, `character_move` (Stage 4), `step`, `write_back`. |
| `PostUpdate`, `before_set(PROPAGATE)` | `reconcile`, `interpolate`, `debug_draw`; then `clear_events` (last, after the frame's `Update` systems have read the events). |
| `Extract` | Nothing from this crate. The renderer reads the `Transform2d`/`GlobalTransform2d` values that `interpolate` already wrote. Feeding `PhysicsDebugDraw` into the renderer is facade glue (see "Debug draw"). |

`PostUpdate` runs every frame, **including while `SimulationMode` is `Frozen`** (the editor's Edit state: gap X3 of `editor-mcp-mvp.md`, merged, and its `app-platform.md` amendment AM3): `Update` and the fixed ticks do not. So in Edit mode the world is still reconciled (a body spawned by an agent exists and can be queried), `debug_draw` still fills its lines, and no tick or event happens. `reconcile` is therefore in `PostUpdate` and not in `Update`.

**When a spawned thing exists in the world (the two-frame rule).** `reconcile` runs `before_set(PROPAGATE)` (`app-platform.md`: it reads the previous frame's `GlobalTransform2d`, and `interpolate` must precede propagation). So a `Dynamic` or `Kinematic` root, whose pose comes from its own `Transform2d`, exists in the world **after the first `PostUpdate`** following its spawn. A **static body, a static collider and a child collider** are created from `GlobalTransform2d`, which `ensure_global_transforms` inserts inside `PROPAGATE` of that same frame, so they exist **after the second `PostUpdate`**. Reconcile skips an entity that has no `GlobalTransform2d` yet, silently (no issue) and retries on the next pass. Level geometry, scene loads and tilemap chunk children therefore appear one frame later than a falling crate. That is a deliberate trade (no change to `app-platform.md`'s ordering). Moving `reconcile` after `PROPAGATE` would give a same-frame rule at the price of an `app-platform.md` amendment; it is not needed by any example.

**What a tick sees.** A static thing is built by the `FixedFirst` `reconcile` of the first tick after the `PROPAGATE` that follows its spawn, so **a tick sees a spawned static thing if and only if a `PROPAGATE` has run since the spawn**. A level spawned by an `Update` system never has a tick without it: the tick of the next frame builds it before it steps. A level spawned in `Startup` or by a `FixedUpdate` system is missing for the ticks of that frame that run before its first `PostUpdate`: **for those ticks, and for `character_move` in them, the world has the dynamic and kinematic bodies and no static geometry** (a falling crate falls through the missing floor for one tick, a character's move meets no wall). The effect lasts for the ticks of one frame at most, so a game that cannot accept it holds its first tick until `PhysicsStats::pending` is 0.

**The signal for an agent or a game: `PhysicsStats::pending`.** It is the number of entities with a `RigidBody` or `Collider` that the latest reconcile pass skipped because they have no `GlobalTransform2d` yet (0 when everything is built). An agent in Edit mode, where no tick runs and `tick` does not move, **waits until `pending` is 0** (read with `ecs_get_resource`, or from the output of any physics query, which carries it, see "MCP / editor surface") instead of counting frames. A query that finds nothing while `pending > 0` may be a miss only because the world is not built yet; `physics_body_state` for a live entity that has a `RigidBody` or `Collider` but no link yet returns the retryable error `busy` (message: "the body is not built yet, retry after the next frame"), not an empty answer.

What the plugin needs from #106 is settled by `app-platform.md` (its resolution table for physics): (a) schedule slots to run before and after the game's default systems in `FixedUpdate`, and late in the frame before transform propagation; (b) the fixed-step alpha; (c) `Transform2d` (position `Vec2`, rotation as an angle in radians, scale) and `GlobalTransform2d`, with `Parent`/`Children` from the ECS (#49); (d) a system-parameter way to reach `&mut World`-level data: a resource, a query with `Changed<T>` and `Without<T>` filters, and `Commands`. Nothing else.

**Components** (all reflected; the first three are the user-facing set):

- `RigidBody`, `Collider`, `Velocity`: the types above, with `#[reflect(range(...))]` on numeric fields (`friction`, `restitution`, `density`, damping and so on) and doc comments (they become MCP `description`). `range` is a UI and schema hint only (reflect spec), so the plugin **validates at reconcile** and reports bad values as an issue (see "Diagnostics") instead of panicking.
- `BodyLink(BodyId)` and `ColliderLink(ColliderId)`: internal, **not reflected**, so a scene save or an editor snapshot skips them. After a restore the next reconcile rebuilds every body from the reflected components. **The physics world is derived state and is never saved.** Consequence: `Velocity` must stay current, so `write_back` keeps it current and reconcile inserts `Velocity::default()` with the `BodyLink`.

**Which entities become what:**

- An entity with `RigidBody` (and optionally `Collider` on the same entity) becomes a body. A `Collider` on the same entity attaches to it with the collider's `offset` and `angle`.
- An entity with `Collider` and **no** `RigidBody` on itself becomes a static collider if no ancestor has a `RigidBody`. If an ancestor has one, the collider attaches to the **nearest ancestor body** (a compound body of several shapes), at the pose relative to that body, computed from the two `GlobalTransform2d` values and then `offset`/`angle`.
- A `Dynamic` or `Kinematic` body must be a hierarchy root (no `Parent`), because physics writes its `Transform2d` as a world pose. A non-root dynamic body is skipped and reported as an issue. A `Static` body or a static collider may be anywhere in the tree (read from `GlobalTransform2d`).

**reconcile** (in `FixedFirst` at the start of every tick, and in `PostUpdate` so a frozen or paused editor still has a world to draw and query). It is idempotent and does work proportional to what changed:
1. Create: a query for `RigidBody`/`Collider` entities `Without<BodyLink>`/`Without<ColliderLink>` calls `add_body`/`add_collider`, and `Commands` inserts the link components (and `Velocity` for a body). Invalid settings go to the issue list and the entity is retried only after the component changes.
2. Update: entities with `Changed<RigidBody>` or `Changed<Collider>` call `update_body`/`update_collider` (an inspector or MCP `ecs_set_field` edit takes effect this way). **Moved static things are re-placed here.** A `Static` body never has its `Transform2d` written by the plugin, so `Changed<GlobalTransform2d>` on it is a reliable "someone moved it" signal (an editor drag, an `ecs_set_field` on `Transform2d`, a game system). The same holds for a parentless static collider and for a child collider of any body. Reconcile calls `set_pose` for a static body and re-places the collider (`update_collider` with the new local pose) for the others. A moved static body is a teleport: it gives nothing a velocity, so bodies resting on it are not carried (use a `Kinematic` body for a moving platform), and `set_pose` wakes the bodies touching it. A change to `PhysicsConfig::units_per_meter` rebuilds the whole world (every body re-created from its components).
   **Config changes.** Reconcile's **Changed check** on the `PhysicsConfig` resource (its change stamp, the cheap gate: an edit through `ecs_set_resource_field`, an undo of it or a direct write all move it) decides whether to look further. It then diffs the resource against `applied_config`, a copy it keeps, to learn *which* field changed. The copy is needed because reconcile is registered twice (`FixedFirst` and `PostUpdate`, each with its own last-run stamp), so the stamp alone would apply one edit twice, and `units_per_meter` would rebuild the world twice; the diff makes the second pass a no-op, and an undo back to the applied value also does nothing. `gravity`, `solver_iterations` and `max_events` are applied in place. `units_per_meter` removes every body and collider and re-creates them from their components with `Velocity` and `Transform2d` unchanged (both are in world units, so the visible state is the same); sleeping bodies wake and pending impulses are dropped. A non-finite or non-positive `units_per_meter`, `gravity` or `solver_iterations` of 0 is an issue and `applied_config` keeps the last valid value.
3. Remove: ECS v1 has no removal hooks or `RemovedComponents` (ECS spec non-goal), so reconcile **sweeps**: it walks its own body and collider tables (O(bodies), a few ns each) and removes any whose entity no longer has the component or the entity is dead (generation mismatch). This also handles despawn.

**sync_in** (`FixedLast`, after the game's `FixedUpdate` systems, before `step`), for each dynamic or kinematic body entity (static bodies are handled by reconcile, above):
- `Transform2d` differs from the pose the plugin last wrote (`written_pose`, stored with the link; compared by value, so it does not depend on change-detection internals): the game or the editor moved it, so it is a **teleport**: `set_pose`. The comparison is O(bodies) and a few ns each.
- `Velocity` differs from `written_velocity`: `set_velocity`.
- Queued impulses (the `ImpulseQueue` resource, filled by the `physics_apply_impulse` command or by game code) are applied and the queue is cleared. The queue holds at most 1,024 entries; an entry for an entity without a live dynamic body is dropped and counted in `PhysicsStats::impulses_dropped`. **No impulse waits for Play:** the command is rejected while `SimulationMode` is `Frozen` (see "MCP / editor surface"), and the queue is also cleared on every `SimulationMode` change and on a world rebuild, so an entry left by game code in `Update` cannot fire at `play_start`.

**write_back** (after `step`): for each **active** body only (`active_bodies()`, O(awake)), write the exact simulation pose into `Transform2d` if the body has `interpolate = false`, write `Velocity`, and record `written_pose` and `written_velocity`. Sleeping bodies and static bodies are not touched, which also keeps their change ticks quiet. A body with `interpolate = true` does not get its `Transform2d` written here; it gets it in `interpolate`.

**interpolate** (`PostUpdate`, before propagation): for each active interpolating body, compute `interpolated_pose(id, alpha)`, convert to world units, write `Transform2d` (only if different) and record `written_pose`. Before it overwrites a body, it runs the same teleport check as `sync_in` (so an Update system or an editor drag that moved the body is applied, not overwritten). After it runs, `Transform2d` holds what is drawn; the exact simulation pose is in the physics world and in `BodyState` for any gameplay code that needs it (`PhysicsWorld::body_state`).

**Hierarchy and `GlobalTransform2d`.** Propagation (#106) produces `GlobalTransform2d` from `Transform2d`. For a root body they are equal after propagation, which is what the renderer reads. `sync_in` reads the local `Transform2d` of a root body and never `GlobalTransform2d`, so it does not wait for propagation. Static and child colliders read `GlobalTransform2d`, so a child collider that moved in tick N is re-placed at the next reconcile after propagation. Acceptable: those are rare.

**Events delivery** (`PhysicsEvents` resource, entity-level):

```rust
pub struct PhysicsEvent { pub kind: CollisionKind, pub a: Entity, pub b: Entity, pub sensor: bool, pub tick: u64 }
pub struct PhysicsEvents { /* Vec<PhysicsEvent>, tick_start: usize */ }
impl PhysicsEvents {
    pub fn this_tick(&self) -> &[PhysicsEvent];      // appended by the latest step: what a FixedUpdate system wants
    pub fn since_frame_start(&self) -> &[PhysicsEvent]; // every tick since the last Update ended: what an Update system wants
}
```

The step system appends each tick's events, converting each side with `Entity::from_bits(user_data)`. That returns `None` when the generation half is 0, and the ECS only hands out entities with a non-zero generation, so the plugin **never creates a body or collider with `user_data == 0`** (`Entity::to_bits()` is never 0). A `None` result therefore means the collider was not created by the plugin (a Stage 1 user of the same world, or a bug), and the event is **dropped and counted**, exactly like an event whose entity is dead (`PhysicsStats::events_dropped`, plus an issue). Stage 1 treats `user_data` as opaque and defaults it to 0. `clear_events` runs at the **end** of the frame's `PostUpdate`, after every `Update` system. So an `Update` system sees every event of the frame exactly once, even when two ticks ran, and a frame with zero ticks has no events. A `FixedUpdate` system uses `this_tick()`. The ECS has a double-buffered event type (#44, designed by #22); this resource is plugin-owned for the MVP because two-per-frame fixed ticks do not fit a once-per-frame buffer swap. If #22's event type handles fixed ticks, `PhysicsEvents` becomes a producer for it without an API change for the readers (open question 3).

**Queries** (entity-level): `PhysicsQueries` wraps `&PhysicsWorld` and `PhysicsConfig` and mirrors the Stage 1 methods in world units, returning `Entity` instead of `ColliderId`. It is a plain resource accessor, not a new kind of system parameter, so it does not depend on #22.

### Debug draw

Plain data that any renderer can consume later. No dependency on `exitium_render`; it uses `Vec2` and `Color` from `exitium_core`:

```rust
pub struct DebugLine { pub a: Vec2, pub b: Vec2, pub color: Color, pub kind: DebugKind } // 32 bytes + 1 byte tag
pub enum DebugKind { Collider, Sensor, Sleeping, Aabb, Contact, Velocity }
pub struct DebugDraw { pub lines: Vec<DebugLine>, pub truncated: bool } // cleared and refilled each call; capacity reused
pub struct DebugDrawOptions {
    pub colliders: bool, pub aabbs: bool, pub contacts: bool, pub velocities: bool,
    pub bounds: Option<Rect>,     // emit only shapes that intersect this rect (the camera view); None = everything
    pub max_lines: usize,         // default 100_000; sets `truncated` when hit
}
```

Stage 1 fills `DebugDraw` in physics units through rapier's `DebugRenderPipeline` with a backend that pushes lines and ignores rapier's HSLA colors: each line's color comes from its `DebugKind` through the table below, so goldens and the editor legend never depend on a rapier version. The backend rejects a collider whose AABB misses `bounds` before it emits any line (O(colliders) with a few ns per rejected collider; a BVH-driven cull is an internal change if the budget is ever missed).

| `DebugKind` | Default color (sRGB) | Drawn for |
|---|---|---|
| `Collider` | `#33CC4D` | awake, non-sensor colliders (static ones included) |
| `Sensor` | `#F2CC33` | sensor colliders |
| `Sleeping` | `#598CF2` | colliders of sleeping bodies |
| `Aabb` | `#999999` at 50% alpha | broad-phase boxes, when `aabbs` is on |
| `Contact` | `#F24040` | contact points and normals, when `contacts` is on |
| `Velocity` | `#40D9F2` | a line from each awake body's center along its velocity, when `velocities` is on |

**Lines show the exact simulation pose, not the interpolated one.** A sprite drawn from the interpolated pose can lag the lines by up to one tick of motion (16.7 ms at 60 Hz). That is what a collision debugger should show: the shape rapier is actually testing.

**Stage 2 data (`PhysicsDebugDraw`).** Two resources, split by who writes them:
- **`PhysicsDebugSettings`** (reflected, undoable, edited by humans and agents): `enabled` (default false, so a shipped game pays nothing), `colliders`, `aabbs`, `contacts`, `velocities`, `max_lines`. This is the only place `enabled` lives.
- **`PhysicsDebugView`** (not reflected, written by the facade glue every frame, so it has no undo history and no inspector field): `bounds: Option<Rect>` (world units; `None` = everything) and `line_cap: Option<usize>`. The effective line limit is `min(max_lines, line_cap)`.
- **`PhysicsDebugDraw { lines: Vec<DebugLine>, truncated: bool, tick: u64 }`** (not reflected) holds the lines **in world units**; `tick` is the world tick they were made from.

The `debug_draw` system runs in `PostUpdate`, so it also runs in Edit mode (`SimulationMode::Frozen`) and while paused. When `PhysicsDebugSettings::enabled` is false it does nothing: no allocation, no query, and the lines are cleared once on the frame that turns it off. When it is true it **recomputes only if something changed** since its last run: the world `tick`, a reconcile that edited the world, the settings or the `PhysicsDebugView` (each compared by value with a copy it keeps). Otherwise `lines` is left as it is.

**The facade glue (issue G1, in `crates/exitium/`, a hotspot).** Physics does not depend on the renderer and the renderer does not depend on physics (runtime crates have no edges between them), so a facade system carries the data. Its contract:
1. Each frame, before physics' `debug_draw` runs, write `PhysicsDebugView::bounds` as the **union of the `visible_rect()` of every active render pass**, previous frame's values, grown by 10% so a one-frame lag never shows a gap. Every active pass means every `CameraViews` entry (each active `Camera2d`) **and the editor's `SceneView` pass** when it is enabled (render-v1.md "Camera", `extract_cameras`; the glue computes the scene view's rect with `Camera::resolve` if `CameraViews` does not list it). Using only `CameraViews::main()` would cull the shapes as soon as the human pans the scene view away from the game camera. With no active pass `bounds` stays `None`. A screenshot request with a custom `view` that leaves the union shows culled lines; agents set the `SceneView` resource instead (render-v1.md), which the union covers, and the render amendment recorded below should expose a pending screenshot view to the union.
2. Also each frame, write `PhysicsDebugView::line_cap = Some(MAX_DEBUG_LINES / active_pass_count)`, **and `None` when `active_pass_count` is 0** (no division; `bounds` is `None` then too, and the effective limit is `max_lines` alone) (render's `MAX_DEBUG_LINES`, 262,144, is **summed over passes**, and a world-space line is drawn in every pass: with game camera, scene view and a screenshot pass, three passes allow 87,381 lines each, below physics' default `max_lines` of 100,000). `truncated` then tells an agent that the cap, not the world, ended the list.
3. In `Extract`, set `EXTRACT`, when `PhysicsDebugSettings::enabled` is true: emit each line as `DebugDraw::line(a, b, color, 1.0, Space::World)` of `exitium_render` (render-v1.md "Debug lines and shapes", issue I11, answers open question 7: the primitive exists). The renderer drops lines past its own cap and counts them in `RenderStats::dropped`.
4. When `enabled` is false, copy nothing and do not touch `PhysicsDebugView`.

**This glue is the same facade issue as `render-v1.md`'s G1c** (its "G1c, physics debug"): the two specs describe one piece of work, filed **once**, with this contract as its acceptance. The physics re-export and the default plugin set ride in the same issue (G1 below).

Circles are 16-segment polylines and boxes are 4 lines, so 1,000 colliders are about 10,000 to 20,000 lines (about 640 KB); `bounds` cuts this to what is visible.

**The agent surface (issue P20).** An agent in Edit mode with no window open still needs to see the shapes. Two ways, both without a physics-specific command:
- **A picture:** `ecs_set_resource_field` on `PhysicsDebugSettings.enabled` (undoable, so the toggle is also the editor's Resources tab control), then the render `render_screenshot` tool **with `include_debug: true`** (render-v1.md `ScreenshotRequest`; without it the screenshot has no debug lines). The glue draws the lines.
- **Numbers:** the `physics_debug_draw` query (see "MCP / editor surface"). It fills a scratch `DebugDraw` through `PhysicsWorld::debug_draw`, **independent of `enabled`**, so reading shapes changes no resource and writes no journal entry beyond the query itself.

### Character controller

A kinematic body that walks, climbs slopes and steps, and slides along walls, without a dynamic body's bouncing and tunnelling. It wraps rapier's `KinematicCharacterController` (`rapier::control`; the implementer confirms the module and its method names against 0.36.1 and that it needs no feature beyond those in "Decisions"). Two Stage 4 issues build it: **P17** is the `PhysicsWorld` method (standalone layer), **P18** is the component and system (plugin layer).

```rust
pub enum CharacterMode { Platformer, TopDown }
pub struct Autostep { pub max_height: f32, pub min_width: f32, pub include_dynamic: bool }   // lengths in the unit of the layer, see below
pub struct CharacterSettings {                     // one definition, reflected when `reflect` is on
    pub mode: CharacterMode,                       // Platformer
    pub up: Vec2,                                  // (0, 1); Platformer only; must be finite and non-zero, normalized by the validator
    pub skin: f32,                                 // gap kept to what it touches, > 0. 0.02 m (0.64 world units in the component at 32 per meter)
    pub max_slope_climb: f32,                      // radians, 0..=pi/2. 0.873 (50 degrees): a ramp up to this angle is climbed, in both directions; a 45 degree tile ramp has margin. Steeper surfaces are not promised: see "Slope angles are signed in rapier 2D"
    pub min_slope_slide: f32,                      // radians, 0..=pi/2. 0.873: equal to max_slope_climb, so no slope the character can climb makes it slide. Below max_slope_climb it slides on slopes between the two, on one side only (same section)
    pub autostep: Option<Autostep>,                // None. Some: climbs a ledge up to max_height if there is min_width of floor on top
    pub snap_to_ground: Option<f32>,               // Some(0.2 m; 6.4 world units in the component): stays on a down slope and on stairs; None in the air by design
    pub push_dynamic: bool,                        // true: dynamic bodies it runs into get an impulse from the move
    pub one_way_layers: u32,                       // 0 (none). A collider whose `memberships` share a bit with this mask is a one-way platform for this character
    pub drop_through: bool,                        // false. The caller sets it and HOLDS it (about 0.2 s) until the character is clear of the one-way platform it stands on; one tick is not enough
}
pub struct CharacterResult {
    pub translation: Vec2,                         // the movement that was possible, same unit as `desired`
    pub grounded: bool,                            // from rapier
    pub sliding_down_slope: bool,                  // derived by us (not rapier's flag): the character is moving down a slope that it is meant to slide on
    pub hit_ceiling: bool, pub hit_wall: bool,     // derived by us from the collisions
    pub ground: Option<ColliderId>,                // the collider it stands on; Some only when grounded
}
```

`move_character(body, settings, desired, dt, filter)` requires `body` to be `Kinematic` with a non-sensor collider; it casts the body's first collider (the shape, its local offset and angle) from the body's **simulation pose**, ignoring the body itself and anything `filter` excludes, and returns the movement that fits. **It does not move the body.** The caller sets `Velocity = translation / dt` and the next `step` moves it exactly that far (kinematic bodies are velocity-based). `dt` must be finite and greater than 0, and `desired` finite, else a `PhysicsError`. With `push_dynamic` it applies impulses to the dynamic bodies it hit through rapier's collision-impulse helper (this is why the method takes `&mut self`). **Before it queries, it refreshes the query structure itself when the world was edited since the last `update_queries`** (the world keeps a dirty flag set by `add_*`, `remove_*`, `update_*` and `set_pose`), so a collider that `reconcile` added in `FixedFirst` this tick is already seen.

**Where each result field comes from.** `grounded`, the one field the game's gravity rule depends on, comes straight from rapier's `EffectiveCharacterMovement`: rapier computes it from the contact manifolds at the end of the move, **also on a tick where the character is not moving into the floor**. It must not be derived from the collision callback: a character standing still (`desired.y == 0`) generates no collision that tick, so a normal-derived `grounded` would flicker, and the game's "zero the vertical speed when grounded, add gravity otherwise" rule would then alternate every tick. **`sliding_down_slope` is not rapier's `is_sliding_down_slope`.** Rapier sets that flag in its "let it slide" branch, which includes climbing: measured against 0.36.1 it is true on every tick of a climb up a 30 degree ramp and on the landing tick on flat ground, and false when standing still or walking, so it does not mean "sliding down". We derive it instead: **true when a collision during the move has a normal `n` whose angle to `up` is greater than `min_slope_slide` and less than `max_slope_climb` (a ground-like slope the character is meant to slide on), and the move's `translation` dot `up` is below 0.** With the equal defaults the angle interval is empty, so `sliding_down_slope` is never true; it is a feature of a game that sets `min_slope_slide` below `max_slope_climb` (and then on one side only, see below). The other three come from our own bookkeeping: `hit_ceiling` and `hit_wall` from the collisions rapier reports during the move (for a collision normal `n` pointing at the character, `dot(n, up) <= -cos(max_slope_climb)` is ceiling, a normal whose `dot(n, up)` is between that and `cos(max_slope_climb)` is wall; ground-like normals set neither; this classification uses the **unsigned** angle, so it is symmetric, unlike rapier's own), and `ground` from the collisions when one is ground-like, else from **one short downward probe**: when rapier says `grounded`, `cast_shape` of the character's shape from its final pose along `-up` for `max(2 * skin, snap_to_ground.unwrap_or(0))` with the same filter, and the hit collider is `ground`. The probe runs only on grounded calls and is in the budget. A grounded result whose probe finds nothing has `ground: None` (a documented rare case); a not-grounded result always has `ground: None`.

**Slope angles are signed in rapier 2D, so the slope promise is narrow.** rapier's `compute_hit_info` (`character_controller.rs`, `up.angle_to(normal)`, about L669; the autostep ramp check at about L831 repeats it) uses glam's **signed** 2D angle: a surface whose normal leans toward `+x` (a ramp rising toward `-x`, for `up = (0, 1)`) has a negative angle. A negative angle is never `>= max_slope_climb` and is always `<= min_slope_slide`. So rapier's two thresholds apply to **one side only**: on the other side every slope is climbed and none makes the character slide. Measured against 0.36.1 (a 0.5 by 1 box, the default 50 degrees for both): 45 and 30 degree ramps are climbed in both directions, but a 60 degree ramp rising toward `+x` blocks the character (x 1.74, y 0.52 after the test move) while the mirror ramp rising toward `-x` is climbed (y 2.13). The tile table only produces 45 degree slopes (see "Tile colliders"), so shipped content is on the safe side of this. What the spec therefore **promises, in both orientations**: a ramp up to `max_slope_climb` (default 50 degrees) is climbed, and with the equal defaults a character stands still on a 45 degree ramp. What it **does not promise**: any behavior on a slope steeper than `max_slope_climb` (it may block or be climbed, depending on the side), and any slide threshold other than on the slopes that rise toward `+x`. **A surface steeper than `max_slope_climb` is authored as a box (a wall), never as a sloped polygon.** P17 documents this on the two fields and adds no wrapper workaround (a symmetric wrapper would mean calling rapier twice with mirrored `up`, a cost the hot path does not need); if a game later needs a symmetric slide threshold, that is an amendment issue, not an edit by the implementer. The tests below cover exactly the promised cases, each in both orientations, and one slide case on the side where the threshold works.

**`TopDown`** has no floor and no gravity, but rapier's controller needs an `up` and its slope code is signed (see above), so the spec fixes the values and a test proves the result is axis-independent. `TopDown` passes `up = (0, 1)`, **`max_slope_climb_angle = 4.0` and `min_slope_slide_angle = -4.0`**, no autostep and no snap. The two values are **internal constants, outside the `0..=pi/2` range that the validator enforces on the public `CharacterSettings` fields**: the wrapper hands them to rapier directly and does not run them through that validator. They work because rapier's signed angle lies in `[-pi, pi]`: `4.0` is above `pi`, so no hit is ever a wall (`angle >= 4.0` never holds) and the character slides along every surface, and `-4.0` is below `-pi`, so no hit is ever a "nonslip slope" (`angle <= -4.0` never holds) and nothing blocks the slide down. Measured against 0.36.1 (ball of radius 0.5, a 0.3 by 0.3 diagonal into each wall): these values give the identical 0.3 slide along all four walls and both diagonals. The first idea, `max_slope_climb = 0` and `min_slope_slide = pi/2`, is wrong: with a slide angle of `pi/2` every surface is nonslip, so a character cannot slide down a vertical wall (measured: the left and right walls let it slide up by 0.3 but down by only 0.0001, while the walls above and below slid both ways, so the four-wall symmetry test fails). The result reports `grounded = false`, `sliding_down_slope = false`, `hit_ceiling = false` and `ground = None` always (rapier's `grounded` against a south wall is meaningless here), and `hit_wall = true` for any collision. The `up`, `max_slope_climb`, `min_slope_slide` and `snap_to_ground` fields of `CharacterSettings` are validated but otherwise ignored in this mode. P17's test checks all four wall orientations and a diagonal slide against each. **If that test fails on some axis, P17 stops and files a spec amendment issue; it does not change these values in its own PR** (P17's footprint is `crates/exitium_physics/` and cannot edit this spec).

**One-way platforms** (jump-through platforms) are a **predicate inside `move_character`**, so there is no solver change, no Stage 1 type change and no `Collider` field. A kinematic character is not resolved by rapier's solver against static colliders, only by this query, so filtering the query is sufficient. Rapier's `QueryFilter` takes a predicate over `(handle, collider)`; the wrapper installs one that **ignores a collider** when its `memberships & one_way_layers != 0` **and** either: (a) `drop_through` is set; or (b) the character's bottom at the start pose (the shape's lowest point along `up`) is lower than the platform's top (the collider's AABB maximum along `up`) minus `skin`. So a character jumps up through a platform (its bottom is below the top while it rises through), lands on it when falling from above, walks sideways into its edge from below without being stopped, and can step down with `drop_through`. **There is deliberately no rule about the direction of `desired`** (an earlier draft ignored the platform whenever `desired` dot `up` > 0): rule (b) already covers every jump-up-through case, and a direction rule would ignore the platform under a character that stands on a rising one-way platform with `inherit_platform_velocity` on (its `desired.y` is the platform's speed times `dt`, above 0), so rapier's grounded detection would also ignore it, `grounded` would be false and `ground` `None` on that tick, nothing would be inherited on the next, gravity would apply, the platform would be solid again, and `grounded` would flip every other tick. With (a) and (b) only, a character standing on a rising or falling one-way platform has its bottom at the top plus the skin gap, so (b) is false and the platform stays solid. The predicate uses the start pose of the call (one decision per call, not per sub-step), and it also applies to the autostep and snap-to-ground casts and to the `ground` probe.

**`drop_through` is held, not pulsed.** After the drop tick the character has moved only its own `desired` (the first tick of gravity from rest at 60 Hz and 9.81 m/s² is about 0.0027 m), so its bottom is still above the platform's top minus `skin`, rule (b) is false, and the platform is solid again on the next tick. The character has to descend **more than `2 * skin`** (0.04 m, which is 1.28 world units at 32 per meter: the standing gap of one skin, plus one more) below the top before the platform stays out of its way without help. So the game **holds `drop_through` until the character is clear**: a timer of about 0.2 s is enough (from rest the game's gravity moves it about 0.2 m in 0.2 s, five times the distance needed), or until the game's own record of the character's bottom is more than `2 * skin` below the platform's top. While the flag is held every one-way platform is ignored, including one the character passes on the way down, so a game should hold it for no longer than it needs. The controller does not time it for the game (it has no clock and the game owns the vertical speed, see below). A one-way platform is a plain static `Collider` whose `layers.memberships` has the bit (an author's choice, e.g. bit 4), and its upper face is its AABB top, so it should be a box or a flat convex polygon. The layer-bit alternative (a game system toggling the platform's layer from `Update`) was rejected: it goes through `Changed<Collider>` and reconcile, so it lags the character by a tick. Dynamic bodies (a pushed crate) do not honor `one_way_layers`, which is the one remaining non-goal in this area.

**Stage 2 component (P18).** Reflected, so the inspector and the generic ECS tools edit it (lengths in world units, converted at the boundary like every other length):

```rust
pub struct CharacterController {                   // needs RigidBody { kind: Kinematic } and a non-sensor Collider on the same entity
    pub settings: CharacterSettings,
    pub desired_velocity: Vec2,                    // world units per second; game code writes it each FixedUpdate tick
    pub inherit_platform_velocity: bool,           // true
}
#[reflect(custom(readonly = true))]
pub struct CharacterState {                        // inserted with the controller, written by the plugin
    pub grounded: bool, pub sliding_down_slope: bool, pub hit_ceiling: bool, pub hit_wall: bool,
    pub ground: Option<Entity>,                    // the entity of the ground collider's body or static collider
    pub velocity: Vec2,                            // the effective velocity of the last move, world units per second
    pub tick: u64,
}
```

The `character_move` system runs in `FixedLast` after `sync_in` and before `step`. For each entity with a `CharacterController` and a `BodyLink`: if `inherit_platform_velocity` and last tick's `ground` belongs to a kinematic body, add that body's `Velocity::linear` to `desired_velocity` (one tick of lag, no search; `ground` is set on every grounded tick, standing still included, see above). **This add is the only thing that carries the character.** `move_character` does not: rapier's kinematic-friction code in the controller runs only after a hit, so a character that stands on a moving platform with `desired = (0, 0)` stays where it was (measured against 0.36.1: x = 0.000 after 60 ticks on a platform moving at 2 units/s, and 2.000 when the platform's velocity is added to `desired`). This spec does not rely on that friction path anywhere. Then call `move_character` (which refreshes the query structure itself if `reconcile` edited the world this tick) with `desired_velocity * dt` and a `QueryFilter` built from the character collider's `layers`, `include_sensors: false` and `exclude_body` the character itself; set the body's velocity to `translation / dt` and record it as `written_velocity`, so `write_back` and the `Velocity` component show the effective velocity; update `CharacterState` only if a field changed (quiet change ticks). A controller on a body that is not kinematic or has no non-sensor collider is an issue in `PhysicsStats` and the entity is skipped. A `Transform2d` write by the game is a teleport through the usual `sync_in` check.

**The controller does not simulate gravity.** The game owns the vertical speed: it adds `gravity * dt` to `desired_velocity.y` each tick and zeroes it when `CharacterState::grounded` or `hit_ceiling` (this keeps jump arcs a game rule, not an engine one). That rule is safe only because `grounded` is stable on a standing character (see "Where each result field comes from"); the tests below pin it. `one_way_layers` and `drop_through` are fields of `CharacterSettings`, so they are in the component's reflected `settings` and a game system sets `drop_through`, and **holds it for about 0.2 s** (see "`drop_through` is held, not pulsed"), through ordinary component access. **Input is not physics' either**: gamepad, keyboard and scripts write `desired_velocity`.

**Interpolation.** A kinematic body interpolates like any other (`interpolate = true` by default), so a character is drawn smoothly at any frame rate.

### Tile colliders

A tilemap of 256 by 256 cells must not become 65,536 colliders. `exitium_physics` provides the merge; it knows nothing about tilemaps. Types are plain data, no ECS, in the standalone layer (`tiles.rs`):

```rust
pub struct CellGrid<'a> { pub width: u32, pub height: u32, pub cells: &'a [u16] }   // row-major, row 0 first; 0 = empty, any other value is a "kind"
pub struct CellRect { pub x: u32, pub y: u32, pub width: u32, pub height: u32, pub kind: u16 }  // cell units
#[derive(Default)] pub struct CellMerger { consumed: Vec<u64> /* a bitset, capacity reused */ }
impl CellMerger { pub fn merge(&mut self, grid: &CellGrid<'_>, out: &mut Vec<CellRect>) -> Result<(), PhysicsError>; }
```

`merge` clears `out`, scans cells in row-major order, and for each non-empty unconsumed cell grows a rectangle to the right while the kind is equal and the cell unconsumed, then downward while the whole row span is equal and unconsumed, and marks the cells consumed. It is greedy, not optimal (a perfect cover is not worth its cost), and **deterministic**: same grid, same rects in the same order. Properties the tests check: the rectangles are disjoint, their union is exactly the non-empty cells, every rectangle has one kind, and there are never more rectangles than non-empty cells. `cells.len() != width * height` or more than 4,194,304 cells (`1 << 22`) is `InvalidShape`. The product is computed in `u64` (in `u32`, a 65,536 by 65,536 grid wraps to 0 and would pass with an empty slice). A 64 by 64 grid is the intended unit (a chunk), merged on change.

**Who turns rectangles into colliders: the facade glue, for each tilemap chunk** (the issue is filed by the tilemap spec, #147; it is `Blocked by` P19). Per chunk the glue merges, compares with the chunk's existing collider children and replaces what differs: each rectangle becomes a **child entity of the chunk** with a `Transform2d` at the rectangle's center and a `Collider { shape: Shape::Box { size }, report_events: false, .. }` and **no `RigidBody`**, so it is a static collider (this spec's existing rule for a collider with no body anywhere above it). The tile `kind` selects the collider from a **per-kind table the tilemap owns** (below). Existing reconcile does the rest: `Changed<GlobalTransform2d>` re-places the colliders when the map moves; a despawned child is swept. No new component, no new collider kind, no edge from `exitium_physics` to `exitium_tilemap` or from `exitium_tilemap` to `exitium_physics` (C1).

**Constraints this puts on the tilemap spec (#147), recorded here because the tilemap spec is written after this one:**
1. Collider cells are in the **orthogonal logical grid**, in ground-plane units, for all three tilemap modes; an isometric map hands over its logical cells and never projected screen positions (see "Game types").
2. The tilemap exposes, per chunk, a `u16` kind grid with 0 meaning no collision, plus a change signal per chunk. It chooses the chunk size (32 recommended, 64 at most).
3. **A kind is more than a number: the tilemap owns a per-kind table** `TileCollision { shape: TileShape, sensor: bool, material: Material, layers: CollisionLayers }` with `TileShape` being `Box` or a per-tile polygon (at least the four 45 degree slope triangles: floor rising left, floor rising right, ceiling falling left, ceiling falling right, each a 3-point `ConvexPolygon` in tile-local space; 45 degrees only, because the character controller promises ramps up to `max_slope_climb` in both directions and nothing steeper, see "Slope angles are signed in rapier 2D"). A `sensor` kind (a hazard or a trigger zone) is a normal merged `Box` whose collider has `sensor: true`; a kind never merges with another kind, so a sensor box never merges with a solid one.
4. **Unmergeable kinds are masked to 0 before `merge`.** The glue copies each chunk's kind grid, sets every cell whose `TileShape` is not `Box` to 0, and merges that. Each masked cell becomes its own child entity with a `Collider` using the per-tile polygon. The merger therefore never sees a slope, and the physics API needs no shape or sensor knowledge: `CellRect.kind` only selects the table row.
5. **Per-edit budget (a number the tilemap spec follows).** A single-cell edit of a 64 by 64 chunk costs at most **0.5 ms** on min-spec for the whole of: re-merge (at most 150 us), diff against the chunk's existing collider children, and the `Commands` for the adds and removes. Greedy re-merging can ripple (changing one cell can change which rectangle the cells after it join), so the diff is keyed on the rectangle `(x, y, width, height, kind)`: a rectangle that is unchanged keeps its entity, and only different ones are despawned or spawned. A whole-chunk replace is at most **2 ms**. The glue issue ships its own benchmark of the edit path (`exitium/tiles/edit`) with the numbers in its PR body, and reports the adds and removes per edit.

### Game types

The roadmap's sample games map onto this crate without a new mechanism. Physics always runs in an orthogonal 2D plane with y up (`app-platform.md`); a game type is a choice of gravity, controller mode and what the camera does with the result.

| Game type | Gravity | Character | Map colliders | Notes |
|---|---|---|---|---|
| Sidescroller | `PhysicsConfig::gravity` default (down) | `CharacterMode::Platformer`, autostep and snap on for tile stairs and slopes, `one_way_layers` for jump-through platforms | merged tile boxes, per-tile slope polygons, one-way platform colliders | the game applies gravity to `desired_velocity.y` |
| Top-down | `(0, 0)` | `CharacterMode::TopDown` | merged tile boxes | dynamic bodies need `linear_damping` (no friction with the floor in a top-down world) |
| Isometric | `(0, 0)` | `CharacterMode::TopDown`, in the **ground plane** | merged boxes of the logical grid | height levels and depth sorting belong to the renderer (render v2); physics sees the ground plane only. A jump is a game-side height variable, not a physics axis. |
| Pixel art | either of the above | | | `units_per_meter` is the pixel density (32 by default); physics positions are in world units, sub-pixel positions are the renderer's to snap |

### Joints

**A post-MVP non-goal. The owner decided on 2026-11-25 that physics joints are not in the MVP; this is a decision, not an open question.** No Stage 4 sample scene and none of the six rehearsal games needs a joint, and each joint kind adds validation, an editor surface with two entity references and two local anchors, undo with entity remapping, and solver cost. The decision is recorded so that it is not mistaken for a cut by omission, and so it stays additive (design rule 5). No issue in this spec builds any part of it. The design below is what a post-MVP spec would start from:

- A reflected `Joint` component on one of the two entities: `{ other: Entity, kind: JointKind, collide_connected: bool }`, with `JointKind` being `Revolute { anchor_a, anchor_b, limits }`, `Prismatic { axis, limits }`, `Distance { rest, stiffness }` or `Fixed`. Anchors are in each body's local frame, in world units.
- `PhysicsWorld::add_joint(a: BodyId, b: BodyId, &JointDesc) -> Result<JointId, PhysicsError>`, `remove_joint`, `update_joint`, backed by rapier's impulse joints. `JointId` is a generational id like the others. rapier's debug renderer also draws joints, so `DebugKind` gains a `Joint` variant and `debug_draw` an option.
- Reconcile creates the rapier joint only when both links exist (it retries each pass) and removes it when either entity or the component goes.
- **An `Entity` field in a component that is saved in scenes needs more than undo.** `Command::remap` covers undo of a despawn, but a scene file, a prefab instantiate and a scene duplicate each have to remap the `other` reference to the new entity (the scene and prefab specs, `assets-scene-export.md`). That is a change in `exitium_scene`, outside this crate.
- No Stage 1 to 4 type changes: `BodyKind`, `RigidBody` and `Collider` keep their fields. **Cost when joints are taken up after the MVP: about 3 PRs**: (1) `PhysicsWorld` joints and the `JointId` plumbing with tests; (2) the `Joint` component, reconcile (create, update, sweep), diagnostics and joint debug draw; (3) the `Entity`-reference remapping for scene save, prefab instantiate and duplicate, in the scene crate.

### Diagnostics

Authoring mistakes must be visible to an agent without reading logs. `PhysicsStats` (a reflected resource) holds `issues: Vec<PhysicsIssue { entity: Entity, message: String }>` (capped at 64, newest last, a counter for the overflow). Reconcile appends one for: an invalid `RigidBody` or `Collider` value, a non-root dynamic body, a body whose `Transform2d::scale` is not 1, a non-finite `Transform2d`, an event whose entity died, a dropped event over the cap, an invalid `PhysicsConfig` value, and (Stage 4) a `CharacterController` without a kinematic body and a non-sensor collider. An entity's issue is cleared when its component changes and reconcile accepts it. An agent reads them with the generic `ecs_get_resource` on `PhysicsStats`.

### Threading

- `PhysicsWorld` is `Send` and not shared: one thread owns it and calls `step(&mut self)`. In Stage 2 that is whichever thread runs `FixedUpdate` (the single-threaded executor in the MVP). Queries take `&self`, so a later parallel executor can run read-only queries from several systems at once; they never take a lock.
- rapier's `parallel` feature (rayon) is off. On wasm there are no threads regardless. The later path (design rule 5): build rapier with `parallel` and hand it a pool through its thread-pool API (added in 0.35), sized by `exitium_tasks`' `ThreadBudget`. It is an internal change to `world.rs` and one intake PR.
- The event collector needs `Sync` (rapier 0.35 changed the requirement from `Send + Sync`). A `std::sync::Mutex<Vec<_>>`, uncontended in the single-threaded step, is enough and works on wasm.
- No global state, no `static mut`, no thread-locals.

## MCP / editor surface

Agent parity (rule 1), reflection as the source of truth (rule 2), mutation through the command bus (rule 3). `exitium_command` (`editor-mcp-mvp.md`) builds the MCP tools and the editor panels generically, from reflection and from the command and query registry. Physics therefore adds **no panel code, and no edge to `exitium_mcp` or `exitium_editor`** (the MCP server is a separate process that cannot link game types). Plugin crates register their own commands and queries with `register_command::<C>(world)` and `register_query::<Q>(world)` from the plugin, not from the facade (D13 of that spec). Physics' job is to expose the following, in the game process, with reflection-derived schemas.

**Reflected components** (inspect and edit with the generic ECS tools and the inspector; `Changed<T>` in reconcile applies an edit): `RigidBody`, `Collider` (with `Shape`, `Material`, `CollisionLayers`, `BodyKind`, `CombineRule`), `Velocity`, `CharacterController` (with `CharacterSettings`, `CharacterMode`, `Autostep`) and `CharacterState` (read-only, Stage 4). Ranges, defaults, descriptions and the `Shape` enum come from the `#[derive(Reflect)]` attributes, never from a parallel schema. Undo is the command bus's undo for the generic component commands.

**Reflected resources** (readable and editable through the generic resource commands):
- `PhysicsConfig` (gravity, `units_per_meter`, `solver_iterations`, `max_events`). **Edited with `ecs_set_resource_field`**, which is undoable. Reconcile applies the change through its Changed check and the `applied_config` diff (see "Stage 2: components and sync"), so an undo of the edit applies the previous value the same way, and a change of `units_per_meter` rebuilds the world with no code in the command path.
- `PhysicsDebugSettings` (`enabled`, `colliders`, `aabbs`, `contacts`, `velocities`, `max_lines`). **The debug draw toggle is `ecs_set_resource_field` on `PhysicsDebugSettings.enabled`**, also the editor's control (its Resources tab). The view `bounds` and `line_cap` are **not** here: they live in the non-reflected `PhysicsDebugView` that the facade glue rewrites every frame, so the Resources tab shows no constantly changing field and nothing meaningless lands on the undo stack.
- `PhysicsStats` (read-only, `custom(readonly = true)` through the AM4 attribute issue for this crate: counts by body kind, awake and sleeping, contact pairs, last step in ms, `tick`, `events_dropped`, `impulses_dropped`, `pending` (entities with a body or collider component that reconcile skipped for want of a `GlobalTransform2d`), `issues`). **Read with `ecs_get_resource`**: there is no `physics_get_stats` tool.

**Superseded: `SetPhysicsConfig` and `SetDebugDraw` are not built** (AM5 of `editor-mcp-mvp.md`; they would duplicate `ecs_set_resource_field` with a second schema for the same edit). Open question 5 is answered: yes, the bus has a generic resource-field command.

**Commands** (typed, serializable, registered from `plugin/commands.rs`; the only physics-specific command):

| Command | Effect | Undo |
|---|---|---|
| `physics_apply_impulse { entity, impulse, point? }` | push an entry on the `ImpulseQueue`, applied in the next `sync_in`. Output: the world `tick` it was queued at. Errors: `unknown_entity` (not a live entity), `validation_failed` (no body, not a dynamic body, a non-finite value, **or `SimulationMode` is `Frozen`**: no tick runs, so the impulse would wait and fire at `play_start`; the message says "simulation is frozen, play first"), `busy` (the queue holds its 1,024 entries). | **`UndoKind::NotUndoable`**: a runtime action, not an authoring edit. An impulse is a one-way change of simulation state, and replaying it on a restored scene would not reproduce the world. It is journalled, is never on the undo stack and does not clear the redo stack (`editor-mcp-mvp.md`, "The bus"). |

**Queries** (read-only, registered with `register_query::<Q>` under these names; the input and output structs are reflected, so the schemas come from `TypeInfo`, and they are built from `PhysicsQueries`). Entities travel as `"{index}v{generation}"` strings, vectors as `[x, y]`, lengths in world units. Every output carries the world `tick` and `pending` (`PhysicsStats::pending`), so an empty answer can be told from a world that is not built yet.

| Query | Input | Output |
|---|---|---|
| `physics_raycast` | `origin`, `direction`, `max_distance`, `all` (false: first hit only), `layers`, `include_sensors`, `exclude: Option<Entity>` | `hits: [{ entity, point, normal, distance }]` sorted by distance |
| `physics_overlap_point` | `point`, `layers`, `include_sensors` | `entities: [Entity]` |
| `physics_overlap_shape` | `shape: Shape`, `position`, `angle`, `layers`, `include_sensors` | `entities: [Entity]` |
| `physics_shape_cast` | `shape: Shape`, `position`, `angle`, `velocity` (direction times speed), `max_distance`, `layers`, `include_sensors`, `exclude` | `hit: Option<{ entity, distance, point, normal }>` |
| `physics_body_state` | `entity` | pose, velocity, sleeping, mass, `kind`, contacts `[{ entity, normal }]` |
| `physics_debug_draw` (Stage 4, P20) | `bounds: Option<Rect>`, `colliders` (true), `aabbs`, `contacts`, `velocities` (false), `max_lines` (default 256, at most 20,000) | `lines: [{ a, b, kind }]` in world units, `truncated` |

Query rules (they follow from the bus, `editor-mcp-mvp.md` "The command model"):
- A query must not change observable state. The five Stage 3 queries may need `PhysicsWorld::update_queries` (the world's own cache refresh) first: they call it through `bypass_change_detection`, which the bus's stamp check allows. `physics_debug_draw` calls `PhysicsWorld::debug_draw` on `&self` and a scratch buffer, so it has no side effect at all.
- Queries run at a boundary between schedule runs against the world as of the last `reconcile`. The **two-frame rule** of "Stage 2: components and sync" applies: a dynamic or kinematic root created by a spawn command is queryable after the next `PostUpdate` (which runs in Edit mode too), a static body, static collider or child collider after the second. Every output carries `tick` (the count of steps, which does not move in Edit mode) and `pending`, which is how an agent sees the world catch up: wait until `pending` is 0. `physics_body_state` for a live entity that has a `RigidBody` or `Collider` and no link yet is the error `busy` ("the body is not built yet, retry after the next frame"): the existing retryable code of the bus (`editor-mcp-mvp.md` "Error codes") is used instead of a new one; the other queries return their normal, possibly empty, answer.
- A non-live `entity` in an input is `unknown_entity`; a non-finite number or an empty direction is `invalid_params`.
- `physics_debug_draw` is **independent of `PhysicsDebugSettings::enabled`**, and its default `max_lines` of 256 keeps the JSON near 13 KB (a line is about 50 bytes: two points and a kind). The 100,000-line default of the resource is for the renderer, not for an agent.

Notes:
- **Recorded for other specs** (this PR may not edit them; the architect files the amendments):

  | Spec | What it must change |
  |---|---|
  | `editor-mcp-mvp.md` (SP8, #149) | Its tool table (the "physics" row and the physics-2d.md exposure row) lists five queries and `physics_apply_impulse`; `physics_debug_draw` is the sixth query, and `CharacterController` and `CharacterState` are two more reflected components. Its parity test needs no new rule: all of them are reachable through the generic surface or the registry. |
  | `render-v1.md` (SP5) | L482 says physics writes the lines "each `Update`": it is `PostUpdate`. L863 (G1c) and physics G1 are **one** facade issue (above). G1c says `bounds` comes from `CameraViews::main()`: it is the union of every active pass including `SceneView`, with `line_cap = Some(MAX_DEBUG_LINES / passes)` and `None` with no active pass (L313 says the cap is summed over passes). **The owner decided on 2026-11-25 that this bounds contract stays the union of every pass, `SceneView` included; the render spec's amendment PR (#171) adopts it.** A pending screenshot request's `view` should be exposed to that union. |
  | tilemap (#147) | The per-kind `TileCollision` table, the unmergeable-kind masking, the chunk size and the per-edit budget in "Tile colliders". |
- The editor's viewport toggle, stats tab and query probe are generic panels of the editor spec, not physics code. Collider gizmos are a non-goal, and the editor has no edge to `exitium_physics`.
- **Play-in-editor.** The physics world is derived state and is never saved or snapshotted. `play_stop` re-instantiates the scene, so every old entity is dead and reconcile's sweep removes their bodies; the new entities get new bodies. `PhysicsConfig` and `PhysicsDebugSettings` are engine resources: they carry no `custom(play_snapshot = true)`, so an edit made during Play stays after Stop (a game that wants its gravity restored marks its own resource). Queued impulses are dropped when the world is rebuilt.
- **Parity.** The facade's scripted-scene test and the Stage 3 parity test (`editor-mcp-mvp.md`) check that every component, resource, command and query above is reachable through the generic surface or the registry.

## Performance

Min-spec is 4 cores and 8 GB. Everything here is single-threaded and native release. Step budgets are the **mean time per step over the busiest 2 seconds** of the scene (120 steps, the settling phase of a drop into a bin), taken as criterion's median over runs. A spike guard sits beside each one: no single step in that run takes more than twice the mean budget (asserted by the bench itself, not by the 5% gate). Query budgets are mean time per call.

**The three `move_character` rows have no probe behind them** (the step and query rows do, below), and rapier documents autostep as very computationally expensive. They are provisional: **P17 takes a probe first** (a scratch bench of the cases above on min-spec-class hardware, before it writes the real bench), puts the numbers in its PR body, and if a row is missed by more than the headroom it **proposes an amendment PR** to this table instead of loosening the bench. The same holds for the 100-character row in P18.

A one-off probe (scratch crate, not committed; rapier2d 0.36.1 with the Stage 1 features; Windows 11 desktop, release, `f32`) gives the order of magnitude. Dropping 1,000 mixed boxes and circles into a bin: mean 0.30 ms per step over the first 2 seconds, and p99 0.53 ms and worst 0.75 ms over all 600 steps. For 2,000 bodies: mean 0.59 ms, p99 0.97 ms, worst 1.37 ms (same windows). A ray over 10,000 static boxes: 0.13 us. That machine is faster than min-spec, so the budgets keep 3 to 4 times of headroom. The first benchmark run may justify tightening them; an amendment PR changes a budget, never an implementer.

| Case | Budget (min-spec, native) |
|---|---|
| `PhysicsWorld::step`, 1,000 awake dynamic bodies (boxes and circles in a bin) | mean <= 1.0 ms, worst step <= 2.0 ms |
| `step`, 2,000 awake bodies | mean <= 2.0 ms, worst step <= 4.0 ms |
| `step`, 200 awake bodies and 5,000 static colliders | mean <= 1.0 ms |
| `step`, 2,000 bodies all asleep | <= 0.05 ms |
| Pose snapshot (`prev` and `pose`) overhead inside `step` | <= 100 ns per awake body |
| `cast_ray`, 10,000 static colliders | <= 2 us per call (<= 4 us at 100,000) |
| `overlap_shape` (circle), 10,000 static colliders | <= 5 us per call (<= 10 us at 100,000) |
| `debug_draw`, 1,000 colliders, `bounds: None` | <= 0.5 ms and no allocation after warm-up |
| Stage 2 per tick, 1,000 bodies (200 awake): reconcile with nothing changed, `sync_in` and `write_back` | <= 0.15 ms in total |
| Stage 2 `interpolate`, 1,000 awake bodies | <= 0.05 ms |
| `debug_draw`, 10,000 colliders of which about 100 are inside `bounds` | <= 0.5 ms (the cull is O(colliders), a few ns each) |
| Stage 2 `debug_draw` system when enabled and nothing changed | <= 5 us (the change check only); when disabled, 0 allocations and no query |
| `physics_debug_draw` query, 256 lines, 10,000 colliders | <= 0.6 ms including the JSON-ready report |
| `move_character`, 10,000 static colliders, slide on, autostep off, including the `ground` probe | <= 10 us per call (provisional, see the note below) |
| `move_character`, 10,000 static colliders, slide, autostep and snap on | <= 25 us per call (provisional) |
| Stage 2 `character_move`, 100 characters per tick (autostep on for half) | <= 2.0 ms in total (provisional): 50 calls at the 25 us autostep row plus 50 at the 10 us row is 1.75 ms before any system overhead |
| `CellMerger::merge`, 64 by 64 cells: all one kind; random 50% fill; checkerboard (the most rectangles, 2,048) | <= 20 us; <= 100 us; <= 150 us |
| `CellMerger::merge`, 256 by 256 cells, random 50% fill | <= 2 ms |
| Memory per body with one collider, including rapier | <= 1 KiB (2,000 bodies <= 2 MiB) |
| Total physics cost of a frame (step plus sync) at 2,000 awake bodies | mean <= 2.2 ms, worst <= 4.3 ms (about 25% of the 16.6 ms frame) |
| Chrome, wasm32 | <= 2.5 times the native budgets, measured by a manual run recorded in the benchmark issue. Not a CI gate (testing covers Windows and Chrome only). |

Complexity: `step` is O(awake bodies plus contacts); the broad phase is a BVH, so queries are O(log n). Reconcile is O(changed plus sweep), the sweep being O(bodies) with a few ns per body. Write-back and interpolation are O(awake).

**Benchmarks to add** (`docs/benchmarks.md` conventions: IDs `<crate>/<area>/<case>` with the size as criterion's parameter, `Throughput::Elements`, inputs built outside `b.iter` from a seeded linear congruential generator in the bench file with no `rand` dependency, `black_box` on inputs and outputs):

| ID | Sizes | Note |
|---|---|---|
| `exitium_physics/step/bin` | 1,000 and 2,000 awake bodies | 1k is the convention's smallest size; 2k is the MVP target on min-spec |
| `exitium_physics/step/static_active` | 5,000 static colliders with 200 awake bodies | one size: it measures the static-to-active ratio |
| `exitium_physics/step/asleep` | 2,000 sleeping bodies | |
| `exitium_physics/query/ray` and `.../query/overlap_circle` | 1,000, 10,000 and 100,000 static colliders | 100k is a real size here (BVH depth), so the convention's 1k and 100k are both kept, with 10k as the MVP scale |
| `exitium_physics/debug/draw` | 1,000 colliders unbounded; 10,000 colliders with `bounds` | the second size is added by P20 |
| `exitium_physics/sync/tick` and `.../sync/interpolate` | 1,000 bodies (200 awake) | Stage 2 |
| `exitium_physics/controller/move` | 1 and 100 characters among 10,000 static colliders | Stage 4 (P17; the 100-character case in the plugin system by P18) |
| `exitium_physics/tiles/merge` | 64 by 64 (all solid, random 50%, checkerboard) and 256 by 256 (random 50%) | Stage 4 (P19) |

**Sizes that differ from the convention (1k, 100k, 1M), and why.** For dynamic bodies, 100k and 1M are meaningless: a single-threaded rapier step at that count takes seconds, and the min-spec target is about 2,000 bodies (the roadmap's 1M-entity benches belong to the ECS and render crates, which restore them; they do not apply to a rigid-body solver). For queries, 1M colliders would only measure memory. For `controller/move` and `tiles/merge` the sizes are the game's own (a crowd of characters, a chunk and a whole map): 100,000 characters or a 1M-cell grid is not a use. Each bench file's `//!` doc states this, as the convention requires for a dropped size. Each bench ships with the change that adds the hot path (design rule 4).

## Test plan

Stage 1 (`cargo test -p exitium_physics --no-default-features`):
- **Validation:** every `Shape` and settings field rejects NaN, infinity and out-of-range values with a `PhysicsError` and never panics (a table of cases plus a seeded loop of random garbage; no new dependency). A convex polygon with 2, 65, collinear or duplicate points is rejected; a slightly non-convex one is hulled.
- **Lifecycle:** add, update and remove a body and a collider; a stale `BodyId` after `remove_body` (and after the slot is reused) is rejected; removing a body removes its colliders.
- **Physics behavior:** a ball falls `0.5 * g * t^2` within a tolerance; a box comes to rest on a static floor and goes to sleep; restitution 1 versus 0 gives a bounce versus none; friction 0 slides and friction 1 sticks on a slope; a kinematic platform carries a body; `lock_rotation` and `gravity_scale` take effect; `ccd` stops a fast body at a thin wall; layers and filters (the `memberships`/`filter` rule, both directions).
- **Events:** a sensor reports `Started` then `Stopped` exactly once; a kinematic body triggers a static sensor; `report_events = false` is silent; the overflow counter works; an event survives its body's removal.
- **Queries:** ray hit point, normal and distance; `cast_ray_all` is sorted; filters exclude a body and sensors; point, overlap and shape-cast cases, including a ray that starts inside a collider.
- **Interpolation:** `alpha` 0, 0.5 and 1; shortest-arc angle across the pi wrap; `set_pose` snaps (no blend across a teleport); sleeping bodies stay put.
- **Repeatability:** two worlds, the same scripted inputs, 600 steps: bit-identical poses and the same event sequence. A world rebuilt from the same descriptions steps identically.
- **Debug draw:** a box produces 4 `Collider` lines at the right corners; `bounds` culls; `max_lines` sets `truncated`; each `DebugKind` has the color of its table row; a sleeping body's lines are `Sleeping` and a sensor's are `Sensor`.
- **Character controller (P17), with the default settings unless stated:**
  - *Flat ground:* `translation == desired` and `grounded`.
  - *Standing still (the gravity-rule test):* a character on flat ground, 120 ticks, each tick `desired = (1, 0)` and then `(0, 0)` and then `(0, -g*dt)` when not grounded and `(0, 0)` when grounded, as the game's rule does: `grounded` is true on **every** tick (never alternates) and `ground` is `Some(floor)` on every tick, including the ticks with `desired.y == 0` and no collision.
  - *Platform ride (Stage 1 half):* a kinematic platform with velocity `(2, 0)` m/s. The test adds the platform's velocity to `desired` itself each tick (as the Stage 2 plugin does), on top of the game's gravity rule: `desired = platform_velocity * dt + (0, 0)` while `grounded`, plus the `-g*dt` term when not. After 120 ticks the character has moved with the platform within `skin`, and `grounded` and `ground == Some(platform)` hold on every tick after the first grounded one. **The test does not rely on rapier's kinematic-friction path:** with `desired = (0, 0)` that path does not run (it needs a hit), and a second assertion pins the documented consequence, that `move_character` alone does not carry a standing character.
  - *Walls:* a wall blocks the normal part of the move and slides the rest.
  - *Slopes (the promised cases; **every case in both orientations**, a ramp rising toward `+x` and its mirror):* a 30 degree ramp is climbed; **a 45 degree ramp is climbed** (the canonical tile ramp: `max_slope_climb` 50 degrees leaves margin over 45); with the equal defaults the character stands still on a 45 degree ramp for 120 ticks under the game's gravity rule (`grounded` every tick, no drift beyond `skin`). Anything steeper than `max_slope_climb` is not tested for a result, because rapier's angle is signed (see "Slope angles are signed in rapier 2D"); a vertical **box** wall blocks from both sides.
  - *`sliding_down_slope`:* false on flat ground on the landing tick, standing still, walking, and on every tick of a climb up a 30 degree ramp (rapier's raw `is_sliding_down_slope` is true on the climb ticks and on the landing tick, so the test fails if the field is copied from it). With `max_slope_climb = 50` and `min_slope_slide = 30` degrees, a character on a 40 degree ramp **rising toward `+x`** (the side where rapier's slide threshold works) slides down it: `sliding_down_slope` is true and `translation` dot `up` is below 0. The other orientation is not tested (not promised).
  - *Autostep and snap:* autostep climbs a 0.3 m ledge and not a 0.6 m one; `snap_to_ground` keeps it on a down slope and is off in the air; a ceiling sets `hit_ceiling`.
  - *Dynamic bodies:* a dynamic crate gets an impulse with `push_dynamic` and none without.
  - *Edits before the move:* a collider added with `add_collider` immediately before `move_character` (no `update_queries` call by the test) blocks the move (the dirty-flag refresh).
  - *One-way platforms:* with `one_way_layers` set, a jump up from below passes through (and does not land on it on the way up); falling from above lands (`grounded`, `ground` is the platform); walking sideways into the edge from below the top is not stopped; **a character standing on a kinematic one-way platform that rises at 1 m/s, with the platform's velocity in `desired`, has `grounded` true and `ground == Some(platform)` on every tick** (the flicker regression: no rule on the direction of `desired`); **`drop_through` is held for 12 ticks (0.2 s at 60 Hz) under the game's gravity rule: the character falls through, is released while below the platform's top by more than `2 * skin`, and the platform is solid again for the next landing from above (it lands on a floor below the platform, not back on it); the same drop with `drop_through` set for a single tick lands back on the platform** (this documents why the flag is held); a collider without the layer bit is solid from below; a one-way platform is ignored by the `ground` probe exactly as by the move.
  - *`TopDown`:* **all four wall orientations** (wall to the left, right, above and below the character), each walked into head-on and **along a diagonal** (the movement slides along the wall, in **both** directions along each wall: a 0.3 by 0.3 diagonal gives the identical 0.3 slide on all four walls and both diagonals with `max_slope_climb_angle = 4.0` and `min_slope_slide_angle = -4.0`): the result is the same up to mirroring (compared to 1e-4), `grounded`, `sliding_down_slope`, `hit_ceiling` and `ground` are never set, `hit_wall` is set on every collision. A character jammed in a corner between two walls does not jitter or tunnel.
  - *Errors and filters:* a sensor and a collider excluded by `filter` are ignored; NaN `desired`, `dt <= 0`, a non-kinematic body and a stale id are errors.
- **Tile merge (P19):** a solid 4 by 4 grid is one rectangle; an L shape is two; a checkerboard of n cells is n/2 rectangles; two kinds side by side are never merged; an empty grid gives none; a seeded loop of random grids checks disjointness, exact coverage of the non-empty cells, one kind per rectangle, rectangles no more than non-empty cells, and identical output on a second run; `cells.len() != width * height` and a grid over `1 << 22` cells are errors, **and so is a grid whose `width * height` overflows `u32` (65,536 by 65,536 with an empty slice must not pass)**.
- **wasm:** `cargo build -p exitium_physics --target wasm32-unknown-unknown` and `cargo xtask ci all`. `crates/exitium_physics` is not `NATIVE_ONLY`.

Stage 2 (a headless `App`, no window):
- Spawning an entity with `RigidBody` and `Collider` creates a body; despawning it removes it (the sweep); removing only the `Collider` component removes the collider.
- A `SetField` on `Collider.material.friction` (through reflection) changes the simulation. A `Changed` edit with an invalid value yields an issue and leaves the old setting.
- Teleport: writing `Transform2d` in a `FixedUpdate` system and in an `Update` system moves the body and does not blend. The plugin's own writes are never mistaken for teleports (no body moves a second time on its own).
- A frame with 0 ticks and a frame with 2 ticks: `interpolate` draws between `prev` and `pose` and the final pose matches the Stage 1 run; `interpolate = false` shows the exact pose.
- Events: delivered once per frame to an `Update` system and once per tick to a `FixedUpdate` system, with zero ticks giving none; an event whose entity is dead, or whose `user_data` is 0 or does not decode (`Entity::from_bits` returns `None`), is dropped and counted.
- Moving a `Static` body or a static collider (`Transform2d` write from a system, as an editor drag would) re-places it in rapier; a body resting on it wakes; the plugin creates no body or collider with `user_data == 0`.
- **The two-frame rule:** a dynamic root spawned between frames exists after one `PostUpdate`; a **static collider** (and a static body, and a child collider) spawned the same way does **not** exist after one `PostUpdate` (no `GlobalTransform2d` yet, no issue reported) and exists, and is hit by a ray, after the second. `PhysicsStats::pending` is the number of skipped entities after the first `PostUpdate` (3 for one of each) and 0 after the second, and `physics_body_state` of a spawned static collider after the first frame is `busy`. A body and a static collider spawned in `Startup` and a tick run before the first `PostUpdate`: the body is stepped and the collider is not in the world (the "what a tick sees" rule).
- A compound body (child colliders) has the summed mass. A non-root dynamic body is skipped with an issue.
- Scene round trip: save the reflected components, restore, run: the world is rebuilt and the bodies resume with their `Velocity`.
- Commands (Stage 3, P15): `physics_apply_impulse` is queued, applies in the next `sync_in`, is journalled and is never on the undo stack (`UndoKind::NotUndoable`); a non-dynamic body is `validation_failed`, a dead entity `unknown_entity`, the 1,025th queued entry `busy`; with `SimulationMode::Frozen` it is `validation_failed` and the queue stays empty, and an entry queued by game code in `Update` is cleared by the change to `Running` instead of firing at `play_start`. `ecs_set_resource_field` on `PhysicsConfig` applies and undoes. A `units_per_meter` change (also through that command, and through its undo) rebuilds the world with the same visible result; `ecs_set_resource_field` on `PhysicsDebugSettings.enabled` turns debug draw on and off.
- Queries (Stage 3, P16): each of the five registers under its `physics_*` name; its input accepts the sample generated from its schema; the bus's read-only stamp check passes (no component or resource change stamp moves); results carry `tick`; a non-live entity is `unknown_entity`.
- Character controller (Stage 4, P18): a kinematic body with a `CharacterController` walks, stops at a wall and climbs a ramp through the plugin with the same results as P17; `CharacterState` is written only when a field changes; with `inherit_platform_velocity` it rides a moving kinematic platform while the game applies its zero-vertical-speed-when-grounded rule (the plugin adds the platform's `Velocity` to the desired velocity, with one tick of lag, which is what carries it: rapier's kinematic-friction path is not relied on; `CharacterState::ground` is `Some(platform)` and `grounded` true on every tick after the first grounded one) and without it stays behind; standing still on a floor for 120 ticks never flickers `grounded`; a `reconcile` that adds a wall in `FixedFirst` blocks the character in that same tick; a controller on a dynamic body or without a collider is an issue and no body moves; 100 characters meet the budget.
- Debug draw (Stage 4, P20): with `SimulationMode::Frozen` and `enabled`, `PhysicsDebugDraw` fills and a `physics_*` query answers (no tick runs), for a dynamic body after one frame and for a **static collider after two** (two-frame rule); `line_cap` and `bounds` from `PhysicsDebugView` limit the lines, the effective limit being `min(max_lines, line_cap)`; nothing is computed or allocated when `enabled` is false; an unchanged frame leaves `lines` untouched; `physics_debug_draw` answers with `enabled` false, changes no resource stamp, and `truncated` is set at `max_lines`.
- Parity: every component, resource, command and query listed in "MCP / editor surface" is reachable through the generic registry (checked with the editor spec's parity tests).

## Dependencies

Only one crates.io dependency is new, plus its transitive set. Declared once in the root `[workspace.dependencies]`; the Stage 1 intake PR adds it with the `Cargo.lock` change, so no implementation issue touches `Cargo.lock`.

| Crate | Version | License | wasm | Why not std |
|---|---|---|---|---|
| `rapier2d` | `0.36.1`, `default-features = false`, `features = ["dim2", "f32", "std", "block-solver", "debug-render"]` | Apache-2.0 | yes (built for `wasm32-unknown-unknown` for this spec) | A 2D rigid-body solver, broad and narrow phase, CCD and query structure is a large body of numerical code that INTERVIEW #20 already chose to reuse. |

Transitive set in that build for `wasm32-unknown-unknown` (from `cargo metadata`): `parry2d 0.31.1`, `nalgebra 0.35.0`, `glamx 0.3.1` and `glam 0.33.12` (a second glam, next to the workspace's 0.34.3), `simba 0.10.2`, `wide 1.7.1`, `spade 2.15.1`, `approx`, `arrayvec`, `bitflags`, `foldhash`, `ordered-float`, `num-traits`, `num-complex`, `num-rational`, `num-integer`, `matrixmultiply`, `typenum`, `thiserror 2`, `log`, `profiling`, `static_assertions`, `slab`, `smallvec`, `either`, `ena`, `downcast-rs`, `hashbrown` (0.16 and 0.17), `libm`, `bytemuck`, `allocator-api2`, `equivalent`, `rawpointer`, `robust`, plus proc-macro crates `syn` (2 and 3), `quote`, `proc-macro2`, `unicode-ident`, `nalgebra-macros`, `num-derive`, `profiling-procmacros`, `thiserror-impl`. All licenses are MIT, Apache-2.0, Zlib or combinations of them, which `deny.toml` allows; `unicode-ident` is `(MIT OR Apache-2.0) AND Unicode-3.0`, also allowed. Two glam, two hashbrown and two syn versions trigger `multiple-versions = "warn"` only. The intake PR re-runs `cargo deny check` and `cargo tree` to confirm this list.

No other dependency: no `serde` (serialization is reflection; the JSON of the queries is `exitium_command`'s), no `rand`, no `bitflags` of our own, no channel crate (a `Mutex<Vec>` collector). `criterion` is the existing workspace dev-dependency.

**SP7 (#143) adds no crates.io dependency.** The character controller is rapier's own `control` module inside `rapier2d` (the implementer confirms it needs no feature beyond the five above; if it did, the change is a feature flag on the existing entry, recorded in the P17 PR and applied by intake 3, with licence and wasm unchanged). Tile merging is std (a `Vec<u64>` bitset), the debug draw query and `physics_apply_impulse` use `exitium_command`, a workspace crate. There is no new licence, no new wasm question and no "why not std" row to add.

### Intake wiring (for the intake PRs, not issues in this spec)

The intake PRs add the union of every merged spec's list (C5 of `mvp-roadmap.md`). This is physics' list: every dependency, crate edge, dev-dependency and feature its issues need, so no implementation issue touches `Cargo.lock` or a `Cargo.toml` dependency table.

**Intake 1 (Stage 1):**
- Root `Cargo.toml` `[workspace.dependencies]`: `rapier2d` as in the table above.
- `crates/exitium_physics/Cargo.toml`: `[dependencies] exitium_core.workspace = true, rapier2d.workspace = true`; `[dev-dependencies] criterion.workspace = true`; no features yet.
- New crate edge: `exitium_physics -> exitium_core`. No other edge.
- Acceptance: `cargo build -p exitium_physics --target wasm32-unknown-unknown`, `cargo deny check`, and the transitive list above re-confirmed with `cargo tree`.
- The `[[bench]]` stanzas are added by the issues that create the bench files. They are inside the crate and do not change `Cargo.lock`.

**Intake 2 (Stage 2):**
- `crates/exitium_physics/Cargo.toml`: optional dependencies `exitium_ecs`, `exitium_app`, `exitium_reflect` (and `exitium_reflect_derive` if `exitium_reflect` does not re-export the derive) and `exitium_command`; features `reflect = ["dep:exitium_reflect"]`, `plugin = ["reflect", "dep:exitium_ecs", "dep:exitium_app", "dep:exitium_command"]`, `default = ["plugin"]`.
- **Dev-dependency `exitium_command`** (AM5): P15 and P16 round-trip every command and query through its schema API (CB9 of `editor-mcp-mvp.md`). It is the same crate as the optional dependency, which a dev-dependency may repeat, so the Stage 1 `--no-default-features` build is unchanged. No `serde_json` dev-dependency: the JSON stays behind `exitium_command`'s `json` module. The headless-app tests use `exitium_app`.
- New crate edges: `exitium_physics -> exitium_ecs`, `-> exitium_app`, `-> exitium_reflect`, `-> exitium_command` (normal, optional, and the dev-dependency above).
- Facade: `exitium -> exitium_physics` (the facade depends on every crate; its re-export and any glue are in G1).
- **No edge** to `exitium_render`, `exitium_mcp`, `exitium_editor`, `exitium_platform`, `exitium_audio`, `exitium_assets`, `exitium_ui`, `exitium_scene` or `exitium_tilemap`. Runtime crates have no edges between them.

**Intake 3 (Stage 4): nothing for physics.** The Stage 4 issues (P17 to P20) add no dependency, no feature and no crate edge: the character controller and the tile merge are inside the existing `rapier2d` and std, and the debug draw query uses the `exitium_command` edge that intake 2 already added. Tilemap colliders reach physics through the facade (the tilemap spec's glue issue, `Blocked by` P19), never through an `exitium_tilemap -> exitium_physics` edge (C1). Intake 3's Footprint still lists `crates/exitium_physics/` under `Edits`, which keeps any physics issue from running beside it (R1); it has no physics change to make unless the P17 PR finds the controller needs a rapier feature.

## Implementation issues

After this spec merges the architect files these into the MVP milestone with the `file-issue` skill. `P1` to `P20` and `G1` are placeholders, replaced by issue numbers when filed; `intake-1`, `intake-2` (#157) and `intake-3` are the intake PRs. `CB9` is `feat(command): JSON Schema from TypeInfo and the tool list` in `editor-mcp-mvp.md`. Each issue edits one crate (or, for G1, only the facade hotspot) and has the line estimate of its change (code, tests and docs) next to it; every estimate is at or under about 600. `exitium_physics` issues share a crate, so the factory runs one at a time even where the dependencies below would allow two; the list is in dependency order. Every Stage 1 issue is `Blocked by intake-1`. Every Stage 2 issue is `Blocked by intake-2` and by the merged and implemented #106 `App`/`Plugin` API (and the ECS, reflect and command crates where named). Every Stage 3 issue is `Blocked by intake-2` and every Stage 4 issue by `intake-3` (C6). The roadmap counts Stage 4 physics tooling as "about 3"; this spec files four (P17 to P20), the character controller being two PRs (wrapper, then component), and the physics lane is not on the Stage 4 critical path (render v2, 14 PRs, is).

**Common acceptance criteria for every issue:** `cargo xtask ci all` passes; new behavior has tests; each hot-path change ships its benchmark with the numbers in the PR body; every new module has `//!` docs; no rapier type is public; no `unsafe`.

**Stage 1 (`exitium_physics`, depends on `exitium_core` and `rapier2d` only):**

**P1. `feat(physics): settings types, ids and validation`** (about 500 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `types.rs`, `error.rs`, `convert.rs`: `BodyKind`, `RigidBody`, `Collider`, `Shape`, `Material`, `CombineRule`, `CollisionLayers`, `Velocity`, `BodyPose`, `BodyId`, `ColliderId`, `PhysicsError`; the glam 0.34 to 0.33 conversions; the crate `CLAUDE.md`.
- Acceptance:
  - [ ] Every field has a documented default and range; each invalid value (NaN, infinity, negative size, polygon with 2, 65, collinear or duplicate points) returns a `PhysicsError`, in table tests plus a seeded garbage loop.
  - [ ] `Shape::ConvexPolygon` hulls a slightly non-convex input.
  - [ ] Conversions round-trip `Vec2` exactly.
  - [ ] Builds for `wasm32-unknown-unknown`.
- Blocked by: intake-1.

**P2. `feat(physics): PhysicsWorld lifecycle`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `world.rs`: `new`, `set_gravity`, `add_body`, `add_collider`, `remove_body`, `remove_collider`, `update_body`, `update_collider`, `body_user_data`, `collider_user_data`; the rapier builder mapping (including `ActiveCollisionTypes` and `ActiveEvents`).
- Acceptance:
  - [ ] Add, update and remove a body and a collider; removing a body removes its colliders.
  - [ ] A stale `BodyId` or `ColliderId` is rejected, also after the slot is reused.
  - [ ] A parentless collider is static; a collider with a parent attaches with its local pose.
  - [ ] `user_data` round-trips.
- Blocked by: intake-1, P1.

**P3. `feat(physics): runtime control, step and body state`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `world.rs`: `set_pose`, `set_velocity`, `apply_impulse`, `step`, `tick`, `body_state`, `active_bodies`, `stats`; `StepStats`.
- Acceptance:
  - [ ] A ball falls `0.5 * g * t^2` within a tolerance; a box rests on a static floor and sleeps.
  - [ ] Restitution 1 versus 0, friction 0 versus 1 on a slope, kinematic platform carries a body, `lock_rotation`, `gravity_scale`, `ccd` at a thin wall, and the layer rule in both directions.
  - [ ] `step` returns `Err` for a negative or non-finite `dt`; `dt == 0` applies pending changes without NaN.
  - [ ] `set_pose` on a static body re-places it and wakes touching bodies.
- Blocked by: intake-1, P2.

**P4. `perf(physics): pose snapshots, interpolation, repeatability and step benchmarks`** (about 350 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `BodyRecord`, the snapshot inside `step`, `interpolated_pose`, the `set_pose` snap; `benches/step.rs` and its `[[bench]]` stanza with the IDs and sizes from "Performance".
- Acceptance:
  - [ ] `interpolated_pose` at alpha 0, 0.5 and 1, shortest-arc angle across the pi wrap; sleeping bodies stay put; `set_pose` does not blend.
  - [ ] Two worlds fed the same scripted inputs for 600 steps give bit-identical poses and events.
  - [ ] Bench results are in the PR body and meet the budgets (mean and spike guard).
- Blocked by: intake-1, P3.

**P5. `feat(physics): collision and trigger events`** (about 400 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `events.rs`: `CollisionEvent`, `CollisionKind`, the `Mutex<Vec>` collector, `drain_events`, the overflow counter, `contacts_of`.
- Acceptance:
  - [ ] A sensor reports `Started` then `Stopped` once; a kinematic body triggers a static sensor; `report_events = false` is silent.
  - [ ] Events carry both `user_data` values and survive the removal of a body in the same tick.
  - [ ] Overflow past `max_events` is counted in `stats`.
- Blocked by: intake-1, P3.

**P6. `feat(physics): raycasts and shape queries`** (about 550 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `query.rs`: `QueryFilter`, `Ray`, `RayHit`, `ShapeHit`, `cast_ray`, `cast_ray_all`, `colliders_at_point`, `overlap_shape`, `cast_shape`, `update_queries`; `benches/query.rs`.
- Acceptance:
  - [ ] Hit point, normal and distance are correct; `cast_ray_all` is sorted by distance, ties by `ColliderId`.
  - [ ] Filters exclude a body, sensors and non-matching layers; a ray starting inside a collider is defined.
  - [ ] A zero or non-finite ray returns `None`.
  - [ ] Bench meets the budgets at 1k, 10k and 100k colliders.
- Blocked by: intake-1, P3.

**P7. `feat(physics): debug draw data`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `debug.rs`: `DebugDraw`, `DebugLine`, `DebugKind`, `DebugDrawOptions`, the `DebugRenderPipeline` backend (the `DebugKind` color table instead of rapier's colors, the per-collider `bounds` cull, `max_lines`); `benches/debug.rs`.
- Acceptance:
  - [ ] A box gives 4 `Collider` lines at the right corners; `bounds` culls; `max_lines` sets `truncated`; every `DebugKind` has the color of its row in "Debug draw", sleeping bodies' lines are `Sleeping` and sensors' are `Sensor`.
  - [ ] No allocation after warm-up; bench meets the budget.
  - [ ] No dependency on `exitium_render`.
- Blocked by: intake-1, P3.

**Stage 2 (`plugin` module of `exitium_physics`):**

**P8. `feat(physics): reflected components, resources and PhysicsPlugin skeleton`** (about 500 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `cfg_attr(feature = "reflect", derive(Reflect))` with `range` and doc attributes on the settings types; `plugin/components.rs`, `plugin/config.rs`: `Component` impls, `BodyLink`, `ColliderLink`, `PhysicsConfig`, `PhysicsDebugSettings`, `PhysicsStats`; `PhysicsPlugin` registering the types and inserting the resources, in contract terms for #106.
- Acceptance:
  - [ ] Every listed type registers and round-trips through reflection; `BodyLink` and `ColliderLink` are not reflected.
  - [ ] Defaults match the spec; the plugin builds the resources in a headless `App`.
  - [ ] Builds for `wasm32-unknown-unknown`.
- Blocked by: intake-2, P1, P3, the reflect derive (#34, #35, #37) and the ECS component registry (#39, #50).

**P9. `feat(physics): reconcile, create and remove`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/reconcile.rs` (create and sweep half): create bodies and colliders for entities without links, insert `BodyLink`, `ColliderLink` and `Velocity`, the sweep that removes bodies and colliders of dead entities or removed components, the issue list, validation reports, `user_data = Entity::to_bits()` (never 0).
- Acceptance:
  - [ ] Spawning an entity with `RigidBody` and `Collider` creates a body; despawning it removes it; removing only the `Collider` removes the collider.
  - [ ] An invalid value yields an issue, no body, and a retry only after the component changes.
  - [ ] A non-root dynamic body is skipped with an issue; a non-finite `Transform2d` is an issue.
- Blocked by: intake-2, P8.

**P10. `feat(physics): reconcile, updates, static re-place, hierarchy, config changes and rebuild`** (about 550 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/reconcile.rs` (update half): `Changed<RigidBody>`/`Changed<Collider>` updates, **re-placing moved static bodies, parentless static colliders and child colliders from `Changed<GlobalTransform2d>`**, compound children of the nearest ancestor body, the `PhysicsConfig` comparison with `applied_config` (gravity, `solver_iterations`, `max_events` in place; invalid values an issue) and the `units_per_meter` rebuild. `reconcile` is registered in `FixedFirst` and in `PostUpdate`.
- Acceptance:
  - [ ] A reflection `SetField` on `Collider.material.friction` changes the simulation; an invalid edit keeps the old setting and adds an issue.
  - [ ] Moving a static body's `Transform2d` (editor or game system) moves its rapier body; a body touching it wakes; bodies on it are not carried.
  - [ ] A compound body has the summed mass; a child collider moved in the hierarchy is re-placed.
  - [ ] Changing `units_per_meter` (a direct resource write here; P15 repeats it through `ecs_set_resource_field` and its undo) rebuilds the world with the same visible result, and changing `gravity` takes effect without a rebuild.
  - [ ] An invalid `PhysicsConfig` value is an issue and the last valid value stays applied.
  - [ ] Run in an `App` whose `SimulationMode` is `Frozen` (once AM3 has landed; before that, by calling the `PostUpdate` schedule alone), a spawned dynamic root exists after one frame and no tick has run. **A spawned static collider, a static body and a child collider do not exist after one frame (no issue is reported) and exist after two**: the test spawns one of each and checks both frames (the two-frame rule), including `PhysicsStats::pending` (3 after the first frame, 0 after the second).
  - [ ] An edit of `PhysicsConfig` through the resource's change stamp applies once even though `reconcile` is registered twice (`units_per_meter` rebuilds the world once), and an edit back to the applied value does nothing.
- Blocked by: intake-2, P9.

**P11. `feat(physics): fixed-step system, sync_in and write_back`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/step.rs`: the `FixedLast` registration (after the game's `FixedUpdate` systems, `app-platform.md`), `sync_in` (with the `ImpulseQueue` resource and its 1,024-entry bound) (teleport detection by value, `Velocity`, queued impulses), `step`, `write_back` (active bodies only; exact pose for non-interpolated bodies).
- Acceptance:
  - [ ] A `Transform2d` write in a `FixedUpdate` system teleports the body; the plugin's own writes are never read back as a teleport.
  - [ ] A `Velocity` write takes effect in the same tick; sleeping and static bodies keep quiet change ticks.
  - [ ] Zero and two ticks per frame behave.
- Blocked by: intake-2, P4, P10.

**P12. `perf(physics): interpolation system and sync benchmarks`** (about 350 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/step.rs` (interpolate): `PostUpdate` system `before_set(PROPAGATE)`, teleport check before overwrite, `Time::fixed_alpha`; `benches/sync.rs`.
- Acceptance:
  - [ ] A frame with 0 and a frame with 2 ticks draws between `prev` and `pose`; `interpolate = false` shows the exact pose.
  - [ ] An `Update` write or an editor drag of a body is applied, not overwritten.
  - [ ] Benches meet the budgets.
- Blocked by: intake-2, P11, #106's `Time::fixed_alpha`.

**P13. `feat(physics): entity-level events and queries`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/events.rs`, `plugin/queries.rs`: `PhysicsEvents` (`this_tick`, `since_frame_start`, `clear_events`), `PhysicsQueries`.
- Acceptance:
  - [ ] An `Update` system sees each event once per frame (also with 2 ticks); a `FixedUpdate` system sees only the current tick's; zero ticks give none.
  - [ ] An event whose entity is dead or whose `user_data` does not decode (`Entity::from_bits` returns `None`) is dropped and counted in `events_dropped`.
  - [ ] Queries return entities and world units.
- Blocked by: intake-2, P5, P6, P11.

**P14. `feat(physics): debug draw resource and stats`** (about 300 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/debug.rs`, the `PhysicsStats` update: `PhysicsDebugDraw` (with `tick` and `truncated`, no `enabled`) filled in `PostUpdate` only when `PhysicsDebugSettings::enabled`, in world units, recomputed only when the tick, a reconcile edit, the settings or the `PhysicsDebugView` (`bounds`, `line_cap`) changed; counts, last step time, `impulses_dropped` and issues. `PhysicsDebugView` is not reflected.
- Acceptance:
  - [ ] Nothing is computed when `enabled` is false.
  - [ ] Lines are in world units; `bounds` and `line_cap` from `PhysicsDebugView` are applied (the limit is `min(max_lines, line_cap)`); an unchanged frame leaves `lines` untouched.
  - [ ] `PhysicsStats` reports the counts, the issues and `pending` (the entities reconcile skipped for want of a `GlobalTransform2d`, recomputed by every reconcile pass).
- Blocked by: intake-2, P7, P11.

**Stage 3 (`plugin` module, the command surface; AM5 of `editor-mcp-mvp.md`).** P15 and P16 are `Blocked by` **CB9** (their round-trip tests use the schema API; CB9 is itself blocked by CB1 to CB8, so `register_command`, `register_query` and `ecs_set_resource_field` exist) **and by the dev-dependency edge that intake 2 (#157) adds** (`exitium_command` as a dev-dependency of `exitium_physics`). They are also `Blocked by intake-2`. `SetPhysicsConfig` and `SetDebugDraw` appear nowhere in this list: they are not built.

**P15. `feat(physics): the physics_apply_impulse command`** (about 250 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/commands.rs`: `PhysicsApplyImpulse { entity, impulse, point? }` registered with `register_command` under the name `physics_apply_impulse`, declaring **`UndoKind::NotUndoable`**; it pushes onto the `ImpulseQueue` (created by P11). It is the only command this crate registers.
- Acceptance:
  - [ ] The command is journalled, never on the undo stack, and does not clear the redo stack; it applies in the next `sync_in`.
  - [ ] A dead entity is `unknown_entity`; no body, a non-dynamic body, a non-finite value or `SimulationMode::Frozen` is `validation_failed` (the frozen message says to play first); the 1,025th queued entry is `busy`; the world is unchanged on every error.
  - [ ] The `ImpulseQueue` is cleared on every `SimulationMode` change and on a world rebuild, so no impulse queued while editing fires at `play_start`.
  - [ ] The command accepts the sample generated from its reflected schema (CB9).
  - [ ] `ecs_set_resource_field` on `PhysicsConfig` (`gravity` and `units_per_meter`) applies and undoes; the `units_per_meter` change and its undo rebuild the world with the same visible result; `ecs_set_resource_field` on `PhysicsDebugSettings.enabled` applies and undoes.
- Blocked by: intake-2 (#157), P14, CB9, the dev-dependency edge from intake 2 (#157).

**P16. `feat(physics): register the five queries`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/tools.rs`: the reflected input and output structs and the registrations, with `register_query`, of `physics_raycast`, `physics_overlap_point`, `physics_overlap_shape`, `physics_shape_cast` and `physics_body_state`, built on `PhysicsQueries`. `physics_get_stats` is **not** a query of this crate: it is the generic `ecs_get_resource` on `PhysicsStats`.
- Acceptance:
  - [ ] All five are registered under exactly those names; each input accepts the sample generated from its schema (CB9) and the schema comes from `TypeInfo`, never from hand-written JSON.
  - [ ] The bus's read-only stamp check passes: no component or resource change stamp moves, including after `update_queries`.
  - [ ] Every output carries the world `tick` and `pending`; entities are `"{index}v{generation}"` strings and lengths are world units.
  - [ ] A dead entity in an input is `unknown_entity`; a non-finite number or an empty direction is `invalid_params`; `physics_body_state` for a live entity with a body or collider component and no link yet is `busy` ("the body is not built yet, retry after the next frame").
- Blocked by: intake-2 (#157), P13, P15, CB9, the dev-dependency edge from intake 2 (#157).

**Stage 4 (tooling).** Every issue is `Blocked by intake-3`. They can run beside render v2, assets and tilemap work (no shared crate), one physics issue at a time.

**P17. `feat(physics): character controller on PhysicsWorld`** (about 600 lines, the largest issue: it carries the controller, the one-way predicate and the probe; if the estimate passes 600 the implementer files the one-way predicate as a follow-up issue `Blocked by` P17 instead of growing the PR)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `controller.rs`: `CharacterMode`, `Autostep`, `CharacterSettings` (reflected under `reflect`, with `one_way_layers` and `drop_through`), `CharacterResult`, `PhysicsWorld::move_character` (`grounded` from rapier's `EffectiveCharacterMovement`; `sliding_down_slope` derived, not rapier's flag; the collision-derived `hit_*`, the `ground` probe, the `TopDown` values `4.0` and `-4.0`, the one-way predicate with rules (a) and (b) only, the dirty-flag refresh of the query structure), validation, and the doc comments on `max_slope_climb` and `min_slope_slide` that state the signed-angle caveat; `benches/controller.rs` with `exitium_physics/controller/move`.
- Acceptance:
  - [ ] Every case in the Stage 1 "Character controller" test list passes, in particular: **standing still keeps `grounded` true and `ground` `Some` on every tick** under the game's zero-when-grounded rule; a **platform ride** (the test adds the platform velocity to `desired`); **30 and 45 degree ramps climbed, each in both orientations**, and standing still on a 45 degree ramp in both; **`sliding_down_slope` false on a climb and a landing**; **all four `TopDown` wall orientations and a diagonal slide** (if one fails, P17 stops and files a spec amendment issue); **one-way platforms** (up through, land from above, sideways, a rising platform without flicker, `drop_through` held for 12 ticks and the single-tick case); a collider added just before the move is seen.
  - [ ] No rapier type is public; the crate builds for `wasm32-unknown-unknown`; no new dependency or feature (if rapier needs one, the PR says so, see "Dependencies").
  - [ ] The PR body has the probe numbers first, then the bench against the provisional budgets in "Performance" (<= 10 us per call without autostep, <= 25 us with it, at 10,000 static colliders), or an amendment proposal for the table.
- Blocked by: intake-3, P3, P6.

**P18. `feat(physics): CharacterController component and system`** (about 500 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/controller.rs`: `CharacterController` and `CharacterState` (reflected, the state `custom(readonly = true)`), the `character_move` system in `FixedLast` between `sync_in` and `step`, platform velocity inheritance, the diagnostics; the 100-character bench case.
- Acceptance:
  - [ ] The Stage 2 "Character controller" test passes: walks, stops at a wall, climbs a ramp with the same results as P17; rides a moving kinematic platform with `inherit_platform_velocity` (and the game's zero-when-grounded rule) and not without; standing still never flickers `grounded`; a wall that `reconcile` adds in `FixedFirst` blocks the character in the same tick.
  - [ ] `CharacterState` is written only when a field changes; a controller without a kinematic body and a non-sensor collider is an issue and moves nothing.
  - [ ] Inspector and generic ECS tools reach both components (schema from reflection, `one_way_layers` and `drop_through` included); 100 characters per tick meet the provisional budget (<= 2.0 ms) or the PR proposes an amendment.
- Blocked by: intake-3, P17, P11.

**P19. `feat(physics): tile-collider merging`** (about 350 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `tiles.rs`: `CellGrid`, `CellRect`, `CellMerger::merge`; `benches/tiles.rs` with `exitium_physics/tiles/merge`. Standalone layer: no ECS, no tilemap type.
- Acceptance:
  - [ ] Every case in the "Tile merge" test list passes, including the seeded property loop (disjoint, exact coverage, one kind per rectangle, deterministic) and the overflow case (`width * height` is computed in `u64`; 65,536 by 65,536 with an empty slice is an error, not a pass).
  - [ ] No allocation after warm-up (the bitset and `out` capacity are reused); builds for `wasm32-unknown-unknown`.
  - [ ] The bench meets the budgets (64 by 64: all solid <= 20 us, random 50% <= 100 us, checkerboard <= 150 us; 256 by 256 random <= 2 ms).
- Blocked by: intake-3.

**P20. `feat(physics): debug draw surfaces and the physics_debug_draw query`** (about 400 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/debug.rs` and `plugin/tools.rs`: the `Frozen`-mode behaviour of the `debug_draw` system (it runs in `PostUpdate`), the `PhysicsDebugView` handling (`bounds`, `line_cap`) it shares with the facade glue, and the `physics_debug_draw` query (`max_lines` default 256, at most 20,000, independent of `enabled`); the 10,000-collider bounded case of `benches/debug.rs`.
- Acceptance:
  - [ ] With `SimulationMode::Frozen` and `enabled`, `PhysicsDebugDraw` fills and the `physics_*` queries answer, with no tick run, for a dynamic body after one frame and for a **static collider after two** (the two-frame rule); with `enabled` false nothing is computed or allocated.
  - [ ] `physics_debug_draw` answers with `enabled` false, moves no change stamp, sets `truncated` at `max_lines`, and its input accepts the sample generated from its schema (CB9).
  - [ ] `ecs_set_resource_field` on `PhysicsDebugSettings.enabled` applies and undoes; `bounds` and `line_cap` are in the non-reflected `PhysicsDebugView` and are not reachable by that command.
  - [ ] Benches meet the budgets (bounded draw <= 0.5 ms at 10,000 colliders, query <= 0.6 ms).
- Blocked by: intake-3, P14, P16, CB9, and the `exitium_app` implementation of `SimulationMode` (AM3).

**Facade glue (not an `exitium_physics` edit):**

**G1. `feat(exitium): physics re-export and debug draw into the renderer`** (about 250 lines). **This is the same issue as `render-v1.md`'s G1c** ("G1c, physics debug"): filed once, not twice. The render lane routes its side (SP5) to this issue, and this spec's "Debug draw" contract is the acceptance.
- Footprint:
  - Edits: none
  - Governs: none
  - Hotspots: `crates/exitium/` (the facade)
  - New crate edges: none (`exitium -> exitium_physics` is in intake-2)
- Re-export the physics API from the facade (`exitium::physics`), add `PhysicsPlugin` to the default plugin set, and add the facade systems of the "Debug draw" glue contract: `PhysicsDebugView::bounds` as the union of every active pass's `visible_rect()` (every `CameraViews` entry and the `SceneView` pass, previous frame, grown by 10%), `PhysicsDebugView::line_cap = Some(MAX_DEBUG_LINES / passes)` (`None` with no active pass: no division by zero), and in `Extract` the copy of `PhysicsDebugDraw::lines` into `exitium_render`'s `DebugDraw::line` primitive (render-v1.md, issue I11).
- Acceptance:
  - [ ] A game depending only on `exitium` can spawn a `RigidBody` and run.
  - [ ] With `PhysicsDebugSettings::enabled`, lines reach the renderer and only shapes near a camera or the scene view are generated: **panning the `SceneView` away from the game camera still shows the shapes under the scene view**, with three passes `line_cap` is `Some(87,381)` and with no active pass `bounds` and `line_cap` are `None` (no panic); with it off, nothing is copied and `PhysicsDebugView` is not touched.
  - [ ] The facade API change gets the extra reviewer scrutiny of ADR-0005 (high-risk area).
- Blocked by: intake-2, P14, the render implementation of #107 (I11, the debug-line primitive), and the facade spec or issue that owns `crates/exitium/` at the time.

**Not filed by this spec: the tile-collider glue.** The facade issue that turns `CellMerger` rectangles into child `Collider` entities per tilemap chunk, and the `examples/` games that use the controller, belong to the tilemap spec (#147) and the facade glue lane. Their physics-side blocker is P19 (and P18 for the examples); the contract they implement is in "Tile colliders" and "Game types". The `exitium_physics` attribute issue of AM4 (`custom(readonly = true)` on `PhysicsStats`) is filed from `editor-mcp-mvp.md`, is not blocking, and is not part of this list.

## Open questions

Answered (kept so the numbers other specs cite stay stable):

1. **World units and the y axis.** Answered by `app-platform.md`: world space is y up, rotation counter-clockwise radians; units belong to the game, the camera and `PhysicsConfig::units_per_meter` (default 32, gravity default `-y`).
2. **Fixed-step alpha.** Answered by `app-platform.md`: `Time::fixed_alpha`, valid in `Update` and later (so in `PostUpdate`).
3. **Event channel.** Plugin-owned `PhysicsEvents` or the ECS event type (#22, #44)? Still open, decided in P13. `app-platform.md` notes that ECS events with the `Fixed` clock can serve per-tick delivery; if P13 finds they do, `PhysicsEvents` becomes a producer for them without an API change for readers.
4. **Cross-platform determinism.** Off for the MVP. When multiplayer needs the server and a client to agree beyond a tolerance (INTERVIEW R7), try rapier's `enhanced-determinism` feature (the changelog says it no longer costs measurable performance) behind a `deterministic` feature, and add a native-versus-wasm state-hash test. Also decide then whether to build wasm with `simd128`. Stage 6 networking (SP16, SP17) is where this is revisited.
5. **Resource edit commands.** **Answered (AM5): yes.** The bus has the generic, undoable `ecs_set_resource_field`; `SetPhysicsConfig` and `SetDebugDraw` are not built. See "MCP / editor surface".
6. **Character controller.** **Answered: built**, as `PhysicsWorld::move_character` (P17) and the `CharacterController` component (P18), Stage 4. See "Character controller".
7. **Debug lines in the renderer.** **Answered:** render-v1.md has the debug-line primitive (`DebugDraw`, issue I11), so G1 has something to feed, and the contract is in "Debug draw".
8. **Edit-mode world.** **Answered:** `reconcile` and `debug_draw` run in `PostUpdate`, which runs while `SimulationMode` is `Frozen`, so Edit mode has a world to draw and query. After `play_stop` the world is rebuilt by reconcile because it is derived state.
9. **Second glam version.** rapier on glam 0.33 next to the workspace's 0.34.3 costs a component copy at the boundary and a second crate in the build. Revisit when rapier moves to glam 0.34 (`convert.rs` is the only place to change).
10. **Joints, tilemap colliders, concave shapes.** Tilemap collider merging is built (P19, "Tile colliders"). Joints are a post-MVP non-goal (question 11). Concave decomposition stays a follow-up: nothing in the roadmap needs it.
11. **Joints. Answered (owner, 2026-11-25): not in the MVP; a post-MVP non-goal.** No issue in this spec builds them. The additive design is kept in "Joints" for whoever takes them up later: about 3 PRs (the `PhysicsWorld` joint API; the `Joint` component with reconcile, diagnostics and joint debug draw; `Entity`-reference remapping for scene save, prefab instantiate and duplicate in the scene crate, because an `Entity` field in a scene-saved component is not covered by undo remap alone). Game types that would need them: rope bridges, chains, hinged doors, swinging platforms and hazards, vehicles (wheels), ragdolls and physics puzzlers; the examples are built without them.
12. **One-way platforms: built, no longer a question.** They are part of the character controller (`one_way_layers` and `drop_through`, a predicate inside `move_character`, P17): a jump-through platform is basic to a sidescroller, needs no solver change and no `Collider` field. The alternative of switching a layer bit from a game system was rejected (it lags a tick through `Changed<Collider>` and reconcile). The one thing not built is one-way behavior for **dynamic** bodies, which would need a `Collider` field and a contact-pair hook in the solver (a hot-path change); additive, filed when a game needs a crate that passes through platforms.

New (for the owner): none. This spec has no open question for the owner.
