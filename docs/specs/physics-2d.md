# Spec: 2D physics on rapier2d

- Issue: #108
- Crates: `exitium_physics` (Stage 1 standalone wrapper, Stage 2 `plugin` module). Depends on (Stage 2): `exitium_ecs`, `exitium_app` (#106), `exitium_reflect`, `exitium_command` (#110). No edge to `exitium_render`, `exitium_mcp` or `exitium_editor`: runtime crates have no edges between them, and #110 builds the MCP tools and editor panels from reflection and the command registry. The facade (`crates/exitium/`, a hotspot) holds any cross-crate glue.
- Status: draft

## Goal

`exitium_physics` gives a Jam MVP game rigid-body physics: falling and colliding bodies, platforms, triggers and ray queries. It is a thin, safe layer over `rapier2d` 0.36.1 (INTERVIEW #20). It is built in two stages that match the Jam MVP crate graph:

- **Stage 1: a standalone wrapper** (`PhysicsWorld`). No ECS, no app. It depends only on `exitium_core` plus `rapier2d`. A hand-written loop can drive it with `exitium_core::time::FixedClock`, so the crate is built and tested in parallel with the ECS.
- **Stage 2: a `plugin` module**. It adds the `RigidBody` and `Collider` components, the fixed-step system, `Transform2d` sync, interpolation for rendering, event delivery, queries, debug draw data, and the typed commands that the MCP tools and the editor use.

The spec decides these things so that the implementation issues need no further design: **physics runs in SI-scaled meters internally and the game sees world units** (one conversion at the boundary); **one rapier step per `FixedUpdate` tick, with render interpolation on by default**; **no rapier type appears in the public API**; **`Transform2d` is the visual transform, the physics world owns simulation state**; **repeatable on one build and platform, not cross-platform deterministic**.

## Non-goals

- **Joints, motors, soft bodies, multibody.** rapier 0.36 has them. They are additive later: a joint is one more component and one more `PhysicsWorld` method, and nothing here blocks them (design rule 5).
- **Concave shapes** (polylines, trimeshes, decomposition) and **tilemap colliders**. Tilemaps are cut from the MVP. Authors build levels from boxes and convex polygons.
- **Character controller helper.** A dynamic body with `lock_rotation`, `Velocity` and the shape queries is enough for a platformer. See open question 6.
- **Position-based kinematic bodies.** Kinematic bodies are velocity-based (a moving platform carries the bodies on it).
- **Cross-platform determinism, lockstep and rollback.** INTERVIEW #10: determinism is not required.
- **Multithreaded stepping and `exitium_tasks` integration** (cut from the MVP). rapier's `parallel` feature stays off. The path stays open, see "Threading".
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
  plugin/       (feature `plugin`, on by default; Stage 2)
    mod.rs        PhysicsPlugin
    components.rs BodyLink, ColliderLink (internal) and Component impls for the types above
    config.rs     PhysicsConfig, PhysicsDebugSettings, PhysicsStats (reflected resources)
    reconcile.rs  create, update and remove bodies and colliders
    step.rs       FixedUpdate sync-in, step, write-back; Update interpolation
    events.rs     PhysicsEvents resource
    queries.rs    entity-level query facade
    debug.rs      PhysicsDebugDraw resource
    commands.rs   typed commands (needs exitium_command)
benches/step.rs  benches/query.rs  benches/debug.rs  benches/sync.rs
```

Everything is safe code. rapier does its own `unsafe` internally; this crate adds none (no `#[allow(unsafe_code)]` module).

Features: `reflect` (derives `Reflect` on the settings types through `cfg_attr`, so Stage 1 builds without `exitium_reflect`) and `plugin` (implies `reflect`; adds the `exitium_ecs`, `exitium_app`, `exitium_reflect` and `exitium_command` dependencies). `default = ["plugin"]`; standalone use is `default-features = false`.

### Units

| Quantity | Stage 1 `PhysicsWorld` | Stage 2 components and resources |
|---|---|---|
| Length (positions, sizes, offsets, radii, query distances) | physics units (meters, by convention; the crate does not enforce it) | world units, divided by `units_per_meter` on the way in and multiplied on the way out |
| Velocity, acceleration, gravity | physics units per second | world units per second (`gravity` too) |
| Mass | kilograms, from `density` (kg per m2 of shape area) | same, `density` is kg per m2 |
| Impulse | kg times physics units per second | kg times world units per second |
| Angle | radians, counter-clockwise | same |
| Time | seconds, `f32` | seconds |

The default gravity is `(0.0, -9.81 * 32.0)` world units per second squared. The physics core is orientation-agnostic: gravity is a plain vector. The default assumes **y-up**; if #106 or #107 decide y-down the default flips, nothing else changes (open question 1). `Transform2d::scale` is **ignored** for physics. Shape sizes live only in `Collider`, and the plugin records an issue (see "Diagnostics") for a body whose scale is not 1.

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
}
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

`Time` (core) today has `delta`, `elapsed`, `tick_count` and `fixed_step`, and no `alpha`. The plugin needs the fraction of the next tick that has accumulated, in `[0, 1)`, at the point where interpolation runs. **Open dependency on #106**: `exitium_app` must expose it (for example `Time::fixed_alpha`). Without it the plugin cannot interpolate.

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

The plugin registers into the named schedules in contract terms. **The exact `App`, `Plugin`, system-set and ordering signatures are #106's; this spec lists what the plugin needs from them (open dependency on #106, and on #22 for the ordering and parameter types)** and does not invent a second API.

| Schedule | What the plugin registers |
|---|---|
| Startup | Insert the `PhysicsConfig`, `PhysicsDebugSettings`, `PhysicsStats`, `PhysicsEvents` and `PhysicsDebugDraw` resources and the `PhysicsWorld`. Register every public type with the reflection registry (components `RigidBody`, `Collider`, `Velocity`; the resources). |
| FixedUpdate, before the game's systems | `reconcile` (below). |
| FixedUpdate, after the game's systems | `sync_in`, `step`, `write_back`. |
| Update, late (after game systems, before transform propagation and extraction) | `reconcile`, `interpolate`, `clear_events`, `debug_draw`. |
| Render extraction | Nothing from this crate. The renderer reads the `Transform2d`/`GlobalTransform2d` values that `interpolate` already wrote. Feeding `PhysicsDebugDraw` into the renderer is facade glue (see "Debug draw"). |

What the plugin needs from #106: (a) a way to order a system before and after the game's default systems in `FixedUpdate`, and late in `Update` before transform propagation; (b) the fixed-step alpha; (c) `Transform2d` (position `Vec2`, rotation as an angle in radians, scale) and `GlobalTransform2d`, with `Parent`/`Children` from the ECS (#49); (d) a system-parameter way to reach `&mut World`-level data: a resource, a query with `Changed<T>` and `Without<T>` filters, and `Commands`. Nothing else.

**Components** (all reflected; the first three are the user-facing set):

- `RigidBody`, `Collider`, `Velocity`: the types above, with `#[reflect(range(...))]` on numeric fields (`friction`, `restitution`, `density`, damping and so on) and doc comments (they become MCP `description`). `range` is a UI and schema hint only (reflect spec), so the plugin **validates at reconcile** and reports bad values as an issue (see "Diagnostics") instead of panicking.
- `BodyLink(BodyId)` and `ColliderLink(ColliderId)`: internal, **not reflected**, so a scene save or an editor snapshot skips them. After a restore the next reconcile rebuilds every body from the reflected components. **The physics world is derived state and is never saved.** Consequence: `Velocity` must stay current, so `write_back` keeps it current and reconcile inserts `Velocity::default()` with the `BodyLink`.

**Which entities become what:**

- An entity with `RigidBody` (and optionally `Collider` on the same entity) becomes a body. A `Collider` on the same entity attaches to it with the collider's `offset` and `angle`.
- An entity with `Collider` and **no** `RigidBody` on itself becomes a static collider if no ancestor has a `RigidBody`. If an ancestor has one, the collider attaches to the **nearest ancestor body** (a compound body of several shapes), at the pose relative to that body, computed from the two `GlobalTransform2d` values and then `offset`/`angle`.
- A `Dynamic` or `Kinematic` body must be a hierarchy root (no `Parent`), because physics writes its `Transform2d` as a world pose. A non-root dynamic body is skipped and reported as an issue. A `Static` body or a static collider may be anywhere in the tree (read from `GlobalTransform2d`).

**reconcile** (at the start of every `FixedUpdate` tick, and in `Update` so a paused editor still has a world to draw and query). It is idempotent and does work proportional to what changed:
1. Create: a query for `RigidBody`/`Collider` entities `Without<BodyLink>`/`Without<ColliderLink>` calls `add_body`/`add_collider`, and `Commands` inserts the link components (and `Velocity` for a body). Invalid settings go to the issue list and the entity is retried only after the component changes.
2. Update: entities with `Changed<RigidBody>` or `Changed<Collider>` call `update_body`/`update_collider` (an inspector or MCP `ecs_set_field` edit takes effect this way). **Moved static things are re-placed here.** A `Static` body never has its `Transform2d` written by the plugin, so `Changed<GlobalTransform2d>` on it is a reliable "someone moved it" signal (an editor drag, an `ecs_set_field` on `Transform2d`, a game system). The same holds for a parentless static collider and for a child collider of any body. Reconcile calls `set_pose` for a static body and re-places the collider (`update_collider` with the new local pose) for the others. A moved static body is a teleport: it gives nothing a velocity, so bodies resting on it are not carried (use a `Kinematic` body for a moving platform), and `set_pose` wakes the bodies touching it. A change to `PhysicsConfig::units_per_meter` rebuilds the whole world (every body re-created from its components).
3. Remove: ECS v1 has no removal hooks or `RemovedComponents` (ECS spec non-goal), so reconcile **sweeps**: it walks its own body and collider tables (O(bodies), a few ns each) and removes any whose entity no longer has the component or the entity is dead (generation mismatch). This also handles despawn.

**sync_in** (FixedUpdate, after game systems, before `step`), for each dynamic or kinematic body entity (static bodies are handled by reconcile, above):
- `Transform2d` differs from the pose the plugin last wrote (`written_pose`, stored with the link; compared by value, so it does not depend on change-detection internals): the game or the editor moved it, so it is a **teleport**: `set_pose`. The comparison is O(bodies) and a few ns each.
- `Velocity` differs from `written_velocity`: `set_velocity`.
- Queued runtime commands (`ApplyImpulse`) are applied and the queue is cleared.

**write_back** (after `step`): for each **active** body only (`active_bodies()`, O(awake)), write the exact simulation pose into `Transform2d` if the body has `interpolate = false`, write `Velocity`, and record `written_pose` and `written_velocity`. Sleeping bodies and static bodies are not touched, which also keeps their change ticks quiet. A body with `interpolate = true` does not get its `Transform2d` written here; it gets it in `interpolate`.

**interpolate** (Update, late): for each active interpolating body, compute `interpolated_pose(id, alpha)`, convert to world units, write `Transform2d` (only if different) and record `written_pose`. Before it overwrites a body, it runs the same teleport check as `sync_in` (so an Update system or an editor drag that moved the body is applied, not overwritten). After it runs, `Transform2d` holds what is drawn; the exact simulation pose is in the physics world and in `BodyState` for any gameplay code that needs it (`PhysicsWorld::body_state`).

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

The step system appends each tick's events, converting each side with `Entity::from_bits(user_data)`. That returns `None` when the generation half is 0, and the ECS only hands out entities with a non-zero generation, so the plugin **never creates a body or collider with `user_data == 0`** (`Entity::to_bits()` is never 0). A `None` result therefore means the collider was not created by the plugin (a Stage 1 user of the same world, or a bug), and the event is **dropped and counted**, exactly like an event whose entity is dead (`PhysicsStats::events_dropped`, plus an issue). Stage 1 treats `user_data` as opaque and defaults it to 0. `clear_events` runs at the **end** of `Update`. So an `Update` system sees every event of the frame exactly once, even when two ticks ran, and a frame with zero ticks has no events. A `FixedUpdate` system uses `this_tick()`. The ECS has a double-buffered event type (#44, designed by #22); this resource is plugin-owned for the MVP because two-per-frame fixed ticks do not fit a once-per-frame buffer swap. If #22's event type handles fixed ticks, `PhysicsEvents` becomes a producer for it without an API change for the readers (open question 3).

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

Stage 1 fills `DebugDraw` in physics units through rapier's `DebugRenderPipeline` with a backend that pushes lines and converts rapier's HSLA color to `Color`. Stage 2 exposes `PhysicsDebugDraw { enabled: bool, lines: Vec<DebugLine> }` **in world units**, filled in `Update` only when `PhysicsDebugSettings::enabled` is true (default false, so a shipped game pays nothing). `PhysicsDebugSettings` is a reflected resource holding `enabled` and the `DebugDrawOptions` fields. Physics does not depend on the renderer, and the renderer does not depend on physics (runtime crates have no edges between them). **The glue lives in the facade** (`crates/exitium/`, a hotspot): a facade system copies `PhysicsDebugDraw::lines` into whatever debug-line primitive the renderer offers (#107). That glue is issue G1 below; until it lands, the lines are readable data only (see open question 7). Circles are 16-segment polylines and boxes are 4 lines, so 1,000 colliders are about 10,000 to 20,000 lines (about 640 KB); `bounds` cuts this to what is visible.

### Diagnostics

Authoring mistakes must be visible to an agent without reading logs. `PhysicsStats` (a reflected resource) holds `issues: Vec<PhysicsIssue { entity: Entity, message: String }>` (capped at 64, newest last, a counter for the overflow). Reconcile appends one for: an invalid `RigidBody` or `Collider` value, a non-root dynamic body, a body whose `Transform2d::scale` is not 1, a non-finite `Transform2d`, an event whose entity died, a dropped event over the cap. An entity's issue is cleared when its component changes and reconcile accepts it. `physics_get_stats` returns them.

### Threading

- `PhysicsWorld` is `Send` and not shared: one thread owns it and calls `step(&mut self)`. In Stage 2 that is whichever thread runs `FixedUpdate` (the single-threaded executor in the MVP). Queries take `&self`, so a later parallel executor can run read-only queries from several systems at once; they never take a lock.
- rapier's `parallel` feature (rayon) is off. On wasm there are no threads regardless. The later path (design rule 5): build rapier with `parallel` and hand it a pool through its thread-pool API (added in 0.35), sized by `exitium_tasks`' `ThreadBudget`. It is an internal change to `world.rs` and one intake PR.
- The event collector needs `Sync` (rapier 0.35 changed the requirement from `Send + Sync`). A `std::sync::Mutex<Vec<_>>`, uncontended in the single-threaded step, is enough and works on wasm.
- No global state, no `static mut`, no thread-locals.

## MCP / editor surface

Agent parity (rule 1), reflection as the source of truth (rule 2), mutation through the command bus (rule 3). **#110 builds the MCP tools and the editor panels generically**, from reflection and from the command and query registry. Physics therefore adds **no tool and no panel code, and no edge to `exitium_mcp` or `exitium_editor`** (the MCP server is a separate process that cannot link game types). Physics' job is to expose the following, in the game process, with reflection-derived schemas.

**Reflected components** (inspect and edit with the generic ECS tools and the inspector; `Changed<T>` in reconcile applies an edit): `RigidBody`, `Collider` (with `Shape`, `Material`, `CollisionLayers`, `BodyKind`, `CombineRule`) and `Velocity`. Ranges, defaults, descriptions and the `Shape` enum come from the `#[derive(Reflect)]` attributes, never from a parallel schema. Undo is the command bus's undo for the generic component commands.

**Reflected resources** (readable and editable through the generic resource access of #110): `PhysicsConfig` (gravity, `units_per_meter`, `solver_iterations`, `max_events`), `PhysicsDebugSettings` (`enabled` plus the `DebugDrawOptions` fields), and `PhysicsStats` (read-only: counts by body kind, awake and sleeping, contact pairs, last step in ms, `tick`, `events_dropped`, `issues`). A debug draw toggle in the editor is an edit of `PhysicsDebugSettings::enabled`.

**Commands** (typed, serializable, registered with the command registry from the plugin's `commands.rs`; if #110's generic resource-field command covers resources, the first two are thin wrappers over it, open question 5):

| Command | Effect | Undo |
|---|---|---|
| `SetPhysicsConfig { patch }` | change `PhysicsConfig` fields | previous values |
| `SetDebugDraw { enabled, options }` | change `PhysicsDebugSettings` | previous values |
| `ApplyImpulse { entity, impulse, point? }` | queue an impulse, applied in the next `sync_in` | **none**: a runtime action, not an authoring edit. An impulse is a one-way change of simulation state, and replaying it on a restored scene would not reproduce the world. #110 must let a command declare "no undo"; until it does, this command is not registered. |

**Queries** (read-only, registered as named queries with the registry's read-only kind, results as JSON built from reflected result types `RayHit`, `ShapeHit`, `ContactInfo`, `BodyState`): `raycast`, `overlap_point`, `overlap_shape`, `shape_cast` (from `PhysicsQueries`), and `body_state { entity }` (pose, velocity, sleeping, mass, contacts with entity and normal). Entities travel as `"{index}v{generation}"` strings, vectors as `[x, y]`, lengths in world units. Whether these are registered as commands with no mutation or through a separate read registry is #110's decision (open question 5); physics only provides the functions and the reflected result types.

Notes:
- Reads run at a sync point between schedule runs against the world as of the last reconcile. A collider created by a spawn command is not queryable until the next `Update`, so every query result carries the world `tick`.
- The parity test (#110) checks every component, resource, command and query above is reachable through the generic surface.
- The editor's viewport toggle, stats tab and query probe are generic panels of #110, not physics code. Collider gizmos are a non-goal.

## Performance

Min-spec is 4 cores and 8 GB. Everything here is single-threaded and native release. Step budgets are the **mean time per step over the busiest 2 seconds** of the scene (120 steps, the settling phase of a drop into a bin), taken as criterion's median over runs. A spike guard sits beside each one: no single step in that run takes more than twice the mean budget (asserted by the bench itself, not by the 5% gate). Query budgets are mean time per call.

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
| `exitium_physics/debug/draw` | 1,000 colliders | |
| `exitium_physics/sync/tick` and `.../sync/interpolate` | 1,000 bodies (200 awake) | Stage 2 |

**Sizes that differ from the convention (1k, 100k, 1M), and why.** For dynamic bodies, 100k and 1M are meaningless: a single-threaded rapier step at that count takes seconds, the min-spec target is about 2,000 bodies, and the 1M-entity benches are cut from the MVP (contract). For queries, 1M colliders would only measure memory. Each bench file's `//!` doc states this, as the convention requires for a dropped size. Each bench ships with the change that adds the hot path (design rule 4).

## Test plan

Stage 1 (`cargo test -p exitium_physics --no-default-features`):
- **Validation:** every `Shape` and settings field rejects NaN, infinity and out-of-range values with a `PhysicsError` and never panics (a table of cases plus a seeded loop of random garbage; no new dependency). A convex polygon with 2, 65, collinear or duplicate points is rejected; a slightly non-convex one is hulled.
- **Lifecycle:** add, update and remove a body and a collider; a stale `BodyId` after `remove_body` (and after the slot is reused) is rejected; removing a body removes its colliders.
- **Physics behavior:** a ball falls `0.5 * g * t^2` within a tolerance; a box comes to rest on a static floor and goes to sleep; restitution 1 versus 0 gives a bounce versus none; friction 0 slides and friction 1 sticks on a slope; a kinematic platform carries a body; `lock_rotation` and `gravity_scale` take effect; `ccd` stops a fast body at a thin wall; layers and filters (the `memberships`/`filter` rule, both directions).
- **Events:** a sensor reports `Started` then `Stopped` exactly once; a kinematic body triggers a static sensor; `report_events = false` is silent; the overflow counter works; an event survives its body's removal.
- **Queries:** ray hit point, normal and distance; `cast_ray_all` is sorted; filters exclude a body and sensors; point, overlap and shape-cast cases, including a ray that starts inside a collider.
- **Interpolation:** `alpha` 0, 0.5 and 1; shortest-arc angle across the pi wrap; `set_pose` snaps (no blend across a teleport); sleeping bodies stay put.
- **Repeatability:** two worlds, the same scripted inputs, 600 steps: bit-identical poses and the same event sequence. A world rebuilt from the same descriptions steps identically.
- **Debug draw:** a box produces 4 `Collider` lines at the right corners; `bounds` culls; `max_lines` sets `truncated`.
- **wasm:** `cargo build -p exitium_physics --target wasm32-unknown-unknown` and `cargo xtask ci all`. `crates/exitium_physics` is not `NATIVE_ONLY`.

Stage 2 (a headless `App`, no window):
- Spawning an entity with `RigidBody` and `Collider` creates a body; despawning it removes it (the sweep); removing only the `Collider` component removes the collider.
- A `SetField` on `Collider.material.friction` (through reflection) changes the simulation. A `Changed` edit with an invalid value yields an issue and leaves the old setting.
- Teleport: writing `Transform2d` in a `FixedUpdate` system and in an `Update` system moves the body and does not blend. The plugin's own writes are never mistaken for teleports (no body moves a second time on its own).
- A frame with 0 ticks and a frame with 2 ticks: `interpolate` draws between `prev` and `pose` and the final pose matches the Stage 1 run; `interpolate = false` shows the exact pose.
- Events: delivered once per frame to an `Update` system and once per tick to a `FixedUpdate` system, with zero ticks giving none; an event whose entity is dead, or whose `user_data` is 0 or does not decode (`Entity::from_bits` returns `None`), is dropped and counted.
- Moving a `Static` body or a static collider (`Transform2d` write from a system, as an editor drag would) re-places it in rapier; a body resting on it wakes; the plugin creates no body or collider with `user_data == 0`.
- A compound body (child colliders) has the summed mass. A non-root dynamic body is skipped with an issue.
- Scene round trip: save the reflected components, restore, run: the world is rebuilt and the bodies resume with their `Velocity`.
- Commands: `SetPhysicsConfig` and `SetDebugDraw` apply and undo. `units_per_meter` change rebuilds the world with the same visible result.
- Parity: every component, resource, command and query listed in "MCP / editor surface" is reachable through the generic registry (checked with #110's parity test).

## Dependencies

Only one crates.io dependency is new, plus its transitive set. Declared once in the root `[workspace.dependencies]`; the Stage 1 intake PR adds it with the `Cargo.lock` change, so no implementation issue touches `Cargo.lock`.

| Crate | Version | License | wasm | Why not std |
|---|---|---|---|---|
| `rapier2d` | `0.36.1`, `default-features = false`, `features = ["dim2", "f32", "std", "block-solver", "debug-render"]` | Apache-2.0 | yes (built for `wasm32-unknown-unknown` for this spec) | A 2D rigid-body solver, broad and narrow phase, CCD and query structure is a large body of numerical code that INTERVIEW #20 already chose to reuse. |

Transitive set in that build for `wasm32-unknown-unknown` (from `cargo metadata`): `parry2d 0.31.1`, `nalgebra 0.35.0`, `glamx 0.3.1` and `glam 0.33.12` (a second glam, next to the workspace's 0.34.3), `simba 0.10.2`, `wide 1.7.1`, `spade 2.15.1`, `approx`, `arrayvec`, `bitflags`, `foldhash`, `ordered-float`, `num-traits`, `num-complex`, `num-rational`, `num-integer`, `matrixmultiply`, `typenum`, `thiserror 2`, `log`, `profiling`, `static_assertions`, `slab`, `smallvec`, `either`, `ena`, `downcast-rs`, `hashbrown` (0.16 and 0.17), `libm`, `bytemuck`, `allocator-api2`, `equivalent`, `rawpointer`, `robust`, plus proc-macro crates `syn` (2 and 3), `quote`, `proc-macro2`, `unicode-ident`, `nalgebra-macros`, `num-derive`, `profiling-procmacros`, `thiserror-impl`. All licenses are MIT, Apache-2.0, Zlib or combinations of them, which `deny.toml` allows; `unicode-ident` is `(MIT OR Apache-2.0) AND Unicode-3.0`, also allowed. Two glam, two hashbrown and two syn versions trigger `multiple-versions = "warn"` only. The intake PR re-runs `cargo deny check` and `cargo tree` to confirm this list.

No other dependency: no `serde` (serialization is reflection), no `rand`, no `bitflags` of our own, no channel crate (a `Mutex<Vec>` collector). `criterion` is the existing workspace dev-dependency.

### Intake wiring (for the Stage 1 and Stage 2 intake PRs, not issues in this spec)

The intake PRs add the union of every merged spec's list. This is physics' list: every dependency, crate edge, dev-dependency and feature its issues need, so no implementation issue touches `Cargo.lock` or a `Cargo.toml` dependency table.

**Intake 1 (Stage 1):**
- Root `Cargo.toml` `[workspace.dependencies]`: `rapier2d` as in the table above.
- `crates/exitium_physics/Cargo.toml`: `[dependencies] exitium_core.workspace = true, rapier2d.workspace = true`; `[dev-dependencies] criterion.workspace = true`; no features yet.
- New crate edge: `exitium_physics -> exitium_core`. No other edge.
- Acceptance: `cargo build -p exitium_physics --target wasm32-unknown-unknown`, `cargo deny check`, and the transitive list above re-confirmed with `cargo tree`.
- The `[[bench]]` stanzas are added by the issues that create the bench files. They are inside the crate and do not change `Cargo.lock`.

**Intake 2 (Stage 2):**
- `crates/exitium_physics/Cargo.toml`: optional dependencies `exitium_ecs`, `exitium_app`, `exitium_reflect` (and `exitium_reflect_derive` if `exitium_reflect` does not re-export the derive) and `exitium_command`; features `reflect = ["dep:exitium_reflect"]`, `plugin = ["reflect", "dep:exitium_ecs", "dep:exitium_app", "dep:exitium_command"]`, `default = ["plugin"]`. No new dev-dependency (the headless-app tests use `exitium_app`).
- New crate edges: `exitium_physics -> exitium_ecs`, `-> exitium_app`, `-> exitium_reflect`, `-> exitium_command`.
- Facade: `exitium -> exitium_physics` (the facade depends on every crate; its re-export and any glue are in G1).
- **No edge** to `exitium_render`, `exitium_mcp`, `exitium_editor`, `exitium_platform`, `exitium_audio`, `exitium_assets`, `exitium_ui` or `exitium_scene`. Runtime crates have no edges between them.

## Implementation issues

After this spec merges the architect files these into the Jam MVP milestone with the `file-issue` skill. `P1` to `P16` and `G1` are placeholders, replaced by issue numbers when filed; `intake-1` and `intake-2` are the intake PRs above. Each issue edits one crate (or, for G1, only the facade hotspot) and has the line estimate of its change (code, tests and docs) next to it; every estimate is at or under about 600. `exitium_physics` issues share a crate, so the factory runs one at a time even where the dependencies below would allow two; the list is in dependency order. Every Stage 1 issue is `Blocked by intake-1`. Every Stage 2 issue is `Blocked by intake-2` and by the merged and implemented #106 `App`/`Plugin` API (and the ECS, reflect and command crates where named).

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
- `debug.rs`: `DebugDraw`, `DebugLine`, `DebugKind`, `DebugDrawOptions`, the `DebugRenderPipeline` backend (color conversion, `bounds`, `max_lines`); `benches/debug.rs`.
- Acceptance:
  - [ ] A box gives 4 `Collider` lines at the right corners; `bounds` culls; `max_lines` sets `truncated`.
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

**P10. `feat(physics): reconcile, updates, static re-place, hierarchy and rebuild`** (about 500 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/reconcile.rs` (update half): `Changed<RigidBody>`/`Changed<Collider>` updates, **re-placing moved static bodies, parentless static colliders and child colliders from `Changed<GlobalTransform2d>`**, compound children of the nearest ancestor body, the `units_per_meter` rebuild.
- Acceptance:
  - [ ] A reflection `SetField` on `Collider.material.friction` changes the simulation; an invalid edit keeps the old setting and adds an issue.
  - [ ] Moving a static body's `Transform2d` (editor or game system) moves its rapier body; a body touching it wakes; bodies on it are not carried.
  - [ ] A compound body has the summed mass; a child collider moved in the hierarchy is re-placed.
  - [ ] Changing `units_per_meter` rebuilds the world with the same visible result.
- Blocked by: intake-2, P9.

**P11. `feat(physics): fixed-step system, sync_in and write_back`** (about 450 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/step.rs`: the `FixedUpdate` ordering requirements, `sync_in` (teleport detection by value, `Velocity`, queued impulses), `step`, `write_back` (active bodies only; exact pose for non-interpolated bodies).
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
- `plugin/step.rs` (interpolate): late `Update` system, teleport check before overwrite, the fixed-step alpha from #106; `benches/sync.rs`.
- Acceptance:
  - [ ] A frame with 0 and a frame with 2 ticks draws between `prev` and `pose`; `interpolate = false` shows the exact pose.
  - [ ] An `Update` write or an editor drag of a body is applied, not overwritten.
  - [ ] Benches meet the budgets.
- Blocked by: intake-2, P11, #106's fixed-step alpha.

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
- `plugin/debug.rs`, the `PhysicsStats` update: `PhysicsDebugDraw` filled in `Update` only when enabled, in world units; counts, last step time and issues.
- Acceptance:
  - [ ] Nothing is computed when `enabled` is false.
  - [ ] Lines are in world units; `bounds` is applied.
  - [ ] `PhysicsStats` reports the counts and issues.
- Blocked by: intake-2, P7, P11.

**P15. `feat(physics): typed commands`** (about 400 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `plugin/commands.rs`: `SetPhysicsConfig`, `SetDebugDraw`, `ApplyImpulse`, registered with the command registry from #110.
- Acceptance:
  - [ ] The first two apply and undo; `units_per_meter` through the command rebuilds the world.
  - [ ] `ApplyImpulse` is queued and applied in the next `sync_in`; it declares no undo and is registered only when the registry supports that.
  - [ ] Each command round-trips through its reflected schema.
- Blocked by: intake-2, P14, #110's command bus implementation.

**P16. `feat(physics): register queries and stats with the #110 registry`** (about 300 lines)
- Footprint:
  - Edits: `crates/exitium_physics/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- The registrations of `raycast`, `overlap_point`, `overlap_shape`, `shape_cast` and `body_state` as read-only queries, and of the reflected resources and result types, using whatever kind #110 defines (open question 5).
- Acceptance:
  - [ ] Every item in "MCP / editor surface" is registered, with a schema derived from reflection.
  - [ ] #110's parity test passes for physics.
  - [ ] Results carry the world `tick`.
- Blocked by: intake-2, P13, P15, #110's registry implementation.

**Facade glue (not an `exitium_physics` edit):**

**G1. `feat(exitium): physics re-export and debug draw into the renderer`** (about 200 lines)
- Footprint:
  - Edits: none
  - Governs: none
  - Hotspots: `crates/exitium/` (the facade)
  - New crate edges: none (`exitium -> exitium_physics` is in intake-2)
- Re-export the physics API from the facade (`exitium::physics`), add `PhysicsPlugin` to the default plugin set, and add the facade system that copies `PhysicsDebugDraw::lines` into the renderer's debug-line primitive (#107).
- Acceptance:
  - [ ] A game depending only on `exitium` can spawn a `RigidBody` and run.
  - [ ] With `PhysicsDebugSettings::enabled`, lines reach the renderer; with it off, nothing is copied.
  - [ ] The facade API change gets the extra reviewer scrutiny of ADR-0005 (high-risk area).
- Blocked by: intake-2, P14, the render implementation of #107, and the facade spec or issue that owns `crates/exitium/` at the time.

## Open questions

1. **World units and the y axis.** Is one `Transform2d` unit one pixel at zoom 1, and is y up or down? This spec assumes pixels and y-up (so gravity defaults to `-y`), with `units_per_meter = 32`. #106 and #107 decide; a y-down world flips the default gravity sign, nothing else.
2. **Fixed-step alpha.** `exitium_app` must expose the accumulated fraction of a tick (#106). Without it the plugin cannot interpolate.
3. **Event channel.** Plugin-owned `PhysicsEvents` or the ECS event type (#22, #44)? Decided by whether #22's type can serve fixed ticks without a per-frame swap.
4. **Cross-platform determinism.** Off for the MVP. When multiplayer needs the server and a client to agree beyond a tolerance (INTERVIEW R7), try rapier's `enhanced-determinism` feature (the changelog says it no longer costs measurable performance) behind a `deterministic` feature, and add a native-versus-wasm state-hash test. Also decide then whether to build wasm with `simd128`.
5. **Resource edit commands.** Does #110's command bus have a generic "set a resource field" command? If yes, `SetPhysicsConfig` and `SetDebugDraw` become wrappers of it.
6. **Character controller.** rapier ships a `KinematicCharacterController` (slopes, steps, snapping). A platformer feels better with it than with a dynamic body. Add it as a Stage 3 helper if the jam game needs it, as a `CharacterController` component. It does not change anything above.
7. **Debug lines in the renderer.** If #107 has no debug-line primitive in the MVP, G1 has nothing to feed, and `PhysicsDebugDraw::lines` stays readable data (for example for the facade or the editor to draw later). Whoever owns the facade glue decides; physics does not change.
8. **Edit-mode world.** Reconcile also runs in `Update`, so a paused editor has bodies for debug draw, picking and queries. If #110's play-and-stop snapshot restore makes this awkward (the world must be rebuilt on stop), the rebuild is already free because the world is derived state.
9. **Second glam version.** rapier on glam 0.33 next to the workspace's 0.34.3 costs a component copy at the boundary and a second crate in the build. Revisit when rapier moves to glam 0.34 (`convert.rs` is the only place to change).
10. **Joints and tilemap colliders.** First follow-ups after the MVP: joints (a `Joint` component and `PhysicsWorld::add_joint`), static collider merging for tilemaps, and concave shape decomposition.
