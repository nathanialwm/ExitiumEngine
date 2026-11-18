# Spec: Command bus, editor shell and MCP server (Jam MVP)

- Issue: #110
- Crates: `exitium_command` (new), `exitium_editor`, `exitium_mcp` (touchpoints: the `exitium` facade, and every merged Jam MVP spec through its "MCP / editor surface")
- Status: draft. Owner-approved: the contract in `docs/specs/jam-mvp.md` (an agent builds a game through MCP, a human refines it in the editor, the editor is a feature of the game binary built on egui, `exitium_mcp` is a separate stdio process that talks to the running editor over localhost). Everything else here is the author's proposal. The cross-spec rules C1 to C6 and the Stage 3 rulings are orchestrator rulings, not owner decisions.

## Goal

Specify the three crates that make the "agent parity" rule (CLAUDE.md, design rules 1 to 3) real for the Jam MVP:

- **`exitium_command`**, a runtime crate (it builds for wasm32), holds the command bus: typed commands and read-only queries that are reflected structs, a registry, undo and redo, merging, and the wire types of the live-control protocol. It is usable without the editor, so physics and UI register their own commands in it.
- **`exitium_editor`** is a feature of the game binary (`--features editor`, through the facade). It hosts the live-control server, the host tools (scene, assets, play control, screenshots, logs) and an egui shell on wgpu. The server also runs headless, without a window.
- **`exitium_mcp`** is a separate native process that speaks MCP over stdio to the agent and the live-control protocol over localhost to the running game. It knows no game type: its tool list is the list the running game reports.

The one rule that holds the design together: **every editor action is a registry entry (a command or a query) that the egui shell and the MCP client both call by name.** The shell never mutates the world on its own, and the MCP crate never lists a tool the game did not report. So an editor feature cannot ship without its tool, and a new command in any crate appears as a tool without an edit to `exitium_mcp`. This is also what lets the Stage 3 lanes stay independent (jam-mvp.md, "Why it passes").

This spec also fixes, for each merged spec, how its "exposure" list is built generically (see "MCP / editor surface"), and names the gaps it found in those specs and in the roadmap.

## Non-goals

- Luau, a script front end for the bus, or replay files. Commands are typed and serializable, so these can be added later (design rule 5). `CommandBus::enqueue` is the seam a script would use.
- Docking, detachable windows, saved layouts. The MVP shell has a fixed layout (open question 5).
- A game view inside the editor (the main camera rendered into a panel). The viewport shows the scene view (open question 4).
- Data Asset editor, UI WYSIWYG editor, profiler panel, offline MCP tools (docs and API query, schema dump without a running game). All are epic #6 and cut from the MVP by the roadmap ("Scope cuts"). `project_run` with `headless` gives an agent the schema without a window.
- Editing systems or schedules (ecs-scheduler.md non-goal): gameplay is Rust.
- Remote control. The server binds loopback only and has no option to do otherwise. The editor, MCP and CLI are native Windows only (the contract).
- Persisting the undo history across runs, and merging history across processes.
- An MCP transport other than stdio (streamable HTTP, SSE), MCP prompts, resources and sampling. Tools only.

## Design

### Decisions at a glance

| Id | Decision | Why |
|---|---|---|
| D1 | A command or query is a **reflected struct** (`Reflect + FromReflect + Clone`). Its fields are the tool's input, `#[reflect(skip)]` fields hold the undo state `apply` captures, and the doc comments become the tool description. | Reflection is the schema source (rule 2). The same struct is built from an MCP call, from an egui widget and from a test. |
| D2 | Values travel as **`RawJson`** (a validated string newtype holding JSON text) inside the generic commands (`SetField`, `InsertComponent`, ...). The command binds it to the target's `TypeInfo` when it applies. | The target type is known only once the entity and the path are resolved, so a value cannot be bound while the call is parsed. JSON text is also what the history shows. The egui shell writes a reflected value to JSON with the same writer. |
| D3 | The bus **applies only outside schedules**, at the frame's boundary hook (app-platform.md). Game code that wants an authoring command calls `CommandBus::enqueue`, which applies at the next boundary. | ecs-scheduler.md ("Boundary with the Phase 4 command bus"). One fixed point per frame (jam-mvp.md, Threading). |
| D4 | **One registry, two kinds:** `Command` (mutates, normally undoable) and `Query` (reads, never mutates), both returning a `Step` that may say "not done yet" (`Wait`). `play_advance` and the screenshot use `Wait`; nothing else does. | The web and the single-threaded loop cannot block (render-v1.md "Readback"). Deferred completion is the only model that works. |
| D5 | The **tool list is the registry.** `schema.get` returns every tool with an input schema derived from `TypeInfo`. `exitium_mcp` forwards `tools/call` by name. Plugin crates register their own commands (`exitium_command` is a runtime crate, so physics and UI depend on it). | C3 and C4. No crate hand-writes a tool, and the MCP crate never changes when a command is added. |
| D6 | **Protocol:** JSON-RPC 2.0 messages, one per line (NDJSON), over a TCP socket on `127.0.0.1`, a random 128-bit token in the first message, protocol version 1. | std only on the game side (no async runtime in the game), debuggable with any line tool, and a browser cannot speak it. |
| D7 | **MCP SDK: `rmcp` 3.11.0**, server side only, with `ServerHandler::list_tools` and `call_tool` written by hand (the `#[tool]` macros are for static tool sets). It lives in `exitium_mcp` only, and brings `tokio` there. | The MCP spec moves (protocol versions, structured content, progress). Alternative rejected: a hand-written JSON-RPC server (about 400 lines, no `tokio`). Fallback if the MC1 spike fails: that alternative. |
| D8 | **egui 0.37.0, egui-wgpu 0.37.0, egui-winit 0.37.0**, with default features off (no `default_fonts`: its licences fail `deny.toml`). The editor draws through a **surface overlay hook** in `exitium_render` (gap X1) on the game's own device. | Versions agree with wgpu 30.0.2 and winit 0.30.13 (see "Dependencies"). One device, one surface (render-v1.md "Editor"). |
| D9 | **Edit, Play, Paused.** The game starts paused in Edit. Play snapshots the active scene (and opted-in resources) in memory, Stop restores it and clears the undo history. | The issue's play semantics. History entries name entities that Stop recreates with new ids, so they cannot survive it. |
| D10 | **Scene commands live in `exitium_editor`, not in `exitium_command`.** The editor may depend on scene, assets, render and platform (C3); the command crate may not (it would add edges among runtime crates, C1). | Keeps `exitium_command` at core, ecs, reflect and serial. |
| D11 | **The editor never mutates the world from GUI code.** GUI functions receive a read-only view and an `ActionSink`; the sink queues `(tool name, arguments)` pairs that the boundary hook submits through the same entry point the server uses. | Parity by construction (see "Parity test"). |
| D12 | The e2e and parity tests drive the **`exitium_mcp` binary as a subprocess**. The editor crate has no dependency or dev-dependency on `exitium_mcp`. The test finds the binary from `current_exe()` (see "Test plan"). | C3. |
| D13 | Plugin crates register through `register_command::<C>(world)` and `register_query::<Q>(world)`, plain functions on `&mut World`. `exitium_command` has no `Plugin` and no edge to `exitium_app`; the registry is created on first use. | The same shape as `exitium_scene` (assets-scene-export.md D7). |
| D14 | **The `editor` feature is the facade's** (C2). The editor crate has no `main`; its `src/main.rs` stub is removed. A game starts the editor with `cargo run --features editor`. | The contract. |

### Crate map and dependency graph

```
exitium_command/ (src/, a runtime crate, builds for wasm32)
  lib.rs
  error.rs        CommandError, ErrorCode
  command.rs      Command, Query, Step, UndoKind, Cx, QueryCx
  registry.rs     CommandRegistry, ToolInfo, register_command, register_query
  bus.rs          CommandBus, History, Journal, Origin, SubmitOptions, entity remap
  builtin/
    entity.rs     SetField, InsertComponent, RemoveComponent, SpawnEntity, DespawnEntity, Reparent, SpawnHooks, CommandConfig
    resource.rs   InsertResource, RemoveResource, SetResourceField, SendEvent
    queries.rs    ecs_get_stats ... ecs_get_component (entities), ecs_list_schedules ... ecs_list_events
    reflect.rs    reflect_list_types, reflect_get_type, reflect_dump_schema, reflect_get_schema_hash, serial_parse_text, serial_write_text
  json.rs         RawJson, reflected value to and from JSON (the serial-text-format.md mapping table)
  schema.rs       TypeInfo to JSON Schema 2020-12, ToolDef list, schema revision
  protocol.rs     wire types, framing, error codes, constants (shared with exitium_mcp)

exitium_editor/ (src/, native only)
  lib.rs
  server/         accept, handshake, reader and writer threads, queue (no World access)
  live.rs         LiveControlPlugin, boundary hook, dispatch, pending replies, discovery file, run_headless
  tools/          host tools: scene.rs, assets.rs, play.rs, render.rs, state.rs, log.rs
  console.rs      tracing layer and ring buffer
  gui/            egui shell: mod.rs (context, input, overlay), widgets.rs (reflection to widgets),
                  hierarchy.rs, inspector.rs, panels.rs (console, history, assets, systems), viewport.rs, actions.rs
  assets/fonts/   the embedded font and its licence
exitium_mcp/ (src/, native only)
  lib.rs  main.rs   stdio MCP server (rmcp), tool table
  live.rs           protocol client, discovery
  ops/              project_build, project_test, project_run, project_stop, project_status; project_new and export (Stage 4)
```

Dependency graph ("A -> B" means A depends on B), everything the three crates need, for the intake 2 PR (C5):

```
exitium_command  -> exitium_core, exitium_ecs, exitium_reflect, exitium_serial
exitium_editor   -> exitium_core, exitium_ecs, exitium_reflect, exitium_serial, exitium_app, exitium_command,
                    exitium_scene, exitium_assets, exitium_render, exitium_platform
exitium_mcp      -> exitium_command                      (and nothing else in the workspace)
exitium_physics  -> exitium_command                      (optional, feature `plugin`; already in physics-2d.md)
exitium_ui       -> exitium_command                      (feature `plugin`; already in ui-mvp.md)
exitium (facade) -> exitium_command (normal), exitium_editor (optional, feature `editor`)
```

- **No edge among the runtime crates** (C1): `exitium_command` has no edge to scene, assets, render, platform, physics, audio, ui or app; the editor reaches them (C3) and nothing reaches the editor except the facade's optional edge.
- **`exitium_mcp` depends on no editor, facade or plugin crate** (C3), and the editor does not depend on `exitium_mcp` (D12). `exitium_mcp -> exitium_command` is for the `protocol` module only (it uses no ecs or reflect type), so the MCP binary links the command crate but never touches a world.
- **The editor has no edge to `exitium_physics`, `exitium_ui` or `exitium_audio`.** Their components, resources, events and commands are reached through reflection and the registry (they register themselves). The audio panel is the generic resource inspector over `AudioSettings`.
- `exitium_command -> exitium_serial` exists because the JSON mapping calls serial's `leaf` helpers, so the spelling of a number can never differ between text and JSON (serial-text-format.md, "JSON mapping"). serial has no edge to ecs, so there is no cycle.
- **`NATIVE_ONLY` crates:** `exitium_editor` (egui-winit, a TCP server and a native window; already listed), `exitium_mcp` (a stdio process on `tokio`; **intake 2 adds it**) and `exitium_cli` (already listed). Not native only: `exitium_command` (physics and UI depend on it, and a shipped game links them), so it builds for wasm32 with no `cfg` and no thread, socket or file in it. The facade builds for wasm32 with `editor` off. (`xtask` and `exitium_reflect_derive` stay listed as they are.)

### The command model (`exitium_command`)

#### Traits

```rust
/// A typed, reflected edit. The fields are the tool input; `#[reflect(skip)]` fields hold undo state.
pub trait Command: Reflect + FromReflect + Clone + Send + 'static {
    const NAME: &'static str;                       // "ecs_set_field": the registry key and the MCP tool name, `<domain>_<verb>`
    type Output: Reflect + Send + 'static;          // reflected, so the tool has an output schema; `()` for none
    /// Validate first, mutate last: on `Err` the world is unchanged (the bus checks this in debug builds and in tests).
    fn apply(&mut self, cx: &mut Cx<'_>) -> Result<Step<Self::Output>, CommandError>;
    /// Reverse the last successful `apply`. Called only when everything applied after it has been undone.
    fn undo(&mut self, cx: &mut Cx<'_>) -> Result<(), CommandError>;
    /// Undoable unless it says otherwise. May depend on the instance and the world (`SetResourceField` on a view resource).
    fn undo_kind(&self, _world: &World) -> UndoKind { UndoKind::Undoable }
    /// Fold a later command of the same type and gesture into this one. Default: never.
    fn merge(&mut self, _next: &Self) -> bool { false }
    /// Rewrite entity ids held in undo state after a despawn undo or redo changed them. Default: walks the reflected fields.
    fn remap(&mut self, map: &EntityRemap) { remap_reflected(self, map) }
}
pub enum UndoKind { Undoable, NotUndoable }          // NotUndoable: recorded in the journal only (see "The bus")

/// A read. It must not change observable state: the bus compares `world.change_cursor()` before and after in debug builds and in tests.
pub trait Query: Reflect + FromReflect + Clone + Send + 'static {
    const NAME: &'static str;
    type Output: Reflect + Send + 'static;
    fn run(&self, cx: &mut QueryCx<'_>) -> Result<Step<Self::Output>, CommandError>;
}
/// `Wait` is polled at every later boundary until it is done or its deadline passes (default 30 s, `timeout`).
pub enum Step<T> { Done(T), Wait(Box<dyn Waiter<T>>) }
pub trait Waiter<T>: Send { fn poll(&mut self, world: &mut World) -> Option<Result<T, CommandError>>; }
```

- `Cx` gives `&mut World`, the `TypeRegistry` read guard (one per submit, never a second: reflect-model.md "Threading model"), the `Origin`, the `EntityRemap` and `SpawnHooks`. `QueryCx` gives `&mut World` too: a query may refresh a cache (a UI layout pass for `ui_get_tree { fresh }`), but only through `bypass_change_detection`, which the cursor check enforces.
- A `Command` that returns `Wait` must be `NotUndoable`: the bus answers `internal` when an undoable command returns `Wait`, and a test over every registered command asserts it never happens.
- `Output` and the input struct register their types in the `TypeRegistry`, so `reflect_get_type` and the schemas see them. Entities are written `"42v1"` (the text form), vectors as the reflect mapping says.
- `merge` and `remap` are the two non-obvious hooks. Everything else is `apply` and `undo`.

#### Registry

```rust
pub fn register_command<C: Command>(world: &mut World);    // creates the CommandRegistry resource on first use
pub fn register_query<Q: Query>(world: &mut World);
pub struct ToolInfo {
    pub name: &'static str, pub kind: ToolKind /* Command | Query */, pub doc: &'static str,
    pub input: TypeId, pub output: TypeId, pub undo: UndoPolicy /* Always | Never | PerInstance */,
    pub read_only: bool, pub destructive: bool,        // MCP annotations; destructive = despawn, remove, replace
}
```

Registration is plain and explicit (no link-time magic; it fails on wasm, reflect-model.md "Registry"). A duplicate name is an error that names both types. The registry has a `revision: u64` that every registration bumps. The server also watches the `TypeRegistry` type count, so a type registered late (a plugin added at run time) changes the schema revision and the MCP client refreshes (`notify.schema_changed`).

#### The bus

`CommandBus` is a resource. The host calls it outside schedules: `submit` applies now, `enqueue` queues for the next boundary (a system may call `enqueue`; it never applies from inside a run).

```rust
impl CommandBus {
    pub fn submit(&mut self, world: &mut World, cmd: Box<dyn DynCommand>, opts: SubmitOptions) -> Result<Outcome, CommandError>;
    pub fn submit_by_name(&mut self, world: &mut World, name: &str, args: &dyn Reflect, opts: SubmitOptions) -> Result<Outcome, CommandError>;
    pub fn enqueue(&mut self, cmd: Box<dyn DynCommand>, opts: SubmitOptions);
    pub fn undo(&mut self, world: &mut World, steps: u32) -> Result<UndoReport, CommandError>;
    pub fn redo(&mut self, world: &mut World, steps: u32) -> Result<UndoReport, CommandError>;
    pub fn batch(&mut self, world: &mut World, cmds: Vec<Box<dyn DynCommand>>, label: &str, atomic: bool) -> Result<Vec<Outcome>, CommandError>;
    pub fn journal(&self, since: u64, limit: usize) -> &[JournalEntry];
    pub fn clear_history(&mut self);   // play stop, scene replace
}
pub struct SubmitOptions { pub origin: Origin /* Editor | Mcp | Host */, pub gesture: Option<GestureId>, pub timeout: Option<Duration> }
pub struct Outcome { pub seq: u64, pub output: Box<dyn Reflect>, pub warnings: Vec<String>, pub tick: u64 }
```

- **Where it runs.** `submit` returns `CommandError::InSchedule` when `exitium_ecs::current_system()` is `Some` (ecs-scheduler.md "Panic context"): the bus never applies while a schedule runs. The server and the shell call it from boundary hooks.
- **History.** An undo stack and a redo stack of `Box<dyn DynCommand>` plus the captured undo state, capped at 1,000 entries and 16 MiB (an estimate from `size_of_val` and snapshot sizes); the oldest entry goes first. A new undoable command clears the redo stack.
- **Journal.** A ring of the last 1,000 entries of everything that ran. Each entry holds the sequence number, the origin, the tool name, a `clone_boxed` copy of the command, the result or error code, the undo class and the frame. `NotUndoable` commands (`SendEvent`, `ApplyImpulse`, `UiClick`, an audio event, a view edit) are journalled, are **not** on the undo stack and do **not** clear the redo stack: they are runtime actions, not authoring edits. This answers physics-2d.md and ui-mvp.md open question 7.
- **Gestures and merging.** The shell sets `SubmitOptions.gesture` while a mouse button is held (a slider or gizmo drag). Consecutive commands of one type with the same gesture id and `merge() == true` fold into the top history entry, so a drag is one undo step. There is no clock. An MCP call has no gesture and never merges.
- **Groups.** `batch` runs a list as one history entry (`command_batch` is its tool). With `atomic` it stops at the first error and undoes what ran, in reverse; if an undo fails too the error is `rollback_failed` and the report lists what is left. An atomic batch may contain only undoable commands.
- **Failure.** A command validates, then writes. `apply` returning `Err` leaves the world unchanged; a command that cannot promise it (it applies a non-atomic reflected `apply`, reflect-model.md "Apply semantics") takes a `clone_boxed` snapshot first and restores it. The error is a `CommandError { code: ErrorCode, message, path: Option<String>, expected, found, hint }`, never a panic. A panic in a command is a bug: native builds catch it at the bus, report `internal`, and clear the history (its undo state is not trustworthy).
- **Entity remap.** Undoing a despawn respawns the entities with **new ids** (the generation is gone, ecs-storage-queries.md). The bus keeps an `EntityRemap { old -> new }` and, after each such undo or redo, calls `remap` on every history entry, so a later `SetField` on the old id still finds the entity. `remap_reflected` walks reflected fields for `Entity` values; commands whose undo state is skipped from reflection override `remap` (the despawn and insert snapshots do, through `remap_value(&mut dyn Reflect, &EntityRemap)`).
- **Writes stamp the current tick** (ecs-scheduler.md): bindings and `Changed<T>` see an editor edit, an agent edit and an undo identically.

#### Boundary with ECS `Commands` (ecs-scheduler.md #22)

| | `Commands` (`exitium_ecs`) | Command bus (`exitium_command`) |
|---|---|---|
| Producers | systems, during a run | editor, MCP, tests, and host code (a system may only `enqueue`) |
| Form | closures, not serializable, no undo | reflected structs, serializable, with undo and a journal |
| Applies | at sync points inside a schedule run | at the boundary hook, never inside a run |
| Failure | skips dead entities and counts them | returns a typed error to the caller, world unchanged |
| Order | recorded order, then plan order | submission order |

The bus calls `World` methods directly and never records through `Commands`. `Commands` never records undo. Both stamp `world.change_tick()`. A bus command that spawns or despawns follows the reserve and flush rules of ecs-scheduler.md. Hierarchy changes use the same `Commands` helpers #49 defines (`set_parent`, `add_child`, `remove_child`, `despawn_recursive`), run through `World::run_system_once` exactly as assets-scene-export.md does for scene loading, so a reparent keeps `Parent` and `Children` consistent (app-platform.md: a raw overwrite of `Parent` is unsupported).

#### Built-in commands and queries (all generic over reflection)

| Name | Kind | Undo | What |
|---|---|---|---|
| `ecs_spawn_entity { components: [{ type_name, value }], name?, parent? }` | command | despawn (redo respawns, remap) | `world.spawn(())`, then `ReflectComponent::insert` for each, then `Parent` through the helper, then every `SpawnHooks` entry. Returns the entity. |
| `ecs_despawn_entity { entity, recursive: true }` | command | respawn from a reflected snapshot of the subtree (pre-order, old-to-new remap) | Warns when an entity has components with no `ReflectComponent` (the same ignore list as scene save, `CommandConfig.internal_components`, set by the facade glue); the undo is then lossy and says so. |
| `ecs_insert_component { entity, type_name, value }` | command | previous value, or removal | `ReflectComponent::insert`; value bound to the type; an empty `value` uses `ReflectDefault`. |
| `ecs_remove_component { entity, type_name }` | command | re-insert the snapshot | |
| `ecs_set_field { entity, type_name, path, value }` | command | previous field value; merges within a gesture | Resolves the path on `&dyn Reflect` (reflect-model.md "Path access") and binds `value` to the field's `TypeInfo` **first**; only then `get_mut` and `as_reflect_mut` to write, so a rejected set flags nothing. |
| `ecs_reparent { entity, parent: Option<Entity> }` | command | previous parent | Cycle check; helpers above. |
| `ecs_insert_resource`, `ecs_remove_resource`, `ecs_set_resource_field { type_name, path, value }` | command | previous value or removal | `ReflectResource`; the write rules of `ecs_set_field`. **Journal-only when the type carries `custom(history = false)`** (view state such as `SceneView`, so a camera drag does not fill the history). |
| `ecs_send_event { type_name, value }` | command | none | `ReflectEvent::send`. Journal only. |
| `ecs_get_stats`, `ecs_list_components`, `ecs_list_archetypes`, `ecs_query_entities`, `ecs_get_entity`, `ecs_get_component` | query | | The storage spec's table, verbatim: `ecs_query_entities` pages by entity index and validates `changed_since`, `added_since` and `as_of` through `DynSince::{changed, added}`, so its typed errors are `cursor_expired`, `invalid_as_of`, `invalid_params`, `untracked_component`. `ecs_get_entity` also lists `unreflected: [names]` so an agent sees what it cannot edit. |
| `ecs_list_schedules`, `ecs_get_schedule`, `ecs_list_resources`, `ecs_get_resource`, `ecs_list_events` | query | | `ScheduleReport`, `ResourceInfo`, `EventInfo`, the last `RunReport` per label (ecs-scheduler.md "MCP / editor surface"). `ecs_get_stats` includes the last `RunReport`s. |
| `reflect_list_types`, `reflect_get_type`, `reflect_dump_schema`, `reflect_get_schema_hash` | query | | reflect-model.md's table. `reflect_get_type` returns the JSON Schema of one type; it is how an agent learns a value's shape, so the generic tools carry only `type_name` enums (see "JSON and schemas"). |
| `serial_parse_text { text, type_name?, mode }`, `serial_write_text { type_name, value }` | query | | `from_text_as` with its `TextError` and `TextWarning` JSON, and `to_text` or `to_document` (serial-text-format.md "MCP / editor surface"). |
| `command_undo { steps? }`, `command_redo { steps? }`, `command_history { since?, limit? }`, `command_batch { commands, label, atomic }` | built into the bus | | History is the journal; entries carry the command's JSON. |

`exitium_command` registers all of this through one function, `register_builtin(world)`, which `LiveControlPlugin` and the tests call. A game that never calls it links none of it, so a shipped game pays nothing for the generic tools.

#### JSON and schemas

`json.rs` implements the **serial-text-format.md "JSON mapping" table** by walking reflection and calling `exitium_serial::leaf` for every spelling. A text-form type (`Entity`, `Guid`) is a string. A reader accepts either spelling of an `i64` beyond 2^53. Binding is type-directed and collects up to 16 errors, each as `{ code, path, expected, found, hint }` (the path is in reflect path syntax, so it can be handed to `ecs_set_field`). The unit tests run every row of the table in both directions, and compare `JSON -> reflect -> text` with the text format.

`schema.rs` builds **JSON Schema 2020-12** from `TypeInfo`: structs are objects with `properties` in declaration order and `$defs` for shared types; `Option` is `anyOf [null, T]`; unit-only enums are string enums and data enums are externally tagged `oneOf`; text-form types are `{"type":"string","pattern":..}` (never the integer or object of their structure); `i64`, `u64`, `i128` and `u128` accept an integer or a decimal string; a plain float also accepts `"nan"`, `"inf"` and `"-inf"`; `range` becomes `minimum` and `maximum`, `tooltip` or the docs `description`, `default` `default`, `hidden` `x-exitium-hidden`, `custom(key)` `x-exitium-<key>`. A `RawJson` field with `custom(json = true)` is `{}` (any JSON).

The **generic tools carry names, not types:** `ecs_insert_component` has `type_name` with an `enum` of every registered component, `ecs_send_event` an `enum` of every reflected event, and so on; the shape of a `value` is fetched with `reflect_get_type`. This keeps the tool list small (budget below) while leaving nothing hand-written. Typed commands (`ui_set_field`, `physics_apply_impulse`, a game's own) carry their full reflected schema.

`ToolDef { name, title, description, input_schema, output_schema, kind, read_only, destructive, undo }` is the list `schema.get` returns. It is cached per `revision`.

### The live-control protocol

**Transport.** A TCP listener on `127.0.0.1`, port 0 (the OS picks) unless `EXITIUM_LIVE_PORT` is set. The address is not configurable: there is no way to ask for another interface. **Framing:** UTF-8 JSON, one JSON-RPC 2.0 message per line (`\n`, `\r\n` tolerated), at most `MAX_FRAME_BYTES = 16 MiB`; a longer line closes the connection with `frame_too_large`. The shared types and the codec are `exitium_command::protocol`.

**Discovery.** The server writes `<project>/.exitium/live.json` (`{ protocol, port, token, pid, started, game, headless }`) by atomic rename once it listens, and deletes it on a clean exit. `exitium_mcp` finds it from `--project`, `EXITIUM_PROJECT_DIR`, or by walking up from its cwd to a directory with `exitium.project`; a stale file (dead pid, refused connection) is `not_connected`. `.exitium/` is gitignored (adjustment AM7).

**Messages** (examples are the golden files):

```
-> {"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol":1,"token":"<32 hex>","client":"exitium_mcp 0.1.0"}}
<- {"jsonrpc":"2.0","id":1,"result":{"protocol":1,"engine":"0.1.0","game":"my_game","mode":"edit","schema_revision":7,"headless":false}}
-> {"jsonrpc":"2.0","id":2,"method":"schema.get","params":{}}
<- {"jsonrpc":"2.0","id":2,"result":{"revision":7,"tools":[{"name":"ecs_set_field","kind":"command","undo":"always","read_only":false,"destructive":false,"description":"..","input_schema":{..},"output_schema":{..}}]}}
-> {"jsonrpc":"2.0","id":3,"method":"tool.call","params":{"name":"ecs_set_field","arguments":{"entity":"42v1","type_name":"exitium_app::Transform2d","path":"translation.x","value":12.5},"timeout_ms":30000}}
<- {"jsonrpc":"2.0","id":3,"result":{"output":null,"warnings":[],"seq":41,"tick":1203}}
<- {"jsonrpc":"2.0","id":3,"error":{"code":-32000,"message":"entity 42v1 is not alive","data":{"code":"unknown_entity","path":"entity","expected":"a live entity","found":"42v1","hint":"call ecs_query_entities"}}}
<- {"jsonrpc":"2.0","method":"notify.schema_changed","params":{"revision":8}}
```

Methods: `hello`, `ping`, `schema.get`, `tool.call`, `bye`. Server notifications: `notify.schema_changed { revision }`, `notify.mode_changed { mode }`, `notify.bye { reason }`. Logs are pulled with the `log_tail` tool, not pushed.

**Error codes** (`error.data.code`, stable strings; the numeric JSON-RPC `code` is `-32000` for every tool error, and the reserved `-326xx` codes for malformed JSON-RPC): `invalid_params`, `unknown_tool`, `unknown_type`, `unknown_entity`, `path_error`, `type_mismatch`, `validation_failed`, `entity_dead`, `not_undoable`, `nothing_to_undo`, `undo_failed`, `rollback_failed`, `in_schedule`, `busy`, `timeout`, `unsupported`, `incompatible_protocol`, `unauthorized`, `frame_too_large`, `not_connected` (client side), `cancelled`, `internal`. A new code is additive.

**Versioning.** `PROTOCOL_VERSION` is a single integer. Removing or renaming a method or field, or changing its meaning, bumps it; adding an optional field, a method or a notification does not. Both sides ignore unknown fields and unknown notifications; an unknown method is `-32601`. A `hello` with another version is refused with `incompatible_protocol` naming both numbers, then the connection closes. `schema_revision` is separate: it changes when tools or types change, never the protocol.

**Security (localhost only).**
1. Loopback bind only, and a test asserts the bound address is `127.0.0.1`.
2. A **token** of 128 bits from `getrandom`, 32 hex characters, in `live.json` only (created in the user's directory with its default ACL; never logged; compared in constant time). The first line must be a valid `hello` within 5 s, or the connection closes silently. A bad token closes the connection, and the third bad token in a row delays the next accept by 1 s.
3. A connection whose first bytes look like an HTTP request (`GET `, `POST `, `PUT `, `OPTIONS `) is closed without a reply, so a web page cannot probe the port with `fetch`.
4. One client at a time; a second `hello` gets `busy`. A silent client is dropped after 60 s (the MCP client pings every 15 s).
5. At most 64 requests queued (`busy` beyond that); JSON nesting is limited to 128; a tool call that outlives `timeout_ms` (default 30 s) is answered `timeout`.
6. The channel is exactly as powerful as the agent already is. It exposes **registry entries only** (no eval, no shell, no raw memory); paths in scene and asset tools are relative to the project root, with `..` and absolute paths into `assets/` refused (`asset_import` may read a source file from anywhere, like a file dialog). The token stops other local software and web pages, not the agent.
7. A shipped game has no server: `editor` is off, and `exitium export` never turns it on (assets-scene-export.md).

### The editor (`exitium_editor`)

#### Embedding and the frame

```rust
pub struct LiveControlConfig {
    pub project_dir: Option<PathBuf>,   // None: EXITIUM_PROJECT_DIR, else the nearest ancestor of cwd with exitium.project
    pub port: u16,                      // 0, or EXITIUM_LIVE_PORT
    pub write_discovery: bool,          // true
    pub start_mode: StartMode,          // Edit (default) | Play, or EXITIUM_EDITOR_START=play
    pub request_budget: Duration,       // 4 ms of tool work per boundary
}
pub struct LiveControlPlugin(pub LiveControlConfig);       // server, host tools, builtin tools; no egui, no window
pub struct EditorPlugin { pub live: LiveControlConfig, pub gui: GuiConfig }   // LiveControlPlugin + the egui shell
pub fn run_headless(app: App, pace: Pace) -> AppExit;      // real-time paced frame loop for a windowless game
```

`exitium::DefaultPlugins` adds `EditorPlugin` when the facade `editor` feature is on (facade hook FH1). With `EXITIUM_HEADLESS=1` it adds the render plugin in headless mode, no platform plugin, `LiveControlPlugin` and `run_headless` as the runner: a game server an agent can drive with no window (screenshots still work through `Renderer::headless`).

**The frame, in a windowed editor.** `App::add_boundary_hook` runs hooks at step 1 of every frame, outside any schedule, in registration order (app-platform.md):

```
winit event --> WindowHandler::raw_event --> egui_winit::State::on_window_event; consumed events never reach the game
run_frame step 1:
   hook 1, live_control:  drain the request queue (up to request_budget), poll Wait replies, apply through the bus, send replies
   hook 2, editor_gui:    take egui input, run the egui frame with a read-only view of the world and an ActionSink,
                          submit the queued actions through the same entry as hook 1, store the FullOutput for the overlay
   steps 2 to 9:          the game's frame
   Extract / RENDER:      render_frame draws the cameras; the surface overlay (egui-wgpu) paints the stored FullOutput
```

The live-control hook runs first so the panels show this frame's state. The GUI therefore lags the world by one frame at most. The overlay needs the render amendment X1 below; the raw-event wiring needs X2.

**Host tools** are registry entries in `exitium_editor::tools`, each a `Command` or `Query` that calls the plain Rust functions of the runtime specs: scene (`scene_new`, `scene_load`, `scene_save`, `scene_list`, `scene_validate`), assets (`asset_list`, `asset_get`, `asset_import`, `asset_register`, `asset_set_settings`, `asset_rename`, `asset_remove`), play (`play_start`, `play_pause`, `play_resume`, `play_step`, `play_stop`, `play_advance`), render (`render_screenshot`, `render_screen_to_world`, `render_world_to_screen`), editor state (`editor_get_state`, `editor_select`, `editor_pick`) and logs (`log_tail`). Each lists its undo in the table under "MCP / editor surface".

**Spawn and the scene.** Every authoring spawn must call `exitium_scene::mark_member(world, entity, active_scene)` or the entity is not saved (assets-scene-export.md, "Membership is the save scope"). The editor registers a `SpawnHooks` entry that does it, and creates an "Untitled" scene when `Scenes.active` is `None`. So `ecs_spawn_entity` from MCP and "Create entity" in the hierarchy behave identically.

**`EditorState`** is a reflected resource (`mode`, `selected: Vec<Entity>`, `scene_path`, `dirty`, `history_epoch`), so the agent and the human see one state. `dirty` is the bus's history revision differing from the revision at the last save.

#### Panels (fixed layout)

| Panel | Built from | Writes through |
|---|---|---|
| Menu bar: Scene (New, Open, Save, Save As), Edit (Undo, Redo), Export (runs the CLI) | the tool list | `scene_*`, `command_undo`, `command_redo`, `project_export` |
| Toolbar: Play, Pause, Step, Stop, speed | `EditorState`, `TimeConfig` | `play_*`, `ecs_set_resource_field` on `TimeConfig.time_scale` |
| Hierarchy (left): tree of `(Entity, Name, Parent, Children)`, selection, rename, create, delete, drag to reparent | `ecs_query_entities` logic | `ecs_spawn_entity`, `ecs_despawn_entity`, `ecs_reparent`, `ecs_set_field`, `editor_select` |
| Inspector (right), tab Entity: every component of the selection, add and remove component | `ReflectComponent` and `TypeInfo` | `ecs_set_field`, `ecs_insert_component`, `ecs_remove_component` |
| Inspector, tab Resources: every `ReflectResource` type, including `TimeConfig`, `SceneView`, `RenderSettings`, `PhysicsConfig`, `AudioSettings` | `ReflectResource` and `TypeInfo` | `ecs_set_resource_field` |
| Inspector, tab Systems: schedule order, access sets, sync points, ambiguities (read-only) | `ecs_get_schedule` | none |
| Viewport (centre): the scene view texture; pan and zoom write `SceneView`; a click picks; a gizmo moves `Transform2d` | `SceneView`, `CameraViews` | `ecs_set_resource_field` (journal only), `editor_select`, `ecs_set_field` |
| Bottom tabs: Console (log ring buffer, level filter), History (journal, undo and redo), Assets (list, import, settings, rename, remove, thumbnails) | `log_tail`, `command_history`, `asset_list` | `command_undo`, `command_redo`, `asset_*` |

**Reflection to widgets** (`gui/widgets.rs`): `ValueRepr` leaves to drag values, checkboxes and text fields (`range` makes a slider, no `range` makes a drag value); structs to collapsing groups in declaration order; unit-only enums to combo boxes; data enums to a combo plus the active variant's fields; `Option` to a checkbox and the inner widget; lists to rows with add and remove; maps to key and value rows; text-form types (`Entity`, `Guid`) to a text field that validates with `from_text` and, for `Entity`, an eyedropper that picks from the hierarchy; `hidden` skips the widget; `tooltip` or docs are the hover text; `custom(widget = "color")` is a colour picker; `custom(widget = "asset", kind = ..)` is the **asset picker** (assets-scene-export.md): a list of `AssetRegistry` entries of that kind with a thumbnail for images, writing the chosen `Guid` through `ecs_set_field`; a `Guid` without the widget stays a text box; `custom(derived = true)` and `custom(readonly = true)` are drawn disabled; `custom(multiline = true)` is a multi-line text edit. A component or resource whose type has no registry entry is listed by name as "not reflected" and nothing else.

**Input.** egui gets every `WindowEvent` first (`raw_event`); it reports `consumed` for events over its panels, so a click on a panel never reaches the game. While Playing, events over the viewport are not consumed, so the game keeps keyboard input; pointer positions reach it as window coordinates, unmapped (open question 4). An agent injects input with `ecs_send_event` of `exitium_platform::InjectedInput`.

#### Play, pause, step, stop (D9)

| State | `TimeConfig.paused` | Entered by |
|---|---|---|
| Edit | true | the start (unless `start_mode = Play`), `play_stop` |
| Playing | false | `play_start`, `play_resume` |
| Paused | true | `play_pause` |

- **`play_start`** (from Edit only). Snapshot, then unpause. The snapshot is: (a) `write_scene(SaveScope::Scene(active))` text, held in memory (its `SceneWarning`s are returned, never fatal: they name components that will not be restored); (b) a `clone_boxed` of every resource whose type carries `custom(play_snapshot = true)` (opt-in: game state; engine and editor resources are never restored); (c) the set of live entities. `start_mode = Play` takes the snapshot at the second boundary, after `Startup` has run.
- **`play_pause`, `play_resume`.** Write `TimeConfig.paused`. **`play_step { ticks = 1 }`** (Paused only) adds to `TimeConfig.step_ticks`; the loop runs exactly that many ticks next frame (app-platform.md). **`play_advance { ticks }`** (Playing or Paused) is a `Wait`: it resumes or steps and replies when `Time::tick_count` has advanced by `ticks`, so an agent can "run 120 ticks and look".
- **`play_stop`.** Pause; send `StopAllAudio`; despawn every entity that is not in the pre-play live set; `despawn_scene` and re-`instantiate` the snapshot text into the same `SceneId` (atomic, assets-scene-export.md); restore the opted-in resources; clear the undo and redo stacks and bump `history_epoch`; remap the selection by position in the snapshot's file order. Physics rebuilds itself from the reflected components (its world is derived state, physics-2d.md). If the restore fails (it should not, the text is our own) the mode stays Edit and the error is returned.
- **What Stop does not restore** (documented limits): entities that existed before Play and are not scene members (code-spawned at `Startup`); the UI tree (`UiTree` is a resource that is not in a scene, ui-mvp.md open question 8); any resource without `play_snapshot`. Authoring data belongs in the scene.
- **Edit mode and `Update`.** In Edit the tick loop does not run, but `Update` and `PostUpdate` still run every frame (app-platform.md: a paused game is still drawn), so a game that mutates the world in `Update` mutates the edit-time world. Gap X3: an amendment adds a frozen mode to the frame driver; until it lands the behaviour is documented in `crates/exitium_editor/CLAUDE.md` and Stop heals it for scene members.

#### Scene save and load

`scene_save` calls `write_scene` (scope `Scene(active)`, `asset_path` from the registry, `ignore_unreflected` from `CommandConfig.internal_components`), then `exitium_assets::native::write_atomic`, then `AssetRegistry::register` and a manifest save for a new file; it returns the `SceneWarning`s, so an agent that forgot `#[reflect(Component)]` is told. It writes a file, so it has no undo. `scene_load { path, mode, parent? }` reads the asset, `ParsedScene::parse`, `instantiate` with `resolve_asset`, requests `asset_refs` from the `AssetServer`; Replace first `despawn_scene`s the active scene. Both `scene_load` and `scene_new` are commands: additive undo despawns the instance; Replace keeps the previous scene's text and undo reloads it, and the history is cleared (entities change), recorded in the result as `history_cleared`. `scene_validate` runs `validate_scene` on a scratch world and touches nothing. The menu items call these tools.

#### Headless mode

`LiveControlPlugin` alone runs without a window, a renderer or egui. It needs `exitium_app` and the plugins the game adds; the boundary hook runs in every `run_frame`, including the headless runner. `run_headless` is that runner with real-time pacing at `fixed_hz` (sleep to the next frame) so a windowless game does not spin. Tests do not use it: they call `app.run_frame` themselves in a loop with a 1 ms sleep, so frames are deterministic and the test owns the thread (`App` is not `Send`).

#### Logs

`console.rs` installs a `tracing` layer (`tracing-subscriber` with `registry`, and `tracing-log` so wgpu and egui's `log` records land too) into a ring buffer of 4,096 `{ seq, level, target, message, frame }` entries, and a panic hook that records the panic and the failing system (`exitium_ecs::current_system()`). If the game already installed a global subscriber, the plugin does not replace it and `log_tail` returns a note saying logs are not captured. `log_tail { since?, level?, limit? }` is the tool; the Console panel reads the same buffer.

### The MCP server (`exitium_mcp`)

`exitium_mcp` is a library (the tool table, the client) and a binary `exitium_mcp` (stdio). Arguments: `--project <dir>`; environment `EXITIUM_PROJECT_DIR`, `EXITIUM_CLI`. It runs a `tokio` current-thread runtime and `rmcp`'s stdio transport; **stdout carries only MCP frames** (every log goes to stderr).

- **Tool table.** `list_tools` returns the ops tools (always present) plus the live tools from the last `schema.get`. `call_tool(name, args)` forwards a live tool as `tool.call` and maps the result: the JSON result as one text block (every client shows it) and as `structuredContent` where the SDK supports it; `render_screenshot` returns an **image block** (`image/png`) and a text block with the size; a protocol error becomes a tool error (`isError: true`) whose text carries the stable `code`, `path`, `expected`, `found` and `hint`. A call to a live tool while disconnected fails with `not_connected` and the hint "call project_run, or start the game with --features editor". When `schema_revision` changes the server sends MCP `notifications/tools/list_changed`.
- **Connection.** Lazy: the first live call, or `project_run`, reads `live.json` and connects; one reconnect attempt per call; a 15-second `ping`. A server `notify.bye` drops the live tools from the list.
- **`instructions`.** The MCP `initialize` result carries a short guide generated from the schema (the game name, mode, the five calls to start with: `ecs_query_entities`, `reflect_get_type`, `ecs_spawn_entity`, `scene_save`, `render_screenshot`; and the recipe for input and audio through `ecs_send_event`).
- **Ops tools** (they need no running game). `project_build { release?, features?, package? }` runs `cargo build --message-format=json` in the project and returns the exit code, the duration and up to 50 diagnostics `{ level, message, file, line }`. `project_test { filter?, package?, release? }` runs `cargo test` (or nextest when present) and returns counts and the failing tests with their output tail. `project_run { headless?, release?, features?, wait_secs = 120 }` starts `cargo run --features editor` (plus `EXITIUM_HEADLESS=1` for `headless`) as a child process, keeps the last 200 KB of its output, waits for `live.json` and a `hello`, and returns `{ pid, port, connected }`. `project_stop { force? }` sends `AppExit` through `ecs_send_event`, waits 5 s, then kills. `project_status { tail? }` returns whether it runs, whether it is connected, its uptime and the output tail. Arguments are validated (features match `[A-Za-z0-9_/-]+`, never `editor` in a build for export), and the only programs started are `cargo` and, in Stage 4, the `exitium` binary. Stage 4 adds `project_new`, `project_export_check` and `project_export { target, out? }`, which start `exitium new` and `exitium export --json` as child processes (jam-mvp.md Stage 4: no `exitium_mcp -> exitium_cli` edge) and report progress lines as MCP progress notifications. The binary is found from `EXITIUM_CLI`, else `exitium` on `PATH`, else `cargo run -p exitium_cli` from the engine checkout recorded in the project's `Cargo.toml`.

### Threading

| Thread | In | Role |
|---|---|---|
| Main (the app thread) | game | Everything that touches the world: bus, tools, hooks, egui, rendering. |
| `exitium-live-accept` | game | `accept`, handshake checks. Never sees the world. |
| `exitium-live-reader`, `exitium-live-writer` | game | One pair per connection: lines in to a bounded `mpsc::sync_channel(64)` of `Incoming { id, method, params }`, replies out from a channel the main thread fills. The main thread never blocks on the socket. |
| PNG worker | game | A short-lived `std::thread` per screenshot encodes and base64-encodes off the main thread; the `Wait` replies when it finishes. |
| tokio current-thread | `exitium_mcp` | MCP stdio and the live client. |

The jam-mvp.md statement "the live-control server is the only extra thread" is refined here: the server is up to three threads, plus the asset workers and audio thread that assets-scene-export.md already lists. None touches the world; the main loop drains the queue at the one boundary point. In the game process the editor adds no thread on the web (it does not build there).

## MCP / editor surface

This section is the contract of the whole feature. Every merged spec lists an "exposure" surface; **all of it is reached through the registry, with schemas derived from reflection, so no crate hand-writes a tool or a panel** (C4).

### Tool list

Names are `<domain>_<verb>`. "Where" is the crate that registers the tool. Kinds: C command, Q query, W query with `Wait`. Undo: U undoable, J journal only, none.

| Domain | Tools | Where | Kind and undo |
|---|---|---|---|
| ecs | `ecs_get_stats`, `ecs_list_components`, `ecs_list_archetypes`, `ecs_query_entities`, `ecs_get_entity`, `ecs_get_component`, `ecs_list_schedules`, `ecs_get_schedule`, `ecs_list_resources`, `ecs_get_resource`, `ecs_list_events` | command | Q |
| ecs | `ecs_spawn_entity`, `ecs_despawn_entity`, `ecs_insert_component`, `ecs_remove_component`, `ecs_set_field`, `ecs_reparent`, `ecs_insert_resource`, `ecs_remove_resource`, `ecs_set_resource_field` | command | C, U (`ecs_set_resource_field` is J for `history = false` types) |
| ecs | `ecs_send_event` | command | C, J |
| reflect, serial | `reflect_list_types`, `reflect_get_type`, `reflect_dump_schema`, `reflect_get_schema_hash`, `serial_parse_text`, `serial_write_text` | command | Q |
| command | `command_undo`, `command_redo`, `command_history`, `command_batch` | command (bus) | C, none (they move the history) |
| scene | `scene_new`, `scene_load` | editor | C, U |
| scene | `scene_save` | editor | C, none (writes a file); `scene_list`, `scene_validate` are Q |
| asset | `asset_import`, `asset_register`, `asset_set_settings`, `asset_rename`, `asset_remove` | editor | C, U (undo moves files back) |
| asset | `asset_list`, `asset_get` | editor | Q |
| play | `play_start`, `play_pause`, `play_resume`, `play_step`, `play_stop` | editor | C, J |
| play | `play_advance` | editor | C with `Wait`, J |
| render | `render_screenshot` | editor | W |
| render | `render_screen_to_world`, `render_world_to_screen` | editor | Q |
| editor | `editor_get_state`, `editor_pick` | editor | Q |
| editor | `editor_select` | editor | C, J |
| log | `log_tail` | editor | Q |
| physics | `physics_apply_impulse` (C, J), `physics_raycast`, `physics_overlap_point`, `physics_overlap_shape`, `physics_shape_cast`, `physics_body_state` (Q) | `exitium_physics` | as listed |
| ui | `ui_build_tree`, `ui_add_node`, `ui_remove_node`, `ui_reparent`, `ui_set_field`, `ui_set_theme` (C, U), `ui_click` (C, J); `ui_get_tree`, `ui_get_node`, `ui_find`, `ui_get_theme`, `ui_hit_test`, `ui_get_stats` (Q) | `exitium_ui` | as listed |
| project | `project_build`, `project_test`, `project_run`, `project_stop`, `project_status`; Stage 4: `project_new`, `project_export_check`, `project_export` | `exitium_mcp` (ops, no running game) | none |

That is about 80 tools with the engine's own crates; a game adds its own commands. `exitium_mcp --groups ecs,scene,...` limits the list when a client's context budget is tight (open question 6).

### Exposure coverage, spec by spec

Each row lists what the spec asks for and how it is built here. "Generic" means no code names the type: reflection and the registry do the work.

| Spec | Its exposure | Built here |
|---|---|---|
| **ecs-storage-queries.md** | `ecs_*` read tools, `DynamicQuery`, the six mutation commands, `command_undo`, `command_redo`, `command_get_history`; hierarchy, inspector and archetype views | All tools in the table above (`command_get_history` is `command_history`). Hierarchy and inspector panels. The archetype and memory view is the profiler panel, cut by the roadmap: an agent has `ecs_get_stats` and `ecs_list_archetypes`, so there is no parity gap (X9). |
| **ecs-scheduler.md** | `InsertResource`, `RemoveResource`, `SetResourceField`, `SendEvent`, nine read tools, a Systems panel, a resource inspector, event counters in the profiler panel | The four commands and the read tools exactly; Systems tab and Resources tab. Event counters: `ecs_list_events` (profiler panel cut, X9). Non-undoable events resolved by `UndoKind` and the journal. |
| **reflect-model.md** | `reflect_list_types`, `reflect_get_type`, `reflect_dump_schema`, `reflect_get_schema_hash`; type picker; inspector layout from attributes; migration warnings | The four tools; the "Add component" menu is the type picker over `ReflectDefault` types; widgets read `range`, `hidden`, `tooltip`, `docs`, `custom`. Migration warnings: no migrations in the MVP, the hash tool exists. |
| **serial-text-format.md** | a parse tool, a write tool, paste and "copy as text", canonical formatting | `serial_parse_text`, `serial_write_text`; "Copy as text" and "Paste" on a component or field in the inspector (paste is `apply_text` then `ecs_set_field`); canonical formatting is parse then write. The JSON mapping is `json.rs`. |
| **app-platform.md** | `Time` (read), `TimeConfig` (read and write), `FrameStats`, `CursorWorld`, `WindowInfo`, `WindowConfig`, `InputSnapshot`, `Transform2d`, `GlobalTransform2d` (read only), events `InjectedInput` and `AppExit`, presets `app_pause`, `app_resume`, `app_step`, `input_inject`, a time control, a gizmo, an input and window panel, the `raw_event` hook | Resources and components are generic. `app_pause` and friends are **superseded by `play_*`** (which also own the snapshot); `input_inject` is `ecs_send_event` of `InjectedInput`, and the event enum in that tool's schema makes it discoverable (X6). The time control is the toolbar. The gizmo writes `Transform2d` through `ecs_set_field`. The input and window panel is the Resources tab over `InputSnapshot` and `WindowInfo`. `raw_event` is the input path above (gap X2). |
| **render-v1.md** | `Sprite`, `Camera2d`, `Text`, `SceneView`, `RenderSettings`, `RenderStats`, `Screenshots`, `CameraView::{screen_to_world, world_to_screen}`, the scene view panel with gizmos, inspector fields | Components and resources are generic. `render_screenshot` wraps `Screenshots::{request, take}` as a `Wait`, resolves the camera the way `Screenshots` does (main camera, then `SceneView`), **encodes the PNG itself** with `exitium_assets::image::encode_png` on a worker thread, refuses sizes above 2048, and halves the size (or fails with `unsupported`) when the base64 payload would pass 8 MiB. `render_screen_to_world` and `render_world_to_screen` wrap the pure functions. The viewport shows `SceneViewTarget` through `egui-wgpu` on `Renderer::device()` and `queue()`, drawn by the overlay (gap X1). Gizmos use `DebugDraw`. |
| **physics-2d.md** | components `RigidBody`, `Collider`, `Velocity`; resources `PhysicsConfig`, `PhysicsDebugSettings`, `PhysicsStats`; commands `SetPhysicsConfig`, `SetDebugDraw`, `ApplyImpulse`; queries `raycast`, `overlap_point`, `overlap_shape`, `shape_cast`, `body_state` | Components and resources generic. **`SetPhysicsConfig` and `SetDebugDraw` are not built:** `ecs_set_resource_field` covers them with undo (physics open question 5 is answered: yes, the bus has a generic resource-field command). Physics registers `physics_apply_impulse` (no undo, `UndoKind::NotUndoable`) and the five queries itself in P15 and P16 (adjustment AM5), through `register_command` and `register_query`. The debug-draw toggle and stats tab are the Resources tab. |
| **ui-mvp.md** | seven commands, six queries, a node tree, an inspector from `UiNodeDesc`, node picking, a theme editor | `exitium_ui` registers them in U13 (D13). `ui_click` is `NotUndoable` (open question 7 answered); `ui_build_tree` is one command whose undo is `detach` and whose redo is `restore` (stable ids need no remap). The UI hierarchy view and inspector are **not** in the MVP shell: the editor has no edge to `exitium_ui` (see the graph above), and the UI editor is epic #6. An agent builds and inspects UI fully through the tools, so there is no parity gap. |
| **assets-scene-export.md** | `asset_*`, `scene_*`, `audio_*`, `project_*` tools; asset browser, scene menu, audio panel, Export menu; the **asset-picker widget** keyed on `custom("widget")` and `custom("kind")`; `mark_member` on spawn; "no undo" declaration | The asset and scene tools are `tools/assets.rs` and `tools/scene.rs`, each wrapping the plain functions of the spec (the table in that spec's "MCP / editor surface"). The picker is in `gui/widgets.rs`. `mark_member` is the `SpawnHooks` entry. No-undo is `UndoKind`. `project_*` are the ops tools. **The named `audio_*` tools are not built:** `AudioSettings`, `PlayAudio`, `PlayMusic`, `StopMusic` and `StopAllAudio` are reflected, so `ecs_set_resource_field` and `ecs_send_event` cover them with discovery through the event enum (X6). The audio panel is the Resources tab. The Export menu starts the same CLI as `project_export`. |
| **jam-mvp.md** | C1 to C6, the Stage 3 rulings, the open questions assigned to #110 | Answered in "Open questions" (the three decisions it left to this spec are D6, the wasm status of `exitium_command`, and the headless server). |

### Gaps this spec found

Each is a decision made here, or a follow-up filed by the architect with `status:needs-approval` (see "Downstream adjustments"). X1 and X2 block ED8 and FH1; the rest block nothing.

| Id | In | Gap | Resolution |
|---|---|---|---|
| X1 | render-v1.md | `Renderer::render` acquires, draws and presents the surface in one call, so the editor cannot draw egui on the window. Texture-only passes may not acquire the surface at all. The game's `Camera2d` passes also draw on the surface under the panels, wasting GPU time in the editor. | **AM1.** A `SurfaceOverlay` trait, `Renderer::set_overlay`, `render` acquiring the surface whenever an overlay is set, and `RenderSettings.draw_cameras_to_surface` (default true). Contract sketch below. Blocks ED8. |
| X2 | app-platform.md | `WindowHandler::raw_event` is described for keyboard, mouse and cursor events. egui-winit also needs focus, IME, modifiers, scale and file-drop events. A hidden or occluded window may stop frames, so a request waits. | **AM2.** `raw_event` receives every `WindowEvent` (consumption applies only to input events); the exact `PlatformPlugin::with_raw_event_hook` signature is pinned; the platform exposes a wake handle (`EventLoopProxy`) so the server can request a frame. Blocks ED8. |
| X3 | app-platform.md | No way to freeze `Update` in Edit mode. | **AM3.** A reflected `SimulationMode { Running, Frozen }` read by `run_frame`: Frozen skips the tick loop and `Update`, and still runs `First`, `PostUpdate`, `Extract` and `Last`. Not blocking: ED6 works without it (documented limit). |
| X4 | reflect and the specs | The attributes this spec relies on are not on the types that need them: `custom(readonly = true)` (`Time`, `FrameStats`, `RenderStats`, `PhysicsStats`, `AudioStats`, `AssetStatus`, `CursorWorld`, `WindowInfo`, `InputSnapshot`), `custom(history = false)` (`SceneView`), `custom(play_snapshot = true)` (opt-in for game resources). | **AM4**, one small issue per crate. Without them the editor treats an unmarked type as editable, undoable and not snapshotted: correct, only less polished. |
| X5 | physics-2d.md | P15 builds `SetPhysicsConfig` and `SetDebugDraw`, which duplicate the generic resource command. | **AM5.** |
| X6 | assets, app | Named `audio_*` and `app_*` presets and `input_inject` in two specs. | Superseded by `play_*` and the generic event tool (decision above). Recorded so the specs are not read as promising them. |
| X7 | ui-mvp.md | The UI tree is not in a scene, so Play and Stop do not snapshot it, and scenes do not save it. | Documented limit; the tree is rebuilt by the game's `Startup` or saved as a `.ui` asset (ui-mvp.md open question 8). Epic #5 and #6. |
| X8 | assets-scene-export.md | Scenes hold entities only; resources are not persisted. | Play uses an opt-in `play_snapshot` attribute; durable resource persistence stays with epic #7 (saves). |
| X9 | storage and scheduler specs | The profiler panel (archetype and memory view, event counters) is cut by the roadmap. | The tools exist, so an agent can do everything; the human panel is epic #6. |
| X10 | the CLI template | The template's `.gitignore` lacks `/.exitium`, and there is no `.mcp.json`. | **AM7.** |
| X11 | egui | `default_fonts` brings `OFL-1.1` and `Ubuntu-font-1.0` through `epaint_default_fonts`, which `deny.toml` does not allow. | Default features off; the editor embeds one font with its licence (owner decision, open question 2). Resolves render-v1.md open question 7 for the editor only. |
| X12 | jam-mvp.md | Stage 3 is larger than its estimate (about 15 PRs). This spec files 29 issues in Stages 3 and 4, plus the amendments AM1 to AM7. | See "Performance"; the roadmap's counts are updated by AM8. |

**AM1 contract sketch** (the render spec's amendment owns the final signatures):

```rust
pub trait SurfaceOverlay: Send + Sync + 'static {
    /// Once per presented frame, after the last camera pass and before submit.
    fn record(&mut self, cx: OverlayCx<'_>);
}
pub struct OverlayCx<'a> { pub device: &'a wgpu::Device, pub queue: &'a wgpu::Queue, pub encoder: &'a mut wgpu::CommandEncoder,
                           pub target: &'a wgpu::TextureView /* format F */, pub size_px: UVec2, pub scale_factor: f32 }
impl Renderer { pub fn set_overlay(&mut self, overlay: Option<Box<dyn SurfaceOverlay>>); }
```

The editor's overlay owns the `egui_wgpu::Renderer` and reads the latest `FullOutput` from an `Arc<Mutex<..>>` that the editor hook fills; it records one `LoadOp::Load` pass over the surface.

### Parity test

The rule is enforced three ways.

1. **By construction.** GUI code takes `&ReadView` and `&mut ActionSink` and cannot name `&mut World`. `ActionSink::push(tool, args)` is the only way a panel acts, and the boundary hook submits it through `submit_by_name`, the function the server calls for `tool.call`.
2. **By table.** `gui/actions.rs` declares `EDITOR_ACTIONS: &[EditorAction { id, label, tool }]`, one row per menu item, button, drag or paste the shell offers. Every panel issue adds its rows.
3. **By test** (XT2, in `exitium_editor`'s tests): for every row, (a) `tool` exists in the schema the server reports and has an input schema; (b) the action is run through the sink in a headless app and the MCP binary runs the same tool with the same arguments in a second headless app; the two worlds, written by `write_scene`, are equal; (c) every registry command has either an `EDITOR_ACTIONS` row or an entry in an explicit `AGENT_ONLY` list with a reason (the reverse direction is not required: the agent may do more than the human). The test fails when a row names a tool the server does not report, so deleting an MCP tool, or adding a button without its tool, breaks CI.

## Performance

Proposals, like the roadmap's. Budgets are p50 on the min-spec reference runner (4 cores, 8 GB), release, one thread unless stated; criterion benches use the 5% gate (docs/benchmarks.md); absolute figures are provisional ceilings that the first benchmark PR may re-baseline with a note to the owner. The roadmap's round-trip budgets (one command in 50 ms at p95, a 1280x720 screenshot in 250 ms) are inputs.

| Operation | Budget |
|---|---|
| `ecs_set_field` `apply`, an `f32` field, excluding JSON | <= 2 us |
| JSON arguments to a `SetField` command (100 bytes) | <= 5 us |
| `ecs_spawn_entity`, 5 components, with bus bookkeeping | <= 25 us |
| `undo` and `redo` of `ecs_set_field` | <= 2 us |
| Despawn and undo of a 50-entity subtree, 4 components each | <= 2 ms |
| History memory | <= 400 B per `ecs_set_field` entry; 1,000 entries or 16 MiB |
| `schema.get`, 200 reflected types, 90 tools, cold | <= 25 ms; warm (same revision) <= 1 us |
| Tool list payload, 90 tools | <= 100 KB (about 25,000 tokens), generic tools <= 1.5 KB each |
| Boundary hook with no request and the GUI hidden | <= 15 us |
| Tool work per boundary | <= 4 ms (`request_budget`); the rest waits one frame |
| One command, MCP to world and back (loopback) | <= 50 ms at p95, including up to one frame of waiting |
| `ecs_query_entities`, a page of 256 entities with 3 components, as JSON | <= 3 ms |
| `render_screenshot` 1280x720 round trip | <= 250 ms; main-thread cost <= 3 ms (PNG and base64 on a worker) |
| Editor frame (egui run), 2,000 entities, inspector with 10 components | <= 2.0 ms CPU; <= 0.3 ms with the panels hidden |
| Overlay record (egui paint), a typical frame | <= 1.0 ms CPU |
| `play_start`, 1,000 entities / 10,000 entities | <= 20 ms / <= 100 ms (scene write budget) |
| `play_stop`, 1,000 entities / 10,000 entities | <= 30 ms / <= 150 ms (scene instantiate budget) |
| Log capture, one event after the level filter | <= 1 us; ring 4,096 entries, <= 1 MiB |
| Idle server | 0 CPU (blocking reads), <= 64 KiB plus the log ring |
| Added cost of `--features editor` with the panels hidden | <= 0.1 ms per frame |

Benchmarks: `exitium_command/bus/{set_field,spawn_5,undo_redo_set_field}`, `exitium_command/json/{bind_component,write_component}`, `exitium_command/schema/build_tools`, `exitium_editor/frame/{inspector_10_components,hierarchy_2000}` (a headless `egui::Context`, no GPU) and `exitium_editor/server/decode_dispatch` (a pure function, not a socket, so the gate is stable). Complexity: the bus is O(1) per command plus the command's own work; history trimming is amortized O(1); the hierarchy rebuild is O(entities) and is throttled to 4 Hz above 10,000 entities.

**Stage 3 size, honest count.** jam-mvp.md estimated about 15 PRs (command bus 3, editor lane 6, MCP lane 4, hook 1, tests 1) and a critical path of 11. The real list below is 11 issues in the command crate, 11 in the editor, 4 in MCP (one of them Stage 4), 1 facade and 2 tests: 29 PRs, and a critical path of about 11 + 11 + 1 + 2 = 25, because the command crate is a serial chain that both lanes wait for (R3) and the editor lane is serial in one crate. The amendments AM1 to AM3 run beside the command chain. If the owner wants a shorter path, the first cuts are the Systems tab, the asset browser panel and `serial_*` text tools (each about one PR), not the bus.

## Test plan

- **Bus.** Apply, undo and redo for every built-in command, including a seeded random sequence of commands followed by undo-all (the world, written by a reflective dump, equals the start) and redo-all (equals the end). A failed command leaves the world unchanged (the entity count, the change cursor and the dump). Non-undoable commands are journalled and do not clear the redo stack. `InSchedule` when called from a system. Merging within a gesture and never without one. `batch` atomic rollback, and `rollback_failed`. Entity remap: despawn, undo, then `SetField` on the old id works, and redo despawns the new one. History caps. A panic in a command is caught, reported as `internal`, and clears the history.
- **Reflection surface.** A rejected `ecs_set_field` flags nothing (`Changed<T>` stays false); an accepted one flags exactly once. `ecs_get_entity` lists unreflected components. Every query leaves `change_cursor` unchanged.
- **JSON and schema.** Every row of the serial JSON mapping table in both directions; `JSON -> reflect -> text` equals the text format on the serial golden files; the schema of every registered test type validates its own sample values; text-form types are `string`; golden schema files for a fixed test component set; a deterministic tool list (the same registry gives the same bytes).
- **Protocol.** Golden JSON for every message above. Framing: a 16 MiB line is accepted and one byte more is refused; random bytes and every truncation never panic; nesting over 128 is an error. Handshake: wrong token, wrong protocol version, no hello within 5 s, an HTTP-looking first line, a second client (`busy`), idle drop. The listener is loopback (the test reads the bound address). Requests are answered in order of submission, a `Wait` reply arrives later than a later `Done` reply.
- **Server and tools (headless app, std TCP client).** Every host tool through `tool.call`: scene save and load round trip (the second world's `write_scene` text equals the first's), unsaved-component warnings, `mark_member` on spawn and the Untitled scene, asset import and undo, `play_start`, `play_step`, `play_advance`, `play_stop` (the world equals the snapshot, new ids, selection remapped, history cleared), `render_screenshot` with a headless renderer (skipped without an adapter unless `EXITIUM_REQUIRE_GPU=1`, as render-v1.md), `log_tail`.
- **GUI without a GPU.** A headless `egui::Context` runs every panel against a test world and a recording `ActionSink`: widgets exist for every `TypeInfo` kind; `range` makes a slider; `hidden` skips; `derived` and `readonly` are disabled; the asset picker lists only the requested kind; an edit pushes exactly one action with the right arguments.
- **MCP crate.** A fake live server (a std TCP listener in the test) drives `exitium_mcp`: `initialize`, `tools/list` before and after connect, `list_changed` on a revision change, `tools/call` forwarding, error mapping with `isError`, an image block for a screenshot, `not_connected` with the hint, reconnect, ping. Ops tools against a tiny fixture project (a fake `cargo` shim, as the CLI tests do): argument validation, diagnostics parsing, `project_run` output capture and `project_stop`.
- **End to end (XT1).** A headless app with `LiveControlPlugin`, the app, ecs, scene and assets plugins and test components; the **`exitium_mcp` binary as a subprocess**, driven by a small MCP client in the test (JSON lines on its stdin and stdout, no SDK). The script: `initialize`, `tools/list`, `ecs_spawn_entity` (three entities, one with a parent), `ecs_set_field`, `command_undo` and `command_redo`, `scene_save`; a second app loads that file through the MCP binary and `scene_save`s again; the two scene texts are equal. Finally `play_start`, `play_advance`, `play_stop`. **Finding the binary:** `tests/common/mcp_bin.rs` uses, in order, the environment variable `EXITIUM_MCP_BIN`; else `current_exe()` (`target/<profile>/deps/<test>`), two parents up, plus `exitium_mcp` (`.exe` on Windows); else it runs `$CARGO build -p exitium_mcp` once (guarded by a `Once`, same profile and target directory) and looks again, and fails with the exact command to run if it is still missing. `CARGO_BIN_EXE_*` is not available because the binary belongs to another package, and artifact dependencies are unstable, so this adds no edge and needs no `xtask` change.
- **Parity (XT2).** As described above.
- **Platform.** `cargo build --workspace --target wasm32-unknown-unknown` covers `exitium_command` (it is not `NATIVE_ONLY`); `cargo xtask ci all` passes; the dependency tree check below.

## Dependencies and wiring (for intake 2)

Everything the implementation issues need from `Cargo.toml`, `Cargo.lock`, `deny.toml` and `xtask/`, so no issue below edits them (C5, C6). Versions were checked on crates.io on 2026-11-18. **Compatibility check:** a scratch crate outside the repo with the exact feature lists below (plus wgpu 30.0.2 with the render spec's features and winit 0.30.13) resolved to **one `wgpu` (30.0.2) and one `winit` (0.30.13)** and passed `cargo check` on `x86_64-pc-windows-msvc`; the 334 packages of its full metadata (all targets) were checked against the `deny.toml` allow list and none was outside it. `cargo deny` itself was not available, so intake 2 runs it and reports `multiple-versions` warnings (the scratch tree has two each of `hashbrown`, `miniz_oxide`, `rustc-hash`, `syn` and `windows-sys`). `egui`, `egui-wgpu` and `egui-winit` 0.37.0 need Rust 1.95 and `rmcp` 1.88; the pinned toolchain is 1.100.0. The editor is native only, so no wasm check applies to it; `exitium_command` adds only `serde` and `serde_json`, both wasm-clean.

| Crate | Version | Features | Used by | Licence | wasm32 | Why not std |
|---|---|---|---|---|---|---|
| `egui` | 0.37.0 | `default-features = false` (**no `default_fonts`**) | `exitium_editor` | MIT OR Apache-2.0 | not needed (native only) | An immediate-mode GUI with text layout and widgets is the editor. Alternatives (iced, slint) need their own renderer and a larger design. No `default_fonts`: `epaint_default_fonts` is `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0`, which `deny.toml` rejects. |
| `egui-wgpu` | 0.37.0 | `default-features = false` (the defaults enable `wgpu/webgl` and `macos-window-resize-jitter-fix`; we want neither) | `exitium_editor` | MIT OR Apache-2.0 | not needed | Paints egui with the game's wgpu device. Needs `wgpu ^30.0`, which is the render spec's 30.0.2. |
| `egui-winit` | 0.37.0 | `default-features = false`, `["x11", "clipboard"]` | `exitium_editor` | MIT OR Apache-2.0 | not needed | Turns winit events into egui input. Needs `winit ^0.30.13`, the workspace entry. `clipboard` pulls `arboard` and, on Windows, `image` 0.25 (`png`, `bmp`); it is there for pasting into the inspector. If `cargo deny` or review rejects that subtree, drop `clipboard` (open question 3). `links` and `accesskit` stay off. |
| `winit` | the workspace entry (0.30.13) | as declared by app-platform.md | `exitium_editor` (also reachable as `egui_winit::winit`) | Apache-2.0 | n/a | The editor names `WindowEvent`; one version. No `Cargo.lock` change. |
| `rmcp` | 3.11.0 | `default-features = false`, `["server", "transport-io"]` (no `macros`: tools are dynamic) | `exitium_mcp` | Apache-2.0 | not needed | The MCP protocol, versions and stdio framing (D7). Pulls `schemars`, `chrono`, `uuid`, `sha2`, `indexmap`, `futures` and others, in the MCP binary only. |
| `tokio` | 1.54.1 | `default-features = false`, `["rt", "macros", "sync", "time", "net", "io-util", "io-std", "process"]` | `exitium_mcp` | MIT | not needed | `rmcp` is async; ops tools spawn child processes with timeouts. Current-thread runtime, no `rt-multi-thread`. |
| `serde` | 1.0.229 | `derive` | `exitium_command` (protocol types), `exitium_mcp` | MIT OR Apache-2.0 | yes | Wire structs with unknown-field tolerance. |
| `serde_json` | 1.0.154 | default | `exitium_command`, `exitium_editor`, `exitium_mcp` | MIT OR Apache-2.0 | yes | A correct JSON parser and writer is high-risk to hand-write; it is already in `rmcp`'s tree. Hidden behind `RawJson` and the `json` module so game code never sees it. The reflect and serial crates stay free of serde. |
| `tracing-subscriber` | 0.3.23 | `default-features = false`, `["std", "registry", "tracing-log"]` | `exitium_editor` | MIT | not needed | A `Layer` for the console, and the `log` bridge so wgpu's and egui's records land. |
| `getrandom` | 0.4.3 | default | `exitium_editor` | MIT OR Apache-2.0 | not needed | The session token needs OS randomness; std has none. (assets-scene-export.md's "not added" list means the runtime crates; the editor is native only.) |
| `tracing` | the workspace entry (0.1.44), feature `std` | | `exitium_editor` | MIT | | Already declared. |
| `criterion` | the workspace entry | dev | `exitium_command`, `exitium_editor` | MIT OR Apache-2.0 | n/a | Benchmarks. |

Not added, on purpose: `egui_dock` and `eframe` (no docking, no second window loop), `tungstenite` or an HTTP server (std TCP is enough), `base64` (a 25-line encoder with RFC 4648 vectors, in `json.rs`), `png` or `image` as a direct dependency (the editor calls `exitium_assets::image::encode_png`), `tempfile` and any MCP client for the tests (std, and a 60-line JSON-lines client), `clap` (the MCP binary takes two flags).

**Crate edges and manifest wiring** (intake 2): the edges in "Crate map and dependency graph"; `crates/exitium_command/` created (it does not exist yet) with `CLAUDE.md`, `[dependencies]` core, ecs, reflect, serial, serde, serde_json, dev `criterion` and the `[[bench]] name = "bus"`; `exitium_editor` and `exitium_mcp` `Cargo.toml` dependency tables as in the table above and the graph; `crates/exitium_mcp/src/main.rs` declared as the `exitium_mcp` binary (the library stays); the optional `exitium_command` dependency of `exitium_physics` and `exitium_ui`, plus a **dev-dependency on `exitium_command`** in both (their tests round-trip each command through its reflected schema, P15 and U13); facade: `exitium -> exitium_command` and `exitium -> exitium_editor` as an optional dependency with `[features] editor = ["dep:exitium_editor"]`.

**Other intake 2 edits this spec asks for:**
- `xtask/src/ci.rs`: add `exitium_mcp` to `NATIVE_ONLY`; extend `FEATURE_CHECKS` with a native-only list so `("exitium", "editor")` runs clippy and test on the host and is **not** built for wasm32 (today `wasm()` builds every `FEATURE_CHECKS` entry for wasm32). Without it the facade's `editor` code is never compiled in CI.
- `crates/exitium_reflect_derive/`: add `exitium_command` and `exitium_editor` to `ENGINE_CRATES` (both derive `Reflect` and depend on `exitium_reflect` directly, so the `crate_paths.rs` drift test fails until they are classified) and `exitium_mcp` to `NOT_DERIVE_USERS`. Intake 2 lists the derive crate under `Edits` for this, as assets-scene-export.md does.
- `deny.toml`: no change expected; intake runs `cargo deny check`.

## Open questions

1. **Edit mode and `Update` (X3).** The frozen-mode amendment is proposed; the owner may prefer "game code must tolerate Edit" (no amendment). Recommendation: amend; it is small and Unity-like.
2. **The editor font and its licence (X11).** The editor needs one embedded font. Recommendation: a font under Apache-2.0, or OFL-1.1 with its licence file beside it, picked by the owner; an OFL font makes the ED8 PR a licensing high-risk change. It could also become the engine's default font (render-v1.md open question 7).
3. **`clipboard` and its `image` subtree.** `egui-winit`'s `clipboard` feature adds `arboard` and, on Windows, the `image` crate (`png`, `bmp`). The tree passes the licence list, but `image` is a large crate the assets spec chose not to depend on. Recommendation: keep for the MVP (humans paste text forms), drop if review objects.
4. **A game view in the editor, and pointer mapping.** The viewport shows only the scene view; the game's cameras draw under the panels (or not at all, with `draw_cameras_to_surface = false`). A game view needs `Camera2d` to target a texture and pointer coordinates mapped through the panel's rectangle. Recommendation: after the MVP, as a render amendment.
5. **Docking and saved layouts.** Cut. `egui_dock` is a dependency and a design of its own. Recommendation: epic #6.
6. **Tool count and context cost.** About 80 tools is about 25,000 tokens of schema in a client that loads them all. Mitigations built in: names-not-types schemas and `--groups`. If it is still too much, a two-level scheme (a `tool_search` meta tool that lists names and a `tool_call` that invokes one) is additive, because the tool table already lives in the MCP crate.
7. **`rmcp` risk.** `rmcp` is 3.x and moves with the MCP spec. MC1 begins with a 100-line spike of the two handler methods and `list_changed`; if the SDK cannot do dynamic tools, structured content or progress cleanly, the fallback is the hand-written server named in D7, and only `exitium_mcp` changes.
8. **A minimized or occluded window.** Frames may stop, so a request waits. AM2 adds the wake handle; XT1 cannot test it (headless). A manual check on Windows is recorded in the ED2 PR.
9. **History after Stop and scene Replace.** Cleared. A smarter remap (old entity to new by label) would keep it. Recommendation: not in the MVP.
10. **More than one client.** One at a time. A second tool (a profiler, a test harness) may want to connect while the MCP server holds the slot. Recommendation: wait for a need.
11. **`play_snapshot` opt-in versus opt-out.** Opt-in is safe (engine state is never rewound) but a game author must mark their resources. If agents forget, `play_stop` leaves game state dirty. Recommendation: keep opt-in, and have `play_start` return the list of reflected resources it will not restore so the agent sees it.
12. **Headless env var.** `EXITIUM_HEADLESS` is read by `DefaultPlugins` (facade). If `DefaultPlugins` should stay free of environment reads, the game's `main` reads it instead and the template does. Recommendation: the facade, so `exitium new` games get it free.

## Implementation issues

Filed by the architect after this spec merges, into the Jam MVP milestone, with the `file-issue` skill. Every issue edits one crate (the facade issue edits none and holds `crates/exitium/` as a hotspot, C1), is about 600 changed lines or fewer, has acceptance criteria, `Blocked by` lines and the full labelled Footprint. Labels: `area:editor` for the command crate and the editor lane (there is no `area:command` label; the bus is Phase 4 work), `area:mcp`, `area:facade`; role `impl-editor-tools` for every issue except the facade issue, which takes the role the facade chain uses. Every issue is `Blocked by <intake-2>` (C6), and an issue in a lane is `Blocked by` the one before it. `<intake-2>` is the Stage 2 to 4 intake issue. Predecessors from other specs are named by their spec's id (for example "app A2" is `app-platform.md` A2, "assets AP3" is `assets-scene-export.md` AP3). Lane order, R3 and R4: the command chain runs alone first; the editor and MCP lanes then run side by side; AM1 to AM3 run beside the command chain.

**Common acceptance criteria for every issue:** `cargo xtask ci all` passes; new behaviour has tests; each hot path ships its benchmark with the numbers in the PR body; every module has `//!` docs and the crate `CLAUDE.md` is updated; no `unsafe`; the PR Risk section names the high-risk areas it touches (new dependency use, the protocol, the localhost server, the facade API).

### Stage 3: the command crate (`exitium_command`, Footprint: Edits `crates/exitium_command/`; Governs none; Hotspots none; New crate edges none)

**CB1. `feat(command): Command, Query, Step, the registry and errors`** (about 450 lines). Blocked by `<intake-2>`, #50 (`ReflectComponent`), ecs-scheduler.md items 12 and 13 (`current_system`, `ReflectResource`, `ReflectEvent`).
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `command.rs`, `registry.rs`, `error.rs`: the traits, `Step`, `Waiter`, `UndoKind`, `register_command`, `register_query`, `ToolInfo`, the registry `revision`, `CommandError` and `ErrorCode`; a test command and query.
- Acceptance criteria:
  - [ ] A duplicate name is an error naming both types; a `Wait` returned by an undoable command is answered `internal` (a test per registered command).
  - [ ] The registry registers the input and output types in the `TypeRegistry`; builds for wasm32 with no `cfg`.
  - [ ] `CLAUDE.md` created from the intake stub states the crate's rules (reflection only, no GUI types, no socket).

**CB2. `feat(command): the bus (submit, undo, redo, journal)`** (about 550 lines). Blocked by `<intake-2>`, CB1.
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `bus.rs`: `CommandBus`, `submit`, `submit_by_name`, `enqueue`, `undo`, `redo`, history caps, the journal, `Origin`, the `InSchedule` guard, the debug check that a failed command and a query change nothing, panic capture; `exitium_command/bus/*` benchmarks.
- Acceptance criteria:
  - [ ] The bus tests of the Test plan that need no built-in command: undo and redo order, journal-only commands, redo stack rules, `InSchedule`, caps.
  - [ ] `set_field`-shaped bench meets its budget; a panicking test command clears the history and returns `internal`.

**CB3. `feat(command): gestures, batches and entity remap`** (about 450 lines). Blocked by `<intake-2>`, CB2.
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `merge` through `GestureId`, `batch` (atomic and not, `rollback_failed`), `EntityRemap`, `remap_reflected` and `remap_value`.
- Acceptance criteria:
  - [ ] A gesture folds consecutive merging commands into one undo step; commands without a gesture never merge.
  - [ ] An atomic batch undoes its prefix on error, in reverse; a failing undo reports `rollback_failed` with what is left.
  - [ ] A remap rewrites `Entity` values in nested reflected fields and in skipped undo state through `remap`.

**CB4. `feat(command): SetField, InsertComponent and RemoveComponent`** (about 550 lines). Blocked by `<intake-2>`, CB3, #50, reflect #32 (path access).
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `builtin/entity.rs` (part one), `RawJson` as a hand-written reflect `Value` kind, `SpawnHooks`, `CommandConfig`, `register_builtin`.
- Acceptance criteria:
  - [ ] Path and value are bound before any write; a rejected `ecs_set_field` leaves `Changed<T>` false and an accepted one sets it once.
  - [ ] Undo restores the previous field value, or removes an inserted component; `ecs_insert_component` with an empty value uses `ReflectDefault`.
  - [ ] Merging within a gesture; the schema of each command is derived from reflection (checked once CB8 lands).

**CB5. `feat(command): SpawnEntity, DespawnEntity and Reparent`** (about 600 lines). Blocked by `<intake-2>`, CB4, #49 (hierarchy), #41 (`world.spawn(())`).
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `builtin/entity.rs` (part two): spawn with components, parent and hooks; recursive despawn with a reflected subtree snapshot and the unreflected-component warning; reparent with a cycle check; all through the `run_system_once` helpers.
- Acceptance criteria:
  - [ ] Despawn then undo restores the subtree (components, parents, sibling order) with new ids, and a later command on an old id works through the remap.
  - [ ] `SpawnHooks` run once per spawned entity; `CommandConfig.internal_components` silences the warning for listed types.
  - [ ] `Parent` and `Children` stay consistent after every command and every undo (a property test).

**CB6. `feat(command): resource and event commands`** (about 350 lines). Blocked by `<intake-2>`, CB5, ecs-scheduler.md item 13.
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `builtin/resource.rs`: `ecs_insert_resource`, `ecs_remove_resource`, `ecs_set_resource_field` (journal-only for `custom(history = false)`), `ecs_send_event`.
- Acceptance criteria:
  - [ ] Resource writes follow the set-field rules (validate first, flag only on write); undo restores the previous value or removal.
  - [ ] `ecs_send_event` is `NotUndoable` and journalled; an unregistered event is `unknown_type`.

**CB7. `feat(command): JSON mapping for reflected values`** (about 600 lines). Blocked by `<intake-2>`, CB6, the Stage 1 serial lane (serial-text-format.md S1 to S12, for the `leaf` helpers and the golden files) and reflect #37.
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `json.rs`: reflected value to JSON and JSON to a reflected value (type-directed, up to 16 collected errors with paths), `RawJson` parse and write, the base64 encoder; `exitium_command/json/*` benchmarks.
- Acceptance criteria:
  - [ ] Every row of the serial JSON mapping table, both directions, and the `JSON -> reflect -> text` equality on the serial golden files.
  - [ ] `i64` beyond 2^53 as a string and a number both read; `nan` and `inf` strings for plain floats; `Num` rejects NaN; a `NonZeroU32` rejects 0.
  - [ ] Base64 passes the RFC 4648 vectors; no panic on any truncation of a valid document.

**CB8. `feat(command): JSON Schema from TypeInfo and the tool list`** (about 500 lines). Blocked by `<intake-2>`, CB7.
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `schema.rs`: JSON Schema 2020-12 from `TypeInfo` and attributes, `ToolDef` list from the registry with the names-not-types rule, schema revision, caching; `exitium_command/schema/build_tools`.
- Acceptance criteria:
  - [ ] Golden schemas for a fixed test set; every registered type's schema accepts its own serialized sample; text-form types are `string`.
  - [ ] The tool list is byte-identical for the same registry; the budget rows for a 90-tool registry hold.

**CB9. `feat(command): wire protocol types and framing`** (about 450 lines). Blocked by `<intake-2>`, CB8.
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `protocol.rs`: the message structs, `PROTOCOL_VERSION`, the constants, the error codes, the NDJSON codec with the frame limit and nesting limit; no ecs or reflect type in the module.
- Acceptance criteria:
  - [ ] Golden JSON for every message of "The live-control protocol"; unknown fields and notifications are ignored.
  - [ ] A 16 MiB line passes and one byte more is refused; random bytes and truncations never panic.

**CB10. `feat(command): entity and component queries`** (about 600 lines). Blocked by `<intake-2>`, CB9, and the ECS lane (#42 queries and `query_dynamic`, #43 change detection, #50).
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `builtin/queries.rs` (part one): `ecs_get_stats`, `ecs_list_components`, `ecs_list_archetypes`, `ecs_query_entities`, `ecs_get_entity`, `ecs_get_component`.
- Acceptance criteria:
  - [ ] Paging is by entity index and weakly consistent as specified; the four typed errors of `ecs_query_entities` are returned by code.
  - [ ] Every query leaves the change cursor unchanged; a page of 256 meets its budget.

**CB11. `feat(command): introspection, reflect and text queries`** (about 550 lines). Blocked by `<intake-2>`, CB10, ecs-scheduler.md item 14, serial S12 (#55).
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `builtin/reflect.rs` and the rest of `queries.rs`: schedules, resources, events; `reflect_*`; `serial_parse_text` and `serial_write_text`.
- Acceptance criteria:
  - [ ] Each tool returns data that equals the Rust API's, as JSON (a test per tool); `serial_parse_text` returns the error JSON shape of serial-text-format.md.
  - [ ] `register_builtin` lists every tool of the table "Built-in commands and queries" (a test compares the name list).

### Stage 3: the editor lane (`exitium_editor`, Footprint: Edits `crates/exitium_editor/`; Governs none; Hotspots none; New crate edges none)

**ED1. `feat(editor): library layout and the live-control server`** (about 550 lines). Blocked by `<intake-2>`, CB9.
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Removes the `main.rs` stub, adds `lib.rs`, `server/`: the loopback listener, the token and `live.json`, the handshake rules, reader and writer threads, the bounded queue, the HTTP-prefix rejection, `busy`, idle drop; `CLAUDE.md`.
- Acceptance criteria:
  - [ ] The handshake and security tests of the Test plan, with a std TCP client; the bound address is `127.0.0.1`; the token never appears in a log.
  - [ ] `server/decode_dispatch` benchmark; the server owns no world reference (a compile-time property: the module imports no `exitium_ecs` type).

**ED2. `feat(editor): LiveControlPlugin, dispatch and the headless runner`** (about 550 lines). Blocked by `<intake-2>`, ED1, CB11, app A1 and A2 (`add_boundary_hook`, `run_frame`).
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `live.rs`: the plugin, the boundary hook, `hello`, `schema.get`, `tool.call` through `submit_by_name`, `Wait` polling with deadlines, the request budget, `notify.schema_changed`, `register_builtin`, `run_headless`.
- Acceptance criteria:
  - [ ] A headless app answers `schema.get` and a spawn then set-field then undo sequence over TCP; replies keep request order and a `Wait` reply may arrive late.
  - [ ] A request over the 4 ms budget leaves the rest for the next frame; the idle hook meets 15 us.
  - [ ] The manual minimized-window check is recorded in the PR (open question 8).

**ED3. `feat(editor): log capture, editor state and selection tools`** (about 400 lines). Blocked by `<intake-2>`, ED2.
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `console.rs`, the `EditorState` resource, `log_tail`, `editor_get_state`, `editor_select`.
- Acceptance criteria:
  - [ ] `tracing` events and `log` records land in the ring buffer; an existing global subscriber is left alone and `log_tail` says so.
  - [ ] `editor_select` is journal-only; `EditorState` round-trips through the text format.

**ED4. `feat(editor): scene tools`** (about 600 lines). Blocked by `<intake-2>`, ED3, scene SC5, assets AP3.
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `tools/scene.rs`: `scene_new`, `scene_load`, `scene_save`, `scene_list`, `scene_validate`; the `mark_member` spawn hook and the Untitled scene; dirty tracking; history clearing on Replace.
- Acceptance criteria:
  - [ ] A scene saved and loaded into a second world writes identical text; unsaved-component warnings are returned; `ecs_spawn_entity` entities are saved.
  - [ ] Additive load undoes by despawn; Replace undo reloads the previous text.

**ED5. `feat(editor): asset tools`** (about 500 lines). Blocked by `<intake-2>`, ED4, assets AP3.
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `tools/assets.rs`: the seven asset tools wrapping `edit::*`, the `AssetEdit` undo records, path rules.
- Acceptance criteria:
  - [ ] Import copies and registers, remove moves to `.trash/`, each undo restores file and entry; `asset_remove` lists scenes that still reference the `Guid`.
  - [ ] Paths with `..` or outside `assets/` are refused.

**ED6. `feat(editor): play, pause, step, advance and stop`** (about 600 lines). Blocked by `<intake-2>`, ED4, app A2.
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `tools/play.rs`: the three states, the snapshot (scene text, `play_snapshot` resources, live set), `play_*` including the `Wait` of `play_advance`, restore on stop, history clearing, `start_mode`.
- Acceptance criteria:
  - [ ] After Stop the scene equals the snapshot (new ids), spawned entities are gone, opted-in resources are restored, selection is remapped.
  - [ ] `play_step` runs exactly the requested ticks; `play_advance` replies after the ticks and times out with `timeout`.
  - [ ] The documented limits (non-member entities, `UiTree`) are in the crate `CLAUDE.md`.

**ED7. `feat(editor): screenshot, picking and coordinate tools`** (about 450 lines). Blocked by `<intake-2>`, ED3, render I17, assets A2 (`encode_png`).
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `tools/render.rs`: `render_screenshot` (a `Wait`, worker-thread PNG and base64, size and 8 MiB rules), `render_screen_to_world`, `render_world_to_screen`, `editor_pick`.
- Acceptance criteria:
  - [ ] A headless renderer screenshot returns a decodable PNG of the requested size (skipped without an adapter unless `EXITIUM_REQUIRE_GPU=1`); a size over 2048 is refused; main-thread cost meets its budget.
  - [ ] The coordinate tools agree with `CameraView` and with each other; `editor_pick` returns the topmost sprite.

**ED8. `feat(editor): egui shell, input and the surface overlay`** (about 600 lines). Blocked by `<intake-2>`, ED2, AM1, AM2, app A2.
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `gui/mod.rs`, `gui/actions.rs`, `gui/viewport.rs`, the embedded font: the egui context and `egui_winit::State`, the raw-event hook handle, the editor boundary hook, the `SurfaceOverlay` that paints through `egui_wgpu::Renderer`, the menu bar and toolbar, the `ActionSink` and `EDITOR_ACTIONS`, the viewport with the scene view texture, pan and zoom, and picking.
- Acceptance criteria:
  - [ ] The shell opens on Windows with the scene view visible; the font's licence file is beside it and is Apache-2.0, MIT or OFL-1.1 (an OFL font makes the PR a licensing high-risk change).
  - [ ] GUI code never names `&mut World` (a unit test greps the module sources, plus the type-level split); the recorded manual run is in the PR.
  - [ ] The overlay costs at most 1.0 ms CPU on a typical frame.

**ED9. `feat(editor): reflection to widgets`** (about 600 lines). Blocked by `<intake-2>`, ED8, render RA1, ui UA1 (the asset-picker attribute).
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `gui/widgets.rs`: widgets for every `TypeInfo` kind with `range`, `hidden`, `tooltip`, `derived`, `readonly`, `custom(widget)`, the colour picker, the asset picker, the text-form field, gesture ids for drags; each edit is one `ActionSink` push.
- Acceptance criteria:
  - [ ] The GUI-without-a-GPU tests of the Test plan for every kind; the asset picker lists only the requested kind.
  - [ ] `exitium_editor/frame/inspector_10_components` meets its budget.

**ED10. `feat(editor): hierarchy and inspector panels`** (about 600 lines). Blocked by `<intake-2>`, ED9.
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `gui/hierarchy.rs`, `gui/inspector.rs`: the tree, selection, rename, create, delete, drag to reparent; the Entity and Resources tabs, add and remove component, copy as text and paste.
- Acceptance criteria:
  - [ ] Each interaction pushes the action row declared in `EDITOR_ACTIONS` with the right arguments; the hierarchy is virtualized and meets its 2,000-entity budget.
  - [ ] Paste applies through `apply_text` and `ecs_set_field`; a parse error shows its line and column.

**ED11. `feat(editor): console, history, assets, systems and play panels`** (about 600 lines). Blocked by `<intake-2>`, ED10, ED5, ED6.
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `gui/panels.rs`: Console, History, Assets (list, import field, settings, rename, remove, thumbnails), the Systems tab, the scene menu wiring, the play toolbar wiring, the dirty marker; the remaining `EDITOR_ACTIONS` rows.
- Acceptance criteria:
  - [ ] Every menu item and button has its `EDITOR_ACTIONS` row; the History panel undoes and redoes through `command_undo` and `command_redo`.
  - [ ] Thumbnails register through `egui-wgpu` on the renderer's device.

### Stage 3: the MCP lane (`exitium_mcp`, Footprint: Edits `crates/exitium_mcp/`; Governs none; Hotspots none; New crate edges none)

**MC1. `feat(mcp): stdio MCP server skeleton on rmcp`** (about 450 lines). Blocked by `<intake-2>`, CB9.
- Footprint:
  - Edits: `crates/exitium_mcp/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds the `exitium_mcp` binary and library: the `rmcp` `ServerHandler` with a dynamic tool table, `initialize`, `instructions`, `list_tools`, `call_tool` routing, stdout-only-frames; the spike notes in `CLAUDE.md`; `CLAUDE.md`.
- Acceptance criteria:
  - [ ] A test MCP client (JSON lines) gets `initialize` and `tools/list`; nothing but frames reaches stdout; the binary exits on stdin EOF.
  - [ ] The spike result is in the PR: dynamic tools, `list_changed` and an image block work, or the fallback decision is recorded (open question 7).

**MC2. `feat(mcp): live-control client and tool forwarding`** (about 600 lines). Blocked by `<intake-2>`, MC1, CB11.
- Footprint:
  - Edits: `crates/exitium_mcp/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `live.rs`: discovery, connect and `hello`, request correlation, ping, reconnect, `schema.get` into the tool table, `tools/list_changed`, `tools/call` forwarding, error mapping with `isError`, the image block, `not_connected` with its hint, `--groups`.
- Acceptance criteria:
  - [ ] The MCP-crate tests of the Test plan against a fake live server, including a revision change and a screenshot.
  - [ ] The crate depends on `exitium_command` only (a test reads `Cargo.toml`); a protocol-version mismatch is reported with both numbers.

**MC3. `feat(mcp): ops tools (build, test, run, stop, status)`** (about 550 lines). Blocked by `<intake-2>`, MC2.
- Footprint:
  - Edits: `crates/exitium_mcp/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `ops/`: the five tools, argument validation, diagnostics parsing, child process management and output capture.
- Acceptance criteria:
  - [ ] Against a fixture project with a fake `cargo` shim: diagnostics are parsed, `project_run` waits for `live.json` and reports connected, `project_stop` ends the child, output tail is bounded.
  - [ ] Invalid features and an `editor` feature in an export build are refused.

**MC4. `feat(mcp): project_new and export tools`** (about 400 lines; Stage 4). Blocked by `<intake-2>`, MC3, assets C3 and C4 (both export PRs).
- Footprint:
  - Edits: `crates/exitium_mcp/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `ops/project.rs`: `project_new`, `project_export_check`, `project_export`; locating the `exitium` binary; progress notifications; parsing the CLI's `--json`.
- Acceptance criteria:
  - [ ] Against a fake `exitium` binary: arguments, progress lines, the final JSON and a failure exit are mapped to tool results.
  - [ ] No edge from `exitium_mcp` to `exitium_cli`.

### Stage 3: facade hook and tests

**FH1. `feat(exitium): the editor feature and headless mode`** (about 250 lines; high-risk: facade API). Blocked by `<intake-2>`, ED8, ED11, ED6, AM1, AM2, and the facade chain (the facade runs alone).
- Footprint:
  - Edits: none
  - Governs: none
  - Hotspots: `crates/exitium/` (the facade)
  - New crate edges: none (`exitium -> exitium_editor` and `exitium -> exitium_command` are in intake 2)
- Adds, under `#[cfg(feature = "editor")]`: `EditorPlugin` in `DefaultPlugins`, the raw-event hook wiring on `PlatformPlugin`, `CommandConfig.internal_components` from the scene glue's `INTERNAL_COMPONENTS`, the `exitium::editor` and `exitium::command` re-exports, and the `EXITIUM_HEADLESS` mode (headless render, no platform, `run_headless`).
- Acceptance criteria:
  - [ ] `cargo run --features editor` on the sample game opens the editor with the scene loaded and the server listening; without the feature, nothing of the editor is compiled or linked.
  - [ ] `EXITIUM_HEADLESS=1` runs with no window and answers `schema.get`; the facade builds for wasm32 with `editor` off.

**XT1. `test(editor): headless end-to-end through the MCP subprocess`** (about 500 lines). Edits one crate. Blocked by `<intake-2>`, ED6, ED7, MC3.
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `tests/e2e.rs` and `tests/common/mcp_bin.rs`: the script and the binary lookup of the Test plan.
- Acceptance criteria:
  - [ ] An MCP client builds a scene (spawn, set, undo, redo, save), a second app loads it and saves identical text, and the play cycle restores the world.
  - [ ] The test finds the binary by the three rules and fails with the exact build command when it cannot; the editor crate has no dependency or dev-dependency on `exitium_mcp`.

**XT2. `test(editor): the parity test`** (about 450 lines). Blocked by `<intake-2>`, XT1, ED11.
- Footprint:
  - Edits: `crates/exitium_editor/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds `tests/parity.rs` as in "Parity test".
- Acceptance criteria:
  - [ ] Every `EDITOR_ACTIONS` row passes checks (a) and (b); every registry command is in a row or in `AGENT_ONLY` with a reason.
  - [ ] Removing a registered tool, or adding a row naming a missing tool, fails the test (shown once in the PR).

## Downstream adjustments

Applied by the architect when this spec merges. The amendment issues **AM1 to AM4, AM7 and AM8** are filed as issues (`status:needs-approval`, one crate or one spec file each, into the Jam MVP milestone) with the Footprints below. AM5, AM6 and the bullets after them edit existing issue bodies. A spec-amendment issue has `Governs` set to the crate it decides, so it never runs beside work on that crate (`factory-run`, Footprint).

**AM1. render: the surface overlay (X1).** Blocks ED8 and FH1. Runs beside the command chain.
- `docs(spec): amend render v1 for the editor surface overlay`. Blocked by render I16 and I17.
  - Edits: `docs/specs/render-v1.md`
  - Governs: `crates/exitium_render/`
  - Hotspots: none
  - New crate edges: none
- `feat(render): SurfaceOverlay, set_overlay and draw_cameras_to_surface` (the sketch above, plus `RenderSettings.draw_cameras_to_surface`). Blocked by `<intake-2>`, the amendment above. Acceptance: with an overlay set, `render` acquires the surface even when every pass targets a texture, and calls the overlay once after the last camera pass, before submit; with the overlay unset nothing changes (the render benchmarks hold); `draw_cameras_to_surface = false` skips surface passes and still serves screenshots.
  - Edits: `crates/exitium_render/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none

**AM2 and AM3. app and platform: editor hooks (X2) and a frozen mode (X3).** One spec amendment for both, then two implementations.
- `docs(spec): amend app-platform for the editor hooks and SimulationMode`. Decides: `raw_event` receives every `WindowEvent` and consumption applies only to keyboard, mouse and cursor events; the exact `PlatformPlugin::with_raw_event_hook` signature (`FnMut(&PlatformWindow, &WindowEvent) -> bool + 'static`); a wake handle so the server can request a frame; the reflected `SimulationMode { Running, Frozen }`, where Frozen skips the tick loop and `Update` and still runs `First`, `PostUpdate`, `Extract` and `Last`.
  - Edits: `docs/specs/app-platform.md`
  - Governs: `crates/exitium_platform/, crates/exitium_app/`
  - Hotspots: none
  - New crate edges: none
- `feat(platform): raw_event for every WindowEvent, the hook signature and a wake handle` (blocks ED8 and FH1). Blocked by `<intake-2>`, the amendment above, app-platform.md PP2 (the windowed runner and `WindowHandler`). Acceptance: the pure translation tests still pass; a test handler sees focus, IME, modifier and scale events through `raw_event`; a consumed keyboard or pointer event never reaches `InputState`.
  - Edits: `crates/exitium_platform/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- `feat(app): SimulationMode in the frame driver` (not blocking; ED6 uses it when present). Blocked by `<intake-2>`, the amendment above, app A2. Acceptance: Frozen runs no tick and no `Update`; `First`, `PostUpdate`, `Extract` and `Last` still run; `SimulationMode` is reflected; the frame-driver benchmark holds.
  - Edits: `crates/exitium_app/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none

**AM4. attributes (X4).** Not blocking. One small issue per crate (`exitium_app`, `exitium_render`, `exitium_physics`, `exitium_audio`, `exitium_assets`, `exitium_platform`), each `Blocked by` `<intake-2>` and the issue that creates the types. It adds `custom(readonly = true)` to the status and info types of that crate (`Time`, `FrameStats`, `CursorWorld`; `RenderStats`, `CameraViews` if reflected; `PhysicsStats`; `AudioStats`; `AssetStatus`; `WindowInfo`, `InputSnapshot`) and, in render, `custom(history = false)` on `SceneView`. Acceptance: each type's `TypeInfo` carries the attribute and its schema hash is unchanged (attributes are not hashed).
  - Edits: `crates/exitium_<crate>/` (one issue per crate named above)
  - Governs: none
  - Hotspots: none
  - New crate edges: none

**AM7. CLI template (X10).** The template `.gitignore` gains `/.exitium`, and `exitium new` writes a `.mcp.json` that starts `exitium_mcp --project .` from the engine checkout; the template `CLAUDE.md` documents `custom(play_snapshot = true)`. Blocked by `<intake-2>` and assets C1.
  - Edits: `crates/exitium_cli/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none

**AM8. jam-mvp.md.** Stage 3's counts (about 15 PRs, critical path 11) are replaced by this spec's list; its line "the live-control server is the only extra thread" is refined to the threads in "Threading"; its known-wiring line for `exitium_command` gains `exitium_serial`.
  - Edits: `docs/specs/jam-mvp.md`
  - Governs: none
  - Hotspots: none
  - New crate edges: none

**Edits to existing issues and specs (no new issue).**
- **AM5. physics P15 and P16 (X5).** P15 drops `SetPhysicsConfig` and `SetDebugDraw` and registers only `physics_apply_impulse` (`NotUndoable`); P16 registers the five queries under the names in the tool table. Both are `Blocked by` CB8 (their round-trip tests use the schema API) and by the dev-dependency edge from intake 2.
- **AM6. ui U13.** `Blocked by` CB8; registers the commands and queries under the `ui_*` names with `register_command` and `register_query`; `ui_click` is `NotUndoable`; `ui_get_tree { fresh }` refreshes layout only through the non-stamping path.
- **ecs-scheduler.md and ecs-storage-queries.md.** The commands they list "for the bus" are CB4 to CB6 and the read tools are CB10 and CB11. #50 and ecs-scheduler.md item 13 are real predecessors of CB1.
- **assets-scene-export.md.** The `audio_*` tools are superseded (X6); `asset_*` and `scene_*` are ED4 and ED5; the "no undo" declaration is `UndoKind`; `mark_member` is the `SpawnHooks` entry; ED9 needs RA1 and UA1.
- **app-platform.md.** `app_pause`, `app_resume`, `app_step` and `input_inject` are superseded (X6); the editor reads `PrimaryWindow` and `WindowInfo` and installs the hook through `PlatformPlugin`.
- **physics-2d.md and ui-mvp.md.** Their open questions on the bus (5 and 7) are answered above.
- **intake 2.** Adds the list in "Dependencies and wiring", including `exitium_mcp` in `NATIVE_ONLY`, the native-only feature check for `exitium/editor`, and the `exitium_reflect_derive` edits.
