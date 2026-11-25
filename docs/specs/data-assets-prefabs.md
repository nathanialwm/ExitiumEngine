# Spec: Data assets and prefab variants

- Issue: #146 (roadmap SP10)
- Crates: `exitium_data`, `exitium_scene` (a small additive change in `exitium_assets`, AK1 of about 250 lines, and the glue in `exitium`, are listed under "Implementation issues")
- Status: draft

## Goal

Give games two kinds of authored, inheritable content. A **data asset** is a typed, reflected value in its own `.data` file (an item, an enemy stat block, a level rule set), and a **variant** of it stores only the fields that differ from a **base** data asset. A **prefab** is a reusable tree of entities in a `.prefab` file, it may contain **instances of other prefabs** (nesting), and a **prefab variant** stores only the nodes, components and fields that differ from a base prefab. A scene places prefab instances and stores only what the instance changes. All of them share one "base plus overrides" engine, so an override means the same thing in data, in prefabs and in scene instances, including **element-level changes to lists and maps** identified by key. Both kinds follow SP4's reload rules (a failed reload keeps the last good value; a reload goes through the same path as a first load), and both are reachable by an AI agent through MCP with the same operations a human has in the editor.

## Decisions by the owner (2026-11-25)

These replace the open questions 1 to 3 of earlier drafts.

| Was | Decision | Where it lands |
|---|---|---|
| OQ1: E1 | **Keep E1** (`exitium_scene -> exitium_data`) and the late edge PR EP1. | D1, "Edge E1: the decision". |
| OQ2: nested prefabs | **In this spec and in the MVP prefab lane**, not a later amendment. | D18, "Nested prefabs", SV1c, SV1d, SV2a to SV4b. |
| OQ3: element-level list and map overrides | **In**, for data variants, prefab variants and scene instance overrides. | D19, D20, "Element ops", DA2, DA4, DA8. |

Open questions 4 to 12 are not answered; each keeps its recommended default.

## Non-goals

- Typed handles (`DataRef<T>`, `AssetRef<T>`). Fields stay plain `Guid`s (SP4 D1). Open question 8.
- Rebasing a variant onto a different base, and reparenting a base node inside a variant. Use `flatten` plus re-create. Open questions 4 and 5.
- Runtime-defined data types (a type created in the editor without Rust code): reflect-model.md open question 9 is answered here as not in the MVP (open question 9).
- Live auto-update of already spawned prefab instances when a prefab changes. It is an explicit `prefab_refresh` (open question 6).
- Element ops on a list that has no key (D19): such a list is replaced whole. Open question 12.
- Composite list keys (two fields), and ops on fixed-length arrays (`[T; N]`): whole replacement.
- Binary form of data assets. `.data` and `.prefab` are text; the binary container (serial-binary-format.md) stays the save system's.
- A `Plugin` in `exitium_data` or `exitium_scene`. Both crates have no `exitium_app` edge (SP4 D7); the facade glue wires them.
- Any change to `exitium_serial`, `exitium_reflect` or `exitium_ecs`. This spec only uses what their specs already provide. The list key is an existing `custom(...)` attribute (no new reflect attribute, no schema-hash change); #207 documents the convention.

## Design

### Decisions at a glance

| # | Decision | Why |
|---|---|---|
| D1 | **E1 (`exitium_scene -> exitium_data`) stands** (owner, 2026-11-25). The shared "base plus overrides" engine is the module `exitium_data::overlay`, not a foundation crate. The edge is **not** in intake 2: a one-crate *edge PR* (EP1) adds it after the data lane and SC5, so SC1 to SC5 run beside the whole data lane (C5). | A serial amendment needs R1 clearance against the live serial lane and puts a data concept into a frozen foundation crate; a new foundation crate is built before its first consumer. See "Edge E1" for the chain-length cost, which grew with element ops and nesting. |
| D2 | `exitium_data -> core, ecs, reflect, serial`. **No `exitium_app`, no `exitium_command`, no `exitium_assets`.** No `Plugin`. | `ecs` only so `DataStore` can `impl Resource`. The data lane never waits for the app spine, and the crate follows C1 and D7 of SP4. |
| D3 | A data type is a reflected **struct** (named or tuple) that carries the type data `ReflectDataAsset` (`#[reflect(data(exitium_data::ReflectDataAsset))]`). It is a marker. | The type list for tools and the file loader come from the registry; no hand-written schema. A struct root keeps override paths simple. |
| D4 | A `.data` file is the envelope `DataFile { base, value }`. `AssetKind::Data`, extension `data`, payload `Payload::Text`. | One path per asset kind (SP4 D13). |
| D5 | **What an override path can reach.** A path goes through **named-field structs** only. A **leaf** is anything else: a value, enum, option, list, map, built-in tuple, **non-transparent tuple struct** (its text form is positional only, so a sparse group cannot write element 1 without element 0), **and every type with `ReflectTextForm` (`Guid`, `Entity`), checked before the struct rule**. A leaf is replaced whole, **except** a keyed list or a map, which may take element ops (D19). An `Overrides` set never holds an entry whose target is a struct: `set` at a struct path expands to one entry per leaf. A non-struct root is one entry at the empty path. | One rule, easy to read and diff; reading and writing round-trip because every entry is a leaf or an ops list. |
| D6 | One engine, `overlay`: `Overrides` (read, write, apply, diff, merge3, element ops) and `BaseGraph` (cycle and depth check, chain, dependents in topological order). Data assets, prefab patches and scene instance patches all use it. | One definition of "an override" (E1). |
| D7 | `DataStore::load_text` is the single path for a first load and a reload. A hard error of any kind keeps the last good value and reports the failure. | SP4 rules 1 and 2 (D12, D13). |
| D8 | A data asset with unsaved in-memory edits ("dirty") does not take a disk reload, and a failed or parked disk text is never overwritten silently: the text is kept in `pending_disk`, and `data_save` refuses (`DiskChanged`) unless `overwrite: true`. | A reload must not destroy an edit, and an edit must not hide a disk change. |
| D9 | Saving goes `write_data` -> `native::write_atomic` -> `note_own_write(WatchKey::Asset(guid))` -> `mark_saved`. A **new** asset is written first and registered after (`write_atomic`, then `register_asset`). | SP4: own writes are not reported as external changes; `register_asset` needs a file under `assets/`. |
| D10 | Play-in-editor: `begin_play` starts a journal of first edits; `end_play` restores them and **keeps** disk reloads. `DataStore` is not part of the generic resource snapshot. Commands that write or remove a file are refused in Play. | Stage 5: "Data Asset edits made while playing are reverted on Stop". |
| D11 | A `.prefab` file is **root** (`base: None`, a list of nodes) or **variant** (`base: Some`, a patch). Node ids are stored labels, stable across saves, and contain no `.`. | Variants and instances refer to nodes by id; ids that re-derive from `Name` would break them. |
| D12 | A scene places an instance as one entity record with `prefab: Some((asset, patch))`. The record's `id` is the instance root label; inner nodes are `@<instance>.<node>`. An instance patch has `overrides` and `removed` only (no `added`): new children are ordinary records. | A scene instance is an anonymous variant attached at a location, so it reuses the patch format and the engine; one canonical form for added children. |
| D13 | The instance root's saved state is its patch, computed by diffing the live entities against the resolved prefab at save time, **plus the stale entries kept from the load** (D14). | The editor edits live entities; the file stores only differences, and nothing a file said is lost. |
| D14 | **Three failure classes for a patch entry, no more.** (1) *Parse or shape error*: the text is malformed or the file has an impossible shape; the whole file fails. (2) *Patch conflict*: the patch contradicts itself, which no base change can cause; the whole file fails. (3) *Stale*: the entry no longer fits its base or its types (missing node, component, field or key, a value that no longer binds, an unknown component type, an id that now collides with the base); it is **skipped with a warning, kept, and written back on save** unless the caller passes `drop_stale`. Every case in the tables below has exactly one class. | A base can change under a variant (a Rust field is renamed, a base gains a node); the variant must still load, and the next save must not destroy what the file said (SP4: "dropping it silently would lose data on the next save"). The same classes apply to data variants, prefab variants and scene instances. |
| D15 | A prefab reload updates the store and raises `PrefabChanged`. Live instances are updated only by `prefab_refresh` (a per-node 3-way merge); a saved scene gets the new prefab on its next load. | No surprise changes to a running or half-edited world. |
| D16 | All mutation is a plain Rust function that returns an undo record (`DataEdit`, `PrefabEdit`). The editor lane wraps them as commands and tools. These crates have no `exitium_command` edge. | C1 and `editor-mcp-mvp.md`: only crates on C1's list have the command edge. Agent parity holds because the editor lane (#110, SP8) owns the wrappers and the parity test. |
| D17 | No new crates.io dependency. | std plus serial and reflect are enough ("Dependencies"). |
| D18 | **Nested prefabs.** A prefab record may be an instance of another prefab (`prefab: Some((asset, patch))`), the same record shape as in a scene. Resolve produces one **flat** node list: a nested prefab's nodes are spliced in with ids `<record id>.<inner id>`. Patches, refresh, spawn and diff all work on the flat list. | Flat resolve keeps every other mechanism (patch order, stale classes, normalisation, refresh, `InstanceOrigin`) unchanged; nesting is only graph bookkeeping and an id rule. |
| D19 | **List element identity is a key field**, not an index. The item struct has exactly one field marked `#[reflect(custom(list_key = true))]` (a string, integer, `bool`, `char`, `Guid` or unit-only enum), or the item is itself such a value (an ordered set). Maps are keyed by their map key. A list without a key is replaced whole. | An index breaks when the base inserts an item before it; a content hash breaks on any edit of the item; a synthetic id stored in every element would change every base format. A key field is a real domain field (an item id) that games already have, and it needs no reflect change. |
| D20 | **Element ops are inline**: a container field's value is `ops([ ... ])` in place of the whole list or map, inside the same sparse group. Four list ops (`insert`, `remove`, `move`, `set`) and three map ops (`put`, `remove`, `set`). | Paths nest naturally (`stats.loot: ops([...])`), there is no second path syntax, and the reader needs no extra envelope member. The reserved members `list_ops` and `ops` of earlier drafts were never written, so dropping them breaks no file. |
| D21 | **One dependency rule for bases and nesting.** The combined graph of base edges and nesting edges is acyclic (`BaseCycle` for a base-only cycle, `NestedPrefabCycle` for any cycle through a nesting edge), and its longest path, counting each edge once, is at most `max_chain_depth` 8. | One limit, one check, consistent with `BaseGraph`. |

### Edge E1: the decision

**E1 stands**, decided by the owner on 2026-11-25. `exitium_scene` depends on `exitium_data`, and the overlay engine lives in `exitium_data::overlay`.

| Option | Cost | Verdict |
|---|---|---|
| A. Keep E1, engine in `exitium_data::overlay` (chosen). | One allowed leaf edge. Scene waits for the data lane before its prefab issues. | The edge is added late (EP1), so the wait covers only the prefab issues. |
| B. Move the engine into `exitium_serial`. | The serial lane (about 17 PRs) runs beside ECS (28), so the extra serial PRs are not on the critical path. The cost is elsewhere: an amendment to `serial-text-format.md` needs R1 clearance against the live serial lane, outside this spec's footprint; it puts an "override" (a data concept) into a frozen foundation crate; and `BaseFailure` would move out of `store.rs`. | Not chosen. Option B also removes EP1: chain 37 instead of 42. |
| C. A new foundation crate `exitium_overlay`. | A new workspace member (C8, hotspot work at the intake), a frozen crate for about 1,500 lines, built before its first consumer. | Rejected. |

**Parallelism.** Scene depends on data only through code that uses the overlay engine, and only the prefab issues use it. SC1 to SC5 do not. Therefore:

- Intake 2 does **not** add `scene -> data`. Scene and data are independent crates until EP1, so R3 does not tie them.
- DA1 to DA9 (data) run beside SC1 to SC5 (scene) and beside the `exitium_app` spine.
- EP1 adds the edge. It is `Blocked by` DA9 and SC5, because after the edge exists the orchestrator will not run a data issue and a scene issue together (factory rule: no crate dependency between in-progress issues). It edits one crate manifest (`exitium_scene/Cargo.toml`) and holds `Cargo.toml` and `Cargo.lock` (C5), so it runs alone.
- The prefab issues SV1a to SV4b are `Blocked by` EP1 (SV1a directly, the rest through each other), in order.

**Chain length for Stage 2:** intake 2 (1), then max(data 9, SC 5) = 9, EP1 (1), prefab issues (13), then the facade (18: FD1 and FD2 take the place of the roadmap's single "data glue" slot, so 17 - 1 + 2): 1 + 9 + 1 + 13 + 18 = **42 serial PRs**, against 32 in the roadmap (1 + 6 data + 8 scene + 17 facade) and 33 in the previous draft of this spec. The growth is the owner's scope decision: element ops add three data PRs (DA2, DA4, DA8) and nesting plus the round 2 write rules add six prefab PRs. **The E1 decision was taken when option B was 2 PRs shorter; with this scope it is 5 PRs shorter** (37 against 42: intake 1, scene 5, prefab 13, facade 18). The owner may reopen it; the recommendation stands, because option B's cost is a serial amendment and a frozen-crate concept, not PRs. "Downstream adjustments" lists the roadmap edit (#204), which must land before intake 2 (#157) starts.

### Crate map

```
exitium_data   -> core, ecs, reflect, serial            (no app, no command, no assets)
exitium_scene  -> core, ecs, reflect, serial            (intake 2: SP4)
exitium_scene  -> exitium_data                          (E1, added by the edge PR EP1)
exitium (facade) -> data, scene, assets, ...            (intake 2)
exitium_editor -> data                                   (intake 3, C3; already in the roadmap)
```

Other leaf crates cannot name data or prefab types (C1). Game systems reach data through the facade (`exitium::data`), and components refer to data and prefabs by `Guid`.

**Determinism.** Every map in both crates is `HashMap<K, V, BuildHasherDefault<DefaultHasher>>` (std, fixed keys, no new dependency), and every listing (`list`, `failures`, `DataLoaded.changed`, event order) is sorted by `Guid`, so output does not depend on the process or the Rust version. Dependents are ordered topologically and ties break by `Guid`.

### Data assets

#### What a data type is

```rust
/// Marker type data. Registering it makes the type listable and loadable as a data asset.
#[derive(Clone, Copy)]
pub struct ReflectDataAsset;
// impl TypeData for ReflectDataAsset  (reflect-model.md, "Type data")

#[derive(Reflect, Clone, Default)]
#[reflect(Default, type_name = "glutton::LootEntry")]
pub struct LootEntry {
    #[reflect(custom(list_key = true))]
    pub id: String,                       // the identity of an entry in a Vec<LootEntry> (D19)
    pub weight: u32,
}

#[derive(Reflect, Clone, Default)]
#[reflect(Default, type_name = "glutton::ItemDef", data(exitium_data::ReflectDataAsset))]
pub struct ItemDef {
    pub name: String,
    pub damage: f32,
    pub tags: Vec<String>,                 // a Vec of values: the item is its own key
    pub loot: Vec<LootEntry>,
    #[reflect(custom(widget = "asset", kind = "image"))]
    pub icon: Option<Guid>,
}
```

Requirements: `Reflect + FromReflect + Clone + Send + Sync`, and a struct root. `Default` is needed only to create a new asset with empty content (`data_create` without text). A type without `ReflectDataAsset` is refused with `NotADataAsset`, and a registered type that is not a struct is `RootNotStruct`, so a component cannot be loaded as a data asset by mistake. `data_types(registry)` lists the registered data types with each type's keyed list fields.

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
    loot: [
      (id: "iron", weight: 5),
    ],
    icon: Some("67e55044-10b1-826f-9247-bb680e5fe0c8"), // sprites/sword.png
  ),
)
```

A variant (`base: Some`) holds only the overrides. It is a sparse group with the **same type tag** as the root of its base chain. A container field may hold element ops:

```
%exitium-text 1 root exitium_data::DataFile
(
  base: Some("0b1cbd5e-3c2e-8f4a-b1d1-2f7a0a8e9c11"), // items/iron_sword.data
  value: glutton::ItemDef(
    damage: 15.0,
    tags: ops([
      remove("melee"),
      insert(at: last, value: "fire"),
    ]),
    loot: ops([
      insert(at: after("iron"), value: (id: "gem", weight: 3)),
      set("iron", (weight: 9)),
    ]),
  ),
)
```

Rules:

- **Envelope.** `base` is `None` or `Some("<uuid>")` (a `Guid`; a `// path` comment is regenerated on write and ignored on read). `value` is required. An unknown envelope field is a warning.
- **Prototype.** `value` is bound in `Complete` mode: missing fields are filled only from explicit `#[reflect(default)]` (serial-text-format.md). Unknown fields use the serial default, `Warn`. `ops(...)` is not allowed in a prototype (`OpsInPrototype`, a shape error).
- **Variant.** `value` is read as overrides ("Override engine"). An override that no longer fits the base chain is **stale** (D14): skipped, reported as `DataWarning::Stale`, kept and written back on save. A tag that differs from the base chain's root type is `BaseTypeMismatch`, a hard error.
- **Layout.** The writer fixes the envelope layout. A prototype value follows serial's layout rules. A sparse group is framed by this crate with the **same predicate** serial uses for groups (flat only when every member is leaf-like and there are at most 4 members, otherwise one member per line with a trailing comma; a struct member counts as non-leaf; a comment on a member forces a break), and each leaf is written by `write_value`. Overrides are written in path order (fields in declaration order), then kept stale entries in their file order, so one changed field is one hunk in a diff.
- **Limits.** `max_data_bytes` 1 MiB per file (the SP4 `max_text_bytes` applies first), `max_chain_depth` 8, `max_overrides` 4,096 per variant, `max_ops_per_container` 1,024, `max_ops_depth` 4.

### Override engine (`exitium_data::overlay`)

The engine is plain data and functions. It knows nothing about assets, entities or files.

```rust
/// A path into a reflected value through named-field structs only: "stats.hp", "tint.r".
/// The empty path is the root, and is valid only when the root type is not a struct.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldPath(Vec<Box<str>>);
impl FieldPath { pub fn root() -> FieldPath; pub fn is_root(&self) -> bool; }

pub struct FieldOverride { pub path: FieldPath, pub value: OverrideValue }
#[non_exhaustive]
pub enum OverrideValue {
    Set(Box<dyn Reflect>),        // a complete value of the leaf type; replaces the leaf whole
    Ops(Vec<ElementOp>),          // element ops on a keyed list or a map leaf, in written order
}

/// A set of overrides on one type. Sorted by path. Every entry is a leaf or an ops list:
/// no entry's target is a struct, no path is a prefix of another, and one path has one entry.
pub struct Overrides { /* Vec<FieldOverride> */ }
```

`Overrides` methods:

```rust
impl Overrides {
    pub fn is_empty(&self) -> bool;  pub fn len(&self) -> usize;
    pub fn iter(&self) -> impl Iterator<Item = &FieldOverride>;
    pub fn get(&self, path: &FieldPath) -> Option<&OverrideValue>;
    /// Normalises. At a leaf path: replaces that entry only (a whole value replaces any ops at that path).
    /// At a struct path: expands to one entry per leaf (recursively through named-field structs),
    /// replacing every entry at or below it. At the root of a non-struct type: one entry at the empty path.
    pub fn set(&mut self, ty: &TypeInfo, path: FieldPath, value: Box<dyn Reflect>) -> Result<(), OverlayError>;
    /// Appends one element op to the ops of a container leaf, compacting it with the earlier ops (table in "Element ops").
    pub fn push_op(&mut self, ty: &TypeInfo, path: FieldPath, op: ElementOp) -> Result<(), OverlayError>;
    pub fn remove(&mut self, path: &FieldPath) -> Option<OverrideValue>;     // a leaf or container path, or a struct path (removes every entry below it)
    pub fn remove_element(&mut self, path: &FieldPath, key: &ElementKey) -> bool;   // drops every op that names `key` in the ops at `path`

    /// Reads a sparse group from the syntax tree. Walks named-field structs itself and calls `bind` in Complete mode per leaf;
    /// a container member written `ops([...])` is read as element ops. A non-struct root is one entry at the empty path.
    pub fn read(syntax: &Syntax, group: NodeId, ty: &TypeInfo, registry: &TypeRegistry, opts: &ReadOptions) -> Result<ReadOverrides, OverlayError>;
    /// Writes a sparse tagged group (nested groups rebuilt from the sorted paths). A non-struct root is written as a complete group.
    pub fn write(&self, ty: &TypeInfo, registry: &TypeRegistry, out: &mut String, indent: usize) -> Result<(), OverlayError>;
    /// Validates every path first, then applies leaf entries, then runs each ops list in written order. On a path error the target is unchanged.
    /// Element ops that do not fit the current value are skipped and reported, never an error (D14).
    pub fn apply(&self, target: &mut dyn Reflect) -> Result<ApplyReport, OverlayError>;

    /// The leaves where `derived` differs from `base`, as overrides taken from `derived`. Keyed lists and maps become element ops when that is shorter (below).
    pub fn diff(base: &dyn Reflect, derived: &dyn Reflect) -> Overrides;
    /// Three-way merge for a refresh: whatever the user changed (live != old) keeps the live value; everything else takes the new base value.
    pub fn merge3(old_base: &dyn Reflect, new_base: &dyn Reflect, live: &mut dyn Reflect) -> Result<MergeReport, OverlayError>;
}

/// What `read` returns: the entries that bind, and the entries kept as text because they are stale (D14).
pub struct ReadOverrides { pub overrides: Overrides, pub stale: Vec<StaleEntry> }
```

**Stale entries (one shape for data, prefabs and scene instances).**

```rust
pub struct StaleEntry {
    pub scope: Option<Box<str>>,         // id of the instance or nested record whose patch holds the entry; None = the file's own patch
    pub node: Option<Box<str>>,          // prefab node id the entry targets (patch-local); None for a data variant
    pub section: StaleSection,           // Field | Set | Add | Remove | Added | Removed
    pub component: Option<Box<str>>,     // the component type name as written (it may no longer be registered)
    pub path: Option<FieldPath>,         // the field path inside a Field or Set entry
    pub text: String,                    // the original member text, verbatim
    pub reason: StaleReason,
}
#[non_exhaustive]
pub enum StaleSection { Field, Set, Add, Remove, Added, Removed }
#[non_exhaustive]
pub enum StaleReason {
    UnknownField, PathThroughLeaf, TypeChanged { expected: String }, EnumVariantGone,        // fields
    MissingNode, MissingComponent, UnknownComponentType, MissingParent, IdCollision,         // prefab structure
    AddedComponentUnbound, UnknownLabel,                                                      // added nodes, entity labels
    OpsUnavailable,                                                                           // the container has no key or is a fixed array
}
```

A `StaleEntry` keeps **text** only for entries that cannot be held as typed values. An element op that binds but does not fit the current list or map (key missing, key exists, anchor missing) stays in `Overrides` as a typed op, is skipped at `apply` with an `ApplyReport` warning, and is retried at every resolve, so it needs no `StaleEntry`. An op whose value no longer binds is stored in the ops list as `ElementOp::Opaque` (its text, at its original position) and is re-bound at each resolve.

**Equality (`leaf_eq`).** `reflect_eq` returns `Option<bool>`; `None` means "different". Floats compare by bit pattern (`to_bits`) wherever they occur in a leaf (a plain float, or inside a list, option or tuple), so `0.0` against `-0.0` is a difference and a NaN equals the same NaN. Every other leaf uses `reflect_eq`.

**Why leaf-wise and whole replacement.** `Reflect::apply` resizes a list and recurses element by element, so a "partial list" would silently keep stale tail elements. The engine therefore never calls `apply` for a leaf. It reads the field with `bind` in `Complete` mode and replaces the field with `Reflect::set` (found with `reflect_path_mut`). A `transparent` wrapper is resolved to its inner type first, so `hp` stays a leaf whether it is a plain `f32` or a wrapper around one. Order of checks for a type met on a path: transparent resolution, then `ReflectTextForm` (leaf), then non-transparent tuple struct and built-in tuple (leaf), then a named-field struct (recurse), else leaf.

#### Element ops (lists and maps)

```rust
pub enum ElementOp {
    // lists
    Insert { at: Anchor, value: Box<dyn Reflect> },                  // a complete element
    Move   { key: ElementKey, to: Anchor },
    // maps
    Put    { key: ElementKey, value: Box<dyn Reflect> },             // insert, or replace the value whole
    // both
    Remove { key: ElementKey },
    Set    { key: ElementKey, overrides: Overrides },                // sparse overrides on the element (a list item struct, or a map value)
    Opaque(OpaqueOp),                                                // an op whose text no longer binds, kept at its position
}
pub enum Anchor { First, Last, After(ElementKey), Before(ElementKey) }
/// A key value (a `Box<dyn Reflect>` of the key leaf type) plus a hashable `KeyId`
/// (Int, UInt, Bool, Char, Str or the unit-variant index). Compared by `KeyId`; no key is a float.
pub struct ElementKey { /* value, id */ }
pub struct OpaqueOp { pub text: String, pub reason: StaleReason }
```

**Identity rule (D19).** `list_identity(list: &ListInfo)` is computed once per list type and cached:

1. The item type (after transparent resolution) is a named-field struct with **exactly one** field carrying `custom(list_key = true)`, and that field's type is a key leaf (an integer `Value`, `String`, `bool`, `char`, `Guid`, or a unit-only enum): the key is that field.
2. The item type is itself a key leaf (`Vec<String>`, `Vec<Guid>`, `Vec<ResourceKind>`): the item is its own key, and the list behaves as an ordered set.
3. Anything else (a struct with no key field or with two, a nested list, a float list, a fixed array): the list is **unkeyed** and is replaced whole. An op on it is `OpsUnavailable`.

A map is keyed by its map key (reflect-model.md restricts map keys to the same leaf kinds).

*Why not an index:* an index op written against a base breaks when the base inserts an item earlier, which is the common case for a base that gains a loot entry. *Why not content:* an item cannot then be `set`. *Why not a stored id per element:* it would change every base file and every list type. The key field is the item's own id. It needs no new reflect attribute: `custom(key = literal)` exists and is not hashed (reflect-model.md "Attributes"), and #207 records the `list_key` convention. A derive-time check that a type has at most one key field is an optional reflect follow-up; this crate validates when a type is listed (`data_types` reports `list_key` and any error).

**Duplicate keys.** A list that currently holds two items with one key is ambiguous: an op naming that key is skipped (`AmbiguousKey`), and `diff` falls back to a whole-list `Set` for such a list (either side).

**Text form.** `ops([...])` replaces the whole list in the member position. The heads are lowercase and unqualified (`ops`, `insert`, `remove`, `move`, `set`, `put`, `first`, `last`, `after`, `before`), so they cannot collide with a type name (a user type always contains `::`, an engine core type is capitalised). Keys are spelled as the key leaf is spelled elsewhere (a string, a number, a unit-variant name, a `Guid` text). An element value is an untagged group (the element type is known from the list), and `at` defaults to `last` on read and is always written.

```
tags: ops([ remove("melee"), insert(at: last, value: "fire") ]),
loot: ops([
  insert(at: first, value: (id: "coin", weight: 50)),
  move("gem", to: after("iron")),
  set("iron", (weight: 9)),
]),
prices: ops([ put("Gold", 5.0), remove("Wood") ]),     // a HashMap<ResourceKind, Num>
```

**Semantics.** The ops of one container run **sequentially**, each on the result of the one before. For a list: `insert` adds the element at the anchor (`first`, `last`, or directly `after` or `before` the element with that key); `remove` deletes the element with that key; `move` removes it and re-inserts it at the anchor; `set` applies the sparse `overrides` to the element (which may itself contain ops for a keyed list inside the element, to depth `max_ops_depth` 4). For a map: `put` inserts or replaces; `remove` deletes; `set` applies sparse overrides to the value. Leaf entries of one `Overrides` apply first (in path order), then each ops list, in path order; paths are independent, so the order between containers cannot matter.

**Failure classes (every case has one).**

| Case | Class | Result |
|---|---|---|
| `ops(...)` is malformed, an unknown op head, a wrong member, nesting deeper than `max_ops_depth` (`OpsTooDeep`) | Parse error | The file fails. |
| `ops(...)` in a prototype value (`OpsInPrototype`) | Shape error | The file fails. |
| `set("k", (...))` whose group names the key field (`KeyInSet`); a `move` or `insert` anchor that is the moved key itself (`SelfAnchor`); more than `max_ops_per_container` ops (`TooManyOverrides`) | Patch conflict | The file fails. |
| The target is not a list or map (`TypeChanged`), is unkeyed, has duplicate keys, or is a fixed array (`OpsUnavailable`, `AmbiguousKey`) | Stale | The op is skipped with a warning and kept; retried at every resolve. |
| `insert` of a key that exists; `remove`, `set` or `move` of a key that does not; an anchor key that does not exist; `set` on a list of plain values; `put` into a list or `insert` into a map (`WrongContainer`) | Stale | Skipped, kept, retried. |
| The key text no longer parses as the key type, or an element value no longer binds | Stale | Stored as `ElementOp::Opaque` at its position, written back verbatim. |

A whole-value entry and an `ops(...)` entry cannot exist for one path in one group (a member name appears once, a parse error otherwise). In the API, `Overrides::set` at a container path replaces its ops, and `push_op` at a path that holds a whole value is `WholeValuePresent` (use `set` to replace, or `remove` first).

**Compaction in `push_op`.** An authored sequence stays short, because an op that follows another one on the same key folds into it:

| Earlier op | New op on the same key | Result |
|---|---|---|
| `insert` or `put` | `remove` | both removed |
| `insert` or `put` | `set` | folded into the inserted value |
| `insert` | `move` | the insert's anchor is replaced |
| `set` | `set` | the sparse groups merge, the newer wins |
| `set` or `move` | `remove` | replaced by the `remove` |
| `move` | `move` | the later one |

`read` never compacts (the file is the author's), so `write` after `read` is byte-identical.

**`diff` emits ops** for a keyed list or a map when the ops are **fewer than the derived length** and neither side has duplicate keys; otherwise it emits a whole `Set`. For a list the algorithm is deterministic: (1) one `remove` per base key missing from derived, in base order; (2) walk the derived list in order, keeping the longest increasing subsequence (LIS) of the common keys by base position as fixed; for each element that is new emit `insert(at: after(previous derived key))` (`first` for the first), for each common element outside the LIS emit `move(key, to: after(previous))`; (3) for each common element that differs, `set(key, Overrides::diff(base_item, derived_item))` (a plain-value list has none). Each placement is directly after the element's predecessor in `derived`, so the result equals `derived` (the property test checks it). A map: removes (sorted by key text), puts for new keys, and `set` (struct values) or `put` (other values) for changed ones, sorted by key text. Floats and equality use `leaf_eq`.

**`merge3` on a keyed list or map** (keys O in the old base, N in the new base, L live). The user's changes are what differs between O and L; the result starts from N and applies them:

- An element the user changed (its sparse diff from O is non-empty) keeps the live fields, merged recursively with the new base's fields (`merge3` on the element), whether or not N changed it.
- An element the user removed (in O and N, not in L) is removed from the result. An element the user inserted (in L, not in O) is inserted after its live predecessor if that is in the result, else appended, and reported (`AnchorMoved`).
- An element the base removed (in O, not in N): removed if the user did not touch it; **kept** if the user changed it (reported `ElementGone`).
- A key present in both L and N but not in O (the user and the base inserted the same key): the live element wins and is reported (`KeyAdded`).
- An element the user moved (outside the LIS of the common keys O to L) is placed after its live predecessor; other elements keep N's order.
- A list that is unkeyed or has duplicate keys is handled as a leaf (a user change wins, else the new value). A map is merged per key as above.

`apply` skips an insert whose anchor is missing and keeps the op (stale), but `merge3` appends it: in a refresh the live world is the only copy of the user's data, so losing the element is worse than misplacing it. `MergeReport` lists these cases.

```rust
/// The base relation of a set of assets. `base` is single-valued, so it is a forest. Used by data; prefabs add a nesting graph (below).
pub struct BaseGraph { /* Guid -> Option<Guid> base; reverse index */ }

impl BaseGraph {
    pub fn set_base(&mut self, guid: Guid, base: Option<Guid>) -> Result<(), OverlayError>;
    pub fn base_of(&self, guid: Guid) -> Option<Guid>;
    pub fn chain(&self, guid: Guid) -> Vec<Guid>;                 // [guid, its base, ..., the root prototype]
    pub fn dependents_topo(&self, guid: Guid) -> Vec<Guid>;       // transitive, bases before variants, ties by Guid
    pub fn height(&self, guid: Guid) -> usize;                    // longest chain of variants below
    pub fn remove(&mut self, guid: Guid);
}
```

`set_base` checks the cycle (`BaseCycle`) and the **depth of the whole result**, not only the entry's own chain: `depth(chain of the new base) + 1 + height(guid's subtree of variants) <= max_chain_depth` (8), else `BaseTooDeep`. Without the subtree term, a chain of 8 could grow to 15 by setting a base on its root.

`OverlayError` is `#[non_exhaustive]`: `UnknownPath { path, suggestions }`, `PathThroughLeaf { path }`, `TypeMismatch { path, expected, found }`, `RootNotStruct`, `BaseCycle(Vec<Guid>)`, `BaseTooDeep`, `Text(SerialError)`, `TooManyOverrides`, `OpsUnavailable`, `WholeValuePresent`, `KeyInSet`, `SelfAnchor`, `OpsTooDeep`. Prefix conflicts cannot exist because `set` normalises.

### `DataStore`

`DataStore` is a `Resource` (`impl Resource for DataStore`). It is **not** reflected (it holds `Box<dyn Reflect>` values and a graph); `DataStatus` is the reflected view.

```rust
pub struct DataStore { /* entries: HashMap<Guid, Entry, BuildHasherDefault<DefaultHasher>>, graph: BaseGraph, wanted: Vec<Guid>, journal: Option<PlayJournal>, next_rev: u32, revision: u64 */ }

struct Entry {
    base: Option<Guid>,
    source: Source,                       // Full(Box<dyn Reflect>) for a prototype (the value is stored here once)
                                          // Overrides(Overrides) for a variant
    stale: Vec<StaleEntry>,               // kept text of stale entries (D14), re-emitted on save
    resolved: Option<Box<dyn Reflect>>,   // None for a prototype (get reads Source::Full); Some for a Ready variant, cloned from the base and overridden
    state: DataState,
    generation: u32,                      // +1 on every change of the resolved value
    rev: u32,                             // state id (see "Undo and dirty")
    saved_rev: Option<u32>,               // the rev of the state last loaded from or written to disk; None = never saved
    pending_disk: Option<String>,         // disk text the store did not apply: parked behind an edit, or a text that failed to load
    force_next: bool,                     // the next payload is applied even though the entry is dirty (data_reload take_disk)
    last_error: Option<DataError>,
    warnings: Vec<DataWarning>,
}
impl Entry { fn dirty(&self) -> bool { Some(self.rev) != self.saved_rev } }

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
    pub fn in_play(&self) -> bool;                                     // the journal is on; the editor lane refuses file-writing commands
}
```

**The single load path.**

```rust
pub fn load_text(&mut self, guid: Guid, text: &str, registry: &TypeRegistry) -> Result<DataLoaded, DataError>;
```

0. **Dirty check.** If the entry exists, is dirty and `force_next` is false, the text is parked in `pending_disk` and the call returns `DataLoaded { parked: true, .. }` having changed nothing else (a conflict, listed in `DataStatus.conflicts`). With `force_next` the flag is cleared and the text proceeds. A payload that is applied (this call reaches step 5 for the entry) clears `pending_disk`; a later good payload therefore supersedes a bad one.
1. Parse the envelope and `value` (`DataFile`). A hard error (parse or shape error, `BaseTypeMismatch`, `NotADataAsset`, `UnknownType`, `TooLarge`) returns `Err` and **changes nothing**: a first load stays absent and is marked `Failed`; a reload keeps the old value and source, sets `last_error`, and keeps the text in `pending_disk`.
2. Set the base in the graph (`BaseCycle` and `BaseTooDeep` are hard errors). A variant whose base is not in the store yet becomes `Waiting(base)` and the base joins `wanted`.
3. Resolve the entry: a prototype is its value; a variant is `clone_boxed` of its base's resolved value with `Overrides::apply`. Stale entries and skipped ops are listed in `warnings`; they are retried on every later resolve, so one that binds again (the field came back) is applied, **unless a live override covers the same path** (below).
4. If the resolved value changed (`leaf_eq` on the whole value), bump `generation`, then re-resolve every entry in `graph.dependents_topo(guid)` the same way. A dependent that fails a hard error keeps its last good value and sets its own `last_error`; its own dependents are not re-resolved.
5. Set `rev` to a fresh state id and `saved_rev = Some(rev)` (the state equals the disk), and return `DataLoaded { guid, generation, changed: Vec<Guid>, failed: Vec<(Guid, DataError)>, parked }`, sorted by `Guid`. The glue turns it into events. (`DataLoaded` is not SP4's `Loaded`.)

All of it is one function, so a first load, a reload, a dependent re-resolve after its base arrived, and a test with text in memory use the same code (SP4 D13).

**Stale and live entries on one path.** A live override always wins over a stale entry for the same path or below it. `set_field`, `set_element` and `revert_field` remove stale entries at or below the path they touch; a stale entry that binds again at resolve is applied only if no live entry covers its path; and `write_data` drops a stale entry that a live entry covers, so a file never holds both. A save with `drop_stale` also clears the in-memory `stale` list after writing.

**A `Waiting` entry must fail loudly.** The glue reports base load failures to the store:

```rust
pub fn note_failed(&mut self, guid: Guid, reason: BaseFailure) -> Vec<Guid>;   // marks every entry Waiting on `guid` as Failed
pub enum BaseFailure { Failed { message: String }, NotData }                    // the server's AssetFailed / NotFound, or a base whose registry kind is not Data
```

An affected entry gets `last_error = BaseFailed { base }` or `BaseNotData { base }` and `DataFailed`; it leaves `Waiting` so the status shows the cause. A later successful load of the base makes it resolve (the failure was not terminal). For prefabs `BaseFailure` is reused with `NotData` read as "not a prefab" (`BaseNotPrefab`, `NestedNotPrefab`); the type lives in `exitium_data::store` and `exitium_scene` imports it through E1.

`DataError` is `#[non_exhaustive]`: `Parse { message, span }`, `UnknownType { name, suggestions }`, `NotADataAsset { type_name }`, `RootNotStruct`, `BaseTypeMismatch { expected, found }`, `BaseCycle(Vec<Guid>)`, `BaseTooDeep`, `BaseFailed { base }`, `BaseNotData { base }`, `Overlay(OverlayError)`, `OpsInPrototype`, `TooLarge { bytes, limit }`, `DiskChanged`, `StaleEdit`, `InPlay`, `NothingToSave`. Every text error carries the line and column. `DataWarning` is `Stale { entry: StaleEntry-summary }` and `OpSkipped { path, index, reason }`.

**Writes** (the editor lane calls these; see "MCP / editor surface").

```rust
pub struct WriteOptions { pub drop_stale: bool /*false: the kept stale entries are written back*/ }
pub fn write_data(&self, guid: Guid, registry: &TypeRegistry, resolve_path: &dyn Fn(Guid) -> Option<String>, opts: &WriteOptions) -> Result<String, DataError>;
                                          // NothingToSave for an entry that never loaded; path comments from the callback

// Authoring. Each returns an undo record; applying the record restores the entry (and re-resolves dependents).
pub fn render_new(type_name: &str, base: Option<Guid>, registry: &TypeRegistry) -> Result<String, DataError>;     // the text of a new asset: default value, or an empty variant
pub fn create(&mut self, guid: Guid, text: &str, registry: &TypeRegistry) -> Result<DataEdit, DataError>;          // after the file was written and registered (D9): a clean entry
pub fn set_field(&mut self, guid: Guid, path: &FieldPath, value_text: &str, registry: &TypeRegistry) -> Result<DataEdit, DataError>;   // prototype: sets the field; variant: adds or replaces an override
pub fn set_element(&mut self, guid: Guid, path: &FieldPath, op_text: &str, registry: &TypeRegistry) -> Result<DataEdit, DataError>;    // prototype: edits the list or map in place; variant: push_op with compaction
pub fn revert_field(&mut self, guid: Guid, path: &FieldPath, key: Option<&ElementKey>) -> Result<DataEdit, DataError>;   // variant only: removes the override; with a key, only the ops that name that key
pub fn flatten(&mut self, guid: Guid, registry: &TypeRegistry) -> Result<DataEdit, DataError>;      // variant becomes a prototype with its resolved value (unlink); stale entries are dropped
pub fn replace_from_text(&mut self, guid: Guid, text: &str, registry: &TypeRegistry) -> Result<DataEdit, DataError>;   // data_reload with the disk text
pub fn apply_edit(&mut self, edit: DataEdit, registry: &TypeRegistry) -> Result<DataEdit, DataError>;   // undo and redo: returns the inverse; StaleEdit if the state moved
pub fn mark_saved(&mut self, guid: Guid, dropped_stale: bool);                                       // saved_rev = rev after the file was written; clears `stale` when dropped
pub fn remove(&mut self, guid: Guid) -> Result<DataEdit, DataError>;                                 // refused while a variant still has this base (the error lists them)
```

**Undo and dirty.** `rev` is a **state id** taken from a store-wide counter: every new state (an edit, a flatten, a disk reload) gets a fresh id, and `apply_edit` puts back the id that the before-state had. A `DataEdit` holds the before state (source, stale list, base) and `applies_to`, the `rev` it can be applied to; a mismatch is `StaleEdit` and changes nothing, so an undo can never overwrite a reloaded value with a pre-reload copy. `dirty` is `rev != saved_rev`. Sequence: edit (rev 2, dirty), save (`saved_rev = 2`, clean), undo (rev back to 1: **dirty**, memory differs from disk), redo (rev 2: clean again). A later disk reload therefore parks behind the undone value instead of replacing it. The bus keeps the `DataEdit` in memory only (like `AssetEdit`), and the editor lane drops the history entries of a guid when `DataLoaded.changed` names it from a disk reload; `StaleEdit` is the safety net (what the bus does with an entry whose undo returns it is listed for #206 in "Downstream adjustments").

`set_field` on a prototype changes the resolved value and re-resolves its variants; a variant that does not override that field then sees the new value. `set_field` on a variant stores an override even if the value equals the base's, so "overridden" is an explicit state a user can revert. (A prefab instance patch computed by `diff` cannot hold an override equal to its base, because it records what *is*, not what was *chosen*; see "Writing: instances and prefabs". In both, the marker means "stored in the patch".)

### Hot reload and edits (relation to SP4)

Data assets use SP4's two consumer rules unchanged:

1. **One swap point per asset.** The glue's system runs in `First`, after set `ASSETS`. A frame sees one value of an asset; the swap never happens in `Update`. It runs in Edit mode too (`SimulationMode::Frozen`), as SP4 requires of authoring-time systems.
2. **A bad reload keeps the good value.** `load_text` returns the error; the entry keeps its value, `last_error` is set, `DataFailed { reload: true }` is sent, and `AssetFailed` from the server is mirrored into the same list.

What is new is the relation to **unsaved edits** (the dirty state) and to disk text the store could not apply (`pending_disk`):

| Entry state | What happens | Result |
|---|---|---|
| Clean | A reload payload (`generation >= 2`) arrives. | `load_text` runs at the swap point. `DataChanged` is sent. |
| Clean | The payload fails a hard error. | The value stays; `last_error` is set; the bad text is kept in `pending_disk`. |
| Dirty | A reload payload arrives. | Step 0 parks the text in `pending_disk`. The entry keeps its edited value. `DataStatus.conflicts` lists it, and the editor shows "changed on disk". No event of change. |
| Any, `pending_disk` set | `data_save`. | Refused with `DiskChanged { guid }` and the parked text in the result. The caller retries with `overwrite: true` (or resolves with `data_reload`). |
| Dirty, no `pending_disk` | `data_save`. | The file is written atomically, `note_own_write` is called, `mark_saved`. |
| Any | `data_reload { take_disk: true }` with `pending_disk` set. | `replace_from_text` applies the parked text now; undoable (`DataEdit`). |
| Any | `data_reload { take_disk: true }` with **no** `pending_disk`. | The command calls `AssetServer::reload(guid)` and sets `force_next`, so the next payload is applied even if the entry is dirty; the result says `reload_requested`. The value changes at the next swap point (one frame later), when `DataChanged` is sent. |
| Any | `data_reload { take_disk: false }`. | `pending_disk` is dropped, the edit stays. |

The server re-reads the file for every change, so the glue never reads a file itself. Own writes are filtered by SP4's `note_own_write`, so a save does not come back as a reload.

A variant is re-resolved whenever its base's resolved value changes, whether the change came from a disk reload, an in-memory edit, or an undo. This is the dependency SP4 left to this spec ("Hot reload", "Data assets").

**Creating and removing assets (D9).** A new asset has no file yet, and SP4's `edit::register_asset` is for a file already under `assets/`. So `data_create` runs: `DataStore::render_new` (pure text) -> `native::write_atomic` to the chosen path -> `register_asset` (mints the `Guid`) -> `note_own_write` -> `DataStore::create(guid, text)`, which makes a **clean** entry. There is never an in-memory-only entry. Undo is `DataStore::remove` plus `edit::remove_asset` (the file goes to `.trash`, SP4). `data_remove` is `DataStore::remove` plus `remove_asset`. Both write or move a file and are refused in Play.

### Play snapshot (Stage 5)

`editor-mcp-mvp.md` snapshots the scene text and the resources marked `custom(play_snapshot = true)` at `play_start`. `DataStore` is not reflected, so that generic mechanism never sees it, and a naive "snapshot every resource" cannot restore it. This crate provides explicit calls, and the play contract of SP8 calls them in `play_start` and `play_stop`:

```rust
pub fn begin_play(&mut self);                  // starts the journal
pub fn end_play(&mut self, registry: &TypeRegistry) -> PlayRestore;   // restores and re-resolves; clears the journal
pub struct PlayRestore { pub restored: Vec<Guid>, pub applied_pending: Vec<Guid> }
```

- While the journal is on, the **first** in-memory change to an entry (an edit, a flatten, a `data_reload`) copies the entry's before state into the journal. Later changes to the same entry add nothing. The cost is proportional to the number of edited entries, not to the store size.
- `end_play` restores every journaled entry (including its `rev`, so the dirty state returns to what it was) and re-resolves the dependents of each changed entry in topological order.
- A **disk reload** that was applied during play is **not** journaled and is **kept**: it came from the file, not from the play session. If a reload had been parked for a dirty entry, `end_play` applies the parked text after restoring (`applied_pending`).
- **File-writing and file-moving commands are refused in Play** (`DataError::InPlay`, enforced by the editor lane, which knows the mode, using `DataStore::in_play()`): `data_save`, `data_create`, `data_remove`, and for prefabs `prefab_save`, `prefab_create`, `prefab_flatten`, `prefab_remove`. They have no journal, and a file written during Play would put play-time state in the project. Recommendation to SP8, which owns the call sites (open question 7). Because creation is refused, nothing needs removing at `end_play`.
- In-memory data edits (`data_set_field`, `data_set_element`, `data_revert_field`, `data_flatten`) are allowed in Play and journaled. Prefabs have no in-memory edit state and no journal: the scene text snapshot already covers live instances.

### Prefabs in `exitium_scene`

A prefab is a tree of entities with a stable id per node. The scene crate gains `prefab`, `patch`, `instance` and `refresh` modules.

#### Prefab file

Header `%exitium-text 1 root exitium_scene::PrefabFile`, extension `.prefab`, `AssetKind::Prefab`, payload `Payload::Text`. Like `SceneFile`, `PrefabFile` is not a registered reflected type. The examples are illustrative (components shortened); the committed goldens are the reference. The envelope, every record, and every patch entry are always laid out broken, one member per line, as scenes are.

A **root prefab** has `base: None` and an `entities` list with the record shape of a scene. The first record is the root (`parent: None`, and not an instance), and every other record has a `parent`. The `id` labels are **node ids**: they are stored, never re-derived from `Name`, and **contain no `.`** (`InvalidNodeId`, a parse error): a dot is how a nested node is addressed (below). A record has either `components` or `prefab` (an instance record), never both.

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
    (
      id: @shield,
      parent: Some(@root),
      prefab: Some((
        asset: "9d2f4c1e-5a3b-8e77-b0c4-1f6d8a2e3b90", // prefabs/shield.prefab
        patch: (
          overrides: [
            (
              node: @root,
              set: [
                exitium_app::Transform2d(
                  translation: (x: 8.0, y: 0.0),
                ),
              ],
            ),
          ],
        ),
      )),
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
      (
        node: @shield.rim,
        set: [
          exitium_render::Sprite(
            tint: (r: 0.2, g: 0.2, b: 1.0, a: 1.0),
          ),
        ],
      ),
    ],
    added: [
      (
        id: @helmet,
        parent: Some(@root),
        components: [
          exitium_scene::Name("Helmet"),
        ],
      ),
    ],
    removed: [@weapon],
  ),
)
```

`node: @shield.rim` names the node `rim` inside the nested instance `@shield` of the base (a path through an instance label). A component group in `set` is a sparse group read by the overlay engine, so it may hold `ops([...])` for a keyed list.

**Labels.** In a root prefab, `@label` uses are checked against the file's own ids (`UnknownLabel` is a parse error). In a **variant**, a label names a node of the resolved base chain or a node this patch adds, so the file-local check does not apply; a label that does not resolve is stale (`MissingNode`, or `UnknownLabel` inside an entity value), not an error.

#### Patch semantics (one definition, used by variants, nested records and scene instances)

A patch has three lists: `removed`, `added`, `overrides`. **A scene instance patch and a nested record's patch have `overrides` and `removed` only**: `added` there is `AddedInInstancePatch`, a shape error, because the canonical form for a child is an ordinary record whose `parent` is the instance node (`@goblin_hat` below). A variant's patch has all three.

A patch is applied to the resolved base (a flat node list) **in this fixed order**:

1. `removed`: every listed node and its descendants are removed. Listing a node and its descendant is idempotent (not an error); the writer emits the minimum.
2. `added`: nodes are placed **in dependency order** (a node after its parent; ties in list order), each appended after its parent's last child. An added node may name a parent that is added later in the list. A nested record in `added` is spliced in the same way.
3. `overrides`: entries in list order; within one entry the steps are **`remove`, then `add`, then `set`**.

`remove` of a type then `add` of the same type replaces the component whole; `remove` then `set` of the same type finds nothing to set (stale). `add` of a type the node already has replaces the component whole. `set` holds **sparse** tagged groups read by `Overrides::read` (leaves replaced whole, a keyed list or map possibly as `ops([...])`, a non-struct component as one complete value). `add` holds **complete** tagged groups. Component types are written by their registered type name.

**Every case has exactly one class (D14):**

| Case | Class | Result |
|---|---|---|
| Malformed text; an id containing `.` (`InvalidNodeId`); both forms in one file (`PrefabShape`); two roots, a missing parent, a parent cycle; a root record that is an instance (`RootIsInstance`); an instance record with `components` (`InstanceHasComponents`); `added` in an instance patch (`AddedInInstancePatch`) | Parse or shape error | The file fails (`PrefabError`). A reload keeps the last good value. |
| `removed` names the root (`RemoveRoot`) | Patch conflict | The file fails. |
| An added node with `parent: None` (`AddedRoot`) | Patch conflict | The file fails. |
| Two added nodes with one id, or an added id equal to an id **this patch removes** (`DuplicateNodeId`) | Patch conflict | The file fails. (To replace a node, use `overrides`.) |
| An added node whose parent is removed by this patch (`AddedUnderRemoved`); added nodes that are each other's ancestors (`AddedCycle`) | Patch conflict | The file fails. |
| `overrides` for a node this patch removes (`OverrideOnRemoved`) or adds (`OverrideOnAdded`) | Patch conflict | The file fails. |
| The same node in two `overrides` entries, the same component type twice in one `set`, `add` or `remove` list, or twice in an added node's `components` (`DuplicateOverride`, `DuplicateComponent`) | Patch conflict | The file fails. |
| An element op that contradicts itself (`KeyInSet`, `SelfAnchor`, `TooManyOverrides`, `OpsTooDeep`: see "Element ops") | Patch conflict | The file fails. |
| An added id equal to an id **in the base chain** (the base gained it) | Stale | The added node and its subtree are skipped; reason `IdCollision`; the text is kept. The id check runs on the flat resolved ids, so an id inside a nested prefab (`shield.rim`) cannot collide with an added id, which has no `.`. |
| `removed` names a node the resolved base lacks | Stale | Skipped (`MissingNode`, section `Removed`). |
| `overrides` for a node the resolved base lacks (it never had it, a base patch removed it, or a path goes through a nested instance that is gone) | Stale | Skipped (`MissingNode`). |
| An added node whose parent is not in the resolved base or an earlier-placed added node (`MissingParent`) | Stale | The node and the nodes added under it are skipped. |
| An added node with a component whose type is unregistered or whose value no longer binds (`AddedComponentUnbound`, `UnknownComponentType`) | Stale | The whole added node and its subtree are skipped and kept as text. (The components of one node are one record, so the record is one entry.) |
| `set` for a component the node lacks, or a field the component no longer has; `remove` of a component the node lacks | Stale | Skipped (`MissingComponent`, `UnknownField`). |
| `set` whose value no longer binds (field type changed, enum variant gone, a path now goes through a leaf) | Stale | Skipped, with `TypeChanged` and the bind message. A bind failure never aborts the load. |
| A component type name that is not registered, in the `set`, `add` or `remove` lists of an `overrides` entry | Stale | Skipped; the warning suggests up to 3 registered names. **This relaxes SP4's "an unknown component type is always an error" for patch entries only**: SP4's rule exists so a save cannot drop data silently, and stale entries are kept and written back. The plain `components` lists of scene records and root prefab records keep SP4's error. (An added node's components are covered by the `AddedComponentUnbound` row above.) |
| An `@label` in an entity value that no longer resolves | Stale | Skipped (`UnknownLabel`); the scene load is not aborted. |
| A nested record's `asset` is not loaded | not a patch case | The container is `Waiting` (below). |

A stale entry is kept (a `StaleEntry`, in the in-memory patch) and written back on save unless the caller passes `drop_stale` (`SaveOptions`, `prefab_save`, `data_save`, `scene_save`). It is retried on every resolve, so one that fits again is applied. Patch conflicts are hard errors even inside a scene instance, because they cannot come from a base changing: they mean the file contradicts itself.

- **Entity references** inside a prefab use node labels (`target: @root`, or `@shield.rim` for a nested node), through the same `TextRefs` hook as scenes. A reference to anything outside the prefab is `ReferenceOutsidePrefab` on write.
- Reparenting a base node is not possible in a patch (open question 5).
- Limits: `max_prefab_nodes` 10,000 (the **flat** count after splicing; `TooManyNodes`), `max_chain_depth` 8 (combined, D21), `max_overrides` 4,096 per patch.

#### Nested prefabs

A record with `prefab: Some((asset, patch))` inside a prefab file is an **instance of another prefab** inside this one. Its `id` is the **nested root id** (no `.`), and its `parent` places it. The nested prefab's own nodes appear in the flat list as `<record id>.<inner id>`, and the nested root is the node `<record id>` itself. A record patch uses **nested-local** node ids (`node: @root`), exactly like a scene instance patch.

**Resolve is topological, not recursive.** `resolve(X)` uses only the already-resolved `Arc<ResolvedPrefab>` of the assets X depends on, so a deep structure costs one splice per level, and a reload re-resolves dependents in order. To resolve X:

1. Start from the base chain's resolved nodes, or for a root prefab the records of its own `entities` in order.
2. For each instance record (own or `added`): take the nested prefab's resolved nodes, apply the record's patch (nested-local ids), prefix every id with `<record id>.` (the root becomes `<record id>`), set the root's parent to the record's `parent`, **rebase the entity placeholders** of the nested components (placeholder index = index in the flat list; the same per-type `Entity` field cache the spawner uses), and splice the nodes in after the parent's last child (or at the record's position).
3. Apply the variant's own patch on the flat ids.

A variant's patch therefore overrides, removes or adds under a nested node by its dotted id (`@shield.rim`), and `removed: [@shield]` removes the whole nested instance.

**The dependency graph (D21).** Prefabs have two kinds of edge: a **base** edge (single-valued, `BaseGraph`) and **nesting** edges (many per prefab, `NestGraph` in `exitium_scene::prefab`). The store checks both together:

- A cycle through any nesting edge is `NestedPrefabCycle(Vec<Guid>)` (a prefab nesting a variant of itself is one). A base-only cycle is `BaseCycle`.
- The longest path in the combined graph, each edge counting 1, must not exceed 8 (`TooDeep`). Setting an edge checks `depth(below) + 1 + height(above) <= 8`, with the height measured over base and nesting dependents.
- `dependents_topo(guid)` for a prefab walks both reverse indexes: variants of `guid`, and prefabs that nest it, then **their** variants and containers, transitively, ordered so every prefab follows everything it depends on (ties by `Guid`). A diamond is possible here (A nests B and C, which both nest D) and is correct.
- A prefab whose nested asset is not loaded is `Waiting` on **all** missing assets (`PrefabState::Waiting(Vec<Guid>)`, base and nested alike); each joins `wanted`. A nested asset that fails or is not a prefab marks the container `Failed` through `note_failed` (`NestedFailed { guid }`, `NestedNotPrefab { guid }`), and a later good load of it resolves the container.
- On a **reload** of any asset, the store re-resolves its dependents in that order; a dependent that hits a hard error keeps its last good `Arc` and does not propagate, as for data.

`ResolvedPrefab.nested` lists the nested roots (`NestedNode { node, asset, generation }`), which is also how each nested origin is known: an instance of the outer prefab needs no separate origin chain.

#### Resolved form and the store

```rust
pub struct ResolvedPrefab {
    pub guid: Guid, pub generation: u32, pub chain: Vec<Guid>,
    pub nodes: Vec<ResolvedNode>,                 // flat, pre-order; nodes[0] is the root
    pub nested: Vec<NestedNode>,                  // nested roots: index into `nodes`, asset, the nested generation used
    pub warnings: Vec<PrefabWarning>,
    pub stale: Vec<StaleEntry>,                   // this file's own stale entries (D14), with node, scope and section
}
pub struct ResolvedNode { pub id: Box<str>, pub parent: Option<u32>, pub components: Vec<Box<dyn Reflect>> /* sorted by type name; pre-bound */ }
```

Components are bound **once at resolve time** (not at each spawn), so a runtime spawn is `clone_boxed` plus `ReflectComponent::insert`. Entity-typed fields cannot be bound to real entities yet, so they hold a **placeholder** `Entity`: `Entity::from_bits(((u32::MAX as u64) << 32) | node_index)`, which `exitium_ecs` offers as a public, checked constructor (ecs-storage-queries.md). The generation `u32::MAX` is **reserved for placeholders by the spawner, not by the ECS**: a slot is retired when generation `u32::MAX` is freed, but a *live* entity can hold it (after 2^32 - 2 frees of one slot, which a long headless run can reach). So when the spawner allocates an entity whose generation is `u32::MAX`, it despawns it (which retires the slot) and allocates another, and it never panics. The spawner rewrites placeholders to the spawned entities. Whether a component type holds `Entity` is found by walking its `TypeInfo` once and cached per `TypeId`; types without `Entity` fields skip the rewrite entirely. A map keyed by `Entity` is rebuilt through `Map::insert`.

```rust
pub struct PrefabStore { /* entries: HashMap<Guid, PrefabEntry, BuildHasherDefault<DefaultHasher>>, bases: BaseGraph, nests: NestGraph, wanted, ... */ }
struct PrefabEntry { source: PrefabSource, resolved: Arc<ResolvedPrefab> /* last good */, state: PrefabState, rev: u32, last_error: Option<PrefabError> }
pub enum PrefabState { Waiting(Vec<Guid>), Ready, Failed }

impl PrefabStore {
    pub fn load_text(&mut self, guid: Guid, text: &str, registry: &TypeRegistry) -> Result<PrefabLoaded, PrefabError>;   // the same rules as DataStore::load_text, steps 1 to 5
    pub fn get(&self, guid: Guid) -> Option<Arc<ResolvedPrefab>>;
    pub fn list(&self) -> impl Iterator<Item = PrefabInfo> + '_;           // sorted by Guid
    pub fn generation(&self, guid: Guid) -> Option<u32>;
    pub fn want(&mut self, guid: Guid);  pub fn take_wanted(&mut self) -> Vec<Guid>;
    pub fn note_failed(&mut self, guid: Guid, reason: BaseFailure) -> Vec<Guid>;
    pub fn dependents(&self, guid: Guid) -> Vec<Guid>;                     // variants and containers, transitive, topological
    pub fn render_root(&self, name: &str, registry: &TypeRegistry) -> String;                       // the text of a new root prefab
    pub fn render_variant(&self, base: Guid, registry: &TypeRegistry) -> Result<String, PrefabError>;   // the text of a new empty variant
    pub fn remove(&mut self, guid: Guid) -> Result<PrefabEdit, PrefabError>;      // refused while a variant has this base or a prefab nests it
    pub fn flatten(&mut self, guid: Guid, deep: bool, registry: &TypeRegistry) -> Result<PrefabEdit, PrefabError>;   // see below
    pub fn apply_edit(&mut self, edit: PrefabEdit, registry: &TypeRegistry) -> Result<PrefabEdit, PrefabError>;      // undo and redo; StaleEdit if rev moved
}
```

`PrefabStore` repeats `DataStore`'s rules on purpose: one `load_text` for a first load and a reload, a failed reload keeps the last good `Arc<ResolvedPrefab>`, a changed dependency re-resolves its dependents in topological order, and a prefab with a missing dependency is `Waiting` and fails loudly through `note_failed`. `PrefabStore` has **no dirty or conflict state, and nothing is held only in memory**: every command that changes a prefab asset **writes the file at once** (`write_atomic`, then `note_own_write`; a new asset is written first and registered after, D9) and then loads it, exactly as `data_create` does. So there is no unsaved prefab edit apart from the live entities in the scene, a later reload cannot silently replace a store-only change, and no journal is needed. A `PrefabEdit` is the previous file text, the guid and the `rev` it applies to; undo writes the previous text back (and `remove_asset` or `register_asset` as needed); `StaleEdit` if the store's `rev` moved (a reload happened). `PrefabStore` is **not** where instances are tracked (see "Instance origin").

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

- The record's `id` is the **instance root label**. The prefab's root node is that entity. Every other node `n` of the flat resolved prefab (nested content included, so `shield.rim` too) is the entity labelled `@<instance>.<n>` (`@goblin_1.weapon`, `@goblin_1.shield.rim`). **An authored `id` (scene record or prefab node) contains no `.`** (`InvalidLabel` in scenes, `InvalidNodeId` in prefabs, parse errors): a dotted label is produced only by instance expansion. This is an addition to SP4's label rule, carried by #205. A hand-written `id: @goblin_1.weapon` therefore has a defined error, and an `@x.y` *use* is split at the first `.` into instance and node id.
- `components` of an instance record must be empty or omitted (`InstanceHasComponents`). An entity added under an instance is an ordinary scene record whose `parent` is an instance label, as `@goblin_hat` shows. Its `SceneMember` is the scene's, so it saves normally.
- `patch` has the shape of an instance patch above (`overrides`, `removed`). `node:` names a **prefab-local** (flat) id, not an instance label. Entity references in an instance patch's values use **scene labels** (`@goblin_1.weapon`), because the values are bound after the instance's entities exist; in a `.prefab` variant they use prefab-local labels.
- **Instantiate** follows SC4's atomic algorithm: the instance's surviving nodes (the flat resolved prefab minus the patch's `removed`) are spawned first, then values are bound, then the patch is applied to the live entities. Any hard error rolls the whole scene back, as in SC4; stale entries only warn (`SceneWarning::StaleInstanceEntry`). The result is one `SceneInstance` with entities in this order: **an instance record contributes its nodes in flat prefab pre-order, root first**, at the record's position in the file. `write_scene` reports entities in the same order in **`SavedScene.entities: Vec<Entity>`** (file order), so the `EntityRemap` that `play_stop` builds and the test that checks the order have a carrier (#205 adds the field).
- **Parse and load.** `ParsedScene::parse` accepts a label `@x.y` as declared by instance `@x` without resolving the prefab (the prefab may not be loaded). `instantiate` resolves the prefab and reports `UnknownLabel` for an inner label that does not exist (with suggestions). **An `@label` inside an instance patch's values is not checked at parse**: it is checked at bind, and one that does not resolve makes that entry stale (`UnknownLabel`), so a dangling label in a kept entry never aborts the load. A prefab that is not in the `PrefabStore` fails `instantiate` with `PrefabNotLoaded { guid }` before anything is spawned. `ParsedScene::prefab_guids()` lets the glue request them first.
- **Scene API plumbing.** `InstantiateOptions`, `validate_scene` and `SaveOptions` each gain `prefabs: Option<&PrefabStore>` (and `SaveOptions` gains `drop_stale: bool`). `validate_scene` runs its scratch-world instantiate with the store it is given. A scene with an instance and `prefabs: None` is `PrefabStoreMissing`. The editor's `scene_load`, `scene_validate` and `scene_save` pass the store; `scene_load` requests missing prefabs and returns `PrefabsPending { guids }` so the agent can retry, while the facade's `LoadScene` waits (see "Facade glue"). These are listed under "Downstream adjustments".
- **New scene errors and warnings (all additive, #205).** `SceneErrorKind` gains `InvalidLabel`, `PrefabNotLoaded`, `PrefabStoreMissing`, `InstanceHasComponents`, `AddedInInstancePatch`, `PatchConflict`. `SceneWarning` gains `MissingPrefabNode { label }`, `PrefabNodeDetached { entity }`, `StaleInstanceEntry { instance, node, reason }`, `StaleEntryDropped { instance, reason }` (dangling label or `drop_stale`) and `InstanceOriginMissing { entity }`.
- **A parent that no longer exists.** If a scene entity's parent label names an instance node that the current prefab no longer has, the entity attaches to the instance root and `SceneWarning::MissingPrefabNode { label }` is recorded.

#### Runtime components (reflected)

```rust
#[derive(Component, Reflect, Clone)] #[reflect(Component, type_name = "exitium_scene::PrefabInstance", custom(derived = true))]
pub struct PrefabInstance { pub asset: Guid, pub generation: u32 }              // on the instance root

#[derive(Component, Reflect, Clone)] #[reflect(Component, type_name = "exitium_scene::PrefabNode", custom(derived = true))]
pub struct PrefabNode { pub instance: Entity, pub node: String }                 // on every spawned node, the root included; `node` is the flat id (`shield.rim`)

#[derive(Component, Reflect, Clone)] #[reflect(Component, type_name = "exitium_scene::PrefabNested", custom(derived = true))]
pub struct PrefabNested { pub asset: Guid, pub generation: u32 }                // on the root of a nested instance, besides its PrefabNode

#[derive(Component, Reflect, Clone)] #[reflect(Component, type_name = "exitium_scene::PrefabNodeId", custom(derived = true))]
pub struct PrefabNodeId(pub String);                                             // authored root-prefab entities: the stable node id
```

All four are `derived`, so a scene write skips them (the instance record carries the same information). The type is `PrefabNodeId`, not `NodeId`, because `exitium_serial::NodeId` (a syntax-tree node) is in scope in the scene crate and the facade.

#### Instance origin

An instance needs to remember which resolved version it was spawned from (refresh needs the *old* base) and which stale entries its patch carried (save must write them back). This is one **non-reflected component** on the instance root, written by whoever spawns the instance:

```rust
#[derive(Component)]
pub struct InstanceOrigin { pub prefab: Arc<ResolvedPrefab>, pub stale: Vec<StaleEntry> }
```

- It is written by `instantiate_prefab` (when `SpawnOptions.track_origin` is true, the default) and by the scene `instantiate`. It dies with the entity, so runtime-spawned bullets cannot grow a table. A game that spawns thousands of short-lived instances sets `track_origin: false` and pays for a `PrefabInstance` only; such an instance cannot be refreshed (`RefreshUnavailable`). **`track_origin: false` together with `scene: Some(..)` is refused** (`SpawnError::UntrackedSceneMember`): a scene member needs a base to diff against when it is saved. An instance that reaches `write_scene` with `PrefabInstance` and no `InstanceOrigin` anyway (an editor or script mistake) is written as plain entities with `SceneWarning::InstanceOriginMissing`, never as a guess.
- **`write_scene` skips `InstanceOrigin` silently**, as it does `SceneMember` (a built-in rule beside the derived-component rule), and the facade lists `exitium_scene::InstanceOrigin` in `INTERNAL_COMPONENTS`, so a scene with instances gives no `UnsavedComponent` warning (and no error under `strict`), and the editor's component-by-component snapshot ignores it. Both are carried by #205 and #206.
- Old resolved versions are kept alive only by the `Arc`s that instances hold, and are freed when the last instance refreshes or despawns.
- **Undo of a despawn.** The editor restores a despawned instance under new entity ids (editor-mcp-mvp.md). It does that by writing the instance with `write_scene` (`SaveScope::Entities`) and instantiating the text again, which creates a fresh `InstanceOrigin` and keeps the stale entries. The editor lane must not capture instance entities component by component (a non-reflected component would be lost); this is a stated requirement on SP8 in "Downstream adjustments". `PrefabInstance` marks which entities need it.
- **Nested instances** have no origin of their own: they live inside the outer instance's flat nodes, and `ResolvedPrefab.nested` records which asset and generation each came from. A nested root (`PrefabNested`) cannot be refreshed, unpacked or reverted on its own: those commands refuse with `NestedInInstance { outermost }`. To change nesting, edit the container prefab.

#### Writing: instances and prefabs

**`write_scene` with instances.** For each entity with `PrefabInstance`, the writer takes `InstanceOrigin.prefab` as the base and compares the live subtree with its flat nodes (members identified by `PrefabNode.node`). Before any comparison, each live component is **normalised**: on a clone of the component, every `Entity` that belongs to the same instance (nested nodes included) is mapped back to its placeholder (node index), through the same walk the spawner uses in reverse. So an intra-instance reference is equal to the base's, and is not an override. An `Entity` that points **outside** the instance is a true difference and is written as a scene label (`ReferenceOutsideScene` if it is not in the scene, as for any component). The engine API does not change: `Overrides::diff` and `merge3` always see placeholders. **When `diff` returns an override value that still holds a placeholder** (a field retargeted to another node of the same instance), the writer maps the placeholder to the label of that node (`@inst.node` in a scene, a prefab-local label in a `.prefab`).

1. A resolved node with no live entity becomes `removed`.
2. A live entity with a component the resolved node lacks becomes `add`.
3. A resolved component missing from the live entity becomes `remove`.
4. A component present in both goes through `Overrides::diff` (resolved against the normalised live component) and becomes `set`, with element ops where D20 gives them. An override equal to the base is **not** written: a diff records what is, not what was chosen.
5. A live entity under the instance with no `PrefabNode` is not part of the patch; it is written as an ordinary record with `parent: Some(@inst.node)`. (This is the only form for a child: an instance patch has no `added`, so a spawned node always comes from the prefab.)
6. A live node whose **parent differs from the resolved parent** is **detached**, whether it was moved out of the instance or **reparented inside it** (`Parent` is not saved, so the new parent would otherwise revert on reload). The detached node **and its live descendants** are written as ordinary records (the new parent as `parent`, `@inst.node` for an in-instance parent), with `SceneWarning::PrefabNodeDetached`, and the instance patch lists the detached node in `removed`.
7. The stale entries in `InstanceOrigin.stale` are written back after the computed entries, unless `SaveOptions.drop_stale` is set or a live entry now covers the same node, component and path (the live one wins and the stale one is dropped, `StaleEntryDropped`). **A stale entry whose text holds an `@label` that is not declared in the output** (the entity was renamed, deleted, or not in a `SaveScope::Entities` write) cannot be kept: it is dropped with `StaleEntryDropped { reason: DanglingLabel }`, since labels are re-derived on every write (SP4).

The skip and warn rules for components are the same as `write_scene`'s (derived components, `SceneMember`, `InstanceOrigin`, `Parent`, `Children`, `UnsavedComponent`), and `SaveOptions` is shared.

**`write_prefab`** writes a **root prefab** from a live subtree: nodes in pre-order, node ids from `PrefabNodeId` (new nodes get an id derived from `Name`, with `_2`, `_3` on collision, never containing `.`, then **keep it**: renaming the entity later does not rename its node id). **A live instance root below the save root becomes a nested record** (`prefab: Some((asset, patch))`, `patch` computed against its `InstanceOrigin` as above, its stale entries re-scoped to the record), and its nested content is not written as nodes. An instance root without `InstanceOrigin` is written as plain nodes with `InstanceOriginMissing`. **`write_prefab` of a subtree whose own root is an instance root is `RootIsInstance`**: that save is a variant, so the caller uses `write_prefab_variant`. `write_prefab_variant` writes a **variant**: it resolves the base chain and diffs the live subtree against it, as for an instance, with the same normalisation. **Its stale entries are the target prefab's own** (`ResolvedPrefab.stale` of the asset being saved to, in this file's patch and its records' scopes), **not `InstanceOrigin.stale`**: stale entries that the instance carried belong to the scene file it came from, and are reported (`StaleEntryDropped`) when they are not written. The same dangling-label rule applies.

**Editing a prefab** has one path: instantiate the prefab (or create a new subtree), edit the live entities, then `prefab_save(root, asset)`. A nested prefab is made by instantiating one prefab under a node of the subtree being edited and saving. If `asset` is a root prefab the whole subtree is written; if it is a variant, only the diff against its base is written. Saving an instance back into its own source prefab is the "apply" operation.

**`flatten`.** `PrefabStore::flatten(guid, deep)` turns a variant into a root prefab. It writes the base chain's records merged with the variant's patch (the base's own components, its nested records, with the variant's dotted overrides **folded into the matching nested record's patch**, prefix stripped), drops the variant's stale entries, and writes the file at once. With `deep: true`, nested instance records are also replaced by plain node records (unlinking the nested prefabs), so the result has no dependency but its own file.

**`unpack`** (`prefab_unpack`) removes `PrefabInstance`, `PrefabNode` and `InstanceOrigin` from an instance and keeps the entities. Its stale entries are **dropped** and reported in the result (`dropped_stale`), as `flatten` does. With `recursive: false` (the default) each nested root becomes an **independent instance**: it gets `PrefabInstance`, an `InstanceOrigin` with the nested prefab's `Arc`, and `PrefabNode` ids relative to the nested prefab, its live values being the truth (the next save diffs them against the nested prefab). With `recursive: true` all of it becomes plain entities. On a nested root `unpack` refuses with `NestedInInstance`.

#### Runtime spawn and refresh

```rust
/// Exclusive form, for editor commands, scene loading and tests.
pub fn instantiate_prefab(world: &mut World, registry: &TypeRegistry, prefab: &Arc<ResolvedPrefab>, opts: SpawnOptions) -> Result<PrefabSpawn, SpawnError>;

/// Deferred form, for ordinary systems that only hold `Commands`. Queued with `Commands::add`; runs when commands apply.
pub trait SpawnPrefabExt { fn spawn_prefab(&mut self, prefab: Arc<ResolvedPrefab>, opts: SpawnOptions); }
impl SpawnPrefabExt for Commands<'_, '_> { /* wraps instantiate_prefab */ }

pub struct SpawnOptions {
    pub parent: Option<Entity>,
    pub scene: Option<SceneId>,                           // adds SceneMember so the instance is saved with that scene (requires track_origin)
    pub root_components: Vec<Box<dyn Reflect>>,           // complete component values inserted on the root after spawning, replacing the prefab's own
                                                          //   (e.g. a Transform2d): the scene crate never names Transform2d
    pub track_origin: bool,                               // true by default
    pub on_spawn: Option<Box<dyn FnOnce(&mut World, &PrefabSpawn) + Send>>,   // the deferred form's way to learn the entities
}
pub struct PrefabSpawn { pub root: Entity, pub entities: Vec<Entity> }   // entities in flat prefab pre-order, root first
```

It spawns `prefab.nodes.len()` entities (nested content included: the resolved list is flat), inserts `clone_boxed` copies of the pre-bound components (plus the placeholder rewrite where needed), builds the hierarchy, and tags the nodes (`PrefabInstance`, `PrefabNode`, `PrefabNested` on nested roots, `InstanceOrigin`, and `SceneMember` when `scene` is set). It is allocation-light on purpose: game code spawns bullets and enemies with it. The runtime form takes **no patch**: a game that wants a variant of a variant uses `PrefabStore` to resolve it. (Scene instance patches are applied by the scene `instantiate`, not here.)

**`refresh(world, instance)`** brings a live instance up to date with its prefab. It works on the **flat** lists, so nesting is transparent to it:

1. `old` is `InstanceOrigin.prefab`, `new` is `PrefabStore::get(asset)`. If the instance has no `InstanceOrigin` (spawned with `track_origin: false`), the refresh is refused with `RefreshUnavailable`; the user unpacks the instance or reloads the scene. On a nested root it is refused with `NestedInInstance`. A change to a nested prefab, or to any base in the chain, is already in `new`, because the store re-resolved the container when the dependency changed.
2. For each node (by flat id) present in both, each component present in both goes through `Overrides::merge3(old, new, live)` on the **normalised** live component (intra-instance entities mapped to placeholders), and the merged result is mapped back to the real entities of the instance. A leaf the user changed keeps the live value; every other leaf takes the new value; a keyed list or map merges per element (D20). Components added in `new` are inserted; components removed in `new` are removed unless the user changed them (then they stay and the report lists them).
3. Nodes present only in `new` are spawned (a whole new nested instance appears as its nodes, `PrefabNested` on its root); nodes only in `old` are despawned unless the user added children to them (then the node stays, reported).
4. The stale entries of `InstanceOrigin` are retried against `new`: one that fits now is applied.
5. `PrefabInstance.generation`, `PrefabNested.generation` and `InstanceOrigin.prefab` are set to the new version.
6. The result is a `RefreshReport`, which carries the **undo record**: the `write_scene(SaveScope::Entities)` text of the instance subtree taken **before** the refresh. Refresh can despawn and spawn nodes, so undo does what the undo of a despawn does: it despawns the instance and instantiates that text again (new entity ids, a fresh `InstanceOrigin`), and the bus's `EntityRemap` applies. The record is plain data, so a later reload of the prefab cannot make it stale; undo restores the instance as it was, on the version it had.

### Facade glue (contract)

The glue lives in `exitium` (facade issues FD1 and FD2; they hold the `crates/exitium/` hotspot and have `Edits: none`). `DataPlugin` and `PrefabPlugin` are added to `DefaultPlugins`.

- **Schedule.** Every system below runs in `First` after set `ASSETS` (or in `PostUpdate` for scans), never in `Update`, so Edit mode (`SimulationMode::Frozen`) works.
- **Delivery.** `Data` and `Prefab` are kinds of their own with a single consumer each, so each frame the glue calls `AssetServer::take_loaded(AssetKind::Data)` and `take_loaded(AssetKind::Prefab)` and passes each `Payload::Text` to `DataStore::load_text` or `PrefabStore::load_text`. The `AssetLoaded` event is a **log signal only**; nothing depends on when it is raised, because SP4 does not say whether it is sent when a payload is queued or when a consumer takes it. A reload payload may have been coalesced (a later change replaced an earlier one), and `take_loaded_guid` may return `None` for a guid whose payload was taken, so delivery tolerates both. Delivery is capped by SP4's `max_reload_deliveries` and `max_reload_bytes` per frame; what is left is taken next frame, and the cap bounds the frame cost. (`take_loaded_guid` is not needed for these kinds. A game that adds its own consumer for a subset of data guids uses it. This is a deliberate difference from the issue text, which names `take_loaded_guid`: that call is for kinds several consumers share, such as `Text`.)
- **Events.** For each guid in `DataLoaded.changed` the glue sends `DataChanged { guid, generation }` and for each failure `DataFailed { guid, message, reload }`. `AssetFailed` for a guid of kind `Data` or `Prefab` is passed to `note_failed`, as is a base or nested asset whose registry kind is not the right one (`BaseNotData`, `NestedNotPrefab`).
- **Requests.** After each delivery the glue calls `AssetServer::request` for `take_wanted()` (bases and nested prefabs) and for `refs_of(guid)` (referenced data assets and prefabs). `DataSettings.preload` (default `true`) also requests every `Data` and `Prefab` asset in the registry once the manifest is `Ready`, and again for entries a manifest reload adds.
- **Scenes with prefabs.** `LoadScene` (FA3) gains a waiting state: after parsing, the glue calls `ParsedScene::prefab_guids()`, requests the prefabs, and instantiates (with `prefabs: Some(&store)`) once all of them are `Ready` (a `Ready` prefab is resolved with every base and nested prefab). A prefab that fails (or that stays `NotFound` after SP4's retry schedule of 1, 2, 4, 8, 16 s) fails the load: the glue sends SP4's existing event `SceneLoadFailed { path, errors }` with `errors` naming `PrefabFailed(guid)` in its text. **No event shape changes**; the scene is not instantiated. The state is `SceneLoadState::WaitingForPrefabs`, reported by the existing scene status.
- **Own writes.** The glue does nothing for them; the editor lane calls `note_own_write` after `data_save`, `prefab_save` and the prefab authoring commands, as for scenes (SP4).
- **Re-exports.** `exitium::data` re-exports every public type that appears in a public signature: `DataStore`, `DataInfo`, `DataState`, `DataError`, `DataWarning`, `DataEdit`, `DataLoaded`, `WriteOptions`, `PlayRestore`, `BaseFailure`, `FieldPath`, `Overrides`, `OverrideValue`, `ElementOp`, `ElementKey`, `Anchor`, `StaleEntry`, `StaleSection`, `StaleReason`, `ReflectDataAsset`, `DataPlugin`, `DataStatus`, `DataSettings`, `DataChanged`, `DataFailed`. `exitium::scene` gains `PrefabStore`, `PrefabState`, `PrefabInfo`, `PrefabLoaded`, `PrefabError`, `PrefabWarning`, `PrefabEdit`, `ResolvedPrefab`, `ResolvedNode`, `NestedNode`, `PrefabInstance`, `PrefabNode`, `PrefabNested`, `PrefabNodeId`, `SpawnOptions`, `SpawnError`, `PrefabSpawn`, `SpawnPrefabExt`, `RefreshReport`, `instantiate_prefab`, `refresh`, `write_prefab`, `write_prefab_variant`, `PrefabPlugin`, `PrefabStatus`, `PrefabChanged`, `PrefabFailed`. **Not re-exported:** `BaseGraph`, `NestGraph`, `InstanceOrigin` (a component, reached through tools), and the engine's internal helpers (path walking, key index, placeholder cache). `StaleEntry` is public because it is a field of `ResolvedPrefab`.

### Reflected surface (C4)

| Item | Kind | Notes |
|---|---|---|
| `ReflectDataAsset` | type data | Marks a type loadable as a data asset. Listed by `data_types`. |
| `DataStatus` | resource, read-only (`custom(readonly = true)`, editor-mcp-mvp.md X4) | `entries`, `ready`, `waiting`, `failed`, `dirty` counts; `failures: Vec<DataFailure { guid, path, message }>`; `warnings`; `conflicts: Vec<Guid>`; `revision: u64` (bumps on any change). Inspector and `ecs_get_resource` read it. |
| `DataSettings` | resource | `preload: bool` (true). `custom(history = false)`: a settings change is not an undo step (as `AssetSettings`). |
| `DataChanged`, `DataFailed` | events | `{ guid, generation }`, `{ guid, message, reload: bool }`. |
| `PrefabInstance`, `PrefabNode`, `PrefabNested`, `PrefabNodeId` | components | `derived`, not saved; readable by `ecs_get_component`. |
| `PrefabStatus` | resource, read-only (`custom(readonly = true)`) | `prefabs`, `ready`, `waiting`, `failed` counts, `failures`, `warnings`, `outdated_instances: u32` (instances whose `PrefabInstance.generation` is older than the store's), `revision`. |
| `PrefabChanged`, `PrefabFailed` | events | As for data. |
| Commands (tools) | see the next section | Registered by the editor lane. |

`outdated_instances` is **not a per-frame scan**: it is recomputed with one query when `PrefabChanged` is sent and after a refresh, spawn or despawn of a tracked instance changes the count.

`DataStore`, `PrefabStore` and `InstanceOrigin` are deliberately not reflected: their contents are reached through the tools below, which read them through the plain functions. The tools are therefore a complete view; nothing the inspector can show is missing from MCP (agent parity).

### Threading

All mutation happens on the main thread: the glue systems in `First`, and the editor's command drain. `DataStore` and `PrefabStore` are `Send + Sync` (`Box<dyn Reflect>` and `Arc<ResolvedPrefab>` are), so any number of systems may read them in parallel through `Res`, and the parallel executor needs no lock. A system sees one version of an asset for a whole frame, because the swap only happens in `First`. Parsing and resolving are done inline: the budgets below keep a reload frame under SP4's 20 ms cap. There is no worker thread in either crate. A `ResolvedPrefab` is immutable once built and is shared through `Arc`, so a reload replaces the pointer and never changes a prefab a system is reading. No `unsafe`.

## MCP / editor surface

The data and scene crates have no `exitium_command` edge (D2, D16). Each row is a plain function the crates expose. The editor lane (#110 and SP8) wraps each as a `Command` or `Query` with the stated undo kind, registers it with `register_command`, and gets a tool with the same name. The parity test grows by these rows.

| Plain Rust function | Tool | Kind and undo |
|---|---|---|
| `data_types` (registry scan for `ReflectDataAsset`, with each type's `list_key` fields) | `data_types` | Query. Names, fields, schema, whether `Default` exists, which list fields are keyed. |
| `DataStore::list` | `data_list` | Query. Filter by type, by base, by state (`dirty`, `waiting`, `failed`). |
| `info`, `get`, `overrides`, `write_data` | `data_get` | Query. The resolved value as text, base, overrides (leaves and element ops), stale entries and warnings, state, last error, parked disk text (conflict). |
| `render_new`, `write_atomic`, `register_asset`, `note_own_write`, `create` | `data_create` | Command. Needs a `path`. Optional `base` makes a variant. Undo: `DataStore::remove` plus `remove_asset`. Refused in Play. |
| `set_field` | `data_set_field` | Command. Value as text through serial; path syntax of reflect-model.md (`stats.hp`). A struct path sets every leaf below it; a list or map path with a whole value replaces it whole. Undo: `DataEdit`. |
| `set_element` | `data_set_element` | Command. Args: `path`, `op` (`insert`, `remove`, `move`, `set`, `put`), `key`, `at`, `value` as text. On a prototype it edits the list or map; on a variant it appends an op with compaction. A list with no key is refused (`OpsUnavailable`). Undo: `DataEdit`. |
| `revert_field` | `data_revert_field` | Command. Optional `key`: revert only that element's ops. Undo: `DataEdit`. |
| `flatten` | `data_flatten` | Command. Undo: `DataEdit`. |
| `write_data`, `native::write_atomic`, `note_own_write`, `mark_saved` | `data_save` | Command, **no undo** (`UndoKind::NotUndoable`: a file write). Args `overwrite` (default false: `DiskChanged` when disk text is parked) and `drop_stale`. Refused in Play. |
| `replace_from_text`, `AssetServer::reload`, drop of `pending_disk` | `data_reload` | Command. `take_disk` bool. With no parked text it requests a reload and reports `reload_requested`. Undo: `DataEdit` (restores the in-memory value; `StaleEdit` if the entry changed since). |
| `DataStore::remove`, then `exitium_assets::edit::remove_asset` | `data_remove` | Command. Refused (with the list) while variants use it as a base or other assets name it. Undo: `DataEdit` plus the asset edit. Refused in Play. |
| `referrers`, `refs_of`, plus `edit::find_references` | `data_find_references` | Query. Data assets, prefabs and scenes that name the guid (AK1 extends `find_references` to `Data` and `Prefab` files). |
| `PrefabStore::list`, `get` | `prefab_list`, `prefab_get` | Query. `prefab_get` returns the records (instance records with their asset and patch), the flat resolved nodes, nested roots, patch, warnings, stale entries and, with an `entity` argument naming a live instance, **the instance's overrides** (the `Overrides::diff` result per node, nested nodes included, element ops shown as ops; this is what the editor shows as override dots). |
| `instantiate_prefab` | `prefab_instantiate` | Command. Undo despawns the instance. Optional parent (a node of a subtree being edited makes a nested prefab), root components and `scene`. |
| `render_root`, `render_variant`, `write_atomic`, `register_asset`, `PrefabStore::load_text` | `prefab_create` | Command. Optional `base` makes a variant. Writes the file first. Undo: `PrefabEdit` (`remove_asset`). Refused in Play. |
| `write_prefab`, `write_prefab_variant`, `native::write_atomic`, `note_own_write` | `prefab_save` | Command, **no undo**. Arg `drop_stale`. Nested instances below the root are written as nested records. Refused in Play. |
| `Overrides::remove` / `remove_element` on an instance's patch (live: restore the base value) | `prefab_revert_override` | Command. Entity, component, optional field path, optional `key` (revert one list or map element). Undo: the previous value. Refused on a nested root (`NestedInInstance`). |
| `unpack` | `prefab_unpack` | Command. `recursive` (default false). Reports `dropped_stale`. Undo restores the components. |
| `PrefabStore::flatten` | `prefab_flatten` | Command. A prefab **variant** becomes a root prefab; `deep` also unlinks nested prefabs. Writes the file first. Undo: `PrefabEdit`. Refused in Play. |
| `refresh` | `prefab_refresh` | Command. Undo: the text snapshot, as for a despawn. Without an `entity` argument it refreshes every outdated instance. |
| `PrefabStore::remove`, then `edit::remove_asset` | `prefab_remove` | Command. Refused while variants or containers (nesting) or loaded scenes' instances use it. Undo: `PrefabEdit` plus the asset edit. Refused in Play. |
| `PrefabStore::dependents`, plus `edit::find_references` | `prefab_find_references` | Query. Variants, containers that nest it, and scenes that place it. |

`asset_remove` on an asset of kind `Data` or `Prefab` goes through the same guard (AK1 extends SP4's guard to the two kinds), so both routes refuse a removal that would leave a variant or container dangling.

That is **12 data tools and 11 prefab tools, 23 in all**, against the roughly 90-tool budget of `editor-mcp-mvp.md`, which other specs also draw on; SP8 totals all of them when it merges its tool table (#206). `data_validate` and `prefab_validate` are not separate tools: validation is part of `data_get`, `prefab_get` and the status resources. Element ops on a **live** list (an instance's component) have no tool of their own: an agent edits the live component with the generic `ecs_*` field tools, exactly as the inspector does, and `prefab_save` or `scene_save` turns the difference into ops.

**Editor panels** (SP8 builds them from the same functions): a Data Asset panel (list by type, an inspector for the resolved value with an "overridden" marker and a revert button per field, base picker, flatten, save, conflict banner), and a Prefab panel plus hierarchy badges (instance icon, override dot per node from `prefab_get` with `entity`, "outdated" marker, Refresh, a nested badge on nested roots with "select outermost" and "open container"). **List and map fields** of a variant or an instance show, per keyed element, a marker (added, changed, moved), removed elements as ghost rows with a restore button (`prefab_revert_override` with `key`, `data_revert_field` with `key`), drag to reorder (a `move` op), and a key-uniqueness check on add; a list with no key shows "replaced as a whole" and no per-element markers. Fields typed `Guid` with `custom(widget = "asset", kind = "data" | "prefab")` get the generic asset picker (ED10), filtered by `kind` and optionally by `data_type`.

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
| `exitium_data/overlay/apply_ops/20_ops_200_list` | at most 20 µs |
| `exitium_data/overlay/read_ops/20_ops` | at most 30 µs |
| `exitium_data/overlay/diff/40_fields` | at most 3 µs |
| `exitium_data/overlay/diff_ops/200_list_10_changes` | at most 40 µs |
| `exitium_data/overlay/merge3/40_fields` | at most 6 µs |
| `exitium_data/overlay/merge3_ops/200_list_10_user_10_base` | at most 60 µs |
| `exitium_data/play/end_play/100_edited` | at most 1 ms |
| `exitium_scene/prefab/resolve_root/20_nodes_60_components` | at most 150 µs |
| `exitium_scene/prefab/resolve_variant/20_nodes_5_overrides` | at most 40 µs |
| `exitium_scene/prefab/resolve_nested/3_levels_100_nodes` (splice, id prefix, placeholder rebase) | at most 250 µs |
| `exitium_scene/prefab/reresolve/1000_variants` (one base changes) | at most 40 ms |
| `exitium_scene/prefab/reresolve/1000_containers` (one nested prefab changes, 20-node containers) | at most 120 ms |
| `exitium_scene/prefab/spawn/5_nodes_15_components` | at most 20 µs (see the basis below) |
| `exitium_scene/prefab/spawn/1000_instances` | at most 25 ms |
| `exitium_scene/prefab/spawn/nested_3_levels_30_nodes` | at most 120 µs |
| `exitium_scene/instantiate/1000_instances` (scene with 1,000 instance records of the 5-node prefab) | at most 60 ms |
| `exitium_scene/write/1000_instances` (normalise and diff against the resolved prefab) | at most 30 ms |
| `exitium_scene/prefab/refresh/1000_instances` | at most 100 ms |
| `exitium_scene/prefab/refresh/1000_nested_instances` (30-node outer prefab) | at most 500 ms |

**Basis of the spawn budget.** 20 µs is measured on a warm world, in a loop of 1,000 spawns, **including** the hierarchy pass, with the hierarchy built by the #49 helpers directly. If SV2a finds that the pass needs `World::run_system_once` per call, the PR reports the measured setup cost and amends this number in the same PR (a loosening that is allowed once, because the basis was unknown when this spec was written); the 1,000-instance figure then becomes the contract. The nested figure scales at about 4 µs per node.

**Memory.** A store entry costs at most 200 bytes beyond the values it holds. A prototype's value is stored once, in `Source::Full`; a variant stores its overrides plus one resolved value. A resolved prefab is shared through `Arc`, so 1,000 instances share one resolved copy; `InstanceOrigin` costs one `Arc` and an empty `Vec` per tracked instance (32 bytes).

**Frame cost.** A reload frame (swap, dependents, events) stays within SP4's 20 ms cap together with the render upload cap. The time from a file write to the new value visible to systems is at most 500 ms on native with the default hot-reload settings (SP4's 250 ms poll plus 50 ms settle, plus one frame), recorded in the PR of FD1.

**Complexity.** `dependents_topo` is linear in the number of dependents and edges. A set of overrides is a sorted vector, so lookup is `O(log n)` and `apply` is `O(n)` (element ops build a key index once per container, `O(n + m)`, plus `O(n)` per insert or remove from the vector). `resolve` clones the base value once per variant.

## Test plan

Per crate, with golden files for the text forms (the goldens are the reference; the examples in this spec are illustrative).

- **Overlay: paths and leaves.** Path syntax and validation (unknown path with suggestions, path through a leaf). **What is a leaf:** a type with `ReflectTextForm` (`Guid`, `Entity`) is a leaf even though `Entity` reflects as a struct; a non-transparent tuple struct and a built-in tuple are leaves; a transparent wrapper is the inner leaf; a named-field struct recurses. **`set` normalisation:** a struct-valued `set` expands to leaves and replaces entries below it; a leaf `set` leaves siblings untouched; `set` at `stats` then `stats.hp` keeps the other fields; a non-struct root is one entry at the empty path; **write then read gives equal overrides** in each of those cases, for an `Entity` field and a tuple-struct field too. `apply` changes nothing on a path error. `diff` then `apply` reproduces the derived value (property test over generated structs, including enums, options, tuple structs, lists and maps). `leaf_eq`: `0.0` against `-0.0` differs, a NaN equals itself, `reflect_eq` returning `None` is "different". `merge3`: a user-changed leaf is kept, an unchanged leaf takes the new base value, a field missing from the new base is skipped and reported. A list leaf with no key is replaced whole (no tail left over). Stale entries from `read` keep their text verbatim.
- **Overlay: element ops.** Identity: a list of structs with one `list_key` field, a list of values (an ordered set), a map; a struct with no key field, with two, a nested list, a float list and a fixed array are unkeyed (`OpsUnavailable`). Each op on each container; `insert` anchors `first`, `last`, `after`, `before`; `move`; `set` with a nested `ops` (depth 4 passes, 5 is `OpsTooDeep`). **Every row of the ops failure table** has a test with its class: the conflicts (`KeyInSet`, `SelfAnchor`, `TooManyOverrides`) and the shape error fail the file; each stale row is skipped, kept, **retried and applied once the base changes**, and written back. `ElementOp::Opaque` keeps its position and is re-bound. A whole value and `ops` for one path in one group is a parse error; `push_op` after a whole value is `WholeValuePresent`. The compaction table, row by row. **Write then read gives equal overrides with ops** (all op kinds, nested ops, an opaque op), and `read` then `write` is byte-identical (no compaction on read). `diff` emits ops only when fewer than the derived length, never for a list with duplicate keys or no key; a property test over random lists (insert, remove, move, edit) checks `apply(diff(a, b), a) == b` including order, and that diff output is deterministic. `merge3` on lists and maps: user insert, remove, move and edit against base insert, remove, move and edit, each combination of the rules above, `AnchorMoved`, `ElementGone`, `KeyAdded`.
- **BaseGraph.** Cycle (self, two, three), depth 8 and 9, **`set_base` on a root whose subtree is deep** (chain 8 plus a new base is `BaseTooDeep`), `dependents_topo` order on a **wide tree** (one base, many variants) and a **deep tree** (a chain), ties by `Guid`, removal. (The data graph is a forest, so there is no diamond case; the prefab graph with nesting has one: see "Nested prefabs" below.)
- **DataFile.** Prototype and variant round trips against checked-in goldens, with and without ops. A tag that differs from the base chain is `BaseTypeMismatch`. A type without `ReflectDataAsset` is `NotADataAsset`, a non-struct is `RootNotStruct`. `ops` in a prototype is `OpsInPrototype`. A stale override (renamed field, changed type) loads with a warning and is written back by `write_data`; with `drop_stale` it is dropped, and the in-memory list is cleared. **A stale entry and a live override for one path: the live one wins, `write_data` emits one member** (and `set_field`, `set_element`, `revert_field` remove the covered stale entries). CRLF and comments parse. Output is identical across two writes.
- **DataStore.** First load, a variant that loads before its base (`Waiting`, then `Ready` when the base arrives), a chain of three, a changed base re-resolves dependents in order, **a reload that fails keeps the last good value** (and the same for a dependent after a hard error), a first load that fails is `Failed` with no value, a cycle created by a reload is rejected and the old graph stays. `note_failed` turns a `Waiting` entry into `Failed` with `BaseFailed` or `BaseNotData`, and a later good base resolves it. **Step 0:** a payload for a dirty entry is parked and changes nothing else; with `force_next` it applies; a later good payload clears `pending_disk`. `load_text` is the only entry point for first load and reload (a test replays a first load as a reload and compares). Determinism: two processes (two hasher instances) list entries in the same order. A prototype's value is stored once (`get` on a prototype reads `Source::Full`).
- **Edit API, undo and conflicts.** Every function's undo and redo. **`apply_edit` after a disk reload returns `StaleEdit` and the reloaded value is intact.** **Edit, save, undo: the entry is dirty and a later disk reload parks instead of replacing; redo makes it clean again.** `remove` refused for a base in use. `flatten` keeps the resolved value and drops stale entries. A dirty entry parks a reload, `conflicts` lists it, `data_save` is refused with `DiskChanged` and works with `overwrite`, `take_disk` with a parked text applies it, `take_disk` without one requests a reload and applies the next payload to a dirty entry (`force_next`), a bad payload on a clean entry is kept in `pending_disk` and blocks a plain save. `data_create` writes the file, then registers, then creates a clean entry (no in-memory-only entry), and its undo removes both.
- **Play.** `begin_play`, edit three entries, apply a disk reload to a fourth, `end_play`: the three are restored (with their dirty state), the reloaded one keeps the disk value, dependents are re-resolved, a parked reload is applied. The editor-lane test: every file-writing and file-moving command (`data_save`, `data_create`, `data_remove`, `prefab_save`, `prefab_create`, `prefab_flatten`, `prefab_remove`) is refused in Play.
- **Prefab file.** Root and variant goldens, with nested records and ops. Both forms at once is `PrefabShape`. A variant chain of three. **Every row of the patch table** has a test with its class: the parse and shape errors, each conflict (including `DuplicateNodeId` for two added ids and for an id removed in the same patch, `AddedCycle`, `OverrideOnAdded`, `DuplicateComponent`) fails the file; each stale case loads with a warning and is written back, and is dropped with `drop_stale`; **`DuplicateNodeId` caused by a base that gained the id is stale** (`IdCollision`: the added subtree is skipped, kept as text, applied again if the base drops the id). An added node whose parent is added later in the list resolves. `removed` of a missing node is stale; a node and its descendant in `removed` is idempotent. The order test: `remove` then `add` of one type replaces the component; `remove` then `set` is stale. An unknown component type in `set` is a warning with suggestions, in a root prefab's `components` it is an error, in an added node it makes the whole node stale. An id containing `.` is `InvalidNodeId`. **A variant file's `@label` uses resolve against the base chain** (the file-local `UnknownLabel` check does not apply) and a dead one is stale. Node ids survive a save after an entity rename.
- **Nested prefabs.** A prefab with a nested instance and a record patch resolves to the flat list with dotted ids; a variant overrides, removes and adds under a nested node (`@shield.rim`); `removed: [@shield]` removes the whole nested instance; a path through a nested instance that the base lost is stale. Entity values inside the nested prefab rebase to the outer indexes. **Graph:** `NestedPrefabCycle` for a self nest, a two-prefab cycle and a prefab nesting its own variant; `TooDeep` at combined depth 9; a **diamond** (A nests B and C, both nest D) resolves and re-resolves D's dependents in order; `Waiting` on several missing assets; `note_failed` gives `NestedFailed` and `NestedNotPrefab`, and a later good load resolves the container; a nested reload that fails keeps the container's last good `Arc`. `reresolve` order on a wide and a deep nesting tree. `RootIsInstance`, `InstanceHasComponents`, `AddedInInstancePatch`, `TooManyNodes` on the flat count. `prefab_remove` is refused while a container nests the prefab.
- **Instances in scenes.** A scene with two instances of the same prefab, an override on one, an ordinary child record under an inner node, and an instance of a prefab that nests another: write, read into a fresh world, write gives identical text. `@inst.node` and `@inst.nested.node` labels resolve; an unknown inner label is `UnknownLabel` with suggestions; a hand-written `id: @goblin_1.weapon` is `InvalidLabel`; a missing prefab fails before anything is spawned (entity count unchanged); `prefabs: None` is `PrefabStoreMissing`; `validate_scene` with a store validates an instance. A patch conflict aborts the scene load, a stale entry only warns, and **a stale entry whose value holds a dead `@label` loads with the warning instead of failing the scene**. **`SceneInstance.entities` lists instance nodes in flat prefab pre-order, root first, and `SavedScene.entities` reports the same order** (the `EntityRemap` test). A prefab edited between save and load: a removed node's child re-attaches to the root with a warning, and a stale instance patch entry loads with a warning and is written back by the next save. **Write rules:** a node reparented inside its instance is written as a plain record with the new parent and a warning; a node moved out likewise, with its descendants; **an instance with `PrefabInstance` and no `InstanceOrigin` is written as plain entities with `InstanceOriginMissing`**; `track_origin: false` with `scene: Some` is refused; an instance patch with `added` is `AddedInInstancePatch`; **a stale entry whose label is not declared in the output is dropped with a warning, and kept when the label is declared**; a stale entry covered by a live one is dropped. **Entity fields:** a component whose `Entity` field points at another node of the same instance writes no override, one retargeted to another node writes that node's label, and one that points outside writes a scene label. **`InstanceOrigin` gives no `UnsavedComponent` warning, also under `strict`.** An instance patch changes a keyed list element: the save writes an op, not the whole list.
- **Write prefab.** `write_prefab` of a subtree with a live instance writes a nested record (patch against the origin, stale re-scoped); an instance without origin is written as plain nodes with a warning; `RootIsInstance` for an instance root; `write_prefab_variant` uses the **target's** stale entries, not the instance's, and reports the dropped ones; node ids never contain `.`; `flatten` folds a variant's dotted overrides into the nested record's patch, `deep` unlinks nested records; the authoring commands write the file first and `register_asset` after (no in-memory-only prefab), and their undo writes the previous text back or reports `StaleEdit`.
- **Spawn.** `instantiate_prefab` produces the same entities as an instance record with an empty patch. Entity references between nodes (nested included) are rewritten to the spawned entities; a map keyed by `Entity` works. **The spawner never panics on an allocated entity with generation `u32::MAX`: it despawns it and allocates again** (a test forces that generation). `spawn_prefab` through `Commands` spawns when commands apply, and `on_spawn` sees the entities. `root_components` replace the prefab's own. `track_origin: false` leaves no `InstanceOrigin`. Nested roots carry `PrefabNested`.
- **Origin and undo.** Despawn an instance, undo it through the `write_scene` and `instantiate` round trip: the new instance has a fresh `InstanceOrigin`, keeps its stale entries, and `prefab_refresh` works on it.
- **Refresh.** A user-changed field survives a refresh, an unchanged field takes the new value, **an `Entity` field pointing inside the instance refreshes** (it is not seen as user-changed), a new node appears, a removed node goes away, a node with user children stays and is reported, a stale entry that fits the new prefab is applied. **Through nesting:** a change to a nested prefab, and to a base of a nested prefab, reaches a live outer instance; a nested root refuses (`NestedInInstance`); a whole new nested instance appears. **Lists:** every `merge3` rule through a refresh. **Undo** restores the instance (new entity ids, the old version) after the prefab changed again. `RefreshUnavailable` for an untracked instance.
- **Unpack.** `prefab_unpack` drops stale entries and reports them; with `recursive: false` nested roots become independent instances whose next save diffs against the nested prefab; `recursive: true` leaves plain entities; undo restores.
- **Facade (FD1, FD2).** A headless app with `MemorySource`: a data asset loads and `get_as` returns it; a variant follows a base rewritten on disk in `SimulationMode::Frozen`; a corrupt rewrite keeps the old value and sends `DataFailed { reload: true }`; a variant whose base is `NotFound` ends `Failed` with `BaseFailed`; a save by the editor path is not reported as a change; a reload payload coalesced under load is applied once; `LoadScene` waits for a prefab (and the prefabs it nests) and then instantiates, and fails cleanly (`SceneLoadFailed` with `PrefabFailed` in `errors`) when the prefab fails. `PrefabStatus.outdated_instances` is updated on `PrefabChanged`, not per frame.
- **No unsafe**; the wasm32 build of both crates passes.

## Dependencies

No new crates.io dependency. The reasons, so a reviewer can check them:

| Need | Choice | Why not a crate |
|---|---|---|
| Maps, vectors, sets | `std::collections` (`HashMap` with `BuildHasherDefault<DefaultHasher>`, `BTreeMap`, `Vec`) | The sizes are small; the fixed hasher and sorted listings make output deterministic without a hasher crate. |
| Longest increasing subsequence (list `diff`) | A small hand-written function (patience sort, `O(n log n)`) in `overlay` | About 30 lines; no crate is worth a dependency. |
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
- **Not in intake 2: `exitium_scene -> exitium_data` (E1).** #157's criterion reads "`exitium_scene -> exitium_data` (E1, or its replacement per SP10 (#146))", and its "New crate edges" line still says "including E1"; this spec's answer is "E1 is kept, and the edge is added later by EP1", so both lines are changed (see "Downstream adjustments", #204; the edit must land before #157 starts, and #157 gets a `Blocked by #204` line).
- Facade: `exitium -> exitium_data` (normal; `exitium -> exitium_scene` is already SP4's).
- `crates/exitium_reflect_derive/`: add `exitium_data` to `ENGINE_CRATES` (the crate derives `Reflect` for `DataStatus` and its events, and the `crate_paths.rs` drift test fails until it is classified; `exitium_scene` is already there from SP4).
- `xtask`: nothing new; `exitium_data` is wasm-capable and goes in the default wasm build.

**Intake 3.** `exitium_editor -> exitium_data` (C3, already listed in the roadmap).

**Edge PR EP1** (not an intake): `exitium_scene -> exitium_data`, normal dependency. One crate (`exitium_scene/Cargo.toml`), `Hotspots: Cargo.toml, Cargo.lock`, `Blocked by` DA9 and SC5.

**Not added, on purpose:** any edge from `exitium_data` or `exitium_scene` to `exitium_app`, `exitium_assets`, `exitium_command` or another leaf crate (C1), and any crates.io dependency.

## Open questions

Each has a recommended answer; the owner decides. Questions 1 to 3 of earlier drafts are decided (see "Decisions by the owner"). The numbers below are kept.

4. **Rebasing a variant onto another base:** support it, or `flatten` plus re-create? Recommended: not supported. `data_flatten` and `prefab_flatten` exist, so an agent has the path.
5. **Reparenting a base node in a variant:** support it? Recommended: no. A variant adds, removes and overrides, and cannot move nodes; `prefab_flatten` then edit is the path.
6. **Live prefab propagation:** explicit `prefab_refresh` (this spec), or automatic update of every live instance on reload? Recommended: explicit. Automatic update can overwrite what an agent or human is editing; a saved scene already gets the new prefab on its next load.
7. **File-writing commands during Play:** refuse `data_save`, `data_create`, `data_remove` and the prefab write commands? Recommended: refuse (D10). SP8 owns the call sites and should state the same.
8. **Typed handles (`DataRef<T>`, `AssetRef<T>`):** wait with SP4's D1 (plain `Guid`), or add a typed handle now? Recommended: wait. It is a facade-level addition later and changes no file format.
9. **Runtime-defined data types** (reflect open question 9; a type created in the editor with no Rust code): answered here as not in the MVP. Recommended: keep it out; a game's data types are Rust structs with `#[derive(Reflect)]`, which is what an agent writes.
10. **Stale entries on save (D14).** A variant or instance override (a leaf, or an element op whose text no longer binds) that no longer fits its base is skipped with a warning, **kept, and written back on save**, so nothing is lost; the caller passes `drop_stale` to clean it up. The alternatives are (a) fail the save until the caller passes `drop_stale`, or (b) drop silently. Recommended: the spec as written. It is the only choice that is both safe and does not block saving after a rename, and it applies to data variants, prefab variants, nested records and scene instances alike. Needed before DA1 (the stale entry shape) and SV1a (the file format).
11. **Node-id reuse.** Variant patches cannot add an id that the base chain used (`IdCollision`, stale), but a **root prefab** save can give a deleted node's id to a new node, and a variant's or container's override for the old node then lands on the new one. Recommended: accept this for the MVP and document it; follow-up #209 makes `prefab_save` return a warning that lists the loaded variants and containers naming a removed node (it sees loaded ones only, not unloaded scenes).
12. **Unkeyed lists.** A list whose item has no `list_key` field is replaced whole, and an element edit to it is a whole-list override. Alternative: a positional fallback (ops by index, valid only while the base length is unchanged, else stale). Recommended: no fallback. A positional op is wrong exactly when the base changes, which is the case ops exist for; add a `list_key` field to the item type instead (one attribute).

## Implementation issues

Each issue is PR-sized (about 600 changed lines or fewer; one that passes about 700 splits), edits one crate (the facade issues edit none and hold `crates/exitium/`), has a labelled Footprint, acceptance criteria and `Blocked by` lines, and goes into the MVP milestone. The architect files them with `file-issue` after this spec merges, labels `area:data`, `area:scene`, `area:assets` and `area:facade`. `<intake-2>` is the intake issue's number (#157), filled in at filing. Every issue is `Blocked by <intake-2>` (C6). One crate has at most one issue in flight, so each lane runs in the order shown. Each issue updates its crate's `CLAUDE.md` (DA1 creates the data crate's; SC1 creates the scene crate's), adds the benchmarks it names, and states the high-risk areas it touches in the PR Risk section (the serialization format, for the formats; the facade API, for FD2). **26 issues in all: 9 data, 1 assets, 1 edge, 13 prefab, 2 facade.**

### Data lane (`exitium_data`, Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none)

DA1 starts right after intake 2, beside the `exitium_app` spine and the scene lane (R3 passes: no edge to either).

**DA1. `feat(data): overlay field paths, leaf Overrides, stale entries and text I/O`** (about 550 lines). Blocked by `<intake-2>`.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `overlay/path.rs`, `overlay/overrides.rs` (`FieldPath`, `FieldOverride`, `OverrideValue::Set`, `Overrides::{set, remove, get, iter, read, write, apply}`, `ReadOverrides`, `StaleEntry`, `StaleSection`, `StaleReason`), the leaf rule of D5 (text-form types and tuple structs are leaves, transparent resolved first), `OverlayError`; creates `CLAUDE.md`.
- Acceptance criteria:
  - [ ] The "Overlay: paths and leaves" tests, including a struct-valued `set` expanding to leaves, the empty path for a non-struct root, an `Entity` field and a tuple-struct field treated as leaves, a transparent wrapper as one leaf; `apply` changes nothing on a path error.
  - [ ] **Written then read gives equal overrides** in every normalisation case (including `Entity` and tuple-struct fields); stale entries keep their text verbatim; output is identical across two writes.
  - [ ] `overlay/apply/16_overrides` meets the budget; no `unsafe`; wasm32 build passes.

**DA2. `feat(data): element ops for keyed lists and maps (identity, apply, text I/O)`** (about 600 lines). Blocked by `<intake-2>`, DA1.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `overlay/ops.rs`: `ElementOp`, `Anchor`, `ElementKey`, `KeyId`, `OpaqueOp`, `list_identity` (the `list_key` custom attribute rule), `OverrideValue::Ops`, `Overrides::push_op` with the compaction table, `remove_element`, the `ops([...])` reader and writer, `ApplyReport`, the ops failure table's classes.
- Acceptance criteria:
  - [ ] The "Overlay: element ops" tests: identity (keyed struct, value list, map, the unkeyed cases), each op on each container, the anchors, nested `set` ops to depth 4, **each row of the ops failure table with its class**, stale ops retried and applied once the base changes, `Opaque` ops kept and re-bound, the compaction table.
  - [ ] **Write then read gives equal overrides with ops** and `read` then `write` is byte-identical; `overlay/apply_ops/20_ops_200_list` and `overlay/read_ops/20_ops` meet their budgets.

**DA3. `feat(data): overlay diff, merge3, leaf equality and BaseGraph`** (about 450 lines). Blocked by `<intake-2>`, DA2.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `Overrides::{diff, merge3}` for leaves (a list or map is a whole-value leaf here), `leaf_eq`, `MergeReport`, `graph.rs` (`BaseGraph` with the subtree-height depth check, `height`, deterministic hasher).
- Acceptance criteria:
  - [ ] The property test `diff` then `apply`, the `leaf_eq` cases (`-0.0`, NaN, `None`) and the leaf `merge3` cases of the Test plan; the `BaseGraph` cycle, depth (including a deep subtree), wide-tree and deep-tree tests.
  - [ ] `overlay/diff/40_fields` and `overlay/merge3/40_fields` meet their budgets.

**DA4. `feat(data): element ops in diff and merge3 (LIS diff, keyed three-way merge)`** (about 550 lines). Blocked by `<intake-2>`, DA3.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds the ops branch of `diff` (removes, LIS placement, sets, sorted map ops, the "fewer than derived length" rule, duplicate-key fallback) and of `merge3` (the keyed three-way rules and `MergeReport` cases).
- Acceptance criteria:
  - [ ] The random-list property test `apply(diff(a, b), a) == b` including order, and deterministic output; the `merge3` list and map combinations of the Test plan.
  - [ ] `overlay/diff_ops/200_list_10_changes` and `overlay/merge3_ops/200_list_10_user_10_base` meet their budgets.

**DA5. `feat(data): ReflectDataAsset and the DataFile envelope`** (about 500 lines; high-risk review: file format). Blocked by `<intake-2>`, DA4.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `asset.rs` (`ReflectDataAsset`, its registration helper and `data_types`), `file.rs` (`DataFile` parse and write, the sparse-group framing rule, limits), `DataError`, `DataWarning`, `WriteOptions`, `OpsInPrototype`.
- Acceptance criteria:
  - [ ] The "DataFile" test group against checked-in golden files (produced by the writer and reviewed; this spec's examples are illustrative), with and without ops; `BaseTypeMismatch`, `NotADataAsset`, `RootNotStruct`, and the CRLF and comment cases.
  - [ ] A stale override loads with a warning and `write_data`-style output keeps it; `drop_stale` drops it; a stale entry covered by a live one is dropped on write.
  - [ ] Prototype 2 KB and variant 200 B parse within the DA6 budgets' parse share (measured here, asserted in DA6).

**DA6. `feat(data): DataStore (load_text, resolve, dependents, reload rules)`** (about 600 lines). Blocked by `<intake-2>`, DA5, `exitium_ecs` `Resource` and resources in the world (#44).
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `store.rs`: `DataStore` (`impl Resource`), the reads, `want` and `take_wanted`, `load_text` with step 0 and the five steps, `note_failed` and `BaseFailure`, `refs_of`, `referrers`, `rev` and `saved_rev`, deterministic listings.
- Acceptance criteria:
  - [ ] The "DataStore" test group: waiting base, chain of three, dependents re-resolved in order, **a failed reload keeps the last good value** for the entry and for a dependent, a cycle from a reload is rejected, **a dirty entry parks a payload and `force_next` applies it**, first load equals reload (replay test), `note_failed` for a `NotFound` base and a non-data base, two hasher instances list in the same order, a prototype stored once.
  - [ ] `store/get/10000`, `store/load_prototype/2kb`, `store/load_variant/200b`, `store/reresolve/1000_dependents` meet the budgets.

**DA7. `feat(data): authoring API, undo records and conflicts`** (about 600 lines). Blocked by `<intake-2>`, DA6.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `edit.rs`: `write_data`, `render_new`, `create`, `set_field`, `revert_field`, `flatten`, `replace_from_text`, `apply_edit` with state ids (`rev`, `saved_rev`) and `StaleEdit`, `mark_saved`, `remove`, `DataEdit`, `pending_disk`, `force_next`, the conflict table (`DiskChanged`), the stale-versus-live rule.
- Acceptance criteria:
  - [ ] The "Edit API, undo and conflicts" tests: every undo and redo, **`StaleEdit` after a reload**, **edit, save, undo is dirty and redo is clean**, `remove` refused for a base in use, `flatten` keeps the resolved value, the conflict table row by row, `data_create`'s write-register-create order.
  - [ ] `store/write/2kb` and `store/set_field/variant` meet the budgets.

**DA8. `feat(data): element authoring (set_element, revert by key) for data assets`** (about 350 lines). Blocked by `<intake-2>`, DA7.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `DataStore::set_element` (prototype in place, variant `push_op` with compaction), `revert_field` with `key`, `OpsUnavailable` for an unkeyed list, undo records, and the `list_key` report used by `data_types`.
- Acceptance criteria:
  - [ ] Each op on a prototype and on a variant, undo and redo, revert of one key's ops leaving the others, an unkeyed list refused, dependents re-resolved after an element edit of a base.

**DA9. `feat(data): play journal, DataStatus, events, registration and benchmarks`** (about 450 lines). Blocked by `<intake-2>`, DA8.
- Footprint: Edits `crates/exitium_data/`; Governs none; Hotspots none; New crate edges none.
- Adds `play.rs` (`begin_play`, `end_play`, `PlayRestore`, `in_play`), the reflected `DataStatus` (`custom(readonly = true)`) and `DataSettings` (`custom(history = false)`), the events `DataChanged` and `DataFailed`, `register_types`, the remaining benchmarks.
- Acceptance criteria:
  - [ ] The "Play" test of the Test plan; the reflected types register and round-trip through the text format; a test asserts `DataSettings`'s `TypeInfo` has `custom("history") == false` and `DataStatus`'s has `custom("readonly") == true`.
  - [ ] `play/end_play/100_edited` meets the budget; the full set of data benchmarks runs in CI.

### Assets row (`exitium_assets`)

**AK1. `feat(assets): AssetKind::Data and AssetKind::Prefab, references and remove guard`** (about 250 lines). Blocked by `<intake-2>`, AP4 (SP4's last assets lane issue; R1).
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds the two `#[non_exhaustive]` variants, the extension rows `data` and `prefab` (both `Payload::Text`) in `path.rs`, the registry and listing tests; **extends `find_references` (AP3: it read only `Scene` and `Text`) to read `Data` and `Prefab` files, and the `asset_remove` guard to refuse a `Data` or `Prefab` asset that others name** (a base, a nested instance, a scene instance).
- Acceptance criteria:
  - [ ] Listing by kind returns data and prefab assets; an unknown extension is unchanged; the kinds round-trip through the manifest **text** (the text and the manifest of existing entries are unaffected; adding variants changes the schema hash of `AssetKind`, `AssetInfo` and `Manifest`, and nothing persists that hash, which a test pins).
  - [ ] `take_loaded(AssetKind::Data)` delivers a `Data` payload and a reload payload (`generation >= 2`) in a `MemorySource` test.
  - [ ] `find_references` finds a guid named by a `.data` and by a `.prefab` file; `asset_remove` of a data asset used as a base is refused with the list; both stay bounded by `max_text_bytes`.

### Scene lane (`exitium_scene`)

SC1 to SC5 are SP4's issues (assets-scene-export.md "Stage 2: scene lane") and run beside the data lane. This spec adds the edge PR and thirteen prefab issues after SC5.

### Edge PR (not an intake, C5)

**EP1. `chore(scene): add the exitium_data dependency (E1)`** (about 20 lines). Blocked by `<intake-2>`, DA9, SC5.
- Footprint: Edits `crates/exitium_scene/Cargo.toml`; Governs none; Hotspots `Cargo.toml`, `Cargo.lock`; New crate edges `exitium_scene -> exitium_data`.
- Acceptance criteria:
  - [ ] `cargo build --workspace` and the wasm build pass; `cargo tree -p exitium_scene` shows the single new edge; no other manifest changes; `exitium_data` has no new edge.

### Scene lane, prefabs (`exitium_scene`, Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none)

**SV1a. `feat(scene): PrefabFile (root and variant) parse and write, patch classes`** (about 600 lines; high-risk review: file format). Blocked by `<intake-2>`, EP1.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `prefab/file.rs` and `prefab/patch.rs`: `PrefabFile`, `PrefabPatch`, parse and write of root and variant, the id rule (`.`), the patch table's parse and conflict classes (`PrefabError`, `PrefabWarning`), the variant label rule; patch `set` groups go through `Overrides::read`, so element ops need no extra code here.
- Acceptance criteria:
  - [ ] The "Prefab file" group against checked-in goldens; both forms at once, two roots, `InvalidNodeId`, and **each parse, shape and conflict row of the patch table** fails with its named error; unknown envelope fields warn.
  - [ ] Stale-looking entries parse (the classification happens at resolve); output is identical across two writes.

**SV1b. `feat(scene): resolve chain, ResolvedPrefab, stale entries and PrefabStore`** (about 600 lines). Blocked by `<intake-2>`, SV1a.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `prefab/resolve.rs` (`ResolvedPrefab`, the fixed patch order, dependency-ordered `added`, the entity placeholder, **every stale row** with the `StaleEntry` shape, retry) and `prefab/store.rs` (`PrefabStore::{load_text, get, list, want, take_wanted, note_failed, remove, apply_edit}`, `PrefabEntry.rev`, the reload rules). Single-level (no nested records yet).
- Acceptance criteria:
  - [ ] The patch order test (`remove` then `add` replaces; `remove` then `set` is stale), **each stale row** loads with its warning and is kept (including `IdCollision` for a base that gained the id), an unknown component type in `set` warns with suggestions, a variant chain of three, variant labels resolving against the chain.
  - [ ] A failed prefab reload keeps the last good `Arc`; a changed base re-resolves its variants in order; `Waiting` fails loudly through `note_failed`; first load equals reload.
  - [ ] `prefab/resolve_root/20_nodes_60_components`, `prefab/resolve_variant/20_nodes_5_overrides` and `prefab/reresolve/1000_variants` meet the budgets.

**SV1c. `feat(scene): nested instance records in prefab files (parse, write, id rules)`** (about 400 lines; high-risk review: file format). Blocked by `<intake-2>`, SV1b.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds the `prefab` field to prefab records (root `entities` and variant `added`), the instance patch shape (no `added`), `RootIsInstance`, `InstanceHasComponents`, `AddedInInstancePatch`, record-scoped `StaleEntry.scope`, goldens with nested records.
- Acceptance criteria:
  - [ ] Root and variant goldens with nested records; each shape error of the table for instance records; a record patch with a conflict fails the file; output identical across two writes.

**SV1d. `feat(scene): nesting graph and flat resolve (cycle, depth, splice, rebase, dependents)`** (about 600 lines). Blocked by `<intake-2>`, SV1c.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `prefab/graph.rs` (`NestGraph`, the combined cycle and depth check of D21, `dependents` over both edge kinds), the splice step of resolve (dotted ids, root parent, placeholder rebase through the per-type cache), `PrefabState::Waiting(Vec<Guid>)`, `NestedPrefabCycle`, `TooDeep`, `TooManyNodes`, `NestedFailed`, `NestedNotPrefab`, `ResolvedPrefab.nested`.
- Acceptance criteria:
  - [ ] The "Nested prefabs" tests of the Test plan: flat dotted ids, a variant overriding, removing and adding under a nested node, a path through a lost nested instance is stale, cycles (self, two, own variant), depth 9, a diamond, `Waiting` on several assets, `note_failed` rows, a failed nested reload keeps the container's last good `Arc`.
  - [ ] `prefab/resolve_nested/3_levels_100_nodes` and `prefab/reresolve/1000_containers` meet the budgets.

**SV2a. `feat(scene): runtime spawn (instantiate_prefab, deferred form, components, origin)`** (about 550 lines). Blocked by `<intake-2>`, SV1d, `Commands::add` (ecs-scheduler.md) and `World` hierarchy helpers (#49).
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `instance/spawn.rs`, `instance/components.rs`, `instance/remap.rs`: `instantiate_prefab`, `SpawnPrefabExt`, `SpawnOptions`, `SpawnError` (`UntrackedSceneMember`), `PrefabSpawn`, the reflected `PrefabInstance`, `PrefabNode`, `PrefabNested` and `PrefabNodeId`, the non-reflected `InstanceOrigin`, the placeholder rewrite with its per-type cache, the **skip-and-retry of an allocated entity with generation `u32::MAX`**.
- Acceptance criteria:
  - [ ] The "Spawn" tests of the Test plan: Entity references rewritten (nested included), a map keyed by `Entity`, the reserved-generation retry, `spawn_prefab` through `Commands` with `on_spawn`, `root_components`, `track_origin: false`, `scene` with `track_origin: false` refused, `PrefabNested` on nested roots.
  - [ ] `prefab/spawn/5_nodes_15_components`, `prefab/spawn/1000_instances` and `prefab/spawn/nested_3_levels_30_nodes` meet the budgets, with the basis and any amendment recorded in the PR.

**SV2b. `feat(scene): instance records in scenes (parse, labels, patch apply, entity order)`** (about 600 lines). Blocked by `<intake-2>`, SV2a.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds the `prefab` field to scene parse and instantiate (`@inst.node` and `@inst.nested.node` labels, the `.`-in-id rule `InvalidLabel`, `PrefabNotLoaded`, `MissingPrefabNode`, `InstanceHasComponents`, `AddedInInstancePatch`, labels in patch values checked at bind so a dead one is stale), the patch applied to live entities, `prefabs: Option<&PrefabStore>` and `PrefabStoreMissing` on `InstantiateOptions`, the defined `SceneInstance.entities` order.
- Acceptance criteria:
  - [ ] The instance tests of the Test plan, including the atomic failure (entity count unchanged on `PrefabNotLoaded`), a patch conflict aborting the scene load while a stale entry only warns, **a stale entry with a dead label loading with a warning**, a hand-written dotted id rejected, a nested instance's labels, and **the entity order that `EntityRemap` relies on**.
  - [ ] `instantiate/1000_instances` meets the budget.

**SV2c. `feat(scene): scene API plumbing for prefabs (validate_scene, SaveOptions, SavedScene.entities, warnings)`** (about 350 lines). Blocked by `<intake-2>`, SV2b.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `prefabs` and `drop_stale` to `SaveOptions` and `prefabs` to `validate_scene`, `ParsedScene::prefab_guids`, `SavedScene.entities`, the new `SceneWarning` and `SceneErrorKind` variants, the silent skip of `InstanceOrigin` in `write_scene`.
- Acceptance criteria:
  - [ ] `validate_scene` with a store validates an instance; `prefab_guids` lists every direct prefab; a scene with instances writes **no `UnsavedComponent` warning, also under `strict`**, because `InstanceOrigin` is skipped silently; `SavedScene.entities` is in file order with instance nodes in flat pre-order.

**SV3a. `feat(scene): write_scene with instances (normalise, diff, detach rules)`** (about 600 lines; high-risk review: file format). Blocked by `<intake-2>`, SV2c.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds the instance diff to `write_scene` (rules 1 to 6), the `Entity` normalisation and the placeholder-to-label mapping of override values, `SceneWarning::PrefabNodeDetached`, ordinary child records under instance labels, element ops from `diff`.
- Acceptance criteria:
  - [ ] A scene with instances writes, reads and writes again to identical text; **a node reparented inside its instance and a node moved out are written as plain records (with their descendants) and a warning**; an intra-instance `Entity` field writes no override, a retargeted one writes the node's label, and an outside one writes a scene label; an instance patch changing one list element writes an op.
  - [ ] `write/1000_instances` meets the budget.

**SV3b. `feat(scene): write_scene stale write-back, dangling labels, fallbacks and drop_stale`** (about 450 lines). Blocked by `<intake-2>`, SV3a.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds rule 7 (stale write-back from `InstanceOrigin`, the covered-by-live rule, `drop_stale`, **dangling-label drop** with `StaleEntryDropped`), `InstanceOriginMissing` plain-entity fallback, `StaleInstanceEntry` warnings on load and save.
- Acceptance criteria:
  - [ ] Stale entries are written back and dropped with `drop_stale`; an entry with an undeclared label is dropped with a warning (also in a `SaveScope::Entities` write) and kept when the label is declared; an instance without origin is written as plain entities with a warning; the origin and undo test (despawn, `write_scene` with `SaveScope::Entities`, instantiate).

**SV3c. `feat(scene): write_prefab (node ids, nested records, RootIsInstance)`** (about 550 lines; high-risk review: file format). Blocked by `<intake-2>`, SV3b.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `write_prefab`, node-id assignment and stability (no `.`), nested records from live instance roots (patch against the origin, stale re-scoped), `RootIsInstance`, the `InstanceOriginMissing` fallback.
- Acceptance criteria:
  - [ ] Node ids survive a rename; a subtree with a live instance writes a nested record; an instance without origin writes plain nodes with a warning; an instance root as the save root is `RootIsInstance`; read-write-read is identical.

**SV3d. `feat(scene): write_prefab_variant, flatten, authoring commands and PrefabEdit`** (about 600 lines; high-risk review: file format). Blocked by `<intake-2>`, SV3c.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `write_prefab_variant` (diff against the base chain, **the target's own stale entries**, dangling-label rule, dropped entries reported), `PrefabStore::flatten(guid, deep)` (folding dotted overrides into nested records), the authoring functions `render_root`, `render_variant`, `PrefabEdit` with state ids, write-through semantics.
- Acceptance criteria:
  - [ ] A variant save writes only the diff and the target's stale entries; `flatten` gives a root prefab whose resolved nodes equal the variant's (with nested records folded, and `deep` unlinking them); `remove` refused while a variant or container uses the prefab; every undo, and `StaleEdit` after a reload; creation writes the file first.

**SV4a. `feat(scene): prefab refresh (merge3 through nesting) and its undo record`** (about 450 lines). Blocked by `<intake-2>`, SV3d.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `refresh`, `RefreshReport` and the text-snapshot undo record, `RefreshUnavailable`, `NestedInInstance`.
- Acceptance criteria:
  - [ ] The "Refresh" tests of the Test plan, including the Entity-field case, list and map merges, refresh through a nested prefab and through a base of a nested prefab, a stale entry that fits again, `RefreshUnavailable`, a nested root refused, and undo after the prefab changed again.
  - [ ] `prefab/refresh/1000_instances` and `prefab/refresh/1000_nested_instances` meet the budgets.

**SV4b. `feat(scene): prefab unpack, PrefabStatus, events and registration`** (about 400 lines). Blocked by `<intake-2>`, SV4a.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `unpack` (`recursive`, re-origin of nested roots, `dropped_stale`), the reflected `PrefabStatus` (`custom(readonly = true)`, `outdated_instances` recomputed on `PrefabChanged` and after refresh, spawn and despawn), the events `PrefabChanged` and `PrefabFailed`, `register_types`.
- Acceptance criteria:
  - [ ] The "Unpack" tests; the reflected types register and round-trip; `outdated_instances` is not a per-frame scan; the crate's wasm32 build passes.

### Facade glue (Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none)

These two run alone, in order, after the plugin lanes they glue (R2, R3). Their place among the other specs' facade issues is the orchestrator's; the lines below are only what each glue needs.

**FD1. `feat(exitium): data glue (DataPlugin)`** (about 450 lines). Blocked by `<intake-2>`, AK1, DA9, AP4, FA1 (schedule and `SimulationMode::Frozen` test pattern, AM3 of SP3), F3 (app-platform.md, `DefaultPlugins`) and FA4 (the `exitium::scene` re-export module).
- Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none.
- Adds `DataPlugin`: delivery through `take_loaded(AssetKind::Data)`, events, `note_failed`, requests for `take_wanted` and `refs_of`, `DataSettings.preload`, the `exitium::data` re-exports, the `DefaultPlugins` entry.
- Acceptance criteria:
  - [ ] **Schedule:** delivery runs in `First` after `ASSETS`, never in `Update`; with `SimulationMode::Frozen` a data file rewritten on disk still changes `DataStore::get_as`.
  - [ ] The facade tests of the Test plan: a variant follows a rewritten base, a corrupt rewrite keeps the old value and sends `DataFailed { reload: true }`, a `NotFound` base ends `Failed`, an own write is not reported, a coalesced reload applies once, a data asset referenced by another is requested once.
  - [ ] The write-to-visible time of a reload is recorded in the PR (budget 500 ms); a compile test names every re-exported item.

**FD2. `feat(exitium): prefab glue, scene loading with prefabs and re-exports`** (about 500 lines; high-risk review: facade API). Blocked by `<intake-2>`, SV4b, FD1, FA3, F3 and FA4.
- Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none.
- Adds `PrefabPlugin` (delivery, requests for bases and nested prefabs, events), the `SceneLoadState::WaitingForPrefabs` state in `LoadScene` (passing `prefabs` to `instantiate`; failure uses the existing `SceneLoadFailed { path, errors }`), the `exitium::scene` re-exports of the prefab items, `exitium_scene::InstanceOrigin` in `INTERNAL_COMPONENTS`, and the additions to `DefaultPlugins` and the sample game (a prefab variant placed twice in the sample scene, a prefab that nests another, a data asset with a keyed list driving unit stats).
- Acceptance criteria:
  - [ ] A scene with an instance loads once its prefab (and every prefab it nests) is `Ready`; a prefab that stays `NotFound` ends in `SceneLoadFailed` (with `PrefabFailed` in `errors`) and spawns nothing; a prefab rewritten on disk gives `PrefabChanged`, leaves live instances unchanged, and the next `LoadScene` replace shows the new prefab.
  - [ ] Every name in "Re-exports" is reachable as `exitium::<module>::<Name>` and `BaseGraph`, `NestGraph` and `InstanceOrigin` are not.

### Not in this spec

The editor wrappers and panels for the tools above belong to SP8 and the editor lane (#110), and the parity-test rows grow there. This spec only fixes the functions they wrap.

## Downstream adjustments

These cross-spec edits touch files this PR must not edit. Each is filed as its own `status:needs-approval` issue with one footprint. They are the contract once this spec merges:

- **`mvp-roadmap.md` and intake 2 (#157), issue #204.** Must land **before #157 starts**, and #157 carries a `Blocked by #204` line:
  - Remove `scene -> data` (E1) from intake 2's list, from #157's criterion ("E1, or its replacement per SP10 (#146)") and from its "New crate edges" line ("including E1", which becomes "E1 is kept and added later by EP1"); add `data -> ecs` and `exitium_data` in `ENGINE_CRATES` (see "Wiring").
  - Replace "Scene lane: about 8 ... after the data lane (E1)" with: scene SC1 to SC5 beside the data lane (DA1 to DA9, 9 PRs), then the edge PR EP1, then SV1a to SV4b (13 prefab PRs). The Stage 2 critical path becomes intake 2 (1), data 9 beside scene 5, EP1 (1), prefab 13, facade 18: **42 serial PRs** (was 32 in the roadmap, 33 in the previous draft of this spec).
  - Add the facade issues FD1 and FD2 in place of the single "data glue" slot; add AK1 to the assets lane (R1: after AP4).
  - Mark E1 "decided: stands" in C1 (owner, 2026-11-25); EP1 is the second edge PR of the plan (C5; the first is `exitium_editor -> exitium_ui`). Close roadmap open question 3 (E1) as decided.
- **`assets-scene-export.md`, issue #205.** Its "Hot reload" row "Data assets, prefab variants" is answered here (the kinds `Data` and `Prefab`, `take_loaded(kind)` for kinds with one consumer, the dirty-entry rule). SC1 to SC5 are unchanged in scope; the edit adds: the `prefab` field of the scene entity record and the instance patch shape; **`prefabs: Option<&PrefabStore>` on `InstantiateOptions`, `validate_scene` and `SaveOptions`, and `drop_stale` on `SaveOptions`**; **`SavedScene.entities: Vec<Entity>`** (file order, instance nodes in flat pre-order); the **`.`-in-authored-id rule** (`InvalidLabel`); **the new `SceneWarning` and `SceneErrorKind` variants** (`StaleInstanceEntry`, `StaleEntryDropped`, `PrefabNodeDetached`, `InstanceOriginMissing`, `MissingPrefabNode`, `PrefabNotLoaded`, `PrefabStoreMissing`, `InstanceHasComponents`, `AddedInInstancePatch`, `PatchConflict`, `InvalidLabel`); the silent skip of `InstanceOrigin` in `write_scene`; SP4's "unknown envelope field is a warning" rule keeps old scene files loading; SP4's "unknown component type is always an error" is relaxed for **patch entries only** (D14). AP3's `find_references` and the `asset_remove` guard are extended to `Data` and `Prefab` by AK1. FA3's `LoadScene` gains the `WaitingForPrefabs` state in FD2 (FA3 is not reopened); no event shape changes.
- **`editor-mcp-mvp.md` and SP8 (#149), issue #206.** Adds the 12 data tools and 11 prefab tools of "MCP / editor surface" to the tool table (23 tools, to be totalled against the roughly 90-tool budget with every other spec's tools) and to the parity test. `scene_load`, `scene_validate` and `scene_save` pass `prefabs` and, for `scene_load`, request missing prefabs and return `PrefabsPending`; **`scene_save` gains `drop_stale`** (parity: an agent must be able to clean a stale instance entry), and `scene_load` and `scene_save` surface `StaleInstanceEntry`. **`exitium_scene::InstanceOrigin` is added to `INTERNAL_COMPONENTS`.** Undo of a despawn, copy and paste of an **instance**, and undo of `prefab_refresh` go through `write_scene` (`SaveScope::Entities`) and `instantiate`, never component by component. **What the bus does with an entry whose undo returns `StaleEdit`** (the data edit or prefab edit no longer applies): recommended that the entry is dropped from the stack and the call returns `undo_failed` with the reason, so the user is not stuck on top of an entry that can never run. `play_start` and `play_stop` call `DataStore::begin_play` and `end_play`; **`data_save`, `data_create`, `data_remove`, `prefab_save`, `prefab_create`, `prefab_flatten` and `prefab_remove` are refused in Play**; the save and authoring commands call `note_own_write`; the editor drops a guid's undo entries when a disk reload applies. SP8 builds the Data Asset and Prefab panels, the list and map element UI and the nested badges of "Editor panels". SP8 is ordered after SP10.
- **`reflect-model.md`, open question 9, issue #207** (runtime-defined types): resolved as not in the MVP (open question 9 above). The same edit documents the well-known `custom` key **`list_key = true`** (one per item struct, on a key leaf) in "Attributes"; no new attribute, no hash change. An optional derive-time check (at most one `list_key` field, a key leaf type) is a separate reflect follow-up if wanted.
- **SP9 (Luau):** reads data assets through `DataStore::get(guid) -> &dyn Reflect`.
- **#61:** unchanged.
- **`CLAUDE.md` files:** DA1 creates `crates/exitium_data/CLAUDE.md`; each scene issue updates `crates/exitium_scene/CLAUDE.md`.

**Follow-ups filed, not in this PR:** a per-entry content hash of resolved data and prefabs for netcode handshakes (#208; a prefab's hash covers its flat resolved nodes, so it includes nested content); a `prefab_save` warning that lists loaded variants and containers naming a removed node (open question 11, #209); a native-only default for `DataSettings.preload` (#210; on web it fetches every data asset at start, six requests in flight at a time).
