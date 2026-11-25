# Spec: Data assets and prefab variants

- Issue: #146 (roadmap SP10)
- Crates: `exitium_data`, `exitium_scene` (a one-row change in `exitium_assets` and the glue in `exitium` are listed under "Implementation issues")
- Status: draft

## Goal

Give games two kinds of authored, inheritable content. A **data asset** is a typed, reflected value in its own `.data` file (an item, an enemy stat block, a level rule set), and a **variant** of it stores only the fields that differ from a **base** data asset. A **prefab** is a reusable tree of entities in a `.prefab` file, and a **prefab variant** stores only the nodes, components and fields that differ from a base prefab. A scene places prefab instances and stores only what the instance changes. Both kinds share one "base plus overrides" engine, so a field override means the same thing in both. Both follow SP4's reload rules (a failed reload keeps the last good value; a reload goes through the same path as a first load), and both are reachable by an AI agent through MCP with the same operations a human has in the editor.

## Non-goals

- Typed handles (`DataRef<T>`, `AssetRef<T>`). Fields stay plain `Guid`s (SP4 D1). Open question 8.
- **Nested prefabs** (a prefab that contains another prefab instance). Not in this spec's MVP scope; the format reserves the field so adding them later changes no existing file. **The owner must confirm this against the no-cuts direction** (open question 2).
- **Element-level overrides of lists and maps.** A list or map field is replaced whole. Same status: the owner confirms (open question 3).
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
| D1 | **E1 (`exitium_scene -> exitium_data`) stands.** The shared "base plus overrides" engine is the module `exitium_data::overlay`, not a foundation crate. The edge is **not** in intake 2: a one-crate *edge PR* (EP1) adds it after the data lane and SC5, so SC1 to SC5 run beside the whole data lane (C5). | A serial amendment needs R1 clearance against the live serial lane and puts a data concept into a frozen foundation crate; a new foundation crate is built before its first consumer. The edge PR keeps the parallelism the roadmap hoped to get from removing E1 (see "Edge E1"). The owner can still choose option B (open question 1). |
| D2 | `exitium_data -> core, ecs, reflect, serial`. **No `exitium_app`, no `exitium_command`, no `exitium_assets`.** No `Plugin`. | `ecs` only so `DataStore` can `impl Resource`. The data lane never waits for the app spine, and the crate follows C1 and D7 of SP4. |
| D3 | A data type is a reflected **struct** (named or tuple) that carries the type data `ReflectDataAsset` (`#[reflect(data(exitium_data::ReflectDataAsset))]`). It is a marker. | The type list for tools and the file loader come from the registry; no hand-written schema. A struct root keeps override paths simple. |
| D4 | A `.data` file is the envelope `DataFile { base, value }`. `AssetKind::Data`, extension `data`, payload `Payload::Text`. | One path per asset kind (SP4 D13). |
| D5 | Override paths recurse through **structs** only. Every other field (value, enum, list, map, option, built-in tuple, transparent wrapper) is a leaf and is replaced whole. **An `Overrides` set never holds an entry whose target is a struct:** `set` at a struct path expands to one entry per leaf. A non-struct root (an enum or Value component) is one entry at the empty path. | One rule, easy to read and diff; reading and writing round-trip because every entry is a leaf. |
| D6 | One engine, `overlay`: `Overrides` (read, write, apply, diff, merge3) and `BaseGraph` (cycle and depth check, chain, dependents in topological order). Data assets and prefab patches both use it. | One definition of "an override" (E1). |
| D7 | `DataStore::load_text` is the single path for a first load and a reload. A hard error of any kind keeps the last good value and reports the failure. | SP4 rules 1 and 2 (D12, D13). |
| D8 | A data asset with unsaved in-memory edits ("dirty") does not take a disk reload, and a failed or parked disk text is never overwritten silently: the text is kept in `pending_disk`, and `data_save` refuses (`DiskChanged`) unless `overwrite: true`. | A reload must not destroy an edit, and an edit must not hide a disk change. |
| D9 | Saving goes `write_data` -> `native::write_atomic` -> `note_own_write(WatchKey::Asset(guid))` -> clear dirty. | SP4: own writes are not reported as external changes. |
| D10 | Play-in-editor: `begin_play` starts a journal of first edits; `end_play` restores them, removes assets created during play, and **keeps** disk reloads. `DataStore` is not part of the generic resource snapshot. | Stage 5: "Data Asset edits made while playing are reverted on Stop (the registry is part of the snapshot)". |
| D11 | A `.prefab` file is **root** (`base: None`, a list of nodes) or **variant** (`base: Some`, a patch). Node ids are stored labels, stable across saves. | Variants and instances refer to nodes by id; ids that re-derive from `Name` would break them. |
| D12 | A scene places an instance as one entity record with `prefab: Some((asset, patch))`. The record's `id` is the instance root label; inner nodes are `@<instance>.<node>`. | A scene instance is an anonymous variant attached at a location, so it reuses the patch format and the engine. |
| D13 | The instance root's saved state is its patch, computed by diffing the live entities against the resolved prefab at save time, **plus the stale entries kept from the load** (D14). | The editor edits live entities; the file stores only differences, and nothing a file said is lost. |
| D14 | **Three failure classes for a patch entry, no more.** (1) *Parse error*: the text is malformed; the whole file fails. (2) *Patch conflict*: the patch contradicts itself; the whole file fails. (3) *Stale*: the entry no longer fits its base (missing node, component or field, a value that no longer binds, an unknown component type); it is **skipped with a warning, kept as text, and written back on save** unless the caller passes `drop_stale`. | A base can change under a variant (a Rust field is renamed); the variant must still load, and the next save must not destroy what the file said (SP4: "dropping it silently would lose data on the next save"). The same classes apply to data variants and prefab variants, so the "overridden" marker means one thing. |
| D15 | A prefab reload updates the store and raises `PrefabChanged`. Live instances are updated only by `prefab_refresh` (a per-node 3-way merge); a saved scene gets the new prefab on its next load. | No surprise changes to a running or half-edited world. |
| D16 | All mutation is a plain Rust function that returns an undo record (`DataEdit`, `PrefabEdit`). The editor lane wraps them as commands and tools. These crates have no `exitium_command` edge. | C1 and `editor-mcp-mvp.md`: only crates on C1's list have the command edge. Agent parity holds because the editor lane (#110, SP8) owns the wrappers and the parity test. |
| D17 | No new crates.io dependency. | std plus serial and reflect are enough ("Dependencies"). |

### Edge E1: the decision

**E1 stands.** `exitium_scene` depends on `exitium_data`, and the overlay engine lives in `exitium_data::overlay`.

The three ways to remove the edge, and why none is taken:

| Option | Cost | Verdict |
|---|---|---|
| A. Keep E1, engine in `exitium_data::overlay` (chosen). | One allowed leaf edge. Scene waits for the data lane before its prefab issues. | The edge is added late (EP1), so the wait covers only the prefab issues. |
| B. Move the engine into `exitium_serial`. | The serial lane (about 17 PRs) runs beside ECS (28), so 3 more serial PRs are not on the critical path. The cost is elsewhere: an amendment to `serial-text-format.md` needs R1 clearance against the live serial lane, outside this spec's footprint, and it puts an "override" (a data concept) into a frozen foundation crate. | Not chosen. Open question 1. **Option B also removes EP1** and lets the prefab issues follow SC5 directly: the chain falls from about 33 to about 31. |
| C. A new foundation crate `exitium_overlay`. | A new workspace member (C8, hotspot work at the intake), a frozen crate for about 800 lines, built before its first consumer. | Rejected. |

**Parallelism.** Scene depends on data only through code that uses the overlay engine, and only the prefab issues use it. SC1 to SC5 do not. Therefore:

- Intake 2 does **not** add `scene -> data`. Scene and data are independent crates until EP1, so R3 does not tie them.
- DA1 to DA6 (data) run beside SC1 to SC5 (scene) and beside the `exitium_app` spine.
- EP1 adds the edge. It is `Blocked by` DA6 and SC5. It edits one crate manifest (`exitium_scene/Cargo.toml`) and holds `Cargo.toml` and `Cargo.lock` (C5), so it runs alone.
- The prefab issues SV1a to SV4 are `Blocked by` EP1, in order.

**Chain length for Stage 2:** intake 2 (1), then max(data 6, SC 5) = 6, EP1 (1), prefab issues (7), then the facade (18: FD1 and FD2 replace the roadmap's single "data glue" slot): **about 33 serial PRs**, against 32 in the roadmap (1 + 6 data + 8 scene + 17 facade). The edge PR saves the five SC PRs a serial wait, but the prefab work is now 7 PRs instead of 3, so the net is one longer. The number changes with open questions 1, 2 and 3 (see there). "Downstream adjustments" lists the roadmap edits, which must land before intake 2 (#157) starts.

### Crate map

```
exitium_data   -> core, ecs, reflect, serial            (no app, no command, no assets)
exitium_scene  -> core, ecs, reflect, serial            (intake 2: SP4)
exitium_scene  -> exitium_data                          (E1, added by the edge PR EP1)
exitium (facade) -> data, scene, assets, ...            (intake 2)
exitium_editor -> data                                   (intake 3, C3; already in the roadmap)
```

Other leaf crates cannot name data or prefab types (C1). Game systems reach data through the facade (`exitium::data`), and components refer to data and prefabs by `Guid`.

**Determinism.** Every map in both crates is `HashMap<K, V, BuildHasherDefault<DefaultHasher>>` (std, fixed keys, no new dependency), and every listing (`list`, `failures`, `Loaded.changed`, event order) is sorted by `Guid`, so output does not depend on the process or the Rust version. `dependents_topo` breaks ties by `Guid`.

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

Requirements: `Reflect + FromReflect + Clone + Send + Sync`, and a struct root. `Default` is needed only to create a new asset with empty content (`data_create` without text). A type without `ReflectDataAsset` is refused with `NotADataAsset`, and a registered type that is not a struct is `RootNotStruct`, so a component cannot be loaded as a data asset by mistake.

#### File format

Header `%exitium-text 1 root exitium_data::DataFile`, extension `.data`. `DataFile` is **not** a registered reflected type (the same rule as `SceneFile`); this crate reads and writes the envelope, and `exitium_serial` reads and writes the leaf values (`bind`, `write_value`). The examples below are **illustrative**: the committed golden files, produced by the writer and reviewed, are the reference for layout.

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
  value: glutton::ItemDef(damage: 15.0),
)
```

Rules:

- **Envelope.** `base` is `None` or `Some("<uuid>")` (a `Guid`; a `// path` comment is regenerated on write and ignored on read). `value` is required. An unknown envelope field is a warning, except the reserved names of "Reserved for later" below, which are an error.
- **Prototype.** `value` is bound in `Complete` mode: missing fields are filled only from explicit `#[reflect(default)]` (serial-text-format.md). Unknown fields use the serial default, `Warn`.
- **Variant.** `value` is read as overrides ("Override engine"). An override that no longer fits the base chain is **stale** (D14): skipped, reported as `DataWarning::Stale { path, reason, suggestions }` (up to 3 near-miss suggestions), kept as text and written back on save. A tag that differs from the base chain's root type is `BaseTypeMismatch`, a hard error.
- **Layout.** The writer fixes the envelope layout. A prototype value follows serial's layout rules. A sparse group is framed by this crate with the **same predicate** serial uses for groups (serial-text-format.md "Layout": flat only when every member is leaf-like and there are at most 4 members, otherwise one member per line with a trailing comma; a comment on a member forces a break), and each leaf is written by `write_value`. Overrides are written in path order (fields in declaration order), then the kept stale entries in their file order, so one changed field is one hunk in a diff.
- **Limits.** `max_data_bytes` 1 MiB per file (the SP4 `max_text_bytes` applies first), `max_chain_depth` 8, `max_overrides` 4,096 per variant. Errors, not truncation.

**Reserved for later.** The envelope names `list_ops` (element-level list and map overrides) and, in prefab records, `prefab` inside a prefab file (nested prefabs) are reserved. A file that uses one is rejected today with `UnsupportedFeature { name }`, an error and not a warning, so a file written by a future engine fails loudly on an older one instead of losing data. Every file written today stays valid when the features arrive (open questions 2 and 3).

### Override engine (`exitium_data::overlay`)

The engine is plain data and functions. It knows nothing about assets, entities or files.

```rust
/// A path into a reflected value through structs only: "stats.hp", "tint.r", "0".
/// The empty path is the root, and is valid only when the root type is not a struct.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldPath(Vec<Box<str>>);
impl FieldPath { pub fn root() -> FieldPath; pub fn is_root(&self) -> bool; }

pub struct FieldOverride { pub path: FieldPath, pub value: Box<dyn Reflect> }   // value is a complete value of the leaf type

/// A set of overrides on one type. Sorted by path. Every entry is a leaf: no entry's target is a struct,
/// and no path is a prefix of another.
pub struct Overrides { /* Vec<FieldOverride> */ }

impl Overrides {
    pub fn is_empty(&self) -> bool;  pub fn len(&self) -> usize;
    pub fn iter(&self) -> impl Iterator<Item = &FieldOverride>;
    pub fn get(&self, path: &FieldPath) -> Option<&dyn Reflect>;
    /// Normalises. At a leaf path: replaces that entry only (siblings are untouched).
    /// At a struct path: the value is expanded to one entry per leaf (recursively through structs),
    /// replacing every entry at or below that path. At the root of a non-struct type: one entry at the empty path.
    pub fn set(&mut self, ty: &TypeInfo, path: FieldPath, value: Box<dyn Reflect>) -> Result<(), OverlayError>;
    pub fn remove(&mut self, path: &FieldPath) -> Option<Box<dyn Reflect>>;       // a leaf path, or a struct path (removes every entry below it)

    /// Reads a sparse group from the syntax tree. Walks struct fields itself and calls `bind` in Complete mode per leaf field.
    /// A value at a non-struct root is read as one entry at the empty path.
    pub fn read(syntax: &Syntax, group: NodeId, ty: &TypeInfo, registry: &TypeRegistry, opts: &ReadOptions) -> Result<ReadOverrides, OverlayError>;
    /// Writes a sparse tagged group (nested groups rebuilt from the sorted paths). A non-struct root is written as a complete group.
    pub fn write(&self, ty: &TypeInfo, registry: &TypeRegistry, out: &mut String, indent: usize) -> Result<(), OverlayError>;
    /// Validates every path first, then applies. On error the target is unchanged.
    pub fn apply(&self, target: &mut dyn Reflect) -> Result<(), OverlayError>;

    /// The leaves where `derived` differs from `base`, as overrides taken from `derived`. Leaf equality is `leaf_eq`.
    pub fn diff(base: &dyn Reflect, derived: &dyn Reflect) -> Overrides;
    /// Three-way merge for a refresh: a leaf the user changed (live != old) keeps the live value; every other leaf takes the new base value.
    pub fn merge3(old_base: &dyn Reflect, new_base: &dyn Reflect, live: &mut dyn Reflect) -> Result<MergeReport, OverlayError>;
}

/// What `read` returns: the entries that bind, and the entries kept as text because they are stale (D14).
pub struct ReadOverrides { pub overrides: Overrides, pub stale: Vec<StaleEntry> }
pub struct StaleEntry { pub path: FieldPath, pub text: String /* the original member text, verbatim */, pub reason: StaleReason }
```

`StaleReason` is `#[non_exhaustive]`: `UnknownField`, `PathThroughLeaf`, `TypeChanged { expected }` (the value parses but no longer binds), `EnumVariantGone`.

**Equality (`leaf_eq`).** `reflect_eq` returns `Option<bool>`; `None` means "different". Floats compare by bit pattern (`to_bits`) wherever they occur in a leaf (a plain float, or inside a list, option or tuple), so `0.0` against `-0.0` is a difference and a NaN equals the same NaN. Every other leaf uses `reflect_eq`.

**Why leaf-wise and whole replacement.** `Reflect::apply` resizes a list and recurses element by element, so a "partial list" would silently keep stale tail elements. The engine therefore never calls `apply` for a leaf. It reads the field with `bind` in `Complete` mode and replaces the field with `Reflect::set` (found with `reflect_path_mut`). A non-transparent tuple struct recurses like a struct (paths `0`, `1`). A `transparent` wrapper is resolved to its inner type first, so `hp` stays a leaf whether it is a plain `f32` or a wrapper around one.

```rust
/// The base relation of a set of assets, shared by data and prefab stores. `base` is single-valued, so it is a forest.
pub struct BaseGraph { /* Guid -> Option<Guid> base; reverse index */ }

impl BaseGraph {
    pub fn set_base(&mut self, guid: Guid, base: Option<Guid>) -> Result<(), OverlayError>;
    pub fn base_of(&self, guid: Guid) -> Option<Guid>;
    pub fn chain(&self, guid: Guid) -> Vec<Guid>;                 // [guid, its base, ..., the root prototype]
    pub fn dependents_topo(&self, guid: Guid) -> Vec<Guid>;       // transitive, bases before variants, ties by Guid
    pub fn remove(&mut self, guid: Guid);
}
```

`set_base` checks the cycle (`BaseCycle`) and the **depth of the whole result**, not only the entry's own chain: `depth(chain of the new base) + 1 + height(guid's subtree of variants) <= max_chain_depth` (8), else `BaseTooDeep`. Without the subtree term, a chain of 8 could grow to 15 by setting a base on its root.

`OverlayError` is `#[non_exhaustive]`: `UnknownPath { path, suggestions }`, `PathThroughLeaf { path }`, `TypeMismatch { path, expected, found }`, `RootNotStruct`, `BaseCycle(Vec<Guid>)`, `BaseTooDeep`, `Text(SerialError)`, `TooManyOverrides`. Prefix conflicts cannot exist because `set` normalises.

### `DataStore`

`DataStore` is a `Resource` (`impl Resource for DataStore`). It is **not** reflected (it holds `Box<dyn Reflect>` values and a graph); `DataStatus` is the reflected view.

```rust
pub struct DataStore { /* entries: FixedMap<Guid, Entry>, graph: BaseGraph, wanted: Vec<Guid>, journal: Option<PlayJournal>, revision: u64 */ }

struct Entry {
    base: Option<Guid>,
    source: Source,                       // Full(Box<dyn Reflect>) for a prototype (the resolved value *is* the source, stored once)
                                          // Overrides(Overrides) for a variant
    stale: Vec<StaleEntry>,               // kept text of stale overrides (D14), re-emitted on save
    resolved: Option<Box<dyn Reflect>>,   // None for a variant until its base chain is Ready; for a prototype it is the source
    state: DataState,
    generation: u32,                      // +1 on every change of the resolved value
    edit_rev: u32,                        // +1 on every change of the source, from any cause (edit, reload, undo)
    dirty: bool,                          // in-memory edit not yet saved
    pending_disk: Option<String>,         // disk text the store did not apply: parked behind an edit, or a text that failed to load
    force_next: bool,                     // the next payload is applied even though the entry is dirty (data_reload take_disk)
    last_error: Option<DataError>,
    warnings: Vec<DataWarning>,
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
    pub fn info(&self, guid: Guid) -> Option<DataInfo>;               // type name, base, state, generation, dirty, override count, last error, warnings
    pub fn list(&self) -> impl Iterator<Item = DataInfo> + '_;        // sorted by Guid
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

1. Parse the envelope and `value` (`DataFile`). A hard error (parse error, `BaseTypeMismatch`, `NotADataAsset`, `UnknownType`, `TooLarge`) returns `Err` and **changes nothing**: a first load stays absent and is marked `Failed`; a reload keeps the old value and source, sets `last_error`, and keeps the text in `pending_disk`.
2. Set the base in the graph (`BaseCycle` and `BaseTooDeep` are hard errors). A variant whose base is not in the store yet becomes `Waiting(base)` and the base joins `wanted`.
3. Resolve the entry: a prototype is the value; a variant is `clone_boxed` of its base's resolved value with `Overrides::apply`. Stale overrides are skipped and listed in `warnings`; they are retried on every later resolve, so one that binds again (the field came back) is applied.
4. If the resolved value changed (`leaf_eq` on the whole value), bump `generation`, then re-resolve every entry in `graph.dependents_topo(guid)` the same way. A dependent that fails a hard error keeps its last good value and sets its own `last_error`; its own dependents are not re-resolved.
5. Return `Loaded { guid, generation, changed: Vec<Guid>, failed: Vec<(Guid, DataError)> }`, sorted by `Guid`. The glue turns it into events.

All of it is one function, so a first load, a reload, a dependent re-resolve after its base arrived, and a test with text in memory use the same code (SP4 D13).

**A `Waiting` entry must fail loudly.** The glue reports base load failures to the store:

```rust
pub fn note_failed(&mut self, guid: Guid, reason: BaseFailure) -> Vec<Guid>;   // marks every entry Waiting on `guid` as Failed
pub enum BaseFailure { Failed { message: String }, NotData }                    // the server's AssetFailed / NotFound, or a base whose registry kind is not Data
```

An affected entry gets `last_error = BaseFailed { base }` or `BaseNotData { base }` and `DataFailed`; it leaves `Waiting` so the status shows the cause. A later successful load of the base makes it resolve (the failure was not terminal).

`DataError` is `#[non_exhaustive]`: `Parse { message, span }`, `UnknownType { name, suggestions }`, `NotADataAsset { type_name }`, `RootNotStruct`, `BaseTypeMismatch { expected, found }`, `BaseCycle(Vec<Guid>)`, `BaseTooDeep`, `BaseFailed { base }`, `BaseNotData { base }`, `Overlay(OverlayError)`, `TooLarge { bytes, limit }`, `DiskChanged`, `StaleEdit`, `SavedDuringPlay`, `NothingToSave`, `UnsupportedFeature { name }`. Every text error carries the line and column. `DataWarning::Stale { path, reason, suggestions }` is the only warning kind in v1.

**Writes** (the editor lane calls these; see "MCP / editor surface").

```rust
pub struct WriteOptions { pub drop_stale: bool /*false: the kept stale entries are written back*/ }
pub fn write_data(&self, guid: Guid, registry: &TypeRegistry, resolve_path: &dyn Fn(Guid) -> Option<String>, opts: &WriteOptions) -> Result<String, DataError>;
                                          // NothingToSave for an entry that never loaded; path comments from the callback

// Authoring. Each returns an undo record; applying the record restores the entry (and re-resolves dependents).
pub fn create(&mut self, guid: Guid, type_name: &str, base: Option<Guid>, registry: &TypeRegistry) -> Result<DataEdit, DataError>;   // default value, or an empty variant
pub fn set_field(&mut self, guid: Guid, path: &FieldPath, value_text: &str, registry: &TypeRegistry) -> Result<DataEdit, DataError>;   // prototype: sets the field; variant: adds or replaces an override
pub fn revert_field(&mut self, guid: Guid, path: &FieldPath) -> Result<DataEdit, DataError>;        // variant only: removes the override (a struct path removes all below it)
pub fn flatten(&mut self, guid: Guid, registry: &TypeRegistry) -> Result<DataEdit, DataError>;      // variant becomes a prototype with its resolved value (unlink); stale entries are dropped
pub fn replace_from_text(&mut self, guid: Guid, text: &str, registry: &TypeRegistry) -> Result<DataEdit, DataError>;   // data_reload with the disk text
pub fn apply_edit(&mut self, edit: DataEdit, registry: &TypeRegistry) -> Result<DataEdit, DataError>;   // undo and redo: returns the inverse; StaleEdit if edit_rev moved
pub fn mark_saved(&mut self, guid: Guid);                                                            // clears dirty after the file was written
pub fn remove(&mut self, guid: Guid) -> Result<DataEdit, DataError>;                                 // refused while a variant still has this base (the error lists them)
```

A `DataEdit` holds the entry's **before** state (source, stale list, base, dirty flag) and the `edit_rev` it applies to. It lives in memory only, like `AssetEdit` in SP4, and is not written to any file.

**Undo after a reload.** Any change of an entry's source, a disk reload included, bumps `edit_rev`. `apply_edit` on a record whose `edit_rev` is older returns `StaleEdit` and changes nothing, so an undo can never overwrite a reloaded value with a pre-reload copy. The editor lane drops the history entries of a guid whenever `Loaded.changed` names it from a disk reload (it already clears entries for other reasons), and the `StaleEdit` check is the safety net.

`set_field` on a prototype changes the resolved value and re-resolves its variants; a variant that does not override that field then sees the new value. `set_field` on a variant stores an override even if the value equals the base's, so "overridden" is an explicit state a user can revert. (A prefab instance patch computed by `diff` cannot hold an override equal to its base, because it records what *is*, not what was *chosen*; "Writing: instances and prefabs". In both, the marker means "stored in the patch".)

### Hot reload and edits (relation to SP4)

Data assets use SP4's two consumer rules unchanged:

1. **One swap point per asset.** The glue's system runs in `First`, after set `ASSETS`. A frame sees one value of an asset; the swap never happens in `Update`. It runs in Edit mode too (`SimulationMode::Frozen`), as SP4 requires of authoring-time systems.
2. **A bad reload keeps the good value.** `load_text` returns the error; the entry keeps its value, `last_error` is set, `DataFailed { reload: true }` is sent, and `AssetFailed` from the server is mirrored into the same list.

What is new is the relation to **unsaved edits** (the dirty flag) and to disk text the store could not apply (`pending_disk`):

| Entry state | What happens | Result |
|---|---|---|
| Clean | A reload payload (`generation >= 2`) arrives. | `load_text` runs at the swap point. `DataChanged` is sent. |
| Clean | The payload fails a hard error. | The value stays; `last_error` is set; the bad text is kept in `pending_disk`. |
| Dirty | A reload payload arrives. | The text is parked in `pending_disk`. The entry keeps its edited value. `DataStatus.conflicts` lists it, and the editor shows "changed on disk". No event of change. |
| Any, `pending_disk` set | `data_save`. | Refused with `DiskChanged { guid }` and the parked text in the result. The caller retries with `overwrite: true` (or resolves with `data_reload`). |
| Dirty, no `pending_disk` | `data_save`. | The file is written atomically, `note_own_write` is called, dirty is cleared. |
| Any | `data_reload { take_disk: true }` with `pending_disk` set. | `replace_from_text` applies the parked text now; undoable (`DataEdit`). |
| Any | `data_reload { take_disk: true }` with **no** `pending_disk`. | The command calls `AssetServer::reload(guid)` and sets `force_next`, so the next payload is applied even if the entry is dirty; the result says `reload_requested`. The value changes at the next swap point (one frame later), when `DataChanged` is sent. |
| Any | `data_reload { take_disk: false }`. | `pending_disk` is dropped, the edit stays. |

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

Header `%exitium-text 1 root exitium_scene::PrefabFile`, extension `.prefab`, `AssetKind::Prefab`, payload `Payload::Text`. Like `SceneFile`, `PrefabFile` is not a registered reflected type. The examples are illustrative (components shortened); the committed goldens are the reference. The envelope, every record, and every patch entry are always laid out broken, one member per line, as scenes are.

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
      ],
    ),
    (
      id: @weapon,
      parent: Some(@root),
      components: [
        exitium_scene::Name("Weapon"),
      ],
    ),
  ],
)
```

A **variant** has `base: Some(guid)` and a `patch`; the two forms are mutually exclusive (`PrefabShape`). The patch has three lists, each of which may be omitted when empty:

```
%exitium-text 1 root exitium_scene::PrefabFile
(
  base: Some("3c6e5d2a-91b4-8d0e-a3f1-5b7c2d9e4a10"), // prefabs/goblin.prefab
  patch: (
    overrides: [
      (
        node: @root,
        set: [
          exitium_render::Sprite(
            tint: (r: 1.0, g: 0.4, b: 0.4, a: 1.0),
          ),
        ],
        add: [
          glutton::Health(max: 40, current: 40),
        ],
        remove: [exitium_physics::CircleCollider],
      ),
    ],
    added: [
      (
        id: @shield,
        parent: Some(@root),
        components: [
          exitium_scene::Name("Shield"),
        ],
      ),
    ],
    removed: [@weapon],
  ),
)
```

#### Patch semantics (one definition, used by variants and by scene instances)

A patch is applied to the resolved base **in this fixed order**:

1. `removed`: every listed node and its descendants are removed.
2. `added`: nodes are appended in list order, each after its parent's last child.
3. `overrides`: entries in list order; within one entry the steps are **`remove`, then `add`, then `set`**.

`remove` of a type then `add` of the same type replaces the component whole; `remove` then `set` of the same type finds nothing to set (stale). `add` of a type the node already has replaces the component whole. `set` holds **sparse** tagged groups read by `Overrides::read` (struct paths, leaves replaced whole, a non-struct component as one complete value). `add` holds **complete** tagged groups. In `set`, `add` and `remove`, component types are written by their registered type name.

**Every failure of a patch entry has exactly one class (D14):**

| Case | Class | Result |
|---|---|---|
| Malformed text (syntax) | Parse error | The file fails (`PrefabError::Parse`). A reload keeps the last good value. |
| `removed` names the root | Patch conflict | `RemoveRoot`. |
| An added node with `parent: None` | Patch conflict | `AddedRoot`. |
| An added id equal to **any** id in the base chain (including one removed by this patch), or two added nodes with one id | Patch conflict | `DuplicateNodeId { id }`. Ids are never reused within a chain. |
| An added node whose parent is removed **by this patch** | Patch conflict | `AddedUnderRemoved { id, parent }`. |
| `overrides` for a node that this patch also removes | Patch conflict | `OverrideOnRemoved { node }`. |
| The same node in two `overrides` entries, or the same component type twice in one entry's `set` (or `add`) | Patch conflict | `DuplicateOverride { node }`. |
| `overrides` for a node the base chain does not have (it never had it, or a base patch removed it) | Stale | Skipped; `PrefabWarning::StaleOverride`. |
| An added node whose parent is not in the resolved base (the base lost it) | Stale | The node and the nodes added under it are skipped; `StaleAddedNode`. |
| `set` for a component the node lacks, or a field the component no longer has | Stale | Skipped. |
| `set` whose value no longer binds (field type changed, enum variant gone, a path now goes through a leaf) | Stale | Skipped, with the reason `TypeChanged` and the bind message. A bind failure never aborts the load. |
| A component type name that is not registered, in `set`, `add` or `remove` | Stale | Skipped; the warning suggests up to 3 registered names. **This relaxes SP4's "unknown component type is always an error" for patch entries only**: SP4's rule exists so a save cannot drop data silently, and stale entries are kept and written back, so nothing is dropped. The plain `components` lists of scenes and root prefabs keep SP4's error. |
| `remove` of a component the node lacks | Stale | Skipped. |

A stale entry is kept as text (`StaleEntry`) in the in-memory patch and written back on save unless the caller passes `drop_stale` (`SaveOptions`, `prefab_save`, `data_save`). It is retried on every resolve, so one that fits again (the field came back) is applied. Patch conflicts are hard errors even inside a scene instance, because they cannot come from a base changing: they mean the file contradicts itself.

- **Entity references** inside a prefab use node labels (`target: @root`), through the same `TextRefs` hook as scenes. A reference to anything outside the prefab is `ReferenceOutsidePrefab` on write.
- Reparenting a base node is not possible in a patch (open question 5).
- Limits: `max_prefab_nodes` 10,000, `max_chain_depth` 8, `max_overrides` 4,096 per patch.

#### Resolved form and the store

```rust
pub struct ResolvedPrefab {
    pub guid: Guid, pub generation: u32, pub chain: Vec<Guid>,
    pub nodes: Vec<ResolvedNode>,                 // pre-order; nodes[0] is the root
    pub warnings: Vec<PrefabWarning>,
    pub stale: Vec<StaleEntry>,                   // the kept text, per node (D14)
}
pub struct ResolvedNode { pub id: Box<str>, pub parent: Option<u32>, pub components: Vec<Box<dyn Reflect>> /* sorted by type name; pre-bound */ }
```

Components are bound **once at resolve time** (not at each spawn), so a runtime spawn is `clone_boxed` plus `ReflectComponent::insert`. Entity-typed fields cannot be bound to real entities yet, so they hold a **placeholder** `Entity`: `Entity::from_bits(((u32::MAX as u64) << 32) | node_index)`, which `exitium_ecs` offers as a public, checked constructor (ecs-storage-queries.md). A real entity reaches generation `u32::MAX` only after 2^32 - 1 frees of one slot, and the slot is retired then, so the generation `u32::MAX` is **reserved for placeholders** and the spawner asserts that no world entity has it. The spawner rewrites placeholders to the spawned entities. Whether a component type holds `Entity` is found by walking its `TypeInfo` once and cached per `TypeId`; types without `Entity` fields skip the rewrite entirely. A map keyed by `Entity` is rebuilt through `Map::insert`.

```rust
#[derive(Resource)]
pub struct PrefabStore { /* sources and resolved: FixedMap<Guid, Arc<ResolvedPrefab>>, graph: BaseGraph (exitium_data::overlay), wanted, ... */ }

impl PrefabStore {
    pub fn load_text(&mut self, guid: Guid, text: &str, registry: &TypeRegistry) -> Result<PrefabLoaded, PrefabError>;   // the same rules as DataStore::load_text
    pub fn get(&self, guid: Guid) -> Option<Arc<ResolvedPrefab>>;
    pub fn want(&mut self, guid: Guid);  pub fn take_wanted(&mut self) -> Vec<Guid>;
    pub fn note_failed(&mut self, guid: Guid, reason: BaseFailure) -> Vec<Guid>;
    pub fn remove(&mut self, guid: Guid) -> Result<PrefabEdit, PrefabError>;      // refused while a variant has this base
    pub fn flatten(&mut self, guid: Guid, registry: &TypeRegistry) -> Result<PrefabEdit, PrefabError>;   // variant becomes a root prefab with its resolved nodes
    // authoring: create, create_variant, ... return PrefabEdit undo records
}
```

`PrefabStore` repeats `DataStore`'s rules on purpose and uses the same `BaseGraph`: one `load_text` for a first load and a reload, a failed reload keeps the last good `Arc<ResolvedPrefab>`, a changed base re-resolves its variants in topological order, and a prefab with a missing base is `Waiting(base)` and fails loudly through `note_failed`. Prefabs have no dirty or conflict state: a prefab file is edited by `prefab_save` (an own write) or by an external tool, and the editor never holds unsaved prefab edits apart from the live entities in the scene. `PrefabStore` is **not** where instances are tracked (see "Instance origin").

#### Instances in a scene

An entity record in a scene file gains an optional field. As everywhere in a scene file, records and patch entries are laid out broken:

```
(
  id: @goblin_1,
  parent: None,
  prefab: Some((
    asset: "3c6e5d2a-91b4-8d0e-a3f1-5b7c2d9e4a10", // prefabs/goblin.prefab
    patch: (
      overrides: [
        (
          node: @root,
          set: [
            exitium_app::Transform2d(
              translation: (x: 4.0, y: 2.0),
            ),
          ],
        ),
      ],
    ),
  )),
),
(
  id: @goblin_hat,
  parent: Some(@goblin_1.weapon),
  components: [
    exitium_scene::Name("Hat"),
  ],
),
```

- The record's `id` is the **instance root label**. The prefab's root node is that entity. Every other node `n` is the entity labelled `@<instance>.<n>` (`@goblin_1.weapon`). A label may contain `.`, so these are valid labels (SP4 label token). The instance's `Transform2d` is an override of the root node (as above).
- `components` of an instance record must be empty or omitted (`InstanceHasComponents`). An entity added under an instance is an ordinary scene record whose `parent` is an instance label, as `@goblin_hat` shows. Its `SceneMember` is the scene's, so it saves normally.
- `patch` is the same shape and the same semantics as a variant's (above), and `node:` names a **prefab-local** id (`@root`), not an instance label. Entity references in an instance patch's values use **scene labels** (`@goblin_1.weapon`), because the values are bound after the instance's entities exist; in a `.prefab` variant they use prefab-local labels.
- **Instantiate** follows SC4's atomic algorithm: the instance's surviving nodes (the resolved prefab minus the patch's `removed`) and its `added` nodes are spawned first, then values are bound, then the patch is applied to the live entities. Any failure rolls the whole scene back, as in SC4. The result is one `SceneInstance` with entities in this order: **an instance record contributes its nodes in prefab pre-order, root first, then its added nodes in list order**, at the record's position in the file, and `write_scene` reports entities in the same order. (`play_stop` builds its `EntityRemap` from this order, so selection and entity references into instance nodes remap correctly.)
- **Parse and load.** `ParsedScene::parse` accepts a label `@x.y` as declared by instance `@x` without resolving the prefab (the prefab may not be loaded). `instantiate` resolves the prefab and reports `UnknownLabel` for an inner label that does not exist (with suggestions). A prefab that is not in the `PrefabStore` fails `instantiate` with `PrefabNotLoaded { guid }` before anything is spawned. `ParsedScene::prefab_guids()` lets the glue request them first.
- **Scene API plumbing.** `InstantiateOptions`, `validate_scene` and `SaveOptions` each gain `prefabs: Option<&PrefabStore>` (and `SaveOptions` gains `drop_stale: bool`). `validate_scene` runs its scratch-world instantiate with the store it is given. A scene with an instance and `prefabs: None` is `PrefabStoreMissing`. The editor's `scene_load`, `scene_validate` and `scene_save` pass the store; `scene_load` requests missing prefabs and returns `PrefabsPending { guids }` so the agent can retry, while the facade's `LoadScene` waits (see "Facade glue"). These are listed under "Downstream adjustments".
- **A parent that no longer exists.** If a scene entity's parent label names an instance node that the current prefab no longer has, the entity attaches to the instance root and `SceneWarning::MissingPrefabNode { label }` is recorded.
- **Nesting.** A prefab file that contains an instance record is `NestedPrefab` (an error, reserved: "Reserved for later").

#### Runtime components (reflected)

```rust
#[derive(Component, Reflect, Clone)] #[reflect(Component, type_name = "exitium_scene::PrefabInstance", custom(derived = true))]
pub struct PrefabInstance { pub asset: Guid, pub generation: u32 }              // on the instance root

#[derive(Component, Reflect, Clone)] #[reflect(Component, type_name = "exitium_scene::PrefabNode", custom(derived = true))]
pub struct PrefabNode { pub instance: Entity, pub node: String }                 // on every spawned node, the root included

#[derive(Component, Reflect, Clone)] #[reflect(Component, type_name = "exitium_scene::PrefabNodeId", custom(derived = true))]
pub struct PrefabNodeId(pub String);                                             // authored root-prefab entities: the stable node id
```

All three are `derived`, so a scene write skips them (the instance record carries the same information). The type is `PrefabNodeId`, not `NodeId`, because `exitium_serial::NodeId` (a syntax-tree node) is in scope in the scene crate and the facade.

#### Instance origin

An instance needs to remember which resolved version it was spawned from (refresh needs the *old* base) and which stale entries its patch carried (save must write them back). This is one **non-reflected component** on the instance root, written by whoever spawns the instance:

```rust
#[derive(Component)]
pub struct InstanceOrigin { pub prefab: Arc<ResolvedPrefab>, pub stale: Vec<StaleEntry> }
```

- It is written by `instantiate_prefab` (when `SpawnOptions.track_origin` is true, the default) and by the scene `instantiate`. It dies with the entity, so runtime-spawned bullets cannot grow a table; a game that spawns thousands of short-lived instances sets `track_origin: false` and pays for a `PrefabInstance` only. Such an instance cannot be refreshed (`RefreshUnavailable`).
- Old resolved versions are kept alive only by the `Arc`s that instances hold, and are freed when the last instance refreshes or despawns.
- **Undo of a despawn.** The editor restores a despawned instance under new entity ids (editor-mcp-mvp.md). It does that by writing the instance with `write_scene` (`SaveScope::Entities`) and instantiating the text again, which creates a fresh `InstanceOrigin` and keeps the stale entries. The editor lane must not capture instance entities component by component (a non-reflected component would be lost); this is a stated requirement on SP8 in "Downstream adjustments". `PrefabInstance` marks which entities need it.

#### Writing: instances and prefabs

**`write_scene` with instances.** For each entity with `PrefabInstance`, the writer takes `InstanceOrigin.prefab` as the base and compares the live subtree with its nodes (members identified by `PrefabNode.node`). Before any comparison, each live component is **normalised**: on a clone of the component, every `Entity` that belongs to the same instance is mapped back to its placeholder (node index), through the same walk the spawner uses in reverse. So an intra-instance reference is equal to the base's, and is not an override. An `Entity` that points **outside** the instance is a true difference and is written as a scene label (`ReferenceOutsideScene` if it is not in the scene, as for any component). The engine API does not change: `Overrides::diff` and `merge3` always see placeholders.

1. A resolved node with no live entity becomes `removed`.
2. A live entity with a component the resolved node lacks becomes `add`.
3. A resolved component missing from the live entity becomes `remove`.
4. A component present in both goes through `Overrides::diff` (resolved against the normalised live component) and becomes `set`. An override equal to the base is **not** written: a diff records what is, not what was chosen.
5. A live entity under the instance with no `PrefabNode` is not part of the patch; it is written as an ordinary record with `parent: Some(@inst.node)`.
6. A live node that was **moved out** of its instance (its parent is no longer inside it) is written as a plain scene entity ("unlink and keep") with `SceneWarning::PrefabNodeDetached`, and the instance patch lists it in `removed`.
7. The stale entries in `InstanceOrigin.stale` are written back after the computed entries, unless `SaveOptions.drop_stale` is set or a live entry now covers the same node and path (the live one wins and the stale one is dropped).

The skip and warn rules for components are the same as `write_scene`'s (derived components, `SceneMember`, `Parent`, `Children`, `UnsavedComponent`), and `SaveOptions` is shared.

**`write_prefab`** writes a **root prefab** from a live subtree: nodes in pre-order, node ids from `PrefabNodeId` (new nodes get an id derived from `Name`, with `_2`, `_3` on collision, then **keep it**: renaming the entity later does not rename its node id). `write_prefab_variant` writes a **variant**: it resolves the base chain and diffs the live subtree against it, as for an instance, with the same normalisation and the same stale write-back.

**Editing a prefab** has one path: instantiate the prefab (or create a new subtree), edit the live entities, then `prefab_save(root, asset)`. If `asset` is a root prefab the whole subtree is written; if it is a variant, only the diff against its base is written. Saving an instance back into its own source prefab is the "apply" operation.

#### Runtime spawn and refresh

```rust
/// Exclusive form, for editor commands, scene loading and tests.
pub fn instantiate_prefab(world: &mut World, registry: &TypeRegistry, prefab: &Arc<ResolvedPrefab>, opts: SpawnOptions) -> Result<PrefabSpawn, SceneErrors>;

/// Deferred form, for ordinary systems that only hold `Commands`. Queued with `Commands::add`; runs when commands apply.
pub trait SpawnPrefabExt { fn spawn_prefab(&mut self, prefab: Arc<ResolvedPrefab>, opts: SpawnOptions); }
impl SpawnPrefabExt for Commands<'_, '_> { /* wraps instantiate_prefab */ }

pub struct SpawnOptions {
    pub parent: Option<Entity>,
    pub scene: Option<SceneId>,                           // adds SceneMember so the instance is saved with that scene
    pub root_components: Vec<Box<dyn Reflect>>,           // complete component values inserted on the root after spawning, replacing the prefab's own
                                                          //   (e.g. a Transform2d): the scene crate never names Transform2d
    pub track_origin: bool,                               // true by default
    pub on_spawn: Option<Box<dyn FnOnce(&mut World, &PrefabSpawn) + Send>>,   // the deferred form's way to learn the entities
}
pub struct PrefabSpawn { pub root: Entity, pub entities: Vec<Entity> }   // entities in prefab pre-order, root first
```

It spawns `prefab.nodes.len()` entities, inserts `clone_boxed` copies of the pre-bound components (plus the placeholder rewrite where needed), builds the hierarchy, and tags the nodes (`PrefabInstance`, `PrefabNode`, `InstanceOrigin`, and `SceneMember` when `scene` is set). It is allocation-light on purpose: game code spawns bullets and enemies with it. The runtime form takes **no patch**: a game that wants a variant of a variant uses `PrefabStore` to resolve it. (Scene instance patches are applied by the scene `instantiate`, not here.)

**`refresh(world, instance)`** brings a live instance up to date with its prefab:

1. `old` is `InstanceOrigin.prefab`, `new` is `PrefabStore::get(asset)`. If the instance has no `InstanceOrigin` (spawned with `track_origin: false`), the refresh is refused with `RefreshUnavailable`; the user unpacks the instance or reloads the scene.
2. For each node present in both, each component present in both goes through `Overrides::merge3(old, new, live)` on the **normalised** live component (intra-instance entities mapped to placeholders), and the merged result is mapped back to the real entities of the instance. A leaf the user changed keeps the live value; every other leaf takes the new value. Components added in `new` are inserted; components removed in `new` are removed unless the user changed them (then they stay and the report lists them).
3. Nodes present only in `new` are spawned; nodes only in `old` are despawned unless the user added children to them (then the node stays, reported).
4. The stale entries of `InstanceOrigin` are retried against `new`: one that fits now is applied.
5. `PrefabInstance.generation` and `InstanceOrigin.prefab` are set to the new version.
6. The result is a `RefreshReport`, and an undo record that restores the previous state.

### Facade glue (contract)

The glue lives in `exitium` (facade issues FD1 and FD2; they hold the `crates/exitium/` hotspot and have `Edits: none`). `DataPlugin` and `PrefabPlugin` are added to `DefaultPlugins`.

- **Schedule.** Every system below runs in `First` after set `ASSETS` (or in `PostUpdate` for scans), never in `Update`, so Edit mode (`SimulationMode::Frozen`) works.
- **Delivery.** `Data` and `Prefab` are kinds of their own with a single consumer each, so each frame the glue calls `AssetServer::take_loaded(AssetKind::Data)` and `take_loaded(AssetKind::Prefab)` and passes each `Payload::Text` to `DataStore::load_text` or `PrefabStore::load_text`. The `AssetLoaded` event is a **log signal only**; nothing depends on when it is raised, because SP4 does not say whether it is sent when a payload is queued or when a consumer takes it. A reload payload may have been coalesced (a later change replaced an earlier one), and `take_loaded_guid` may return `None` for a guid whose payload was taken, so delivery tolerates both. Delivery is capped by SP4's `max_reload_deliveries` and `max_reload_bytes` per frame; what is left is taken next frame, and the cap bounds the frame cost. (`take_loaded_guid` is not needed for these kinds. A game that adds its own consumer for a subset of data guids uses it. This is a deliberate difference from the issue text, which names `take_loaded_guid`: that call is for kinds several consumers share, such as `Text`.)
- **Events.** For each guid in `Loaded.changed` the glue sends `DataChanged { guid, generation }` and for each failure `DataFailed { guid, message, reload }`. `AssetFailed` for a guid of kind `Data` or `Prefab` is passed to `note_failed`, as is a base whose registry kind is not the right one (`BaseNotData`).
- **Requests.** After each delivery the glue calls `AssetServer::request` for `take_wanted()` (bases) and for `refs_of(guid)` (referenced data assets and prefabs). `DataSettings.preload` (default `true`) also requests every `Data` and `Prefab` asset in the registry once the manifest is `Ready`, and again for entries a manifest reload adds.
- **Scenes with prefabs.** `LoadScene` (FA3) gains a waiting state: after parsing, the glue calls `ParsedScene::prefab_guids()`, requests the prefabs, and instantiates (with `prefabs: Some(&store)`) once all of them are `Ready`. A prefab that fails (or that stays `NotFound` after SP4's retry schedule of 1, 2, 4, 8, 16 s) gives `SceneLoadFailed { PrefabFailed(guid) }`; the scene is not instantiated. The state is `SceneLoadState::WaitingForPrefabs`, reported by the existing scene status.
- **Own writes.** The glue does nothing for them; the editor lane calls `note_own_write` after `data_save` and `prefab_save`, as for scenes (SP4).
- **Re-exports.** `exitium::data`: `DataStore`, `DataInfo`, `DataState`, `DataError`, `DataWarning`, `DataEdit`, `FieldPath`, `Overrides`, `ReflectDataAsset`, `DataPlugin`, `DataStatus`, `DataSettings`, `DataChanged`, `DataFailed`. `exitium::scene` gains `PrefabStore`, `ResolvedPrefab`, `PrefabInstance`, `PrefabNode`, `PrefabNodeId`, `SpawnOptions`, `PrefabSpawn`, `SpawnPrefabExt`, `instantiate_prefab`, `write_prefab`, `PrefabPlugin`, `PrefabStatus`, `PrefabChanged`, `PrefabFailed`. `BaseGraph`, `InstanceOrigin` and the overlay internals stay private.

### Reflected surface (C4)

| Item | Kind | Notes |
|---|---|---|
| `ReflectDataAsset` | type data | Marks a type loadable as a data asset. Listed by `data_types`. |
| `DataStatus` | resource, read-only (`custom(readonly = true)`, editor-mcp-mvp.md X4) | `entries`, `ready`, `waiting`, `failed`, `dirty` counts; `failures: Vec<DataFailure { guid, path, message }>`; `warnings`; `conflicts: Vec<Guid>`; `revision: u64` (bumps on any change). Inspector and `ecs_get_resource` read it. |
| `DataSettings` | resource | `preload: bool` (true). `custom(history = false)`: a settings change is not an undo step (as `AssetSettings`). |
| `DataChanged`, `DataFailed` | events | `{ guid, generation }`, `{ guid, message, reload: bool }`. |
| `PrefabInstance`, `PrefabNode`, `PrefabNodeId` | components | `derived`, not saved; readable by `ecs_get_component`. |
| `PrefabStatus` | resource, read-only (`custom(readonly = true)`) | `prefabs`, `ready`, `waiting`, `failed` counts, `failures`, `warnings`, `stale_instances: u32`, `revision`. |
| `PrefabChanged`, `PrefabFailed` | events | As for data. |
| Commands (tools) | see the next section | Registered by the editor lane. |

`DataStore`, `PrefabStore` and `InstanceOrigin` are deliberately not reflected: their contents are reached through the tools below, which read them through the plain functions. The tools are therefore a complete view; nothing the inspector can show is missing from MCP (agent parity).

### Threading

All mutation happens on the main thread: the glue systems in `First`, and the editor's command drain. `DataStore` and `PrefabStore` are `Send + Sync` (`Box<dyn Reflect>` and `Arc<ResolvedPrefab>` are), so any number of systems may read them in parallel through `Res`, and the parallel executor needs no lock. A system sees one version of an asset for a whole frame, because the swap only happens in `First`. Parsing and resolving are done inline: the budgets below keep a reload frame under SP4's 20 ms cap. There is no worker thread in either crate. A `ResolvedPrefab` is immutable once built and is shared through `Arc`, so a reload replaces the pointer and never changes a prefab a system is reading. No `unsafe`.

## MCP / editor surface

The data and scene crates have no `exitium_command` edge (D2, D16). Each row is a plain function the crates expose. The editor lane (#110 and SP8) wraps each as a `Command` or `Query` with the stated undo kind, registers it with `register_command`, and gets a tool with the same name. The parity test grows by these rows.

| Plain Rust function | Tool | Kind and undo |
|---|---|---|
| `DataStore::types` (registry scan for `ReflectDataAsset`) | `data_types` | Query. Names, fields, schema, whether `Default` exists. |
| `DataStore::list` | `data_list` | Query. Filter by type, by base, by state (`dirty`, `waiting`, `failed`). |
| `info`, `get`, `overrides`, `write_data` | `data_get` | Query. The resolved value as text, base, overrides, stale entries and warnings, state, last error, parked disk text (conflict). |
| `create` plus `exitium_assets::edit::register` (AP3) | `data_create` | Command. Optional `base` makes a variant. Undo: `DataEdit` plus the asset edit. |
| `set_field` | `data_set_field` | Command. Value as text through serial; path syntax of reflect-model.md (`stats.hp`). A struct path sets every leaf below it. Undo: `DataEdit`. |
| `revert_field` | `data_revert_field` | Command. Undo: `DataEdit`. |
| `flatten` | `data_flatten` | Command. Undo: `DataEdit`. |
| `write_data`, `native::write_atomic`, `note_own_write`, `mark_saved` | `data_save` | Command, **no undo** (`UndoKind::NotUndoable`: a file write). Args `overwrite` (default false: `DiskChanged` when disk text is parked) and `drop_stale`. Refused in Play. |
| `replace_from_text`, `AssetServer::reload`, drop of `pending_disk` | `data_reload` | Command. `take_disk` bool. With no parked text it requests a reload and reports `reload_requested`. Undo: `DataEdit` (restores the in-memory value; `StaleEdit` if the entry changed since). |
| `DataStore::remove`, then `exitium_assets::edit::remove` | `data_remove` | Command. Refused (with the list) while variants use it as a base or other assets name it. Undo: `DataEdit` plus the asset edit. |
| `referrers`, `refs_of`, plus `AssetEdit::find_references` | `data_find_references` | Query. Data assets, prefabs and scenes that name the guid (AK1 extends `find_references` to `Data` and `Prefab` files). |
| `PrefabStore::list`, `get` | `prefab_list`, `prefab_get` | Query. `prefab_get` returns nodes, patch, warnings, stale entries and, with an `entity` argument naming a live instance, **the instance's overrides** (the `Overrides::diff` result per node, which the editor shows as override dots). |
| `instantiate_prefab` | `prefab_instantiate` | Command. Undo despawns the instance. Optional parent, root components and `scene`. |
| `create` (a subtree, or empty) plus `register` | `prefab_create` | Command. Optional `base` makes a variant. Undo: `PrefabEdit` plus the asset edit. |
| `write_prefab`, `write_prefab_variant`, `native::write_atomic`, `note_own_write` | `prefab_save` | Command, **no undo**. Arg `drop_stale`. Refused in Play. |
| `Overrides::remove` on an instance's patch (live: restore the base value) | `prefab_revert_override` | Command. Entity, component, optional field path. Undo: the previous value. |
| unpack | `prefab_unpack` | Command. Removes `PrefabInstance`, `PrefabNode` and `InstanceOrigin` and keeps the entities (they become plain scene entities). Undo restores them. |
| `PrefabStore::flatten` | `prefab_flatten` | Command. A prefab **variant** becomes a root prefab with its resolved nodes (the answer to "rebase" and "reparent", open questions 4 and 5). Undo: `PrefabEdit`. |
| `refresh` | `prefab_refresh` | Command. Undo restores the instance. Without an `entity` argument it refreshes every stale instance. |
| `PrefabStore::remove`, then `exitium_assets::edit::remove` | `prefab_remove` | Command. Refused while variants or loaded scenes' instances use it. Undo: `PrefabEdit` plus the asset edit. |

`asset_remove` on an asset of kind `Data` or `Prefab` goes through the same guard (AK1 extends SP4's guard to the two kinds), so both routes refuse a removal that would leave a variant dangling.

That is **11 data tools and 10 prefab tools, 21 in all**, against the roughly 90-tool budget of `editor-mcp-mvp.md`. `data_validate` and `prefab_validate` are not separate tools: validation is part of `data_get`, `prefab_get` and the `DataStatus` and `PrefabStatus` resources. SP8 confirms the count when it merges its tool table.

**Editor panels** (SP8 builds them from the same functions): a Data Asset panel (list by type, an inspector for the resolved value with an "overridden" marker and a revert button per field, base picker, flatten, save, conflict banner), and a Prefab panel plus hierarchy badges (instance icon, override dot per node from `prefab_get` with `entity`, "stale" marker, Refresh). Fields typed `Guid` with `custom(widget = "asset", kind = "data" | "prefab")` get the generic asset picker (ED10), filtered by `kind` and optionally by `data_type`.

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
| `exitium_scene/prefab/spawn/5_nodes_15_components` | at most 20 µs (see the basis below) |
| `exitium_scene/prefab/spawn/1000_instances` | at most 25 ms |
| `exitium_scene/instantiate/1000_instances` (scene with 1,000 instance records of the 5-node prefab) | at most 60 ms |
| `exitium_scene/write/1000_instances` (normalise and diff against the resolved prefab) | at most 30 ms |
| `exitium_scene/prefab/refresh/1000_instances` | at most 100 ms |

**Basis of the spawn budget.** 20 µs is measured on a warm world, in a loop of 1,000 spawns, **including** the hierarchy pass, with the hierarchy built by the #49 helpers directly. If SV2a finds that the pass needs `World::run_system_once` per call, the PR reports the measured setup cost and amends this number in the same PR (a loosening that is allowed once, because the basis was unknown when this spec was written); the 1,000-instance figure then becomes the contract.

**Memory.** A store entry costs at most 200 bytes beyond the values it holds. A prototype's value is stored once (the source and the resolved value are the same box); a variant stores its overrides plus one resolved value. A resolved prefab is shared through `Arc`, so 1,000 instances share one resolved copy; `InstanceOrigin` costs one `Arc` and an empty `Vec` per tracked instance (32 bytes).

**Frame cost.** A reload frame (swap, dependents, events) stays within SP4's 20 ms cap together with the render upload cap. The time from a file write to the new value visible to systems is at most 500 ms on native with the default hot-reload settings (SP4's 250 ms poll plus 50 ms settle, plus one frame), recorded in the PR of FD1.

**Complexity.** `dependents_topo` is linear in the number of dependents. A set of overrides is a sorted vector, so lookup is `O(log n)` and `apply` is `O(n)`. `resolve` clones the base value once per variant.

## Test plan

Per crate, with golden files for the text forms (the goldens are the reference; the examples in this spec are illustrative).

- **Overlay.** Path syntax and validation (unknown path with suggestions, path through a leaf). **`set` normalisation:** a struct-valued `set` expands to leaves and replaces entries below it; a leaf `set` leaves siblings untouched; `set` at `stats` then `stats.hp` keeps the other fields; a non-struct root is one entry at the empty path; **write then read gives equal overrides** in each of those cases. `apply` changes nothing on error. `diff` then `apply` reproduces the derived value (property test over generated structs, including enums, options, lists and maps as leaves). `leaf_eq`: `0.0` against `-0.0` differs, a NaN equals itself, `reflect_eq` returning `None` is "different". `merge3`: a user-changed leaf is kept, an unchanged leaf takes the new base value, a field missing from the new base is skipped and reported. A list leaf is replaced whole (no tail left over). A transparent wrapper is one leaf. Stale entries from `read` keep their text verbatim.
- **BaseGraph.** Cycle (self, two, three), depth 8 and 9, **`set_base` on a root whose subtree is deep** (chain 8 plus a new base is `BaseTooDeep`), `dependents_topo` order on a **wide tree** (one base, many variants) and a **deep tree** (a chain), ties by `Guid`, removal. (The graph is a forest, so there is no diamond case.)
- **DataFile.** Prototype and variant round trips against checked-in goldens. A tag that differs from the base chain is `BaseTypeMismatch`. A type without `ReflectDataAsset` is `NotADataAsset`, a non-struct is `RootNotStruct`. A stale override (renamed field, changed type) loads with a warning and is written back by `write_data`; with `drop_stale` it is dropped. `list_ops` is `UnsupportedFeature`. CRLF and comments parse. Output is identical across two writes.
- **DataStore.** First load, a variant that loads before its base (`Waiting`, then `Ready` when the base arrives), a chain of three, a changed base re-resolves dependents in order, **a reload that fails keeps the last good value** (and the same for a dependent after a hard error), a first load that fails is `Failed` with no value, a cycle created by a reload is rejected and the old graph stays. `note_failed` turns a `Waiting` entry into `Failed` with `BaseFailed` or `BaseNotData`, and a later good base resolves it. `load_text` is the only entry point for first load and reload (a test replays a first load as a reload and compares). Determinism: two processes (two hasher instances) list entries in the same order.
- **Edit API and conflicts.** Every function's undo and redo. **`apply_edit` after a disk reload returns `StaleEdit` and the reloaded value is intact.** `remove` refused for a base in use. `flatten` keeps the resolved value. A dirty entry parks a reload, `conflicts` lists it, `data_save` is refused with `DiskChanged` and works with `overwrite`, `take_disk` with a parked text applies it, `take_disk` without one requests a reload and applies the next payload to a dirty entry (`force_next`), a bad payload on a clean entry is kept in `pending_disk` and blocks a plain save.
- **Play.** `begin_play`, edit three entries, create one, apply a disk reload to a fourth, `end_play`: the three are restored, the created one is gone, the reloaded one keeps the disk value, dependents are re-resolved, a parked reload is applied. `data_save` during play is refused.
- **Prefab file.** Root and variant goldens. Both forms at once is `PrefabShape`. A variant chain of three. **Every row of the patch table** has a test with its class: the conflicts (`RemoveRoot`, `AddedRoot`, `DuplicateNodeId` including an id removed in the same patch, `AddedUnderRemoved`, `OverrideOnRemoved`, `DuplicateOverride`) fail the file; each stale case loads with a warning and is written back, and is dropped with `drop_stale`. The order test: `remove` then `add` of one type replaces the component; `remove` then `set` is stale. An unknown component type in `set` is a warning with suggestions, in a root prefab's `components` it is an error. `NestedPrefab`, two roots, a cycle, a missing parent. Node ids survive a save after an entity rename.
- **Instances in scenes.** A scene with two instances of the same prefab, an override on one, an added child under an inner node: write, read into a fresh world, write gives identical text. `@inst.node` labels resolve; an unknown inner label is `UnknownLabel` with suggestions; a missing prefab fails before anything is spawned (entity count unchanged); `prefabs: None` is `PrefabStoreMissing`; `validate_scene` with a store validates an instance. **`SceneInstance.entities` lists instance nodes in prefab pre-order, root first, and `write_scene` reports the same order** (the `EntityRemap` test). A prefab edited between save and load: a removed node's child re-attaches to the root with a warning, and a stale instance patch entry loads with a warning and is written back by the next save. A node moved out of an instance is written as a plain entity. **Entity fields:** a component whose `Entity` field points at another node of the same instance writes no override, and one that points outside writes a scene label.
- **Spawn.** `instantiate_prefab` produces the same entities as an instance record with an empty patch. Entity references between nodes are rewritten to the spawned entities; a map keyed by `Entity` works; the placeholder generation is never produced by a real entity. `spawn_prefab` through `Commands` spawns when commands apply, and `on_spawn` sees the entities. `root_components` replace the prefab's own. `track_origin: false` leaves no `InstanceOrigin`.
- **Origin and undo.** Despawn an instance, undo it through the `write_scene` and `instantiate` round trip: the new instance has a fresh `InstanceOrigin`, keeps its stale entries, and `prefab_refresh` works on it.
- **Refresh.** A user-changed field survives a refresh, an unchanged field takes the new value, **an `Entity` field pointing inside the instance refreshes** (it is not seen as user-changed), a new node appears, a removed node goes away, a node with user children stays and is reported, a stale entry that fits the new prefab is applied, undo restores. `RefreshUnavailable` for an untracked instance.
- **Facade (FD1, FD2).** A headless app with `MemorySource`: a data asset loads and `get_as` returns it; a variant follows a base rewritten on disk in `SimulationMode::Frozen`; a corrupt rewrite keeps the old value and sends `DataFailed { reload: true }`; a variant whose base is `NotFound` ends `Failed` with `BaseFailed`; a save by the editor path is not reported as a change; a reload payload coalesced under load is applied once; `LoadScene` waits for a prefab and then instantiates, and fails cleanly when the prefab fails.
- **No unsafe**; the wasm32 build of both crates passes.

## Dependencies

No new crates.io dependency. The reasons, so a reviewer can check them:

| Need | Choice | Why not a crate |
|---|---|---|
| Maps, vectors, sets | `std::collections` (`HashMap` with `BuildHasherDefault<DefaultHasher>`, `BTreeMap`, `Vec`) | The sizes are small; the fixed hasher and sorted listings make output deterministic without a hasher crate. |
| Shared immutable prefabs | `std::sync::Arc` | Built in. |
| Text format | `exitium_serial` (workspace) | The spec of record for the text form. |
| Reflection, type data, paths | `exitium_reflect` (workspace) | Same. |
| File write | `exitium_assets::native::write_atomic` through the editor lane (the data crate itself writes no file) | Keeps `exitium_data` free of an assets edge. |
| Benchmarks | `criterion` (workspace, dev-dependency) | Already the workspace's choice. |

Edit distance for suggestions uses the helper `exitium_serial` already exposes for its near-miss rule.

## Wiring for the intake PRs

What this spec needs from `Cargo.toml`, `Cargo.lock`, `deny.toml` and `xtask/` (C5). Intake 2 (#157) adds the union of the merged specs' lists.

**Intake 2.**

- Workspace edges: `exitium_data -> exitium_core, exitium_ecs, exitium_reflect, exitium_serial` (normal), dev-dependency `criterion`. **No** `exitium_app`, `exitium_command`, `exitium_assets`.
- **Not in intake 2: `exitium_scene -> exitium_data` (E1).** #157's criterion reads "`exitium_scene -> exitium_data` (E1, or its replacement per SP10 (#146))"; this spec's answer is "the edge is added later by EP1", so that bullet is removed from #157 (see "Downstream adjustments"; the edit must land before #157 starts).
- Facade: `exitium -> exitium_data` (normal; `exitium -> exitium_scene` is already SP4's).
- `crates/exitium_reflect_derive/`: add `exitium_data` to `ENGINE_CRATES` (the crate derives `Reflect` for `DataStatus` and its events, and the `crate_paths.rs` drift test fails until it is classified; `exitium_scene` is already there from SP4).
- `xtask`: nothing new; `exitium_data` is wasm-capable and goes in the default wasm build.

**Intake 3.** `exitium_editor -> exitium_data` (C3, already listed in the roadmap).

**Edge PR EP1** (not an intake): `exitium_scene -> exitium_data`, normal dependency. One crate (`exitium_scene/Cargo.toml`), `Hotspots: Cargo.toml, Cargo.lock`, `Blocked by` DA6 and SC5. If the owner chooses option B of open question 1, EP1 is not filed and the prefab issues are `Blocked by` SC5 instead.

**Not added, on purpose:** any edge from `exitium_data` or `exitium_scene` to `exitium_app`, `exitium_assets`, `exitium_command` or another leaf crate (C1), and any crates.io dependency.

## Open questions

Each has a recommended answer; the owner decides.

1. **E1: keep the edge (with the late edge PR), or move the overlay engine into `exitium_serial`?** **Must be answered before intake 2 (#157) starts**, because it decides whether EP1 exists and what #157 lists. Recommended: keep E1 and the edge PR (D1). Option B would cost a serial spec amendment that needs R1 clearance against the live serial lane (the serial lane runs beside ECS, so the PRs themselves are off the critical path) and puts a data concept into a frozen crate. Option B removes EP1 and the chain falls from about 33 to about 31.
2. **Nested prefabs** (a prefab containing instances of other prefabs): **owner to confirm against the no-cuts direction of 2026-11-24.** This spec keeps them out of the MVP scope and does not decide it. Cost of adding them later: about 4 to 5 PRs in `exitium_scene` (instance records inside prefab nodes, patch paths through instance labels, a recursive resolve with `NestedPrefabCycle`, refresh through nesting, the editor tools), plus the chain grows by the same number. **File compatibility:** the `prefab` field on records inside a prefab file is reserved and rejected today with `UnsupportedFeature`, so every file written now stays valid and a future file fails loudly on an old engine; no existing field changes meaning. If the owner says "include now", the 4 to 5 PRs join the prefab lane (chain about 38) and the `NestedPrefab` error rows leave the Test plan. Recommended: include them in a **later** spec amendment, not in this MVP lane, unless the owner's no-cuts direction requires them for Glutton.
3. **List and map elements:** **owner to confirm against the no-cuts direction.** Whole-field replacement only, or element-level overrides (insert, remove, set by index or key)? Cost of adding element-level overrides later: about 3 PRs (a list identity rule, an ops form in the overlay text I/O, `diff` to ops, `merge3` on lists, the editor's list UI). **File compatibility:** the envelope member `list_ops` (data) and the patch entry member `ops` (prefab) are reserved and rejected today with `UnsupportedFeature`, so existing files stay valid and a future file fails loudly on an old engine. Recommended: whole field for the MVP; Glutton-style data (item lists, loot tables) is small, and element ops need an identity rule for list items. Add ops later if a game needs them.
4. **Rebasing a variant onto another base:** support it, or `flatten` plus re-create? Recommended: not supported. `data_flatten` and `prefab_flatten` exist, so an agent has the path.
5. **Reparenting a base node in a variant:** support it? Recommended: no. A variant adds, removes and overrides, and cannot move nodes; `prefab_flatten` then edit is the path.
6. **Live prefab propagation:** explicit `prefab_refresh` (this spec), or automatic update of every live instance on reload? Recommended: explicit. Automatic update can overwrite what an agent or human is editing; a saved scene already gets the new prefab on its next load.
7. **`data_save` during Play:** refuse? Recommended: refuse (D10). SP8 owns the call site and should state the same.
8. **Typed handles (`DataRef<T>`, `AssetRef<T>`):** wait with SP4's D1 (plain `Guid`), or add a typed handle now? Recommended: wait. It is a facade-level addition later and changes no file format.
9. **Runtime-defined data types** (reflect open question 9; a type created in the editor with no Rust code): answered here as not in the MVP. Recommended: keep it out; a game's data types are Rust structs with `#[derive(Reflect)]`, which is what an agent writes.
10. **Stale entries on save (D14).** A variant or instance override that no longer fits its base (a renamed Rust field) is skipped with a warning, **kept as text, and written back on save**, so nothing is lost; the caller passes `drop_stale` to clean it up. The alternatives are (a) fail the save until the caller passes `drop_stale`, or (b) drop silently. Recommended: the spec as written (keep and write back, explicit `drop_stale`). It is the only choice that is both safe and does not block saving after a rename, and it applies to data variants, prefab variants and scene instances alike. Needed before SV1a (the file format).
11. **Node-id reuse.** Variant patches cannot add an id that the base chain used (`DuplicateNodeId`), but a **root prefab** save can give a deleted node's id to a new node, and a variant's override for the old node then lands on the new one. Recommended: accept this for the MVP and document it; a follow-up issue makes `prefab_save` return a warning that lists the loaded variants naming a removed node (it sees loaded variants only, not unloaded scenes).

## Implementation issues

Each issue is PR-sized (about 600 changed lines or fewer), edits one crate (the facade issues edit none and hold `crates/exitium/`), has a labelled Footprint, acceptance criteria and `Blocked by` lines, and goes into the MVP milestone. The architect files them with `file-issue` after this spec merges, labels `area:data`, `area:scene`, `area:assets` and `area:facade`. `<intake-2>` is the intake issue's number (#157), filled in at filing. Every issue is `Blocked by <intake-2>` (C6). One crate has at most one issue in flight, so each lane runs in the order shown. Each issue updates its crate's `CLAUDE.md` (DA1 creates the data crate's; SC1 creates the scene crate's), adds the benchmarks it names, and states the high-risk areas it touches in the PR Risk section (the serialization format, for the formats; the facade API, for FD2).

### Data lane (`exitium_data`, Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none)

DA1 starts right after intake 2, beside the `exitium_app` spine and the scene lane (R3 passes: no edge to either).

**DA1. `feat(data): overlay field paths, Overrides apply and text I/O`** (about 550 lines). Blocked by `<intake-2>`.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `overlay/path.rs`, `overlay/overrides.rs` (`FieldPath`, `FieldOverride`, `Overrides::{set, remove, get, iter, read, write, apply}`, `ReadOverrides`, `StaleEntry`), `OverlayError`; creates `CLAUDE.md`.
- Acceptance criteria:
  - [ ] The "Overlay" path, normalisation, read, write and apply tests, including a struct-valued `set` expanding to leaves, the empty path for a non-struct root, a list leaf replaced whole and a transparent wrapper as one leaf; `apply` changes nothing on error.
  - [ ] **Written then read gives equal overrides** in every normalisation case; stale entries keep their text verbatim; output is identical across two writes.
  - [ ] `overlay/apply/16_overrides` meets the budget; no `unsafe`; wasm32 build passes.

**DA2. `feat(data): overlay diff, merge3, leaf equality and BaseGraph`** (about 450 lines). Blocked by `<intake-2>`, DA1.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `Overrides::{diff, merge3}`, `leaf_eq`, `MergeReport`, `graph.rs` (`BaseGraph` with the subtree-height depth check and `FixedMap`).
- Acceptance criteria:
  - [ ] The property test `diff` then `apply`, the `leaf_eq` cases (`-0.0`, NaN, `None`) and the `merge3` cases of the Test plan; the `BaseGraph` cycle, depth (including a deep subtree), wide-tree and deep-tree tests.
  - [ ] `overlay/diff/40_fields` and `overlay/merge3/40_fields` meet their budgets.

**DA3. `feat(data): ReflectDataAsset and the DataFile envelope`** (about 500 lines; high-risk review: file format). Blocked by `<intake-2>`, DA2.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `asset.rs` (`ReflectDataAsset` and its registration helper), `file.rs` (`DataFile` parse and write, the sparse-group framing rule, limits, reserved names), `DataError`, `DataWarning`, `WriteOptions`.
- Acceptance criteria:
  - [ ] The "DataFile" test group against checked-in golden files (produced by the writer and reviewed; this spec's examples are illustrative); `BaseTypeMismatch`, `NotADataAsset`, `RootNotStruct`, `UnsupportedFeature`, and the CRLF and comment cases.
  - [ ] A stale override loads with a warning and `write_data`-style output keeps it; `drop_stale` drops it.
  - [ ] Prototype 2 KB and variant 200 B parse within the DA4 budgets' parse share (measured here, asserted in DA4).

**DA4. `feat(data): DataStore (load_text, resolve, dependents, reload rules)`** (about 600 lines). Blocked by `<intake-2>`, DA3, `exitium_ecs` `Resource` and resources in the world (#44).
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `store.rs`: `DataStore` (`impl Resource`), the reads, `want` and `take_wanted`, `load_text` with the five steps, `note_failed`, `refs_of`, `referrers`, deterministic listings.
- Acceptance criteria:
  - [ ] The "DataStore" test group: waiting base, chain of three, dependents re-resolved in order, **a failed reload keeps the last good value** for the entry and for a dependent, a cycle from a reload is rejected, first load equals reload (replay test), `note_failed` for a `NotFound` base and a non-data base, two hasher instances list in the same order.
  - [ ] `store/get/10000`, `store/load_prototype/2kb`, `store/load_variant/200b`, `store/reresolve/1000_dependents` meet the budgets.

**DA5. `feat(data): authoring API, undo records and conflicts`** (about 600 lines). Blocked by `<intake-2>`, DA4.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `edit.rs`: `write_data`, `create`, `set_field`, `revert_field`, `flatten`, `replace_from_text`, `apply_edit` with `edit_rev` and `StaleEdit`, `mark_saved`, `remove`, `DataEdit`, the dirty flag, `pending_disk`, `force_next` and the rows of the conflict table (`DiskChanged`).
- Acceptance criteria:
  - [ ] The "Edit API and conflicts" tests: every undo and redo, **`StaleEdit` after a reload**, `remove` refused for a base in use, `flatten` keeps the resolved value, the conflict table row by row.
  - [ ] `store/write/2kb` and `store/set_field/variant` meet the budgets.

**DA6. `feat(data): play journal, DataStatus, events, registration and benchmarks`** (about 450 lines). Blocked by `<intake-2>`, DA5.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `play.rs` (`begin_play`, `end_play`, `PlayRestore`, `SavedDuringPlay`), the reflected `DataStatus` (`custom(readonly = true)`) and `DataSettings` (`custom(history = false)`), the events `DataChanged` and `DataFailed`, `register_types`, the remaining benchmarks.
- Acceptance criteria:
  - [ ] The "Play" test of the Test plan; the reflected types register and round-trip through the text format; a test asserts `DataSettings`'s `TypeInfo` has `custom("history") == false` and `DataStatus`'s has `custom("readonly") == true`.
  - [ ] `play/end_play/100_edited` meets the budget; the full set of data benchmarks runs in CI.

### Assets row (`exitium_assets`)

**AK1. `feat(assets): AssetKind::Data and AssetKind::Prefab, references and remove guard`** (about 250 lines). Blocked by `<intake-2>`, AP4 (SP4's last assets lane issue; R1).
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds the two `#[non_exhaustive]` variants, the extension rows `data` and `prefab` (both `Payload::Text`) in `path.rs`, the registry and listing tests; **extends `find_references` (AP3: it read only `Scene` and `Text`) to read `Data` and `Prefab` files, and the `asset_remove` guard to refuse a `Data` or `Prefab` asset that others name**.
- Acceptance criteria:
  - [ ] Listing by kind returns data and prefab assets; an unknown extension is unchanged; the kinds round-trip through the manifest **text** (the text and the manifest of existing entries are unaffected; adding variants changes the schema hash of `AssetKind`, `AssetInfo` and `Manifest`, and nothing persists that hash, which a test pins).
  - [ ] `take_loaded(AssetKind::Data)` delivers a `Data` payload and a reload payload (`generation >= 2`) in a `MemorySource` test.
  - [ ] `find_references` finds a guid named by a `.data` and by a `.prefab` file; `asset_remove` of a data asset used as a base is refused with the list; both stay bounded by `max_text_bytes`.

### Scene lane (`exitium_scene`)

SC1 to SC5 are SP4's issues (assets-scene-export.md "Stage 2: scene lane") and run beside the data lane. This spec adds the edge PR and seven prefab issues after SC5.

### Edge PR (not an intake, C5)

**EP1. `chore(scene): add the exitium_data dependency (E1)`** (about 20 lines). Blocked by `<intake-2>`, DA6, SC5.
- Footprint: Edits `crates/exitium_scene/Cargo.toml`; Governs none; Hotspots `Cargo.toml`, `Cargo.lock`; New crate edges `exitium_scene -> exitium_data`.
- Acceptance criteria:
  - [ ] `cargo build --workspace` and the wasm build pass; `cargo tree -p exitium_scene` shows the single new edge; no other manifest changes; `exitium_data` has no new edge.

### Scene lane, prefabs (`exitium_scene`, Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none)

**SV1a. `feat(scene): PrefabFile (root and variant) parse and write, patch classes`** (about 550 lines; high-risk review: file format). Blocked by `<intake-2>`, EP1.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `prefab/file.rs` and `prefab/patch.rs`: `PrefabFile`, `PrefabPatch`, parse and write of root and variant, the patch table's classes (`PrefabError`, `PrefabWarning`, `StaleEntry` reuse), `UnsupportedFeature` for `prefab` and `ops`.
- Acceptance criteria:
  - [ ] The "Prefab file" group against checked-in goldens; both forms at once, two roots, and **each conflict row of the patch table** fails with its named error; unknown envelope fields warn.
  - [ ] Stale-looking entries parse (the classification happens at resolve); output is identical across two writes.

**SV1b. `feat(scene): resolve chain, ResolvedPrefab and PrefabStore`** (about 550 lines). Blocked by `<intake-2>`, SV1a.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `prefab/resolve.rs` (`ResolvedPrefab`, the fixed patch order, the entity placeholder with the reserved generation, stale classification and retry) and `prefab/store.rs` (`PrefabStore::{load_text, get, want, take_wanted, note_failed, remove}`, the reload rules).
- Acceptance criteria:
  - [ ] The patch order test (`remove` then `add` replaces; `remove` then `set` is stale), **each stale row** loads with its warning, an unknown component type in `set` warns with suggestions, a variant chain of three.
  - [ ] A failed prefab reload keeps the last good `Arc`; a changed base re-resolves its variants in order; `Waiting` fails loudly through `note_failed`; first load equals reload.
  - [ ] `prefab/resolve_root/20_nodes_60_components` and `prefab/resolve_variant/20_nodes_5_overrides` meet the budgets.

**SV2a. `feat(scene): runtime spawn (instantiate_prefab, deferred form, components, origin)`** (about 550 lines). Blocked by `<intake-2>`, SV1b, `Commands::add` (ecs-scheduler.md) and `World` hierarchy helpers (#49).
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `instance/spawn.rs`, `instance/components.rs`, `instance/remap.rs`: `instantiate_prefab`, `SpawnPrefabExt`, `SpawnOptions`, `PrefabSpawn`, the reflected `PrefabInstance`, `PrefabNode` and `PrefabNodeId`, the non-reflected `InstanceOrigin`, the placeholder rewrite with its per-type cache.
- Acceptance criteria:
  - [ ] The "Spawn" tests of the Test plan: Entity references rewritten, a map keyed by `Entity`, the reserved generation assertion, `spawn_prefab` through `Commands` with `on_spawn`, `root_components`, `track_origin: false`.
  - [ ] `prefab/spawn/5_nodes_15_components` and `prefab/spawn/1000_instances` meet the budgets, with the basis and any amendment recorded in the PR.

**SV2b. `feat(scene): instance records in scenes (parse, labels, patch apply, prefabs option)`** (about 600 lines). Blocked by `<intake-2>`, SV2a.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds the `prefab` field to scene parse and instantiate (`@inst.node` labels, `PrefabNotLoaded`, `MissingPrefabNode`, `InstanceHasComponents`, `PrefabStoreMissing`, `ParsedScene::prefab_guids`), the patch applied to live entities, `prefabs: Option<&PrefabStore>` on `InstantiateOptions`, `validate_scene` and `SaveOptions`, the defined `SceneInstance.entities` order.
- Acceptance criteria:
  - [ ] The instance tests of the Test plan, including the atomic failure (entity count unchanged on `PrefabNotLoaded`), `validate_scene` with a store, a patch conflict aborting the scene load while a stale entry only warns, and **the entity order that `EntityRemap` relies on**.
  - [ ] `instantiate/1000_instances` meets the budget.

**SV3a. `feat(scene): write_scene with instances (normalise, diff, stale write-back)`** (about 600 lines; high-risk review: file format). Blocked by `<intake-2>`, SV2b.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds the instance diff to `write_scene` (the seven rules), the Entity normalisation, `SceneWarning::PrefabNodeDetached`, `SaveOptions.drop_stale`, the stale write-back from `InstanceOrigin`.
- Acceptance criteria:
  - [ ] A scene with instances writes, reads and writes again to identical text; a moved-out node is a plain entity with a warning; an intra-instance `Entity` field writes no override and an outside one writes a label; stale entries are written back and dropped with `drop_stale`.
  - [ ] The origin and undo test (despawn, `write_scene` with `SaveScope::Entities`, instantiate); `write/1000_instances` meets the budget.

**SV3b. `feat(scene): write_prefab and write_prefab_variant (node ids, diff, flatten)`** (about 500 lines; high-risk review: file format). Blocked by `<intake-2>`, SV3a.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `write_prefab`, `write_prefab_variant`, node-id assignment and stability, `drop_stale` for prefab files, `PrefabStore::flatten`, the authoring functions `create` and `create_variant` with `PrefabEdit`.
- Acceptance criteria:
  - [ ] Node ids survive a rename; a variant save writes only the diff and its stale entries; `flatten` gives a root prefab whose resolved nodes equal the variant's; `remove` refused while a variant has the base; every undo.

**SV4. `feat(scene): prefab refresh, PrefabStatus and events`** (about 450 lines). Blocked by `<intake-2>`, SV3b.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `refresh`, `RefreshReport` and its undo record, the stale-instance count, the reflected `PrefabStatus` (`custom(readonly = true)`), the events `PrefabChanged` and `PrefabFailed`, `register_types` additions.
- Acceptance criteria:
  - [ ] The "Refresh" tests of the Test plan, including the Entity-field case, a stale entry that fits again, `RefreshUnavailable` and undo; the reflected types register and round-trip.
  - [ ] `prefab/refresh/1000_instances` meets the budget; the crate's wasm32 build passes.

### Facade glue (Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none)

These two run alone, in order, after the plugin lanes they glue (R2, R3). Their place among the other specs' facade issues is the orchestrator's; the lines below are only what each glue needs.

**FD1. `feat(exitium): data glue (DataPlugin)`** (about 450 lines). Blocked by `<intake-2>`, AK1, DA6, AP4, FA1 (schedule and `SimulationMode::Frozen` test pattern, AM3 of SP3), F3 (app-platform.md, `DefaultPlugins`) and FA4 (the `exitium::scene` re-export module).
- Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none.
- Adds `DataPlugin`: delivery through `take_loaded(AssetKind::Data)`, events, `note_failed`, requests for `take_wanted` and `refs_of`, `DataSettings.preload`, the `exitium::data` re-exports, the `DefaultPlugins` entry.
- Acceptance criteria:
  - [ ] **Schedule:** delivery runs in `First` after `ASSETS`, never in `Update`; with `SimulationMode::Frozen` a data file rewritten on disk still changes `DataStore::get_as`.
  - [ ] The facade tests of the Test plan: a variant follows a rewritten base, a corrupt rewrite keeps the old value and sends `DataFailed { reload: true }`, a `NotFound` base ends `Failed`, an own write is not reported, a coalesced reload applies once, a data asset referenced by another is requested once.
  - [ ] The write-to-visible time of a reload is recorded in the PR (budget 500 ms); a compile test names every re-exported item.

**FD2. `feat(exitium): prefab glue, scene loading with prefabs and re-exports`** (about 500 lines; high-risk review: facade API). Blocked by `<intake-2>`, SV4, FD1, FA3, F3 and FA4.
- Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none.
- Adds `PrefabPlugin` (delivery, requests, events), the `SceneLoadState::WaitingForPrefabs` state in `LoadScene` (passing `prefabs` to `instantiate`), the `exitium::scene` re-exports of the prefab items, and the additions to `DefaultPlugins` and the sample game (a prefab variant placed twice in the sample scene, a data asset driving unit stats).
- Acceptance criteria:
  - [ ] A scene with an instance loads once its prefab is `Ready`; a prefab that stays `NotFound` ends in `SceneLoadFailed` and spawns nothing; a prefab rewritten on disk gives `PrefabChanged`, leaves live instances unchanged, and the next `LoadScene` replace shows the new prefab.
  - [ ] Every name in "Re-exports" is reachable as `exitium::<module>::<Name>` and `BaseGraph`, `InstanceOrigin` and the overlay internals are not.

### Not in this spec

The editor wrappers and panels for the tools above belong to SP8 and the editor lane (#110), and the parity-test rows grow there. This spec only fixes the functions they wrap.

## Downstream adjustments

These cross-spec edits touch files this PR must not edit. Each is filed as its own `status:needs-approval` issue with one footprint (the numbers are in the PR). They are the contract once this spec merges:

- **`mvp-roadmap.md` and intake 2 (#157), issue #204.** Must land **before #157 starts**:
  - Remove `scene -> data` (E1) from intake 2's list and from #157's criterion ("E1, or its replacement per SP10 (#146)"); add `data -> ecs` and `exitium_data` in `ENGINE_CRATES` (see "Wiring").
  - Replace "Scene lane: about 8 ... after the data lane (E1)" with: scene SC1 to SC5 beside the data lane, then the edge PR EP1, then SV1a to SV4 (7 prefab PRs). The Stage 2 critical path becomes intake 2 (1), data 6 beside scene 5, EP1 (1), prefab 7, facade 18: **about 33 serial PRs** (was 32).
  - Add the facade issues FD1 and FD2 in place of the single "data glue" slot; add AK1 to the assets lane (R1: after AP4).
  - Mark E1 "decided: stands" in C1; EP1 is the second edge PR of the plan (C5; the first is `exitium_editor -> exitium_ui`). Close roadmap open question 3 (E1) as decided, subject to open question 1 above.
- **`assets-scene-export.md`, issue #205.** Its "Hot reload" row "Data assets, prefab variants" is answered here (the kinds `Data` and `Prefab`, `take_loaded(kind)` for kinds with one consumer, the dirty-entry rule). SC1 to SC5 are unchanged; SV2a and SV2b extend the scene envelope with the `prefab` field and add `prefabs: Option<&PrefabStore>` to `InstantiateOptions`, `validate_scene` and `SaveOptions`; SP4's "unknown envelope field is a warning" rule keeps old scene files loading. SP4's "unknown component type is always an error" is relaxed for **patch entries only** (D14). AP3's `find_references` and the `asset_remove` guard are extended to `Data` and `Prefab` by AK1. FA3's `LoadScene` gains the `WaitingForPrefabs` state in FD2 (FA3 is not reopened).
- **`editor-mcp-mvp.md` and SP8 (#149), issue #206.** Adds the 11 data tools and 10 prefab tools of "MCP / editor surface" to the tool table (about 21 tools) and to the parity test. `scene_load`, `scene_validate` and `scene_save` pass `prefabs` and, for `scene_load`, request missing prefabs and return `PrefabsPending`. Undo of a despawn and copy and paste of an **instance** go through `write_scene` (`SaveScope::Entities`) and `instantiate`, never component by component. `play_start` and `play_stop` call `DataStore::begin_play` and `end_play`; `data_save` and `prefab_save` are refused in Play; both call `note_own_write` after saving; the editor drops a guid's undo entries when a disk reload applies. SP8 builds the Data Asset and Prefab panels. SP8 is ordered after SP10.
- **`reflect-model.md`, open question 9, issue #207** (runtime-defined types): resolved as not in the MVP (open question 9 above).
- **SP9 (Luau):** reads data assets through `DataStore::get(guid) -> &dyn Reflect`.
- **#61:** unchanged.
- **`CLAUDE.md` files:** DA1 creates `crates/exitium_data/CLAUDE.md`; each scene issue updates `crates/exitium_scene/CLAUDE.md`.

**Follow-ups filed, not in this PR:** a per-entry content hash of resolved data and prefabs for netcode handshakes (#208); a `prefab_save` warning that lists loaded variants naming a removed node (open question 11, #209); a native-only default for `DataSettings.preload` (#210; on web it fetches every data asset at start, six requests in flight at a time).
