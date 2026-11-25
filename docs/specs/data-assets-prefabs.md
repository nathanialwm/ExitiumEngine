# Spec: Data assets and prefab variants

- Issue: #146 (roadmap SP10)
- Crates: `exitium_data`, `exitium_scene` (a one-row change in `exitium_assets` and the glue in `exitium` are listed under "Implementation issues")
- Status: draft

## Goal

Give games two kinds of authored, inheritable content. A **data asset** is a typed, reflected value in its own `.data` file (an item, an enemy stat block, a level rule set), and a **variant** of it stores only the fields that differ from a **base** data asset. A **prefab** is a reusable tree of entities in a `.prefab` file, and a **prefab variant** stores only the nodes, components and fields that differ from a base prefab. A scene places prefab instances and stores only what the instance changes. Both kinds share one "base plus overrides" engine, so a field override means the same thing in both. Both follow SP4's reload rules (a failed reload keeps the last good value; a reload goes through the same path as a first load), and both are reachable by an AI agent through MCP with the same operations a human has in the editor.

## Non-goals

- Typed handles (`DataRef<T>`, `AssetRef<T>`). Fields stay plain `Guid`s (SP4 D1). Open question 8.
- **Nested prefabs** (a prefab that contains another prefab instance). The format reserves the field so adding them later changes no file. Open question 2.
- Element-level overrides of lists and maps. A list or map field is replaced whole. Open question 3.
- Rebasing a variant onto a different base, and reparenting a base node inside a variant. Use `flatten` plus re-create. Open questions 4 and 5.
- Runtime-defined data types (a type created in the editor without Rust code). Reflect open question 9 is answered here: not in the MVP. Open question 9.
- Live auto-update of already spawned prefab instances when a prefab changes. It is an explicit `prefab_refresh` (open question 6).
- Binary form of data assets. `.data` and `.prefab` are text; the binary container (serial-binary-format.md) stays the save system's.
- A `Plugin` in `exitium_data` or `exitium_scene`. Both crates have no `exitium_app` edge (SP4 D7); the facade glue wires them.
- Any change to `exitium_serial`, `exitium_reflect` or `exitium_ecs`. This spec only uses what their specs already provide.

## Design

### Decisions at a glance

| # | Decision | Why |
|---|---|---|
| D1 | **E1 (`exitium_scene -> exitium_data`) stands.** The shared "base plus overrides" engine is the module `exitium_data::overlay`, not a foundation crate. The edge is **not** in intake 2: a one-crate *edge PR* (EP1) adds it after the data lane and the first five scene issues, so SC1 to SC5 run beside the whole data lane (C5). | Serial and reflect are frozen and near the critical path; the engine would be built before its first consumer; a mistake in a frozen crate is costly. Moving it costs about 3 PRs in the serial lane and a serial spec amendment outside this footprint. The edge PR keeps the parallelism the roadmap hoped to get from removing E1 (see "Edge E1"). |
| D2 | `exitium_data -> core, ecs, reflect, serial`. **No `exitium_app`, no `exitium_command`, no `exitium_assets`.** No `Plugin`. | `ecs` only so `DataStore` can `impl Resource`. The data lane never waits for the app spine, and the crate follows C1 and D7 of SP4. |
| D3 | A data type is a reflected struct that carries the type data `ReflectDataAsset` (`#[reflect(data(exitium_data::ReflectDataAsset))]`). It is a marker. | The type list for tools and the file loader come from the registry; no hand-written schema. |
| D4 | A `.data` file is the envelope `DataFile { base, value }`. `AssetKind::Data`, extension `data`, payload `Payload::Text`. | One path per asset kind (SP4 D13). |
| D5 | Override paths recurse through **structs** only. Every other field (value, enum, list, map, option, built-in tuple, transparent wrapper) is a leaf and is replaced whole. No path is a prefix of another. | Avoids `Reflect::apply`'s element-wise list recursion; one rule, easy to read and diff. |
| D6 | One engine, `overlay`: `Overrides` (read, write, apply, diff, merge3) and `BaseGraph` (cycle check, chain, dependents in topological order). Data assets and prefab patches both use it. | One definition of "an override" (E1). |
| D7 | `DataStore::load_text` is the single path for a first load and a reload. A failure of any kind, including a dependent whose overrides no longer fit, keeps the last good value and reports the failure. | SP4 rules 1 and 2 (D12, D13). |
| D8 | A data asset with unsaved in-memory edits ("dirty") does not take a disk reload. The disk text is parked in `pending_disk` and shown as a conflict until `data_reload` resolves it. | A reload must not silently destroy an edit, and an edit must not silently hide a disk change. |
| D9 | Saving goes `write_data` -> `native::write_atomic` -> `note_own_write(WatchKey::Asset(guid))` -> clear dirty. | SP4: own writes are not reported as external changes. |
| D10 | Play-in-editor: `begin_play` starts a journal of first edits; `end_play` restores them, removes assets created during play, and **keeps** disk reloads. `DataStore` is not part of the generic resource snapshot. | Stage 5: "Data Asset edits made while playing are reverted on Stop (the registry is part of the snapshot)". |
| D11 | A `.prefab` file is **root** (`base: None`, a list of nodes) or **variant** (`base: Some`, a patch). Node ids are stored labels, stable across saves. | Variants and instances refer to nodes by id; ids that re-derive from `Name` would break them. |
| D12 | A scene places an instance as one entity record with `prefab: Some((asset, patch))`. The record's `id` is the instance root label; inner nodes are `@<instance>.<node>`. | A scene instance is an anonymous variant attached at a location, so it reuses the patch format and the engine. |
| D13 | The instance root's saved state is its patch, computed by diffing the live entities against the resolved prefab at save time. | The editor edits live entities; the file stores only differences. |
| D14 | A prefab reload updates the store and raises `PrefabChanged`. Live instances are updated only by `prefab_refresh` (a per-node 3-way merge); a saved scene gets the new prefab on its next load. | No surprise changes to a running or half-edited world. |
| D15 | All mutation is a plain Rust function that returns an undo record (`DataEdit`, `PrefabEdit`). The editor lane wraps them as commands and tools. These crates have no `exitium_command` edge. | C1 and `editor-mcp-mvp.md`: only crates on C1's list have the command edge. Agent parity holds because the editor lane (#110, SP8) owns the wrappers and the parity test. |
| D16 | No new crates.io dependency. | std plus serial and reflect are enough ("Dependencies"). |

### Edge E1: the decision

**E1 stands.** `exitium_scene` depends on `exitium_data`, and the overlay engine lives in `exitium_data::overlay`.

The three ways to remove the edge, and why none is taken:

| Option | Cost | Verdict |
|---|---|---|
| A. Keep E1, engine in `exitium_data::overlay` (chosen). | One allowed leaf edge. Scene waits for the data lane before its prefab issues. | The edge is added late (EP1), so the wait covers only SV1 to SV4. |
| B. Move the engine into `exitium_serial`. | About 3 more PRs in the serial lane (which is near the 37-PR critical path), and an amendment to `serial-text-format.md`, outside this spec's footprint. Serial then knows what an "override" is. | Rejected. Offered as open question 1. |
| C. A new foundation crate `exitium_overlay`. | A new workspace member (C8, hotspot work at the intake), a frozen crate for about 800 lines, built before its first consumer. | Rejected. |

**Parallelism.** The roadmap hoped that removing E1 would let scene run beside data. The edge PR gets most of that without removing the edge. Scene depends on data only through code that uses the overlay engine, and only the prefab-variant issues (SV1 to SV4) use it. SC1 to SC5 do not. Therefore:

- Intake 2 does **not** add `scene -> data`. Scene and data are independent crates until EP1, so R3 does not tie them.
- DA1 to DA6 (data) run beside SC1 to SC5 (scene) and beside the `exitium_app` spine.
- EP1 adds the edge. It is `Blocked by` DA6 and SC5. It edits one crate manifest (`exitium_scene/Cargo.toml`) and holds `Cargo.toml` and `Cargo.lock` (C5), so it runs alone.
- SV1 to SV4 are `Blocked by` EP1, in that order.

Chain length for Stage 2: intake 2 (1), then max(data 6, SC 5) = 6, EP1 (1), SV1 to SV4 (4), then the facade (18, one more than the roadmap's 17: FD1 and FD2 replace the single "data glue" slot): **about 30 serial PRs, against 32 in the roadmap** (not the roadmap's estimate of 6 PRs saved; a path through the spine and physics keeps the stage from shrinking more). "Downstream adjustments" lists the roadmap edits.

### Crate map

```
exitium_data   -> core, ecs, reflect, serial            (no app, no command, no assets)
exitium_scene  -> core, ecs, reflect, serial            (intake 2: SP4)
exitium_scene  -> exitium_data                          (E1, added by the edge PR EP1)
exitium (facade) -> data, scene, assets, ...            (intake 2)
exitium_editor -> data                                   (intake 3, C3; already in the roadmap)
```

Other leaf crates cannot name data or prefab types (C1). Game systems reach data through the facade (`exitium::data`), and components refer to data and prefabs by `Guid`.

### Data assets

#### What a data type is

```rust
/// Marker type data. Registering it makes the type listable and loadable as a data asset.
#[derive(Clone, Copy)]
pub struct ReflectDataAsset;
// impl TypeData for ReflectDataAsset  (reflect-model.md, "Type data")

#[derive(Reflect, Clone, Default)]
#[reflect(Default, type_name = "glutton::ItemDef", data(exitium_data::ReflectDataAsset))]
pub struct ItemDef {
    pub name: String,
    pub damage: f32,
    pub tags: Vec<String>,
    #[reflect(custom(widget = "asset", kind = "image"))]
    pub icon: Option<Guid>,
}
```

Requirements: `Reflect + FromReflect + Clone + Send + Sync`. `Default` is needed only to create a new asset with empty content (`data_create` without text). A type without `ReflectDataAsset` is refused with `NotADataAsset`, so a component cannot be loaded as a data asset by mistake.

#### File format

Header `%exitium-text 1 root exitium_data::DataFile`, extension `.data`. `DataFile` is **not** a registered reflected type (the same rule as `SceneFile`); this crate reads and writes the envelope, and `exitium_serial` reads and writes the value (`bind_tagged`, `write_tagged`).

A prototype (`base: None`) holds a complete value:

```
%exitium-text 1 root exitium_data::DataFile
(
  base: None,
  value: glutton::ItemDef(
    name: "Iron sword",
    damage: 12.0,
    tags: ["weapon", "melee"],
    icon: Some("67e55044-10b1-826f-9247-bb680e5fe0c8"), // sprites/sword.png
  ),
)
```

A variant (`base: Some`) holds only the overrides. It is a sparse group with the **same type tag** as the root of its base chain:

```
%exitium-text 1 root exitium_data::DataFile
(
  base: Some("0b1cbd5e-3c2e-8f4a-b1d1-2f7a0a8e9c11"), // items/iron_sword.data
  value: glutton::ItemDef(
    damage: 15.0,
  ),
)
```

Rules:

- **Envelope.** `base` is `None` or `Some("<uuid>")` (a `Guid`; a `// path` comment is regenerated on write and ignored on read). `value` is required. An unknown envelope field is a warning.
- **Prototype.** `value` is bound in `Complete` mode: missing fields are filled only from explicit `#[reflect(default)]` (serial-text-format.md). Unknown fields use the serial default, `Warn`.
- **Variant.** `value` is read as overrides ("Override engine"). An unknown field is an **error** `StaleOverride` with up to 3 near-miss suggestions, because a silently dropped override changes the game's numbers. A tag that differs from the base chain's root type is `BaseTypeMismatch`.
- **Layout.** The writer fixes the envelope layout. The value follows serial's layout rules. Overrides are written in path order (fields in declaration order), so one changed field is one hunk in a diff.
- **Limits.** `max_data_bytes` 1 MiB per file (the SP4 `max_text_bytes` applies first), `max_chain_depth` 8, `max_overrides` 4,096 per variant. Errors, not truncation.

### Override engine (`exitium_data::overlay`)

The engine is plain data and functions. It knows nothing about assets, entities or files.

```rust
/// A path into a reflected value through structs only: "stats.hp", "tint.r".
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldPath(Vec<Box<str>>);

pub struct FieldOverride { pub path: FieldPath, pub value: Box<dyn Reflect> }   // value is a complete value of the leaf type

/// A set of overrides on one struct type. Sorted by path. No path is a prefix of another.
pub struct Overrides { /* Vec<FieldOverride> */ }

impl Overrides {
    pub fn is_empty(&self) -> bool;  pub fn len(&self) -> usize;
    pub fn iter(&self) -> impl Iterator<Item = &FieldOverride>;
    pub fn get(&self, path: &FieldPath) -> Option<&dyn Reflect>;
    pub fn set(&mut self, path: FieldPath, value: Box<dyn Reflect>) -> Result<(), OverlayError>;   // checks the path against the type, replaces a deeper or shallower entry
    pub fn remove(&mut self, path: &FieldPath) -> Option<Box<dyn Reflect>>;

    /// Reads a sparse group from the syntax tree. Walks struct fields itself and calls `bind` in Complete mode per leaf field.
    pub fn read(syntax: &Syntax, group: NodeId, ty: &TypeInfo, registry: &TypeRegistry, opts: &ReadOptions) -> Result<Overrides, OverlayError>;
    /// Writes a sparse tagged group (nested groups rebuilt from the sorted paths).
    pub fn write(&self, ty: &TypeInfo, registry: &TypeRegistry, out: &mut String, indent: usize) -> Result<(), OverlayError>;
    /// Validates every path first, then applies. On error the target is unchanged.
    pub fn apply(&self, target: &mut dyn Reflect) -> Result<(), OverlayError>;

    /// The leaves where `derived` differs from `base` (`reflect_eq`), as overrides taken from `derived`.
    pub fn diff(base: &dyn Reflect, derived: &dyn Reflect) -> Overrides;
    /// Three-way merge for a refresh: a leaf the user changed (live != old) keeps the live value; every other leaf takes the new base value.
    pub fn merge3(old_base: &dyn Reflect, new_base: &dyn Reflect, live: &mut dyn Reflect) -> Result<MergeReport, OverlayError>;
}
```

**Why leaf-wise and whole replacement.** `Reflect::apply` resizes a list and recurses element by element, so a "partial list" would silently keep stale tail elements. The engine therefore never calls `apply` for a leaf. It reads the field with `bind` in `Complete` mode and replaces the field with `Reflect::set` (found with `reflect_path_mut`). A non-transparent tuple struct recurses like a struct (paths `0`, `1`). A `transparent` wrapper is resolved to its inner type first, so `hp` stays a leaf whether it is a plain `f32` or a wrapper around one.

```rust
/// The base relation of a set of assets, shared by data and prefab stores.
pub struct BaseGraph { /* Guid -> Option<Guid> base; reverse index */ }

impl BaseGraph {
    pub fn set_base(&mut self, guid: Guid, base: Option<Guid>) -> Result<(), OverlayError>;   // BaseCycle, BaseTooDeep (max 8)
    pub fn base_of(&self, guid: Guid) -> Option<Guid>;
    pub fn chain(&self, guid: Guid) -> Vec<Guid>;                 // [guid, its base, ..., the root prototype]
    pub fn dependents_topo(&self, guid: Guid) -> Vec<Guid>;       // transitive, bases before variants
    pub fn remove(&mut self, guid: Guid);
}
```

`OverlayError` is `#[non_exhaustive]`: `UnknownPath { path, suggestions }`, `PathThroughLeaf { path }` (a path goes through a non-struct), `TypeMismatch { path, expected, found }`, `PrefixConflict`, `BaseCycle(Vec<Guid>)`, `BaseTooDeep`, `Text(SerialError)`, `TooManyOverrides`.

### `DataStore`

`DataStore` is a `Resource` (`impl Resource for DataStore`). It is **not** reflected (it holds `Box<dyn Reflect>` values and a graph); `DataStatus` is the reflected view.

```rust
pub struct DataStore { /* entries: HashMap<Guid, Entry>, graph: BaseGraph, wanted: Vec<Guid>, journal: Option<PlayJournal>, revision: u64 */ }

struct Entry {
    base: Option<Guid>,
    source: Source,                       // Full(Box<dyn Reflect>) for a prototype (the resolved value *is* the source, stored once)
                                          // Overrides(Overrides) for a variant
    resolved: Option<Box<dyn Reflect>>,   // None for a variant until its base chain is Ready; for a prototype it is the source
    state: DataState,
    generation: u32,                      // +1 on every change of the resolved value
    dirty: bool,                          // in-memory edit not yet saved
    pending_disk: Option<String>,         // parked disk text while dirty
    last_error: Option<DataError>,
}

#[derive(Clone, PartialEq)]
pub enum DataState { Waiting(Guid) /* base not loaded yet */, Ready, Failed }
```

**Reads** (any system, `Res<DataStore>`):

```rust
impl DataStore {
    pub fn get(&self, guid: Guid) -> Option<&dyn Reflect>;            // None while Waiting, or Failed with no earlier good value
    pub fn get_as<T: Reflect>(&self, guid: Guid) -> Option<&T>;       // None if the guid holds another type
    pub fn generation(&self, guid: Guid) -> Option<u32>;
    pub fn state(&self, guid: Guid) -> Option<&DataState>;
    pub fn info(&self, guid: Guid) -> Option<DataInfo>;               // type name, base, state, generation, dirty, override count, last error
    pub fn list(&self) -> impl Iterator<Item = DataInfo> + '_;
    pub fn overrides(&self, guid: Guid) -> Option<&Overrides>;        // empty for a prototype
    pub fn is_overridden(&self, guid: Guid, path: &FieldPath) -> bool;
    pub fn referrers(&self, guid: Guid) -> Vec<Guid>;                  // data assets whose value holds this Guid (see refs_of)
    pub fn refs_of(&self, guid: Guid) -> Vec<Guid>;                    // Guid fields marked custom(widget = "asset", kind = "data" | "prefab")
    pub fn want(&mut self, guid: Guid);                                // game code asks for a load; the glue takes the list
    pub fn take_wanted(&mut self) -> Vec<Guid>;
}
```

**The single load path.**

```rust
pub fn load_text(&mut self, guid: Guid, text: &str, registry: &TypeRegistry) -> Result<Loaded, DataError>;
```

1. Parse the envelope and `value` (`DataFile`). Any error returns `Err` and **changes nothing** (a first load stays absent and is marked `Failed`; a reload keeps the old value and sets `last_error`).
2. Set the base in the graph (`BaseCycle` and `BaseTooDeep` are errors). A variant whose base is not in the store yet becomes `Waiting(base)` and the base joins `wanted`.
3. Resolve the entry: a prototype is the value; a variant is `clone_boxed` of its base's resolved value with `Overrides::apply`. The type tag must equal the chain's root type.
4. If the resolved value changed (`reflect_eq` against the previous value), bump `generation`, then re-resolve every entry in `graph.dependents_topo(guid)` the same way. A dependent that fails (for example an override path the new base value no longer has) **keeps its last good value**, sets its own `last_error`, and its own dependents are not re-resolved.
5. Return `Loaded { guid, generation, changed: Vec<Guid>, failed: Vec<(Guid, DataError)> }`. The glue turns it into events.

All of it is one function, so a first load, a reload, a dependent re-resolve after its base arrived, and a test with text in memory use the same code (SP4 D13).

`DataError` is `#[non_exhaustive]`: `Parse { message, span }`, `UnknownType { name, suggestions }`, `NotADataAsset { type_name }`, `BaseTypeMismatch { expected, found }`, `BaseCycle(Vec<Guid>)`, `BaseTooDeep`, `StaleOverride { path, suggestions }`, `Overlay(OverlayError)`, `TooLarge { bytes, limit }`. Every text error carries the line and column.

**Writes** (the editor lane calls these; see "MCP / editor surface").

```rust
pub fn write_data(&self, guid: Guid, registry: &TypeRegistry, resolve_path: &dyn Fn(Guid) -> Option<String>) -> Result<String, DataError>;   // the file text; path comments from the callback

// Authoring. Each returns an undo record; applying the record restores the entry (and re-resolves dependents).
pub fn create(&mut self, guid: Guid, type_name: &str, base: Option<Guid>, registry: &TypeRegistry) -> Result<DataEdit, DataError>;   // default value, or an empty variant
pub fn set_field(&mut self, guid: Guid, path: &FieldPath, value_text: &str, registry: &TypeRegistry) -> Result<DataEdit, DataError>;   // prototype: sets the field; variant: adds or replaces an override
pub fn revert_field(&mut self, guid: Guid, path: &FieldPath) -> Result<DataEdit, DataError>;        // variant only: removes the override
pub fn flatten(&mut self, guid: Guid, registry: &TypeRegistry) -> Result<DataEdit, DataError>;      // variant becomes a prototype with its resolved value (unlink)
pub fn replace_from_text(&mut self, guid: Guid, text: &str, registry: &TypeRegistry) -> Result<DataEdit, DataError>;   // used by data_reload with take_disk
pub fn apply_edit(&mut self, edit: DataEdit, registry: &TypeRegistry) -> Result<DataEdit, DataError>;   // undo and redo: returns the inverse
pub fn mark_saved(&mut self, guid: Guid);                                                            // clears dirty after the file was written
pub fn remove(&mut self, guid: Guid) -> Result<DataEdit, DataError>;                                 // refused while a variant still has this base
```

A `DataEdit` holds the entry's **before** state (source, base, dirty flag). It lives in memory only, like `AssetEdit` in SP4, and is not written to any file.

`set_field` on a prototype changes the resolved value and re-resolves its variants; a variant that does not override that field then sees the new value. `set_field` on a variant stores an override even if the value equals the base's, so "overridden" is an explicit state a user can revert.

### Hot reload and edits (relation to SP4)

Data assets use SP4's two consumer rules unchanged:

1. **One swap point per asset.** The glue's system runs in `First`, after set `ASSETS`. A frame sees one value of an asset; the swap never happens in `Update`. It runs in Edit mode too (`SimulationMode::Frozen`), as SP4 requires of authoring-time systems.
2. **A bad reload keeps the good value.** `load_text` returns the error; the entry keeps its value, `last_error` is set, `DataFailed { reload: true }` is sent, and `AssetFailed` from the server is mirrored into the same list.

What is new is the relation to **unsaved edits** (the dirty flag), since a data asset, unlike a texture, is also edited in memory:

| Entry state | A disk change arrives | Result |
|---|---|---|
| Clean | A reload payload (`generation >= 2`). | `load_text` runs at the swap point. `DataChanged` is sent. |
| Dirty | A reload payload. | The text is parked in `pending_disk`. The entry keeps its edited value. `DataStatus.conflicts` lists it, and the editor shows "changed on disk". No event of change. |
| Dirty | `data_save`. | The file is written, `note_own_write` is called, `pending_disk` is dropped (the save overwrote the disk change; the conflict was visible before the save), dirty is cleared. |
| Dirty or clean | `data_reload { guid, take_disk: true }`. | The disk text (or `pending_disk`) replaces the entry; the edit is undoable (`replace_from_text` returns a `DataEdit`). |
| Dirty | `data_reload { take_disk: false }`. | `pending_disk` is dropped and the edit stays. |

The server re-reads the file for every change, so the glue never reads a file itself. Own writes are filtered by SP4's `note_own_write`, so a save does not come back as a reload.

A variant is re-resolved whenever its base's resolved value changes, whether the change came from a disk reload, an in-memory edit, or an undo. This is the dependency SP4 left to this spec ("Hot reload", "Data assets").

### Play snapshot (Stage 5)

`editor-mcp-mvp.md` snapshots the scene text and the resources marked `custom(play_snapshot = true)` at `play_start`. `DataStore` is not reflected, so that generic mechanism never sees it, and a naive "snapshot every resource" cannot restore it. This crate provides explicit calls, and the play contract of SP8 calls them in `play_start` and `play_stop`:

```rust
pub fn begin_play(&mut self);                  // starts the journal
pub fn end_play(&mut self, registry: &TypeRegistry) -> PlayRestore;   // restores, removes, re-resolves; clears the journal
pub struct PlayRestore { pub restored: Vec<Guid>, pub removed: Vec<Guid>, pub applied_pending: Vec<Guid> }
```

- While the journal is on, the **first** in-memory change to an entry (an edit, a flatten, a `data_reload`) copies the entry's before state into the journal. Later changes to the same entry add nothing. The cost is proportional to the number of edited entries, not to the store size.
- `end_play` restores every journaled entry, removes every entry created during play, and re-resolves the dependents of each changed entry in topological order.
- A **disk reload** that was applied during play is **not** journaled and is **kept**: it came from the file, not from the play session. If a reload had been parked for a dirty entry, `end_play` applies the parked text after restoring (`applied_pending`).
- `data_save` is refused during play (`DataError::SavedDuringPlay`): saving an edit made while playing would write play-time state to the project. Recommendation to SP8, which owns the call site (open question 7).
- Prefabs have no journal: the scene text snapshot already covers instances, and prefab files are edited only in Edit mode (the editor lane refuses `prefab_save` during play, the same rule).

### Prefabs in `exitium_scene`

A prefab is a tree of entities with a stable id per node. The scene crate gains `prefab`, `patch`, `instance` and `refresh` modules.

#### Prefab file

Header `%exitium-text 1 root exitium_scene::PrefabFile`, extension `.prefab`, `AssetKind::Prefab`, payload `Payload::Text`. Like `SceneFile`, `PrefabFile` is not a registered reflected type.

A **root prefab** has `base: None` and an `entities` list with exactly the record shape of a scene. The first record is the root (`parent: None`), and every other record has a `parent`. The `id` labels are **node ids**: they are stored, never re-derived from `Name`.

```
%exitium-text 1 root exitium_scene::PrefabFile
(
  base: None,
  entities: [
    (
      id: @root,
      parent: None,
      components: [
        exitium_scene::Name("Goblin"),
        exitium_app::Transform2d(
          translation: (x: 0.0, y: 0.0),
          rotation: 0.0,
          scale: (x: 1.0, y: 1.0),
        ),
        exitium_render::Sprite(image: Some("67e55044-10b1-826f-9247-bb680e5fe0c8"), tint: (r: 1.0, g: 1.0, b: 1.0, a: 1.0)), // sprites/goblin.png
      ],
    ),
    (
      id: @weapon,
      parent: Some(@root),
      components: [
        exitium_scene::Name("Weapon"),
        exitium_app::Transform2d(translation: (x: 0.5, y: 0.0), rotation: 0.0, scale: (x: 1.0, y: 1.0)),
      ],
    ),
  ],
)
```

A **variant** has `base: Some(guid)` and a `patch`; the two forms are mutually exclusive (`PrefabShape` error). The patch has three lists, each of which may be omitted when empty:

```
%exitium-text 1 root exitium_scene::PrefabFile
(
  base: Some("3c6e5d2a-91b4-8d0e-a3f1-5b7c2d9e4a10"), // prefabs/goblin.prefab
  patch: (
    overrides: [
      (
        node: @root,
        set: [exitium_render::Sprite(tint: (r: 1.0, g: 0.4, b: 0.4, a: 1.0))],
        add: [glutton::Health(max: 40, current: 40)],
        remove: [exitium_physics::CircleCollider],
      ),
    ],
    added: [
      (id: @shield, parent: Some(@root), components: [exitium_scene::Name("Shield")]),
    ],
    removed: [@weapon],
  ),
)
```

Patch rules:

- `overrides` has one entry per touched node. `node` is a node id of the resolved base (including nodes the base chain added). `set` holds **sparse** tagged component groups, read by `Overrides::read` (struct paths, leaves replaced whole). `add` holds **complete** tagged groups for components the node does not have (a component the node already has is replaced whole). `remove` lists component type names.
- `added` holds full node records. `parent` is required and names a node of the resolved base or an earlier added node. Added nodes are appended after the parent's last child.
- `removed` lists node ids. Descendants go with them. The root cannot be removed (`RemoveRoot`).
- **Stale entries are warnings, not errors**: a `set` for a field or component the base no longer has, an override for a missing node, a `remove` of a component that is gone. The entry is skipped and `PrefabWarning::StaleOverride { node, path }` is recorded in the resolved prefab. A variant whose base changed stays loadable. (A data variant is stricter: it has a last good value to fall back to, a first scene load has none. "Hot reload and edits" and open question 10 explain.)
- Reparenting a base node is not possible in a patch (open question 5).
- **Entity references** inside a prefab use node labels (`target: @root`), through the same `TextRefs` hook as scenes. A reference to anything outside the prefab is `ReferenceOutsidePrefab` on write.
- Limits: `max_prefab_nodes` 10,000, `max_chain_depth` 8, `max_overrides` 4,096 per patch.

#### Resolved form and the store

```rust
pub struct ResolvedPrefab {
    pub guid: Guid, pub generation: u32, pub chain: Vec<Guid>,
    pub nodes: Vec<ResolvedNode>,                 // pre-order; nodes[0] is the root
    pub warnings: Vec<PrefabWarning>,
}
pub struct ResolvedNode { pub id: Box<str>, pub parent: Option<u32>, pub components: Vec<Box<dyn Reflect>> /* sorted by type name; pre-bound */ }
```

Components are bound **once at resolve time** (not at each spawn), so a runtime spawn is `clone_boxed` plus `ReflectComponent::insert`. Entity-typed fields cannot be bound to real entities yet, so they hold a **placeholder** `Entity` (the node index, with a generation no real entity has). Placeholders are built through the `Entity` text form, so no ECS change is needed; if `Entity` offers a public constructor by then, SV1 uses it. The spawner rewrites placeholders to the spawned entities. Whether a component type holds `Entity` is found by walking its `TypeInfo` once and cached per `TypeId`; types without `Entity` fields skip the rewrite entirely. A map keyed by `Entity` is rebuilt through `Map::insert`.

```rust
#[derive(Resource)]
pub struct PrefabStore { /* sources and resolved: HashMap<Guid, Arc<ResolvedPrefab>>, graph: BaseGraph (exitium_data::overlay), instances: HashMap<Entity, Arc<ResolvedPrefab>>, wanted, ... */ }

impl PrefabStore {
    pub fn load_text(&mut self, guid: Guid, text: &str, registry: &TypeRegistry) -> Result<PrefabLoaded, PrefabError>;   // the same rules as DataStore::load_text
    pub fn get(&self, guid: Guid) -> Option<Arc<ResolvedPrefab>>;
    pub fn want(&mut self, guid: Guid);  pub fn take_wanted(&mut self) -> Vec<Guid>;
    pub fn stale_instances(&self) -> Vec<Entity>;
    // authoring: create, create_variant, set_override ... return PrefabEdit undo records
}
```

`PrefabStore` repeats `DataStore`'s rules on purpose and uses the same `BaseGraph`: one `load_text` for a first load and a reload, a failed reload keeps the last good `Arc<ResolvedPrefab>`, and a changed base re-resolves its variants in topological order. A prefab with a missing base is `Waiting(base)`. Prefabs have no dirty/conflict state: a prefab file is edited by `prefab_save` (an own write) or by an external tool, and the editor never holds unsaved prefab edits apart from the live entities in the scene.

#### Instances in a scene

An entity record in a scene file gains an optional field:

```
(
  id: @goblin_1,
  parent: None,
  prefab: Some((
    asset: "3c6e5d2a-91b4-8d0e-a3f1-5b7c2d9e4a10", // prefabs/goblin.prefab
    patch: (
      overrides: [
        (node: @root, set: [exitium_app::Transform2d(translation: (x: 4.0, y: 2.0))]),
      ],
    ),
  )),
),
(
  id: @goblin_hat,
  parent: Some(@goblin_1.weapon),
  components: [exitium_scene::Name("Hat")],
),
```

- The record's `id` is the **instance root label**. The prefab's root node is that entity. Every other node `n` is the entity labelled `@<instance>.<n>` (`@goblin_1.weapon`). A label may contain `.`, so these are valid labels (SP4 label token). The instance's `Transform2d` is an override of the root node (as above).
- `components` of an instance record must be empty (`InstanceHasComponents`). An entity added under an instance is an ordinary scene record whose `parent` is an instance label, as `@goblin_hat` shows. Its `SceneMember` is the scene's, so it saves normally.
- `patch` is the same shape as a variant's, and `node:` names a **prefab-local** id (`@root`), not an instance label.
- **Parse and load.** `ParsedScene::parse` accepts a label `@x.y` as declared by instance `@x` without resolving the prefab (the prefab may not be loaded). `instantiate` resolves the prefab and reports `UnknownLabel` for an inner label that does not exist (with suggestions). A prefab that is not in the `PrefabStore` fails `instantiate` with `PrefabNotLoaded { guid }` before anything is spawned (the load is atomic, as in SP4). `ParsedScene::prefab_guids()` lets the glue request them first ("Facade glue").
- **A parent that no longer exists.** If a scene entity's parent label names an instance node that the current prefab no longer has, the entity attaches to the instance root and `SceneWarning::MissingPrefabNode { label }` is recorded.
- **Nesting.** A prefab file that contains an instance record is `NestedPrefab` (an error). The `prefab` field is accepted in `PrefabFile` records by the parser only to produce that error, which keeps the format open for open question 2.

#### Runtime components (reflected)

```rust
#[derive(Component, Reflect, Clone)] #[reflect(Component, type_name = "exitium_scene::PrefabInstance", custom(derived = true))]
pub struct PrefabInstance { pub asset: Guid, pub generation: u32 }              // on the instance root

#[derive(Component, Reflect, Clone)] #[reflect(Component, type_name = "exitium_scene::PrefabNode", custom(derived = true))]
pub struct PrefabNode { pub instance: Entity, pub node: String }                 // on every spawned node, the root included

#[derive(Component, Reflect, Clone)] #[reflect(Component, type_name = "exitium_scene::NodeId", custom(derived = true))]
pub struct NodeId(pub String);                                                   // authored root-prefab entities: the stable node id
```

All three are `derived`, so a scene write skips them (the instance record carries the same information). `PrefabStore.instances` keeps the `Arc<ResolvedPrefab>` each live instance was spawned from, so a refresh knows the *old* base without storing it per generation. The map is pruned of dead entities on `stale_instances` and `refresh`.

#### Writing: instances and prefabs

**`write_scene` with instances.** For each entity with `PrefabInstance`, the writer resolves `asset` and compares the live subtree with the resolved nodes (members identified by `PrefabNode.node`):

1. A resolved node with no live entity becomes `removed`.
2. A live entity with a component the resolved node lacks becomes `add`.
3. A resolved component missing from the live entity becomes `remove`.
4. A component present in both goes through `Overrides::diff` (resolved against live) and becomes `set`.
5. A live entity under the instance with no `PrefabNode` is not part of the patch; it is written as an ordinary record with `parent: Some(@inst.node)`.
6. A live node that was **moved out** of its instance (its parent is no longer inside it) is written as a plain scene entity ("unlink and keep") with `SceneWarning::PrefabNodeDetached`, and the instance patch lists it in `removed`.

The skip and warn rules for components are the same as `write_scene`'s (derived components, `SceneMember`, `Parent`, `Children`, `UnsavedComponent`), and `SaveOptions` is shared.

**`write_prefab`** writes a **root prefab** from a live subtree: nodes in pre-order, node ids from `NodeId` (new nodes get an id derived from `Name`, with `_2`, `_3` on collision, then **keep it**: renaming the entity later does not rename its node id). `write_prefab_variant` writes a **variant**: it resolves the base chain and diffs the live subtree against it, as for an instance.

**Editing a prefab** has one path: instantiate the prefab (or create a new subtree), edit the live entities, then `prefab_save(root, asset)`. If `asset` is a root prefab the whole subtree is written; if it is a variant, only the diff against its base is written. Saving an instance back into its own source prefab is the "apply" operation.

#### Runtime spawn and refresh

```rust
pub fn instantiate_prefab(world: &mut World, registry: &TypeRegistry, prefab: &ResolvedPrefab, opts: &SpawnOptions) -> Result<PrefabSpawn, SceneErrors>;
pub struct SpawnOptions { pub parent: Option<Entity>, pub transform: Option<Transform2dOverride>, pub scene: Option<SceneId>, pub patch: Option<PatchRef> }
pub struct PrefabSpawn { pub root: Entity, pub entities: Vec<Entity> }
```

It spawns `prefab.nodes.len()` entities, inserts `clone_boxed` copies of the pre-bound components (plus the placeholder rewrite where needed), builds the hierarchy through `World::run_system_once` as scenes do, and tags the nodes (`PrefabInstance`, `PrefabNode`, and `SceneMember` when `scene` is set). It is allocation-light on purpose: game code spawns bullets and enemies with it.

**`refresh(world, instance)`** brings a live instance up to date with its prefab:

1. `old` is the `Arc` the instance was spawned from, `new` is `PrefabStore::get(asset)`. If `old` is missing (the instance was not spawned by this store), the refresh is refused with `RefreshUnavailable`; the user unpacks the instance or reloads the scene.
2. For each node present in both, each component present in both goes through `Overrides::merge3(old, new, live)`: a leaf the user changed keeps the live value; every other leaf takes the new value. Components added in `new` are inserted; components removed in `new` are removed unless the user changed them (then they stay and the report lists them).
3. Nodes present only in `new` are spawned; nodes only in `old` are despawned unless the user added children to them (then the node stays, reported).
4. `PrefabInstance.generation` is set to the new generation; the store's `instances` map points to `new`.
5. The result is a `RefreshReport`, and an undo record that restores the previous state.

### Facade glue (contract)

The glue lives in `exitium` (facade issues FD1 and FD2; they hold the `crates/exitium/` hotspot and have `Edits: none`). `DataPlugin` and `PrefabPlugin` are added to `DefaultPlugins`.

- **Schedule.** Every system below runs in `First` after set `ASSETS` (or in `PostUpdate` for scans), never in `Update`, so Edit mode (`SimulationMode::Frozen`) works.
- **Delivery (data).** For each `AssetLoaded { guid, kind: Data, generation }` event the glue calls `AssetServer::take_loaded_guid(guid)` (several consumers share `Text`, SP4) and passes the `Payload::Text` to `DataStore::load_text`. It sends `DataChanged { guid, generation }` for each guid in `Loaded.changed` and `DataFailed { guid, message, reload }` for each failure. The same applies to `Prefab` and `PrefabStore`. **Implementer check:** the glue relies on `AssetLoaded` being sent for a reload with `generation >= 2`, as SP4's plugin section states; if the event semantics differ, take the payloads with `take_loaded(AssetKind::Data)` instead and filter by guid. The `DataStore` does not know which guids another consumer owns, so the glue only takes guids that are in `AssetRegistry` with kind `Data`.
- **Requests.** After each delivery the glue calls `AssetServer::request` for `DataStore::take_wanted()` (bases) and for `refs_of(guid)` (referenced data assets and prefabs). `DataSettings.preload` (default `true`) also requests every `Data` and `Prefab` asset in the registry once the manifest is `Ready`, and again for entries a manifest reload adds.
- **Scenes with prefabs.** `LoadScene` (FA3) gains a waiting state: after parsing, the glue calls `ParsedScene::prefab_guids()`, requests the prefabs, and instantiates once all of them are `Ready`. A prefab that fails (or that stays `NotFound` after SP4's retry schedule of 1, 2, 4, 8, 16 s) gives `SceneLoadFailed { PrefabFailed(guid) }`; the scene is not instantiated. The state is `SceneLoadState::WaitingForPrefabs`, reported by the existing scene status.
- **Own writes.** The glue does nothing for them; the editor lane calls `note_own_write` after `data_save` and `prefab_save`, as for scenes (SP4).
- **Re-exports.** `exitium::data`: `DataStore`, `DataInfo`, `DataState`, `DataError`, `DataEdit`, `FieldPath`, `Overrides`, `ReflectDataAsset`, `DataPlugin`, `DataStatus`, `DataSettings`, `DataChanged`, `DataFailed`. `exitium::scene` gains `PrefabStore`, `ResolvedPrefab`, `PrefabInstance`, `PrefabNode`, `NodeId`, `instantiate_prefab`, `write_prefab`, `PrefabPlugin`, `PrefabStatus`, `PrefabChanged`, `PrefabFailed`. `BaseGraph` and the `overlay` internals stay private.

### Reflected surface (C4)

| Item | Kind | Notes |
|---|---|---|
| `ReflectDataAsset` | type data | Marks a type loadable as a data asset. Listed by `data_types`. |
| `DataStatus` | resource, read-only | `entries`, `ready`, `waiting`, `failed`, `dirty` counts; `failures: Vec<DataFailure { guid, path, message }>`; `conflicts: Vec<Guid>`; `revision: u64` (bumps on any change). Inspector and `ecs_get_resource` read it. |
| `DataSettings` | resource | `preload: bool` (true). `custom(history = false)`: a settings change is not an undo step (as `AssetSettings`). |
| `DataChanged`, `DataFailed` | events | `{ guid, generation }`, `{ guid, message, reload: bool }`. |
| `PrefabInstance`, `PrefabNode`, `NodeId` | components | `derived`, not saved; readable by `ecs_get_component`. |
| `PrefabStatus` | resource, read-only | `prefabs`, `ready`, `waiting`, `failed` counts, `failures`, `stale_instances: u32`, `revision`. |
| `PrefabChanged`, `PrefabFailed` | events | As for data. |
| Commands (tools) | see the next section | Registered by the editor lane. |

`DataStore` and `PrefabStore` are deliberately not reflected: their contents are reached through the tools below, which read them through the plain functions. The tools are therefore a complete view; nothing the inspector can show is missing from MCP (agent parity).

### Threading

All mutation happens on the main thread: the glue systems in `First`, and the editor's command drain. `DataStore` and `PrefabStore` are `Send + Sync` (`Box<dyn Reflect>` and `Arc<ResolvedPrefab>` are), so any number of systems may read them in parallel through `Res`, and the parallel executor needs no lock. A system sees one version of an asset for a whole frame, because the swap only happens in `First`. Parsing and resolving are done inline: the budgets below keep a reload frame under SP4's 20 ms cap. There is no worker thread in either crate. A `ResolvedPrefab` is immutable once built and is shared through `Arc`, so a reload replaces the pointer and never changes a prefab a system is reading. No `unsafe`.

## MCP / editor surface

The data and scene crates have no `exitium_command` edge (D2, D15). Each row is a plain function the crates expose. The editor lane (#110 and SP8) wraps each as a `Command` or `Query` with the stated undo kind, registers it with `register_command`, and gets a tool with the same name. The parity test grows by these rows.

| Plain Rust function | Tool | Kind and undo |
|---|---|---|
| `DataStore::types` (registry scan for `ReflectDataAsset`) | `data_types` | Query. Names, fields, schema, whether `Default` exists. |
| `DataStore::list` | `data_list` | Query. Filter by type, by base, by state (`dirty`, `waiting`, `failed`). |
| `info`, `get`, `overrides`, `write_data` | `data_get` | Query. The resolved value as text, base, overrides, state, last error, parked disk text (conflict). |
| `create` plus `exitium_assets::edit::register` (AP3) | `data_create` | Command. Optional `base` makes a variant. Undo: `DataEdit` plus the asset edit. |
| `set_field` | `data_set_field` | Command. Value as text through serial; path syntax of reflect-model.md (`stats.hp`). Undo: `DataEdit`. |
| `revert_field` | `data_revert_field` | Command. Undo: `DataEdit`. |
| `flatten` | `data_flatten` | Command. Undo: `DataEdit`. |
| `write_data`, `native::write_atomic`, `note_own_write`, `mark_saved` | `data_save` | Command, **no undo** (`UndoKind::NotUndoable`: a file write). Refused in Play. |
| `replace_from_text`, drop of `pending_disk` | `data_reload` | Command. `take_disk` bool. Undo: `DataEdit` (restores the in-memory value). |
| `referrers`, `refs_of`, plus `AssetEdit::find_references` | `data_find_references` | Query. Data assets and scenes that name the guid. |
| `PrefabStore::list`, `get` | `prefab_list`, `prefab_get` | Query. `prefab_get` returns nodes, patch, warnings and stale instances. |
| `instantiate_prefab` | `prefab_instantiate` | Command. Undo despawns the instance. Optional parent, transform and `scene`. |
| `create` (a subtree, or empty) plus `register` | `prefab_create` | Command. Optional `base` makes a variant. Undo: `PrefabEdit` plus the asset edit. |
| `write_prefab`, `write_prefab_variant`, `native::write_atomic`, `note_own_write` | `prefab_save` | Command, **no undo**. Refused in Play. |
| `Overrides::remove` on an instance's patch (live: restore the base value) | `prefab_revert_override` | Command. Entity, component, optional field path. Undo: the previous value. |
| unpack | `prefab_unpack` | Command. Removes `PrefabInstance` and `PrefabNode` and keeps the entities (they become plain scene entities). Undo restores the tags. |
| `refresh` | `prefab_refresh` | Command. Undo restores the instance. Without an `entity` argument it refreshes every stale instance. |

That is **10 data tools and 8 prefab tools, 18 in all**, against the roughly 90-tool budget of `editor-mcp-mvp.md`. `data_validate` and `prefab_validate` are not separate tools: validation is part of `data_get`, `prefab_get` and the `DataStatus`/`PrefabStatus` resources. SP8 confirms the count when it merges its tool table.

**Editor panels** (SP8 builds them from the same functions): a Data Asset panel (list by type, an inspector for the resolved value with an "overridden" marker and a revert button per field, base picker, flatten, save, conflict banner), and a Prefab panel plus hierarchy badges (instance icon, override dot per node, "stale" marker, Refresh). Fields typed `Guid` with `custom(widget = "asset", kind = "data" | "prefab")` get the generic asset picker (ED10), filtered by `kind` and optionally by `data_type`.

**Luau.** `exitium_script` reads data assets through `DataStore::get(guid) -> &dyn Reflect` (SP9 wraps it as a reflected value). This crate does not depend on the script crate, and it needs no change if SP9 decides how the call looks.

## Performance

Budgets are targets on the reference desktop and on the min-spec profile (4 cores, 8 GB). A benchmark ID has the form `<crate>/<area>/<case>/<size>`, and the first bench PR may tighten a number but not loosen it without a spec amendment. CI fails a regression over 5%.

| Benchmark ID | Budget |
|---|---|
| `exitium_data/store/get/10000` (`get_as` over 10,000 entries) | at most 60 ns per call |
| `exitium_data/store/load_prototype/2kb` | at most 100 µs |
| `exitium_data/store/load_variant/200b` | at most 30 µs |
| `exitium_data/store/reresolve/1000_dependents` (one base changes, 1,000 variants) | at most 5 ms |
| `exitium_data/store/write/2kb` (`write_data`) | at most 60 µs |
| `exitium_data/store/set_field/variant` (one override, no dependents) | at most 5 µs |
| `exitium_data/overlay/apply/16_overrides` | at most 3 µs |
| `exitium_data/overlay/diff/40_fields` | at most 3 µs |
| `exitium_data/overlay/merge3/40_fields` | at most 6 µs |
| `exitium_data/play/end_play/100_edited` | at most 1 ms |
| `exitium_scene/prefab/resolve_root/20_nodes_60_components` | at most 150 µs |
| `exitium_scene/prefab/resolve_variant/20_nodes_5_overrides` | at most 40 µs |
| `exitium_scene/prefab/spawn/5_nodes_15_components` | at most 20 µs |
| `exitium_scene/prefab/spawn/1000_instances` | at most 25 ms |
| `exitium_scene/instantiate/1000_instances` (scene with 1,000 instance records of the 5-node prefab) | at most 60 ms |
| `exitium_scene/write/1000_instances` (diff against the resolved prefab) | at most 30 ms |
| `exitium_scene/prefab/refresh/1000_instances` | at most 100 ms |

**Memory.** A store entry costs at most 200 bytes beyond the values it holds. A prototype's value is stored once (the source and the resolved value are the same box); a variant stores its overrides plus one resolved value. A resolved prefab is shared through `Arc`, so 1,000 instances share one resolved copy.

**Frame cost.** A reload frame (swap, dependents, events) stays within SP4's 20 ms cap together with the render upload cap. The time from a file write to the new value visible to systems is at most 500 ms on native with the default hot-reload settings (SP4's 250 ms poll plus 50 ms settle, plus one frame), recorded in the PR of FD1.

**Complexity.** `dependents_topo` is linear in the number of dependents. A set of overrides is a sorted vector, so lookup is `O(log n)` and `apply` is `O(n)`. `resolve` clones the base value once per variant.

## Test plan

Per crate, with golden files for the text forms.

- **Overlay.** Path syntax and validation (unknown path with suggestions, path through a leaf, prefix conflict). `apply` changes nothing on error. `diff` then `apply` reproduces the derived value (property test over generated structs, including enums, options, lists and maps as leaves). `merge3`: a user-changed leaf is kept, an unchanged leaf takes the new base value, a field missing from the new base is skipped and reported. A list leaf is replaced whole (no tail left over). A transparent wrapper is one leaf.
- **BaseGraph.** Cycle (self, two, three), depth 8 and 9, `dependents_topo` order on a diamond, removal.
- **DataFile.** Prototype and variant round trips with goldens that equal the examples above. A tag that differs from the base chain is `BaseTypeMismatch`. A type without `ReflectDataAsset` is `NotADataAsset`. An unknown override field is an error with suggestions; an unknown prototype field is a warning. CRLF and comments parse. Output is identical across two writes.
- **DataStore.** First load, a variant that loads before its base (`Waiting`, then `Ready` when the base arrives), a chain of three, a changed base re-resolves dependents in order, **a reload that fails keeps the last good value** (and the same for a dependent whose override path vanished), a first load that fails is `Failed` with no value, a cycle created by a reload is rejected and the old graph stays. `load_text` is the only entry point for first load and reload (a test replays a first load as a reload and compares). Dirty entry: a reload is parked, `conflicts` lists it, `data_save` clears it, `take_disk` replaces it, undo restores the edit.
- **Edit API.** Every function's undo and redo. `remove` refused while a variant has the base. `flatten` gives a value equal to the previous resolved value.
- **Play.** `begin_play`, edit three entries, create one, apply a disk reload to a fourth, `end_play`: the three are restored, the created one is gone, the reloaded one keeps the disk value, dependents are re-resolved, a parked reload is applied. `data_save` during play is refused.
- **Prefab file.** Root and variant goldens equal the examples above. Both forms at once is `PrefabShape`. A variant chain of three. Stale overrides give warnings and skip. `RemoveRoot`, `NestedPrefab`, two roots, a cycle, a missing parent. Node ids survive a save after an entity rename.
- **Instances in scenes.** A scene with two instances of the same prefab, an override on one, an added child under an inner node: write, read into a fresh world, write gives identical text. `@inst.node` labels resolve; an unknown inner label is `UnknownLabel` with suggestions; a missing prefab fails before anything is spawned (entity count unchanged). A prefab edited between save and load: a removed node's child re-attaches to the root with a warning. A node moved out of an instance is written as a plain entity.
- **Spawn.** `instantiate_prefab` produces the same entities as an instance record with an empty patch. Entity references between nodes are rewritten to the spawned entities; a map keyed by `Entity` works.
- **Refresh.** A user-changed field survives a refresh, an unchanged field takes the new value, a new node appears, a removed node goes away, a node with user children stays and is reported, undo restores. `RefreshUnavailable` for an instance this store did not spawn.
- **Facade (FD1, FD2).** A headless app with `MemorySource`: a data asset loads and `get_as` returns it; a variant follows a base rewritten on disk in `SimulationMode::Frozen`; a corrupt rewrite keeps the old value and sends `DataFailed { reload: true }`; a save by the editor path is not reported as a change; `LoadScene` waits for a prefab and then instantiates, and fails cleanly when the prefab fails.
- **No unsafe**; the wasm32 build of both crates passes.

## Dependencies

No new crates.io dependency. The reasons, so a reviewer can check them:

| Need | Choice | Why not a crate |
|---|---|---|
| Maps, vectors, sets | `std::collections` (`HashMap`, `BTreeMap`, `Vec`) | The sizes are small; the writer sorts, so deterministic output needs no hasher crate. |
| Shared immutable prefabs | `std::sync::Arc` | Built in. |
| Text format | `exitium_serial` (workspace) | The spec of record for the text form. |
| Reflection, type data, paths | `exitium_reflect` (workspace) | Same. |
| File write | `exitium_assets::native::write_atomic` through the facade glue (the data crate itself writes no file) | Keeps `exitium_data` free of an assets edge. |
| Benchmarks | `criterion` (workspace, dev-dependency) | Already the workspace's choice. |

Edit distance for suggestions uses the helper `exitium_serial` already exposes for its near-miss rule.

## Wiring for the intake PRs

What this spec needs from `Cargo.toml`, `Cargo.lock`, `deny.toml` and `xtask/` (C5). Intake 2 adds the union of the merged specs' lists.

**Intake 2.**

- Workspace edges: `exitium_data -> exitium_core, exitium_ecs, exitium_reflect, exitium_serial` (normal), dev-dependency `criterion`. **No** `exitium_app`, `exitium_command`, `exitium_assets`.
- **Not in intake 2: `exitium_scene -> exitium_data` (E1).** The roadmap lists it there; this spec moves it to the edge PR EP1 (D1, "Downstream adjustments").
- Facade: `exitium -> exitium_data` (normal; `exitium -> exitium_scene` is already SP4's).
- `crates/exitium_reflect_derive/`: add `exitium_data` to `ENGINE_CRATES` (the crate derives `Reflect` for `DataStatus` and its events, and the `crate_paths.rs` drift test fails until it is classified; `exitium_scene` is already there from SP4).
- `xtask`: nothing new; `exitium_data` is wasm-capable and goes in the default wasm build.

**Intake 3.** `exitium_editor -> exitium_data` (C3, already listed in the roadmap).

**Edge PR EP1** (not an intake): `exitium_scene -> exitium_data`, normal dependency. One crate (`exitium_scene/Cargo.toml`), `Hotspots: Cargo.toml, Cargo.lock`, `Blocked by` DA6 and SC5.

**Not added, on purpose:** any edge from `exitium_data` or `exitium_scene` to `exitium_app`, `exitium_assets`, `exitium_command` or another leaf crate (C1), and any crates.io dependency.

## Open questions

Each has a recommended answer; the owner decides.

1. **E1: keep the edge (with the late edge PR), or move the overlay engine into `exitium_serial`?** Recommended: keep E1 and the edge PR (D1). Moving the engine would cost about 3 PRs in the near-critical serial lane and an amendment of `serial-text-format.md`, and it puts a data concept into a frozen foundation crate.
2. **Nested prefabs** (a prefab containing instances of other prefabs): cut from the MVP, or include now? Recommended: cut. The `prefab` field is reserved in the format, so adding them later changes no file; the extra work (instances inside instances, patch paths through two levels, refresh through nesting) is the largest single risk in this spec.
3. **List and map elements:** whole-field replacement only, or element-level overrides (insert, remove, set by index or key)? Recommended: whole field for the MVP. Glutton-style data (item lists, loot tables) is small, and element-level patches need identity rules for list items.
4. **Rebasing a variant onto another base:** support it, or `flatten` plus re-create? Recommended: not supported. Overrides are tied to the base's shape.
5. **Reparenting a base node in a variant:** support it? Recommended: no. A variant adds, removes, overrides and cannot move nodes.
6. **Live prefab propagation:** explicit `prefab_refresh` (this spec), or automatic update of every live instance on reload? Recommended: explicit. Automatic update can overwrite what an agent or human is editing; a saved scene already gets the new prefab on its next load.
7. **`data_save` during Play:** refuse? Recommended: refuse (D10). SP8 owns the call site and should state the same.
8. **Typed handles (`DataRef<T>`, `AssetRef<T>`):** wait with SP4's D1 (plain `Guid`), or add a typed handle now? Recommended: wait. It is a facade-level addition later and changes no file format.
9. **Runtime-defined data types** (reflect open question 9; a type created in the editor with no Rust code): answered here as not in the MVP. Recommended: keep it out; a game's data types are Rust structs with `#[derive(Reflect)]`, which is what an agent writes.
10. **Conflict policy and stale overrides.** (a) A dirty data asset parks a disk change (D8) rather than letting the disk win. (b) A stale override in a data variant is an error (keeps the last good value), but in a prefab variant it is a warning that skips the override. Recommended: both as written. Data has a last good value to keep; a first scene load has none, so failing there would make a prefab edit break every scene that places it.
11. **Node-id reuse.** A node id freed by a deleted node can be taken by a new node in a later save, and a variant's override for the old node then lands on the new one. Recommended: accept this for the MVP, document it, and let `prefab_get` show the variants that name a node when it is deleted.

## Implementation issues

Each issue is PR-sized (about 600 changed lines or fewer), edits one crate (the facade issues edit none and hold `crates/exitium/`), has a labelled Footprint, acceptance criteria and `Blocked by` lines, and goes into the MVP milestone. The architect files them with `file-issue` after this spec merges, labels `area:data`, `area:scene`, `area:assets` and `area:facade`. `<intake-2>` is the intake issue's number, filled in at filing. Every issue is `Blocked by <intake-2>` (C6). One crate has at most one issue in flight, so each lane runs in the order shown. Each issue updates its crate's `CLAUDE.md` (DA1 creates the data crate's; SC1 creates the scene crate's), adds the benchmarks it names, and states the high-risk areas it touches in the PR Risk section (the serialization format, for the formats; the facade API, for FD2).

### Data lane (`exitium_data`, Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none)

DA1 starts right after intake 2, beside the `exitium_app` spine and the scene lane (R3 passes: no edge to either).

**DA1. `feat(data): overlay field paths, Overrides apply and text I/O`** (about 550 lines). Blocked by `<intake-2>`.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `overlay/path.rs`, `overlay/overrides.rs` (`FieldPath`, `FieldOverride`, `Overrides::{set, remove, get, iter, read, write, apply}`), `OverlayError`; creates `CLAUDE.md`.
- Acceptance criteria:
  - [ ] The "Overlay" path, read, write and apply tests, including a list leaf replaced whole and a transparent wrapper as one leaf; `apply` changes nothing on error.
  - [ ] A sparse group written then read gives equal overrides; output is identical across two writes.
  - [ ] `overlay/apply/16_overrides` meets the budget; no `unsafe`; wasm32 build passes.

**DA2. `feat(data): overlay diff, merge3 and BaseGraph`** (about 450 lines). Blocked by `<intake-2>`, DA1.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `Overrides::{diff, merge3}`, `MergeReport`, `graph.rs` (`BaseGraph`).
- Acceptance criteria:
  - [ ] The property test `diff` then `apply` and the `merge3` cases of the Test plan; the `BaseGraph` cycle, depth and diamond tests.
  - [ ] `overlay/diff/40_fields` and `overlay/merge3/40_fields` meet their budgets.

**DA3. `feat(data): ReflectDataAsset and the DataFile envelope`** (about 450 lines; high-risk review: file format). Blocked by `<intake-2>`, DA2.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `asset.rs` (`ReflectDataAsset` and its registration helper), `file.rs` (`DataFile` parse and write, limits), `DataError`.
- Acceptance criteria:
  - [ ] The "DataFile" test group and goldens equal to this spec's examples; `BaseTypeMismatch`, `NotADataAsset`, `StaleOverride` with suggestions, and the CRLF and comment cases.
  - [ ] Prototype 2 KB and variant 200 B parse within the DA4 budgets' parse share (measured here, asserted in DA4).

**DA4. `feat(data): DataStore (load_text, resolve, dependents, reload rules)`** (about 600 lines). Blocked by `<intake-2>`, DA3, `exitium_ecs` `Resource` (#39).
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `store.rs`: `DataStore` (`impl Resource`), the reads, `want` and `take_wanted`, `load_text` with the five steps, `refs_of`, `referrers`.
- Acceptance criteria:
  - [ ] The "DataStore" test group: waiting base, chain of three, dependents re-resolved in order, **a failed reload keeps the last good value** for the entry and for a dependent, a cycle from a reload is rejected, first load equals reload (replay test).
  - [ ] `store/get/10000`, `store/load_prototype/2kb`, `store/load_variant/200b`, `store/reresolve/1000_dependents` meet the budgets.

**DA5. `feat(data): authoring API, undo records and conflicts`** (about 550 lines). Blocked by `<intake-2>`, DA4.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `edit.rs`: `write_data`, `create`, `set_field`, `revert_field`, `flatten`, `replace_from_text`, `apply_edit`, `mark_saved`, `remove`, `DataEdit`, the dirty flag and `pending_disk` rules of the conflict table.
- Acceptance criteria:
  - [ ] The "Edit API" and dirty-conflict tests: every undo and redo, `remove` refused for a base in use, `flatten` keeps the resolved value, a dirty entry parks a reload, `take_disk` and `mark_saved` behave as in the table.
  - [ ] `store/write/2kb` and `store/set_field/variant` meet the budgets.

**DA6. `feat(data): play journal, DataStatus, events, registration and benchmarks`** (about 450 lines). Blocked by `<intake-2>`, DA5.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `play.rs` (`begin_play`, `end_play`, `PlayRestore`, `SavedDuringPlay`), the reflected `DataStatus` and `DataSettings` (`custom(history = false)`), the events `DataChanged` and `DataFailed`, `register_types`, the remaining benchmarks.
- Acceptance criteria:
  - [ ] The "Play" test of the Test plan; the reflected types register and round-trip through the text format; a test asserts `DataSettings`'s `TypeInfo` has `custom("history") == false`.
  - [ ] `play/end_play/100_edited` meets the budget; the full set of data benchmarks runs in CI.

### Assets row (`exitium_assets`)

**AK1. `feat(assets): AssetKind::Data and AssetKind::Prefab`** (about 120 lines). Blocked by `<intake-2>`, AP4 (SP4's last assets lane issue; R1).
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds the two `#[non_exhaustive]` variants, the extension rows `data` and `prefab` (both `Payload::Text`) in `path.rs`, and the registry and listing tests.
- Acceptance criteria:
  - [ ] `scene_list`-style listing by kind returns data and prefab assets; an unknown extension is unchanged; the kinds round-trip through the manifest text form (the schema hash of existing types does not change).
  - [ ] `take_loaded_guid` delivers a `Data` payload and a reload payload (`generation >= 2`) in a `MemorySource` test.

### Scene lane (`exitium_scene`)

SC1 to SC5 are SP4's issues (assets-scene-export.md "Stage 2: scene lane") and run beside the data lane. This spec adds four prefab issues after the edge PR.

### Edge PR (not an intake, C5)

**EP1. `chore(scene): add the exitium_data dependency (E1)`** (about 20 lines). Blocked by `<intake-2>`, DA6, SC5.
- Footprint: Edits `crates/exitium_scene/Cargo.toml`; Governs none; Hotspots `Cargo.toml`, `Cargo.lock`; New crate edges `exitium_scene -> exitium_data`.
- Acceptance criteria:
  - [ ] `cargo build --workspace` and the wasm build pass; `cargo tree -p exitium_scene` shows the single new edge; no other manifest changes; `exitium_data` has no new edge.

### Scene lane, prefabs (`exitium_scene`, Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none)

**SV1. `feat(scene): PrefabFile, PrefabStore and the resolve chain`** (about 600 lines; high-risk review: file format). Blocked by `<intake-2>`, EP1.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `prefab/file.rs` (`PrefabFile`, `PrefabPatch`, parse and write of root and variant), `prefab/resolve.rs` (`ResolvedPrefab`, the entity placeholder, `PrefabWarning`), `prefab/store.rs` (`PrefabStore::load_text`, the reload rules), `PrefabError`.
- Acceptance criteria:
  - [ ] The "Prefab file" group and goldens equal to this spec's examples; both forms at once, two roots, `RemoveRoot`, `NestedPrefab`, a stale override is a warning and skipped.
  - [ ] A failed prefab reload keeps the last good `Arc`; a changed base re-resolves its variants in order; first load equals reload.
  - [ ] `prefab/resolve_root/20_nodes_60_components` and `prefab/resolve_variant/20_nodes_5_overrides` meet the budgets.

**SV2. `feat(scene): instance records, instantiate and runtime spawn`** (about 600 lines). Blocked by `<intake-2>`, SV1.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds the `prefab` field to scene parse and instantiate (`@inst.node` labels, `PrefabNotLoaded`, `MissingPrefabNode`, `InstanceHasComponents`, `ParsedScene::prefab_guids`), the reflected `PrefabInstance`, `PrefabNode` and `NodeId`, `instantiate_prefab` and `SpawnOptions`, the Entity-placeholder rewrite with its per-type cache.
- Acceptance criteria:
  - [ ] The instance and spawn tests of the Test plan, including the atomic failure (entity count unchanged on `PrefabNotLoaded`) and Entity references between nodes (and a map keyed by `Entity`).
  - [ ] `prefab/spawn/5_nodes_15_components`, `prefab/spawn/1000_instances` and `instantiate/1000_instances` meet the budgets.

**SV3. `feat(scene): write instances and prefab files (diff against the resolved prefab)`** (about 600 lines; high-risk review: file format). Blocked by `<intake-2>`, SV2.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds the instance diff to `write_scene` (the six rules), `write_prefab`, `write_prefab_variant`, `SceneWarning::PrefabNodeDetached`, node-id assignment and stability.
- Acceptance criteria:
  - [ ] A scene with instances writes, reads and writes again to identical text; a moved-out node is a plain entity with a warning; node ids survive a rename; a variant save writes only the diff.
  - [ ] `write/1000_instances` meets the budget.

**SV4. `feat(scene): prefab refresh, stale instances and PrefabStatus`** (about 450 lines). Blocked by `<intake-2>`, SV3.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `refresh`, `RefreshReport` and its undo record, `PrefabStore::stale_instances`, the reflected `PrefabStatus`, the events `PrefabChanged` and `PrefabFailed`, `register_types` additions.
- Acceptance criteria:
  - [ ] The "Refresh" tests of the Test plan, including `RefreshUnavailable` and undo; the reflected types register and round-trip.
  - [ ] `prefab/refresh/1000_instances` meets the budget; the crate's wasm32 build passes.

### Facade glue (Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none)

These two run alone, in order, after the plugin lanes they glue (R2, R3). Their place among the other specs' facade issues is the orchestrator's; the lines below are only what each glue needs.

**FD1. `feat(exitium): data glue (DataPlugin)`** (about 450 lines). Blocked by `<intake-2>`, AK1, DA6, AP4, FA1 (schedule and `SimulationMode::Frozen` test pattern, AM3 of SP3).
- Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none.
- Adds `DataPlugin`: delivery through `take_loaded_guid`, events, requests for `take_wanted` and `refs_of`, `DataSettings.preload`, the `exitium::data` re-exports.
- Acceptance criteria:
  - [ ] **Schedule:** delivery runs in `First` after `ASSETS`, never in `Update`; with `SimulationMode::Frozen` a data file rewritten on disk still changes `DataStore::get_as`.
  - [ ] The facade tests of the Test plan: a variant follows a rewritten base, a corrupt rewrite keeps the old value and sends `DataFailed { reload: true }`, an own write is not reported, a data asset referenced by another is requested once.
  - [ ] The write-to-visible time of a reload is recorded in the PR (budget 500 ms); a compile test names every re-exported item.

**FD2. `feat(exitium): prefab glue, scene loading with prefabs and re-exports`** (about 500 lines; high-risk review: facade API). Blocked by `<intake-2>`, SV4, FD1, FA3.
- Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none.
- Adds `PrefabPlugin` (delivery, requests, events), the `SceneLoadState::WaitingForPrefabs` state in `LoadScene`, the `exitium::scene` re-exports of the prefab items, and the additions to `DefaultPlugins` and the sample game (a prefab variant placed twice in the sample scene, a data asset driving unit stats).
- Acceptance criteria:
  - [ ] A scene with an instance loads once its prefab is `Ready`; a prefab that stays `NotFound` ends in `SceneLoadFailed` and spawns nothing; a prefab rewritten on disk gives `PrefabChanged`, leaves live instances unchanged, and the next `LoadScene` replace shows the new prefab.
  - [ ] Every name in "Re-exports" is reachable as `exitium::<module>::<Name>` and `BaseGraph` and the overlay internals are not.

### Not in this spec

The editor wrappers and panels for the tools above belong to SP8 and the editor lane (#110), and the parity-test rows grow there. This spec only fixes the functions they wrap.

## Downstream adjustments

Applied by the architect when this spec merges (the bullets are the contract):

- **`mvp-roadmap.md`, Stage 2 and intake 2:**
  - Remove `scene -> data` (E1) from intake 2's list; add `data -> ecs` (already in the "Wiring" section above); add `exitium_data` to `ENGINE_CRATES`.
  - Replace "Scene lane: about 8 ... after the data lane (E1)" with: scene SC1 to SC5 beside the data lane, then the edge PR EP1, then SV1 to SV4 (9 scene PRs plus EP1). The Stage 2 "Why it passes" bullet for data and scene follows D1, and the critical path becomes intake 2 (1), data 6 beside scene 5, EP1 (1), SV 4, facade 18: **about 30 serial PRs** (was 32).
  - Add the facade issues FD1 and FD2 in place of the single "data glue" slot (facade 18, was 17).
  - Add the assets issue AK1 to the assets lane (R1: after AP4).
  - Mark E1 "decided: stands" in C1. The edge PR is the second edge PR of the plan (C5; the first is `exitium_editor -> exitium_ui`).
  - Close roadmap open question 3 (E1) as decided here.
- **`assets-scene-export.md`:** its "Hot reload" row "Data assets, prefab variants" is answered here (the kinds `Data` and `Prefab`, `take_loaded_guid`, the dirty-entry rule). SC1 to SC5 are unchanged; SV2 and SV3 extend the scene envelope with the `prefab` field and the instance diff, and SP4's "unknown envelope field is a warning" rule keeps old scene files loading. FA3's `LoadScene` gains the `WaitingForPrefabs` state in FD2 (FA3 is not reopened).
- **`editor-mcp-mvp.md` and SP8 (#149):** adds the 10 data tools and 8 prefab tools of "MCP / editor surface" to the tool table (about 18 tools) and to the parity test; calls `DataStore::begin_play` and `end_play` in `play_start` and `play_stop`; refuses `data_save` and `prefab_save` in Play; calls `note_own_write` after both saves; builds the Data Asset and Prefab panels. SP8 is ordered after SP10.
- **`reflect-model.md`, open question 9** (runtime-defined types): resolved as not in the MVP (open question 9 above).
- **SP9 (Luau):** reads data assets through `DataStore::get(guid) -> &dyn Reflect`.
- **#61:** unchanged.
- **`CLAUDE.md` files:** DA1 creates `crates/exitium_data/CLAUDE.md`; each scene issue updates `crates/exitium_scene/CLAUDE.md`.
