# Spec: Reflection model, type registry, derive macro and schema hash

- Issue: #20
- Crates: `exitium_reflect`, `exitium_reflect_derive` (touchpoints: `exitium_core`, `exitium_ecs`, `exitium_serial`, `exitium_mcp`, `exitium_script`, `exitium_editor`)
- Status: draft

## Goal

One `#[derive(Reflect)]` makes a Rust type visible at runtime: its fields, enum variants, attributes, docs and a stable layout hash. The inspector, text/binary serialization, Luau bindings, MCP tool schemas and save migration are all derived from that single description (design rule 2, INTERVIEW principle 2). This spec fixes the trait shapes, metadata model, registry, attribute set, derive limits and the schema hash so that the Phase 1 implementation issues (#31 to #37, #50) and the serialization specs (#23, #24) can proceed in parallel against one contract.

## Non-goals

- Serialization formats themselves (text: #23, binary: #24). This spec only says what they may read from reflection.
- The command bus. Reflection supplies the mutation primitives (`apply`, `path_mut`, `clone_boxed`); commands are specced separately.
- ECS storage and change detection (see #21). Only the reflection side of the component hook is defined here.
- Types defined purely at runtime with no Rust type (data assets defined in text). Open question 9.
- Sets (`HashSet`, `BTreeSet`), `VecDeque`, `std::time::Duration`, `usize`/`isize`, trait-object fields, borrowed fields (`&'a T`). None are reflectable in v1 (open question 10; a game that needs a duration stores `f32`/`Num` seconds, a set stores a `Vec` or a `HashMap<K, ()>`).
- Persisting `Handle<T>` (a process-local arena id). Only `RawHandle` is reflected; typed asset references are open question 13.
- Entity and component access by MCP tools and commands (`ecs_*`, `SetField`, `InsertComponent`): owned by #21 (see "MCP / editor surface"). This spec owns only the `reflect_*` tools.
- Compile-fail tests with `trybuild` (a dependency; #34 defers it).

## Design

### Crate layout and dependency direction

```
exitium_core  <-  exitium_reflect  <-  exitium_ecs, exitium_serial, exitium_data, exitium_script, exitium_mcp, exitium_editor
                       ^
              exitium_reflect_derive (proc-macro, re-exported by exitium_reflect)
```

- `exitium_reflect` depends on `exitium_core` only (#37: `reflect -> core`). Core types (`Num`, `Guid`, `RawHandle`, glam math) get their `Reflect` impls inside `exitium_reflect`, which satisfies the orphan rule because the trait is local.
- `exitium_reflect` has **no external dependencies**. It does not use `serde`; serializers walk reflection directly.
- `exitium_reflect_derive` is the only crate using `syn`, `quote`, `proc-macro2` (see Dependencies).
- Everything is safe Rust (no `unsafe`, no Miri module). Downcasting uses `std::any::Any`; leaked `TypeInfo` for generics uses `Box::leak`.
- `exitium_ecs` owns `Component` and `Resource`. The Reflect derive never implements them (see "ECS integration hook").

### The model in one picture

Two layers, both read through `&dyn Reflect`:

1. **Static metadata** (`TypeInfo`): immutable, built once per type, `'static`, shared by every thread. Answers "what *is* this type".
2. **Dynamic access** (`Reflect` plus the kind traits `Struct`, `TupleStruct`, `Enum`, `List`, `Map`, `Option`): answers "what is in this *value*" and lets you change it.

A consumer never downcasts to learn a shape. It matches on `reflect_ref()` and gets an exhaustive enum of kind traits:

```rust
pub enum ReflectRef<'a> {
    Struct(&'a dyn Struct), TupleStruct(&'a dyn TupleStruct), Enum(&'a dyn Enum),
    List(&'a dyn List), Map(&'a dyn Map), Option(&'a dyn OptionReflect),
    Value(&'a dyn Reflect),      // bool, ints, floats, char, String, ()
}
// ReflectMut<'a> is the same shape with &mut.
```

### `TypeInfo` (data layout)

All of this is built once (`OnceLock` for non-generic types, a `GenericTypeCell` for generic ones, below) and then only read. Strings are `&'static str`; field and variant lists are owned `Box<[T]>` inside the `TypeInfo`, so nothing is leaked for non-generic types.

```rust
pub enum TypeInfo {
    Struct(StructInfo), TupleStruct(TupleStructInfo), Enum(EnumInfo),
    List(ListInfo), Map(MapInfo), Option(OptionInfo), Value(ValueInfo),
}

pub struct TypeMeta {
    pub type_id: TypeId,
    pub name: &'static str,         // stable type name, the registry key (see "Stable type names")
    pub docs: &'static str,         // joined `///` lines; empty with the `strip_docs` feature (see Attributes)
    pub version: Option<u32>,       // #[reflect(version = N)]: stored, NOT hashed, written to binary headers (see Schema hash)
    pub attrs: Attributes,
    pub is_dynamic: bool,           // true only for the Dynamic* types
}

pub struct StructInfo      { pub meta: TypeMeta, pub fields: Box<[FieldInfo]>, pub transparent: bool }
pub struct TupleStructInfo { pub meta: TypeMeta, pub fields: Box<[FieldInfo]>, pub transparent: bool,
                             pub is_tuple: bool }  // names "0", "1", ...; is_tuple only for built-in `(A, B)` (hashed structurally)
pub struct EnumInfo        { pub meta: TypeMeta, pub variants: Box<[VariantInfo]> }
pub struct ListInfo        { pub meta: TypeMeta, pub item: TypeRef, pub fixed_len: Option<usize> }   // Vec<T> and [T; N]
pub struct MapInfo         { pub meta: TypeMeta, pub key: TypeRef, pub value: TypeRef }
pub struct OptionInfo      { pub meta: TypeMeta, pub inner: TypeRef }
pub struct ValueInfo       { pub meta: TypeMeta, pub repr: ValueRepr }

pub struct FieldInfo {
    pub name: &'static str, pub index: usize, pub ty: TypeRef,
    pub docs: &'static str, pub attrs: Attributes,
}
pub struct VariantInfo {
    pub name: &'static str, pub index: u32, pub kind: VariantKind, // Unit | Tuple | Struct
    pub fields: Box<[FieldInfo]>, pub docs: &'static str, pub attrs: Attributes,
}

/// Lazy reference to another type's info. A function pointer (not a `&'static TypeInfo`)
/// so recursive types (`struct Tree { kids: Vec<Tree> }`) can be described.
pub struct TypeRef { pub type_id: TypeId, get: fn() -> &'static TypeInfo }

pub enum ValueRepr { Unit, Bool, Char, I8, I16, I32, I64, I128, U8, U16, U32, U64, U128, F32, F64, String }
```

Rules:

- **Fields are in declaration order** and `index` is that order. Binary layout follows it (see Schema hash).
- **`transparent`** (`#[reflect(transparent)]`, only legal on a single-field struct): the type is layout-identical to its field. `Num(f64)`, `Guid(u128)` and `Box<T>` use it. Consumers treat it as its inner field (the inspector shows one number, text format writes `3.5` not `Num(3.5)`). In the schema hash a transparent type is invisible: it resolves to its inner field's type (see Schema hash). Chains of transparent types resolve to the first non-transparent type.
- **Tuples** `(A, B)` up to arity 8 are `TupleStruct` kind with the synthetic name `(A,B)`. No ninth kind.
- **`Box<T>`** is a transparent `TupleStruct` named `Box<T>`.
- **Value** covers only primitives and `String`. Domain types such as `Num`, `Guid`, `RawHandle`, `Entity`, `Rect`, `Color`, and glam types are `transparent` wrappers or ordinary structs (`Vec2` is a struct `{x, y}`), never opaque. Consequence: every reflected value bottoms out in the 16 `ValueRepr` leaves, so the **binary** format needs exactly 16 leaf encoders and no per-type codecs.
- **Text form hook.** A few types have a canonical text spelling that is not the structural one: `Guid` text must equal `Guid::Display` (#37), and `Entity` is `"42v1"`, never a bare integer (#21). They register the type data `ReflectTextForm` (below). The **text and JSON/MCP** serializers (#23) consult it before walking structure; the **binary** format never does, so binary layout stays exactly what the schema hash describes. The hook changes nothing about `TypeInfo` or the hash.
- **`Entity`** is *not* a transparent `u64`. It reflects as a struct `{ index: u32, generation: u32 }` (declared in `exitium_ecs`, which derives `Reflect` on it; the `NonZeroU32` generation is a validity rule enforced by `from_reflect`/`set`/`apply`, which return an error on `0`) plus `ReflectTextForm` for `"{index}v{generation}"`. Whether #24 writes `to_bits` instead of two `u32`s is #24's call and does not touch this spec: the schema hash describes the reflected structure only.
- **`RawHandle`/`Handle<T>`.** `RawHandle { index: u32, generation: u32 }` (#15) is an ordinary struct. `Handle<T>` gets **no** `Reflect` impl in v1: it is a process-local arena id with no meaning after a reload, so it must not be persisted or hashed as if it did, and giving it a stable name would force every handle target `T` to be `Reflectable`. See open question 13 for how typed asset references persist (default: a `Guid`-backed `AssetRef<T>` in `exitium_assets`).
- `usize`/`isize` have no `Reflect` impl (width differs between wasm32 and 64-bit; it would break hash and binary stability). Use explicit widths.
- Field lookup by name is a linear scan of the `fields` slice. Components have few fields; a scan of 16 short names beats hashing.

### Stable type names

`std::any::type_name` is explicitly unspecified across compiler versions, so it is never used. Instead:

| Type | Stable name |
|---|---|
| primitives, `String` | `u32`, `f64`, `bool`, `String`, `()` |
| engine core types (pinned, unqualified) | `Num`, `Guid`, `RawHandle`, `Entity`, `Rect`, `Color`, and the glam math types under their glam names: `Vec2`, `Vec3`, `Vec4`, `IVec2`, `IVec3`, `IVec4`, `UVec2`, `UVec3`, `UVec4`, `Quat`, `Mat3`, `Mat4` |
| user type | `module_path!()` + `::` + identifier, e.g. `glutton::economy::Wallet`; override with `#[reflect(type_name = "glutton::Wallet")]` |
| generics | base name + stable names of the arguments, no spaces: `Vec<f32>`, `Option<glutton::Wallet>`, `HashMap<String,u32>`, `[f32;4]`, `(f32,f32)`, `Box<glutton::Wallet>`, `glutton::Pair<u8,String>` |

Rules: a user type name always contains `::` (it starts with the crate name), and a `type_name` override without `::` is a compile error, so no user type can collide with an engine core name. Engine core names are fixed by this table and are part of the persisted-format contract (changing one needs a new `SCHEMA_HASH_VERSION` and an owner-approved PR). Glam types are reflected as plain structs of their public fields (`Vec2 { x, y }`, `Quat { x, y, z, w }`, matrices as structs of column vectors `x_axis`, `y_axis`, `z_axis`[, `w_axis`]), registered by exitium_reflect, so they cost no dependency in `exitium_core`.

Moving or renaming a type changes its name, and the name is part of the hash, so it is treated as a schema change. Types that are persisted should pin their name with `type_name`. Generic names are composed at runtime in the `GenericTypeCell` (not in `const`), then cached. The registry name of a generic embeds the written names of its arguments, but the schema hash treats built-in containers structurally (see Schema hash), so only *user* generic structs hash differently for `Pair<Num,u8>` and `Pair<f64,u8>`.

### Reflect trait and kind traits (API sketch)

```rust
/// Everything reflected implements this. Components and resources are `Send + Sync` already.
pub trait Reflect: Any + Send + Sync {
    /// Instance side of the type's info. Named differently from `Typed::type_info()` on purpose:
    /// with both traits in scope, one shared name makes `Wallet::type_info()` ambiguous (E0034).
    fn reflect_type_info(&self) -> &'static TypeInfo;
    /// Concrete types: `Some(own info)`. Dynamic* types: the type they were built to represent, if known.
    fn represented_type_info(&self) -> Option<&'static TypeInfo>;

    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
    fn as_reflect(&self) -> &dyn Reflect;
    fn as_reflect_mut(&mut self) -> &mut dyn Reflect;

    fn reflect_ref(&self) -> ReflectRef<'_>;
    fn reflect_mut(&mut self) -> ReflectMut<'_>;

    /// Patch `self` from `other` (see "apply semantics").
    fn apply(&mut self, other: &dyn Reflect) -> Result<(), ApplyError>;
    /// Replace the whole value; hands the value back on a type mismatch.
    fn set(&mut self, value: Box<dyn Reflect>) -> Result<(), Box<dyn Reflect>>;
    /// Deep copy as a boxed concrete value. Used for undo snapshots and prefab instancing.
    fn clone_boxed(&self) -> Box<dyn Reflect>;
    /// Structural equality; `None` if the two cannot be compared (different kinds).
    fn reflect_eq(&self, other: &dyn Reflect) -> Option<bool>;
}

/// Static side: `T::type_info()` needs no instance. `Typed` is the only trait with a method called `type_info`.
pub trait Typed: Reflect { fn type_info() -> &'static TypeInfo; }

/// Rebuild a concrete value from a dynamic one (the last step of deserialization).
pub trait FromReflect: Sized + Reflect { fn from_reflect(value: &dyn Reflect) -> Option<Self>; }

pub trait Struct: Reflect {
    fn field_len(&self) -> usize;
    fn name_at(&self, index: usize) -> Option<&str>;
    fn field(&self, name: &str) -> Option<&dyn Reflect>;
    fn field_mut(&mut self, name: &str) -> Option<&mut dyn Reflect>;
    fn field_at(&self, index: usize) -> Option<&dyn Reflect>;
    fn field_at_mut(&mut self, index: usize) -> Option<&mut dyn Reflect>;
    fn iter_fields(&self) -> FieldIter<'_>;
}
pub trait TupleStruct: Reflect { /* field_len, field(i), field_mut(i), iter_fields */ }
pub trait Enum: Reflect {
    fn variant_name(&self) -> &str;
    fn variant_index(&self) -> u32;
    fn variant_kind(&self) -> VariantKind;
    /* field_len, name_at, field(name), field_at(i) and the _mut forms, on the ACTIVE variant */
}
pub trait List: Reflect {
    fn len(&self) -> usize;
    fn get(&self, i: usize) -> Option<&dyn Reflect>;
    fn get_mut(&mut self, i: usize) -> Option<&mut dyn Reflect>;
    fn push(&mut self, v: Box<dyn Reflect>) -> Result<(), Box<dyn Reflect>>;       // Err for fixed arrays or wrong type
    fn insert(&mut self, i: usize, v: Box<dyn Reflect>) -> Result<(), Box<dyn Reflect>>;
    fn remove(&mut self, i: usize) -> Option<Box<dyn Reflect>>;                      // None for fixed arrays
    fn iter(&self) -> ListIter<'_>;
}
pub trait Map: Reflect {
    fn len(&self) -> usize;
    fn get(&self, key: &dyn Reflect) -> Option<&dyn Reflect>;
    fn get_mut(&mut self, key: &dyn Reflect) -> Option<&mut dyn Reflect>;
    fn insert(&mut self, k: Box<dyn Reflect>, v: Box<dyn Reflect>) -> Result<Option<Box<dyn Reflect>>, ApplyError>; // Ok(old value)
    fn remove(&mut self, key: &dyn Reflect) -> Option<Box<dyn Reflect>>;
    fn iter(&self) -> MapIter<'_>;   // BTreeMap: key order; HashMap: arbitrary (ordering is the serializer's job)
}
pub trait OptionReflect: Reflect {
    fn inner(&self) -> Option<&dyn Reflect>;
    fn inner_mut(&mut self) -> Option<&mut dyn Reflect>;
    fn set_some(&mut self, v: Box<dyn Reflect>) -> Result<(), Box<dyn Reflect>>;
    fn set_none(&mut self);
}
```

Design choices a reviewer should know:

- **Why no `default()` on `Reflect`:** a trait object cannot call a static method. "Default" is the type data `ReflectDefault` in the registry (below).
- **Why `Clone` is required:** `clone_boxed` is `Box::new(self.clone())`, so reflected types must be `Clone + Send + Sync + 'static`. Undo, prefab instancing and the command bus need cheap exact copies anyway (open question 1).
- **Map keys** are limited to (a) `Value` kinds except floats and `()` (ints, `bool`, `char`, `String`), (b) `transparent` wrappers of those (`Guid`), (c) **enums whose variants are all unit variants** (`HashMap<ResourceKind, Num>` is the idiomatic incremental-game state and must work), and (d) any type registered with `ReflectTextForm` (`Entity`). Lookup downcasts the key to `K` (which supplies `Hash + Eq` or `Ord`), so no reflected hashing is needed. Key spelling in text/JSON: variant name for enums, the text form for (d), decimal for ints. Binary: leaf encoding, enums as the `u32` variant index. Deterministic order when a `HashMap` is written: ints numeric, strings and text forms by UTF-8 bytes, enums by variant index. Registration rejects any other key type with `RegistryError::InvalidMapKey`.
- **`Dynamic*` types** (`DynamicStruct`, `DynamicTupleStruct`, `DynamicEnum`, `DynamicList`, `DynamicMap`, `DynamicOption`) implement `Reflect` and the matching kind trait with `is_dynamic = true`. They are what the text and binary readers produce when a value has no concrete type yet or does not match the current schema (unknown fields are kept by name). `DynamicValue` in the migration API (#59) is `Box<dyn Reflect>` holding one of these or a primitive. A reader sets `represented_type_info` from the registry when it knows the target type.

### Apply semantics

`a.apply(b)` makes `a` match `b`, restricted to what `b` contains:

| Kind | Behaviour |
|---|---|
| Struct / TupleStruct | For each field present in `b`: recurse. Fields missing from `b` keep their value. Fields in `b` that `a` lacks are ignored. |
| Enum | Same variant: recurse into fields. Different variant: build the new variant from `b` (missing fields filled from `#[reflect(default)]`, else `ReflectDefault`, else `ApplyError::MissingField`). |
| List | Resize to `b.len()`, recurse element-wise. Fixed arrays require equal length. |
| Map | Make `a`'s key set equal `b`'s: insert/update present keys, remove the rest. |
| Option | `None` -> `None`; `Some(x)` recurses or creates the inner value. |
| Value | Downcast and assign, else `ApplyError::TypeMismatch { expected, found }`. |

`apply` is **not atomic**: on error `self` may be partly modified. The command bus takes a `clone_boxed()` snapshot first and restores it on failure (also the undo record). Atomic apply is open question 6.

### Path access

```
path     = segment { segment }
segment  = field | index | key
field    = [ "." ] ident              ; leading "." optional on the first segment
index    = "[" digits "]"  |  "." digits   ; list/array element, tuple-struct field, or integer map key
key      = "[" '"' chars '"' "]"          ; string map key, \" and \\ escapes
```

Examples: `position.x`, `items[3].name`, `pair.0`, `stats["hp"]`, `inventory[12].count`.
Enum paths walk the *active* variant's fields. An `Option` is traversed transparently: `a.b` where `a: Option<S>` reads `S.b`, and `None` yields `PathError::NoneValue`.

```rust
pub struct ParsedPath(Vec<PathSegment>);                  // parse once, reuse
impl ParsedPath { pub fn parse(s: &str) -> Result<Self, PathError>; }

pub trait GetPath: Reflect {
    fn reflect_path(&self, path: &str) -> Result<&dyn Reflect, PathError>;
    fn reflect_path_mut(&mut self, path: &str) -> Result<&mut dyn Reflect, PathError>;
    fn path<T: Reflect>(&self, path: &str) -> Result<&T, PathError>;     // typed convenience
    fn reflect_parsed(&self, p: &ParsedPath) -> Result<&dyn Reflect, PathError>;
    fn reflect_parsed_mut(&mut self, p: &ParsedPath) -> Result<&mut dyn Reflect, PathError>;
}
pub struct PathError { pub segment_index: usize, pub segment: String, pub kind: PathErrorKind }
pub enum PathErrorKind { Syntax { offset: usize }, NotFound, OutOfBounds { len: usize }, NoneValue, KindMismatch { expected: &'static str, found: &'static str } }
```

Parsing never panics on arbitrary input (tested by a randomized test).

### Attributes

Written as `#[reflect(...)]` on types, fields and variants. Unknown attributes are a compile error that lists the valid set for that position.

| Attribute | On | Meaning | Hashed? |
|---|---|---|---|
| `range(min = a, max = b [, step = s])` | field | Numeric bounds, inspector slider, JSON Schema `minimum`/`maximum`. Legal when the field's type, **after resolving `transparent` wrappers**, is an integer or float `Value`, so `range` works on a `Num` field (bounds are in the unit of the inner `f64`). Rejected at registration (`RegistryError::InvalidAttribute`) on `bool`, `char`, `String`, containers, structs and enums, including `Option<f32>` and `Vec<f32>` (v1 has no per-element range). Bounds and `step` are stored as `f64`; for `i64`/`u64`/`i128`/`u128` beyond 2^53 they are inspector hints only, not exact limits. `range` is a UI and schema hint: `set`/`apply` do not clamp. | no |
| `tooltip = "text"` | type, field, variant | Human text. Falls back to `///` docs when absent. | no |
| `hidden` | field, variant | Hide from the **inspector** only. Still serialized, still in MCP schemas (marked hidden) so agents keep parity. | no |
| `skip` | field | Not reflected, not serialized, not in `TypeInfo` or the hash. Rebuilt with `Default::default()` or its `default` expression in `from_reflect`. | no (absent) |
| `default` / `default = "path::to_fn"` | field | Value used when the field is missing in dynamic/serialized data. Stored as `fn() -> Box<dyn Reflect>` so schemas can show defaults. | no |
| `custom(key = literal, ...)` | type, field, variant | Extensible key/value store (`bool`, integer, float, string), e.g. `custom(widget = "color")`. | no |
| `type_name = "..."` | type | Override the stable name. | yes (name) |
| `transparent` | single-field struct | See "TypeInfo". | yes |
| `version = N` | type | Declared type version (`u32`) for migrations. Stored in `TypeMeta` and in binary headers, not hashed (see "Schema hash"). | no |
| `crate = path` | type | Override where generated code finds `exitium_reflect` (and, with `ecs_crate = path`, `exitium_ecs`). | no |
| `Default`, `TextForm`, `Component`, `Resource` | type | Register type data `ReflectDefault`, `ReflectTextForm`, `ReflectComponent`, `ReflectResource` (see Registry). These four idents are built in; the derive expands each to a fully qualified path. `Component`/`Resource` only register type data, they never implement the traits. | no |
| `data(path::ReflectFoo)` | type | Register a user-defined `TypeData` (`FromType<T>` must exist). The path is used exactly as written, resolved at the user's site. | no |

```rust
pub struct Attributes {
    pub range: Option<Range>,                    // Range { min: f64, max: f64, step: Option<f64> }
    pub tooltip: Option<&'static str>,
    pub hidden: bool,
    pub default: Option<fn() -> Box<dyn Reflect>>,
    pub custom: Box<[(&'static str, AttrValue)]>, // AttrValue = Bool | Int(i64) | Float(f64) | Str(&'static str)
}
impl Attributes { pub fn custom(&self, key: &str) -> Option<&AttrValue>; }
```

How consumers read them: the inspector uses `range`, `hidden`, `tooltip`/`docs` and `custom("widget")`. `exitium_serial` reads only `default` (fill missing fields) and relies on `skip` being absent from `TypeInfo`. The MCP schema generator maps `range` to `minimum`/`maximum`, `tooltip`/`docs` to `description`, `default` to `default`, `hidden` to `x-exitium-hidden`, `custom` to `x-exitium-<key>`. Attributes are a closed struct on purpose: a reader can find every attribute with one go-to-definition, with no `dyn Any` attribute store.

**`strip_docs`.** `cfg!` inside macro-generated code is evaluated in the *user's* crate, where the feature does not exist, so the derive cannot read it. Instead the derive always emits docs through a function owned by `exitium_reflect`: `::exitium_reflect::doc_str("joined docs")`, a `const fn` whose body is `#[cfg(feature = "strip_docs")] { "" }` / `#[cfg(not(feature = "strip_docs"))] { s }`. The feature is therefore read where it is declared. Wasm size relies on the optimizer dropping the now-unused literals; the first bench PR records the shipped `.wasm` size with and without the feature.

*Mechanism.* Cargo features are unified across one build graph, so the rule is: the feature is **off by default and never enabled by any workspace crate's default features** (so `cargo build --workspace`, the editor and the MCP server keep docs). Only shipping builds turn it on, through the facade feature `exitium/strip_docs = ["exitium_reflect/strip_docs"]` that `cargo xtask package` passes for a game's release build. A CI check (`cargo tree -e features -p exitium_editor -p exitium_mcp`) asserts the feature is absent from the editor and MCP graphs.

### Type registry

```rust
pub struct TypeRegistry {
    by_id:   HashMap<TypeId, TypeRegistration>,
    by_name: HashMap<&'static str, TypeId>,
}
pub struct TypeRegistration {
    info: &'static TypeInfo,
    data: Vec<(TypeId, Box<dyn TypeData>)>,   // small; linear scan
    shallow: OnceLock<SchemaHash>,            // per-type shallow hash, cached (see Schema hash)
    deep: OnceLock<SchemaHash>,               // deep hash over the type closure, cached
}

impl TypeRegistry {
    pub fn register<T: GetTypeRegistration>(&mut self) -> Result<(), RegistryError>;
    pub fn register_type_data<T: Reflect, D: TypeData + FromType<T>>(&mut self);
    pub fn get(&self, id: TypeId) -> Option<&TypeRegistration>;
    pub fn get_by_name(&self, stable_name: &str) -> Option<&TypeRegistration>;
    pub fn type_data<D: TypeData>(&self, id: TypeId) -> Option<&D>;
    pub fn schema_hash(&self, id: TypeId) -> Option<SchemaHash>;            // deep hash
    pub fn shallow_hash(&self, id: TypeId) -> Option<SchemaHash>;
    pub fn schema_table(&self, id: TypeId) -> Option<SchemaTable>;          // header table, see Schema hash
    pub fn iter(&self) -> impl Iterator<Item = &TypeRegistration>;
}
```

- **Registration is explicit** (`registry.register::<Wallet>()`, normally via `app.register_type::<Wallet>()` in `exitium_app`). No `inventory`/`ctor` link-time auto-registration: it fails on wasm, depends on link order and is hidden magic.
- `register::<T>` also registers every type reachable from `T`'s fields and items (`GetTypeRegistration::register_type_dependencies`), so `Vec<Wallet>` registers `Wallet`.
- Same `TypeId` twice is a no-op. A different `TypeId` with an already-used stable name is `RegistryError::DuplicateName { name }` (#31). Registration stops at the first error; earlier registrations stay.
- **Type data** is how other crates attach behavior to a type without `exitium_reflect` knowing them: `TypeData` is a `Send + Sync + 'static` trait, `FromType<T>` builds it. `#[reflect(Default)]` in the derive is sugar for `register_type_data::<T, ReflectDefault>()`, emitted as a **fully qualified path** (see "Derive macro: paths"), so the user imports nothing and no type-data ident has to be in scope.

  | Type data | Defined in | Purpose |
  |---|---|---|
  | `ReflectFromReflect` | `exitium_reflect`, always registered by the derive | build `T` from a dynamic value |
  | `ReflectDefault` | `exitium_reflect`, on `#[reflect(Default)]` | `fn() -> Box<dyn Reflect>` |
  | `ReflectTextForm` | `exitium_reflect`, on `#[reflect(TextForm)]` | canonical text spelling, see below |
  | `ReflectComponent` | `exitium_ecs` (#50), on `#[reflect(Component)]` | see "ECS integration" |
  | `ReflectResource` | `exitium_ecs`, on `#[reflect(Resource)]` | same for resources |

- **`ReflectTextForm`** is built by `FromType<T: Reflect + TextForm>`, where `trait TextForm: Sized { fn to_text(&self) -> String; fn from_text(s: &str) -> Result<Self, TextFormError>; }` is implemented by hand on the type. `exitium_reflect` implements it for `Guid` (`to_text` is exactly `Guid::Display`, `from_text` its inverse; #37 adds the parse if `exitium_core` lacks it). `exitium_ecs` implements it for `Entity` (`"{index}v{generation}"`, never a bare integer). A `to_text`/`from_text` round trip is a required test for every implementor. Only the text/JSON/MCP serializers consult it; map keys of such types are written as the text string. It is not hashed: a change to a text form is a text-format change reviewed with the type, and golden tests for `Guid` and `Entity` text pin it.
- **Generics and containers:** the registry is keyed by concrete instantiation (`Vec<f32>` and `Vec<u8>` are separate entries). Derived generic types need `T: Reflectable`, a blanket-implemented alias for `Reflect + Typed + FromReflect + GetTypeRegistration`. A static in a generic function is shared by all instantiations, so generic `TypeInfo` is cached in `GenericTypeCell`: `RwLock<HashMap<TypeId, &'static TypeInfo>>`, write-once per instantiation, entries `Box::leak`ed (bounded by the number of distinct instantiations in the program).

  **No user code and no re-entry under the lock.** Building the name and info of `Vec<Vec<f32>>`, `Pair<Pair<u8,u8>,u8>` or `Option<Option<T>>` calls `type_info()` of the argument types, which lands in the *same* cell (one `static` per generic function). So the lookup is:
  1. read lock, look up `TypeId::of::<Self>()`, release; return on hit;
  2. on a miss, **build the `TypeInfo` with no lock held** (this may recursively fill the cell for the argument types);
  3. take the write lock and `entry(id)`: `Occupied` returns the existing `&'static`, dropping the freshly built value (nothing leaked); `Vacant` does `Box::leak` and inserts.

  Two threads racing on a miss may both build; one wins and both return the same reference. No `Mutex` poisoning is possible (no user code runs under a guard); the cell uses `PoisonError::into_inner` anyway. Non-generic types use `OnceLock`, which cannot re-enter either: building a `TypeInfo` only stores `TypeRef` function pointers for field types, it never calls their `type_info()` (this is also what makes recursive types such as `Tree { kids: Vec<Tree> }` buildable). Test: `Vec<Vec<f32>>`, `Pair<Pair<u8,u8>,u8>` and `Option<Option<u32>>` first-use from 8 threads, no deadlock, equal `&'static` pointers.
- Built-in impls (#31, #33): `bool`, `char`, all fixed-width ints incl. `i128`/`u128`, `f32`, `f64`, `String`, `()`, `Vec<T>`, `[T; N]`, `Option<T>`, `Box<T>`, `HashMap<K,V>`, `BTreeMap<K,V>`, tuples up to arity 8 (small private `macro_rules!`, not public API). Core types come with #37.

### Threading model

- **Everything static is shareable.** `TypeInfo` and its parts are immutable `'static` data; `Reflect` and `TypeData` are `Send + Sync` (supertraits), so reflected types, their type data and `TypeInfo` can cross threads freely. A type that is not `Send + Sync` cannot be reflected (consistent with the ECS non-goal on non-`Send` components, #21). No reflected value contains interior locking; `&mut dyn Reflect` is exclusive by Rust's rules, and concurrent access to *world* data is governed by the ECS access rules, not by reflection.
- **First-use caches are thread-safe and idempotent.** `OnceLock` (non-generic `TypeInfo`, hash caches) and `GenericTypeCell` give the same result no matter which thread initializes first.
- **Registry ownership.** `TypeRegistry` is an ordinary struct with no interior mutability besides the `OnceLock` hash caches, hence `Send + Sync`. `exitium_app` owns one instance as the resource `AppTypeRegistry(Arc<RwLock<TypeRegistry>>)`; MCP, editor, script and serializer code clone the `Arc` and read through it.
- **Read-mostly.** All game types are registered during app build, before the first update, through `app.register_type::<T>()`, which takes the write lock. After startup the registry is only read. A reader takes **one** read guard per operation (one serialize call, one MCP request, one inspector frame), never one per field, and never holds a guard across a system run or command application. Cost of that guard is covered by the "registry lookup" budgets. `&'static TypeInfo` values outlive the guard; `&TypeRegistration` borrows do not.
- **Late registration** (editor loading a game library, hot reload of a Rust plugin) is allowed and takes the write lock; it must happen at a sync point on the thread that owns `&mut App`/editor state, never from a system. Systems and MCP handlers only read, so they cannot deadlock against it.
- **Registration is order-independent and idempotent.** Same `TypeId` twice is a no-op; hashes and tables depend only on the set of registered types, not the order (tested by registering in two orders); a name collision is reported as `DuplicateName` whichever of the two types registers second.

### Derive macro

`#[derive(Reflect)]` on a struct or enum generates plain `impl` blocks, nothing else:

1. `Reflect` and the matching kind trait (`Struct`, `TupleStruct` or `Enum`)
2. `Typed` (the `TypeInfo`, behind a `OnceLock`, or `GenericTypeCell` if generic)
3. `FromReflect`
4. `GetTypeRegistration` (registers the type and its field types, plus any `#[reflect(Default, ...)]` type data)

**The derive does not implement `Component` or `Resource`.** Those traits belong to `exitium_ecs`, which provides its own `#[derive(Component)]` (where that macro lives is #21/#50's call). A game writes `#[derive(Component, Reflect)] #[reflect(Component)] struct Wallet { .. }`: the first derive makes it a component, the second reflects it, and `#[reflect(Component)]` only registers the `ReflectComponent` type data (which requires `T: Component + Reflect + FromReflect`, so forgetting `derive(Component)` is a normal trait-bound error at the registration site). This keeps a single direction of dependency (`ecs -> reflect`) and means a type can be reflected without being a component.

**Paths** (the derive emits fully qualified paths only; the user imports nothing). Two roots are resolved: `exitium_reflect` for the reflect items, and `exitium_ecs` for `ReflectComponent`/`ReflectResource`.

- *Engine crates* get `::exitium_reflect` / `::exitium_ecs`. The decision is an **explicit allow-list inside the derive** (`ENGINE_CRATES`, matched against `CARGO_PKG_NAME`), not a name-prefix rule: it contains every `exitium_*` workspace crate that depends on `exitium_reflect` directly, plus the `exitium` facade itself (which depends on `exitium_reflect`, so `::exitium_reflect` resolves inside its own source and no `extern crate self as exitium;` is needed). `exitium_reflect` declares `extern crate self as exitium_reflect;` for its own derive uses and tests.
- *Everything else* (games, examples, user crates, **including a user crate that happens to be named `exitium_something`**) gets `::exitium::reflect` / `::exitium::ecs`, which the facade re-exports. This `reflect` (and `ecs`) re-export is part of the facade's public API and therefore a human-gated surface; this spec requires it, the PR carries `gate:human` anyway.
- `#[reflect(crate = path)]` and `#[reflect(ecs_crate = path)]` override either root, for a crate that renames its dependency or re-exports through its own prelude.
- **Drift guard:** `exitium_reflect_derive/tests/crate_paths.rs` lists the crate directories under `crates/` using `std::fs` and asserts each is either in `ENGINE_CRATES` or in a short `NOT_DERIVE_USERS` list in the same test (for example `exitium_core`, which cannot depend on reflect, and `exitium_cli`). A newly added crate fails that test until someone classifies it, so the allow-list cannot silently rot. No manifest-parsing dependency is needed.

**Macro-magic limits (CLAUDE.md rule 6).** The derive must stay readable to a basic-level Rust reviewer:

- It only parses a struct/enum, its generics, and `#[reflect(...)]` / `#[doc]` attributes. No function-like or attribute proc macros, no `macro_rules!` DSL in public API.
- It emits only the four impl groups above, no new public items, hidden fields or helper modules. All paths are fully qualified (see "Paths"). No global state.
- Each generated impl method is a direct match/field access. A hand-written equivalent for a sample struct and enum lives in `exitium_reflect/tests/manual_impl.rs`, with a test that it behaves like the derive output. The derive module docs show the same example.
- Derive crate: at most 800 lines excluding tests, no `unsafe`, no `unwrap`/`panic` (errors are `compile_error!` with a span).
- Unsupported input is a clear compile error: unions, lifetime parameters, borrowed fields, `usize`/`isize` fields.
- Attribute parsing is hand-written on `syn`'s `parse_nested_meta` (about 150 lines), no `darling`.

### Schema hash

Purpose: a 64-bit identifier of a type's **schema**, written into binary headers and compared on load. The schema is everything that changes how a persisted value must be read: kinds, stable names, field and variant names and order, and field types. It is a *schema* hash, not a pure byte-layout hash: names are hashed, so renaming a field or a type changes it even if the bytes would not. Nobody should assume a rename is layout-neutral. It catches "the schema changed but nobody wrote a migration". It is not a security feature (tamper detection belongs to `exitium_save`).

```rust
pub struct SchemaHash(pub u64);                       // Display: 16 lowercase hex digits
pub const SCHEMA_HASH_VERSION: u8 = 1;
pub fn shallow_hash(info: &TypeInfo) -> Option<SchemaHash>;            // None for anonymous kinds (below)
pub fn deep_hash(root: &TypeInfo) -> Result<SchemaHash, SchemaError>;  // the type plus everything it reaches
pub fn schema_table(root: &TypeInfo) -> Result<SchemaTable, SchemaError>;
```

**Algorithm.** FNV-1a, 64-bit (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`), written by hand (about 10 lines, no dependency, `const`-capable) over a canonical byte stream. `std::hash::DefaultHasher` is not used because its algorithm is unspecified across Rust versions.

**Canonical encoding.** Integers are fixed-width little-endian. A `str` is a `u32` byte length then UTF-8. **Every length and count is a `u32` produced with `u32::try_from`** (overflow is `SchemaError::TooLarge`, never a silent cast); the one exception is a fixed array length `N`, written as `u64` via `u64::try_from`. There is no `usize`, pointer width, `TypeId`, `type_name` or map iteration order anywhere in the input, so the result is identical on every compiler, OS, architecture and on wasm32. Every hashed stream starts with the one-byte `SCHEMA_HASH_VERSION`. A stream that embeds another hash embeds only its 8-byte result, so the version byte is written exactly once per stream (and is implicitly inside every embedded hash, since each of those streams started with it too).

**Named and anonymous types.** A *named* type is a struct, tuple struct or enum declared by a user or by the engine (`Wallet`, `Entity`, `Vec2`; `Num` and `Guid` are transparent, so they are not nodes). Everything else is *anonymous*: `Value`, `Vec<T>`, `[T; N]`, `HashMap`/`BTreeMap`, `Option<T>` and the built-in tuples `(A, B)` (`TupleStructInfo.is_tuple = true`). Anonymous types are identified by their structure alone, never by their composed name. Consequences: `HashMap` and `BTreeMap` hash the same, `Vec<Num>` hashes like `Vec<f64>`, and the closure and the table contain only named types. (User *generic* structs such as `Pair<u8,String>` are named types, and their name embeds the written argument names, so `Pair<Num,u8>` and `Pair<f64,u8>` differ.)

**Transparent resolution.** A `transparent` type is not a node. Wherever a type would be referenced, or hashed as a root, it is replaced by its inner field's type (repeatedly). So a field of type `Num` is a reference to `f64`, and `deep_hash(Num) == deep_hash(f64)`.

**Type reference** `ref(T)` (after transparent resolution) is how one type appears inside another's stream. It is a pure function of `T`'s `TypeInfo` and contains no hash and no walk state:

| `T` | `ref(T)` |
|---|---|
| Value | `0x01`, repr code (`Unit`=0, `Bool`=1, `Char`=2, `I8`=3, `I16`=4, `I32`=5, `I64`=6, `I128`=7, `U8`=8, `U16`=9, `U32`=10, `U64`=11, `U128`=12, `F32`=13, `F64`=14, `String`=15) |
| List | `0x05`, `u8` presence (0 = growable `Vec`, 1 = fixed `[T; N]`), if 1 then `u64` N, then `ref(item)`. Presence makes a zero-length array differ from a `Vec`. |
| Map | `0x06`, `ref(key)`, `ref(value)` |
| Option | `0x07`, `ref(inner)` |
| built-in tuple | `0x08`, `u32` arity, then `ref` of each element in order |
| named struct / tuple struct / enum | kind byte (`Struct`=2, `TupleStruct`=3, `Enum`=4), `str stable name` |

Anonymous refs nest finitely: a cycle must pass through a named type (a `Vec<Tree>` field is `0x05 0 [0x02 "Tree"]`), and a named type ends the nesting.

**Shallow hash** of a named type: FNV-1a over the stream below. Other types appear only as `ref(..)`, so it is a pure function of one `TypeInfo`: no recursion, no cycles, nothing that depends on where a walk started.

| Kind | Stream (version byte first) |
|---|---|
| Struct / TupleStruct | kind byte (2/3), `str name`, `u32` field count, then per field in declaration order: `str field name`, `ref(field type)` |
| Enum | `0x04`, `str name`, `u32` variant count, then per variant in declaration order: `u32` index, `str name`, `u8` kind (0 unit, 1 tuple, 2 struct), `u32` field count, fields as above |

**Deep hash** of a root: 1. Resolve the root through transparency. 2. Compute the **closure**: the set of all *named* types reachable from the root through field, item, key, value, inner and tuple-element references, by a graph walk with a visited set keyed by `TypeId`. Each named type is visited once, so self-recursion, mutual recursion `A -> B -> A` and diamonds all terminate in time linear in the closure size. 3. Sort the closure by stable name (bytewise on UTF-8). Two different `TypeId`s with the same name in one closure is `SchemaError::AmbiguousName` (impossible for types that went through the registry's duplicate-name check). 4. Hash the stream: version byte, `ref(root)`, `u32` closure size, then for each closure member in sorted order `str name` followed by its `u64` shallow hash. The sort makes the result independent of the walk, of registration order and of `HashMap` iteration. For a named root, `ref(root)` carries its name, so the two members of a mutual recursion (identical closures) get different deep hashes.

**Memoization.** Shallow hashes are cached per type (`TypeRegistration.shallow`), deep hashes per root (`deep`), both in `OnceLock`s. Nothing cached depends on a walk, so no cycle-dependent intermediate can poison a cache (the earlier "distance up the stack" idea had that flaw). Deep hashing a diamond-shaped type is O(named types in the closure), not O(paths).

**Worked example** (`struct demo::Pt { x: f32, y: f32 }`):

- shallow stream: `01 02 08000000 "demo::Pt" 02000000 01000000 "x" 010d 01000000 "y" 010d` -> `0x7085092b080ffcd2`
- deep stream: `01 | ref(Pt) = 02 08000000 "demo::Pt" | 01000000 | 08000000 "demo::Pt" d2fc0f082b098570` -> `0x2a7a8a41e74c164b`
- deep hash of the anonymous root `f64` (and so of `Num`): `01 010e 00000000` -> `0xa82cc9124d4432cb`

These three numbers are the first checked-in vectors (computed with the algorithm above).

**What is hashed:** named-type kind and stable name, field names and order, field type references (structurally), enum variant names, order, kinds and fields, list presence and fixed length, `transparent` (by erasure). **What is not:** `range`, `tooltip`, `hidden`, `default`, `custom`, `docs`, `version`, `skip`ped fields, the Rust container (`HashMap` vs `BTreeMap`), text forms, Rust-level details (`TypeId`, module path beyond the stable name, generic parameter names).

Consequences by design: renaming or reordering a field, changing a field type, renaming or reordering a variant, renaming or moving a named type (without `type_name`) and changing array length **change the hash**. Adding a tooltip or range does not.

**Header table and versions.** The hash alone cannot drive migrations: migrations are keyed `(type_name, from_version, to_version)` (#59), old hashes cannot be recomputed after a type changes, and a single root hash cannot say *which* nested type changed. So a binary header (format owned by #24) carries a **schema table** rather than one hash:

```rust
pub struct SchemaTable { pub root: String, pub deep: SchemaHash, pub entries: Vec<SchemaEntry> }
pub struct SchemaEntry { pub name: String, pub version: u32, pub shallow: SchemaHash }  // sorted by name
```

`entries` has one row per named type in the root's closure, in the same sorted order as the deep hash. `root` is the root's registry name (informational; for an anonymous root such as `Vec<Wallet>` it is that name and the deep hash uses the structural `ref`). `version` is the declared `#[reflect(version = N)]`, or `0` when undeclared. `deep` can be recomputed from the table alone (the deep stream needs only `ref(root)` and the rows), so a reader checks the whole file in one comparison and falls back to row-by-row comparison to find what changed.

*Relation between `version` and the hash.* The hash is computed from the schema; the version is **declared by the author and never hashed**. The author raises `version` whenever the type's shallow hash changes. A reader compares each file row to the current registry:

| Version | Shallow hash | Meaning | Reader action |
|---|---|---|---|
| equal | equal | unchanged | decode directly |
| differs | any | declared change | run the registered migration chain `from_version -> current` (#59); no chain: error naming the type and both versions |
| equal | differs | **unversioned schema change** (the bug this hash exists to catch) | reject with an error naming the type, or decode dynamically; policy is #24's. A migration may declare `from_hash` to cover types that never had a version. |

Only the row of the type that changed differs in its shallow hash; types that merely *contain* it keep their rows, so the error points at the changed type. CI enforcement of "hash changed implies version raised" is the `schemas.lock` proposal (open question 5), which records `(name, version, shallow hash)` per persisted type.

**Stability contract and golden vectors.** Vectors are checked in with #36 and are the compatibility contract: changing one is a breaking binary-format change that needs a new `SCHEMA_HASH_VERSION` and an owner-approved PR. They test **three layers**, because testing only the hash function would miss the place native and wasm could diverge (the `TypeInfo` to bytes walk, `usize` casts, ordering):

1. *FNV-1a only:* `""` -> `0xcbf29ce484222325`, `"a"` -> `0xaf63dc4c8601ec8c`, `"foobar"` -> `0x85944171f73967e8`.
2. *Encoder:* `TypeInfo` values built **by hand** (no derive, so the encoder is tested in isolation) with the exact expected **byte stream** (hex) and shallow hash. Required samples: a struct with fields of several kinds, a zero-field struct, a tuple struct, an enum with unit, tuple and struct variants, and `ref` encodings for `Vec<u8>`, `[u8; 0]` (**must differ from `Vec<u8>`**), `[u8; 4]`, `HashMap<String,u32>` (equal to the `BTreeMap` encoding), `Option<f32>`, a built-in tuple, and a field whose type is a transparent wrapper (resolves to the inner type). The `demo::Pt` example above is the first of these.
3. *Deep hash of derived sample types:* struct, enum, nested struct, **self-recursive** (`Tree { kids: Vec<Tree> }`), **mutually recursive** (`A { b: Option<Box<B>> }`, `B { a: Vec<A> }`), diamond (`Top { l: Mid1, r: Mid2 }`, both holding `Leaf`), `Num` (equals `f64`) and `Guid` (equals `u128`). Expected `u64`s are fixed in the test source.

The encoder uses only `u8`/`u32`/`u64`/UTF-8 and `u32::try_from` for every length, so there is nothing width-dependent for wasm32 to disagree on. CI only *builds* wasm; running the golden tests on wasm32 (for example under `wasmtime`) is a recommended follow-up (open question 14).

**Roles** (details in #24, #59):

- *Binary header:* stores the `SchemaTable`. The reader recomputes the table from the registry and applies the version/hash decision table.
- *Text format:* has no hash. It is self-describing by names, so additive changes migrate through `default` and unknown-field reporting.
- *Migrations:* registered by `(type_name, from_version, to_version)` (#59); the hash is the guard that detects an unversioned change.
- *CI guard (proposed, open question 5):* `cargo xtask schema export` dumps `(name, version, shallow hash)` for every registered persisted type and `schema check` compares to a committed `schemas.lock`. A hash change without a version bump shows up as a visible, gate-labelled diff. Not part of #36.

### ECS integration hook

`exitium_ecs` depends on `exitium_reflect` (#50), never the reverse. The reflection side defines only `TypeData`/`FromType`. `ReflectComponent` lives in `exitium_ecs` and is a struct of function pointers built by `FromType<T: Component + Reflect + FromReflect>`.

**Who implements what.** `Component` comes from `exitium_ecs`'s own `#[derive(Component)]` (or a hand `impl`), never from `#[derive(Reflect)]`. `#[reflect(Component)]` (capital `C`, the same spelling as `Default` and `Resource`; the lowercase `#[reflect(component)]` in the ECS draft is a typo to fix there) only registers the `ReflectComponent` type data. `#[reflect(Resource)]` does the same for `ReflectResource`.

```rust
pub struct ReflectComponent {
    contains:    fn(&World, Entity) -> bool,
    get:         fn(&World, Entity) -> Option<&dyn Reflect>,
    get_mut:     fn(&mut World, Entity) -> Option<ReflectedMut<'_>>,                // change-detection guard, see below
    insert:      fn(&mut World, Entity, &dyn Reflect) -> Result<(), ApplyError>,   // FromReflect, or apply if present
    remove:      fn(&mut World, Entity) -> bool,
}
```

**Change detection contract for `get_mut`.** It must not flag the component changed just because it was called: the inspector and MCP read through it, and a flag on every call would raise false `Changed<T>` for UI bindings. It returns a guard with the ECS `Mut<T>` semantics defined in #21: `Deref<Target = dyn Reflect>` is free and stamps nothing; `DerefMut` stores the current tick into the row's `changed` slot and returns `&mut dyn Reflect`; a guard that is never dereferenced mutably flags nothing; `bypass_change_detection()` exists for callers that knowingly do not change data. Whether `ReflectedMut` is `Mut<dyn Reflect>` (requires `Mut<T: ?Sized>`) or a small dedicated type is #21/#50's call, as long as it keeps that contract. `insert` onto a component the entity already has applies through the same guard, so it flags `changed` (not `added`), matching the ECS rule for overwriting an existing component; `insert` of a new component stamps `added = changed`. Test (in #50): `get` and an unused `get_mut` leave `Changed<T>` false; a field set through `get_mut` makes it true.

`World::register_reflected::<T>()` registers the component and calls `registry.register::<T>()` plus `ReflectComponent`. Enumerating an entity's components is: for each component id, find its `TypeId`, look up `ReflectComponent` in the registry, call `get`. The inspector, `exitium_serial` world snapshots (#61) and MCP all use this path, so none of them see ECS storage internals. Storage, component ids and change-tick semantics: see #21. Reflection never touches ECS hot loops: components are stored as concrete `T`, and the derive adds no fields or vtables to them.

### Later consumers (shape only)

- **Luau (`exitium_script`):** userdata `__index`/`__newindex` go through `Struct`/`List`/`Map` and `GetPath`; `.d.luau` type stubs are generated from `TypeRegistry::iter()`; `range`/`hidden` drive the safe-tier API.
- **MCP / schemas:** `TypeInfo` to JSON Schema lives in `exitium_mcp` (reflect stays JSON-free). See next section.

### Implementation slices

| Slice | Issue | Covers |
|---|---|---|
| Model, `Reflect`, registry, primitives | #31 | `TypeInfo`, `Attributes`, `TypeRegistry`, `TypeData`, primitive impls |
| Dynamic access | #32 | kind-trait access by name/index, `GetPath`, `apply`, `Dynamic*` |
| Containers | #33 | `Vec`, `Option`, arrays, tuples, `Box`, maps, `GenericTypeCell` |
| Derive structs | #34 | the derive and attribute parser, `syn`/`quote`/`proc-macro2` |
| Derive enums | #35 | variants, variant switching through `apply` |
| Schema hash | #36 | `SchemaHash`, golden vectors |
| Core impls | #37 | `Num`, `Guid`, `RawHandle`, `Rect`, `Color`, glam, `ReflectTextForm` for `Guid` (no `Handle<T>` impl, open question 13) |
| ECS hook | #50 | `ReflectComponent`, `register_reflected`, `Entity` reflection and its `TextForm` (`"42v1"`) |

### Dependencies (human gate)

Added to `[workspace.dependencies]` once, used only by `exitium_reflect_derive` (#34):

| Crate | Version req | Features | Why |
|---|---|---|---|
| `syn` | `2` | `default-features = false`, `derive`, `parsing`, `printing`, `proc-macro` | Parses struct/enum/generics/attribute syntax with correct spans and error messages. |
| `quote` | `1` | default | Builds the generated impls as readable token templates. |
| `proc-macro2` | `1` | default | Token types for `syn`/`quote`; lets the macro logic be unit-tested outside a proc-macro context. |

All three are `MIT OR Apache-2.0`, already allowed by `deny.toml`. They run only at compile time: they are not linked into games or wasm output. The ecosystem crates this engine will use anyway (`serde_derive`, `mlua`, `wgpu`, `egui`) already pull them in transitively, so the practical new supply-chain surface is near zero. Exact patch versions come from `Cargo.lock` at implementation time. `exitium_reflect` itself takes no external dependency.

Alternatives rejected: raw `proc_macro` token parsing (zero deps but roughly 1500 lines of fragile parsing, poor error spans, harder to review); `macro_rules!` (cannot read struct attributes or handle generics/enums reliably); `darling`/`venial`/`proc-macro-crate` (extra dependencies for savings of about 150 lines); adopting `bevy_reflect`/`facet` (large dependency trees, outside control of our hash stability and `Num`/`Guid` data model).

## MCP / editor surface

Agent parity (design rule 1): reflection is read-only metadata plus mutation primitives. Mutation reaches users only through command-bus commands (design rule 3). Reflection is also what lets those commands be typed without a hand-written schema (design rule 2). No `exitium_reflect` code ships an editor-only feature.

Tool names are snake_case `<domain>_<verb>` (dots are not valid in many MCP clients). This spec owns only the `reflect_*` tools. **Entity, component and resource access is owned by #21** (`ecs_get_entity`, `ecs_get_component`, `ecs_query_entities`, and the `SpawnEntity`/`InsertComponent`/`RemoveComponent`/`SetField { entity, type_name, path, value }` commands with their undo records); this spec defines none of them and does not re-state their payloads. It only guarantees the primitives they are built on.

| Capability | MCP tool (in `exitium_mcp`) | Editor | Built from |
|---|---|---|---|
| List registered types (name, kind, `SchemaHash`, version, docs) | `reflect_list_types` | type picker | `TypeRegistry::iter` |
| JSON Schema of one type, or all types (offline dump, PLAN phase 4) | `reflect_get_type`, `reflect_dump_schema` | inspector layout | `TypeInfo` + `Attributes` |
| Schema hash and header table of a type, for migration warnings | `reflect_get_schema` | migration warnings | `deep_hash`, `schema_table` |

Primitives provided for #21's tools (named so the parity test can map them): `ReflectComponent::get`/`get_mut`/`insert`/`remove`, `GetPath::reflect_path`/`reflect_path_mut`, `Reflect::apply`/`clone_boxed` (undo snapshot), `ReflectDefault` (add a component with defaults), `ReflectTextForm` and the text format (#23) for values in and out.

Tool input and output schemas are generated from `TypeInfo`, never hand-written. Values cross MCP in the text format (#23). The `exitium_mcp` parity test (PLAN phase 4 item 6) must see an MCP equivalent for every inspector command. Inspector widgets read `range`, `hidden`, `tooltip`/`docs` and `custom("widget")`. `hidden` fields are not shown in the inspector but remain visible and editable through MCP (marked `x-exitium-hidden`). This spec adds no tools itself; the tool list above is the contract the Phase 4 issues implement.

## Performance

Reflection is not on the ECS iteration hot path (components stay concrete `T`), but serialization of 1M-entity worlds and editor/MCP edits go through it. Budgets are p50 single-thread on the min-spec reference runner; benches land in `exitium_reflect/benches/` (criterion per #18) and are held by the 5% regression gate once baselined. Targets are initial and may be re-baselined by the first benchmark PR with a note to the owner.

| Operation | Budget |
|---|---|
| `T::type_info()` after first use (non-generic) | <= 5 ns |
| `T::type_info()` generic, cell hit | <= 40 ns |
| Registry lookup by `TypeId` | <= 25 ns |
| Registry lookup by stable name (<= 40 chars) | <= 60 ns |
| Register one type (incl. type data) | <= 5 µs; 1000 types <= 5 ms |
| `field_at(i)` via `dyn Struct` | <= 10 ns |
| `field(name)`, struct with <= 16 fields | <= 40 ns |
| `reflect_parsed` path, 3 segments | <= 80 ns |
| `reflect_path` from `&str`, 3 segments (parse + walk) | <= 250 ns |
| `ParsedPath::parse`, 3 segments | <= 150 ns |
| `apply` onto a struct of 8 primitive fields | <= 150 ns |
| `clone_boxed` of a struct of 8 primitive fields | <= 80 ns |
| Walk all leaves of a reflected value | >= 50 M leaves/s (<= 20 ns/leaf), so 1M entities x 5 leaves has <= 100 ms reflection overhead |
| `shallow_hash` of a struct with 16 fields (uncached) | <= 3 µs |
| `deep_hash` of a root whose closure is 100 named types x 8 fields (uncached, shallow hashes cold) | <= 400 µs; with shallow hashes cached <= 50 µs; cached read <= 5 ns |
| `GenericTypeCell` miss (build `Vec<Vec<f32>>`, one-time) | <= 5 µs |
| Take one registry read guard (`RwLock`, uncontended) | <= 30 ns |

Memory, derived from the specced layout on a 64-bit target: `FieldInfo` is about 144 B (name 16, index 8, `TypeRef` 24 with a 16-byte `TypeId` and a fn pointer, docs 16, `Attributes` about 80 of which `Option<Range>` is 32, tooltip 16, default 8, custom 16). Budget: `TypeInfo` <= 192 B per type plus <= 160 B per field or variant field; registry bookkeeping <= 512 B per registered type, <= 1 KiB including its type data. So 2000 types averaging 8 fields <= 5 MiB. If the first bench PR shows this is too heavy, `range` and `custom` can move behind an `Option<Box<..>>` inside `Attributes` (an internal change, the public accessors stay the same); the budget is re-baselined, not worked around. Compile time: derive expansion <= 2 ms per type; a clean build of the derive crate with `syn`/`quote`/`proc-macro2` is a one-time cost, not rebuilt per game build with a warm cache. No `unsafe`, so no Miri module. Complexity: lookups O(1) average (registry), O(f) for field-by-name with f fields, O(s) for a path of s segments.

## Test plan

- **Unit tests per kind** (struct, tuple struct, enum with all three variant kinds, list, array, map, option, value): metadata queries, dynamic get/set, error cases for every access method.
- **Registry:** register/lookup by `TypeId` and name, idempotent re-register, duplicate name error (either registration order), dependency registration, type data, invalid attribute rejected (`range` on `String`, `Option<f32>`; accepted on `Num`), `InvalidMapKey` for float and struct keys, accepted for `HashMap<ResourceKind, Num>` with a unit-only enum key, `BTreeMap<Guid, _>` and `HashMap<Entity, _>`; registering in two orders gives identical hashes and tables.
- **Threading:** `GenericTypeCell` first use of `Vec<Vec<f32>>`, `Pair<Pair<u8,u8>,u8>` and `Option<Option<u32>>` from 8 threads: no deadlock, equal `&'static` references, only one leaked value per instantiation (counted); `TypeRegistry`, `TypeInfo`, `ReflectComponent` and every `TypeData` in the crate are `Send + Sync` (compile-time assertion); concurrent readers with one writer registering a late type.
- **Method names:** a test with `Reflect` and `Typed` both imported calls `Wallet::type_info()` and `wallet.reflect_type_info()` and compiles (guards against the E0034 ambiguity coming back).
- **Paths:** every grammar production, error `segment_index`, enum and option traversal, a randomized no-panic test on arbitrary strings (fixed seed, std only).
- **`apply`/`clone_boxed`/`reflect_eq`:** per row of the apply table, including variant switching and map key removal; round-trip "build, mutate dynamically, compare".
- **Derive:** nested structs, generics (`Pair<A, B>`), all attributes, `skip`+`default` through `from_reflect`, `transparent`, recursive type via `Box`, `crate = ...` and `ecs_crate = ...` overrides, `#[reflect(Default, TextForm)]` compiling in a crate that imports nothing, `#[reflect(Component)]` on a type that is also `derive(Component)` (test in `exitium_ecs`), and the same attribute on a type that is *not* a `Component` failing with a trait-bound error. Crate-path resolution: engine crate names resolve to `::exitium_reflect`, any other name (including `exitium_mygame`) to `::exitium::reflect`, and the `crate_paths.rs` drift test over `crates/`. `compile_fail` doc tests (no `trybuild`) cover unknown attribute, `type_name` without `::` and unsupported input.
- **Text form:** `Guid` `to_text` equals `Display` and round-trips; `Entity` is `"42v1"` and `"0v0"`/missing generation is an error; map keys of these types; binary output ignores the hook.
- **Manual-impl parity:** `tests/manual_impl.rs` hand-implements the sample types and asserts identical `TypeInfo` and behavior to the derive.
- **Schema hash** (three vector layers, see "Schema hash"): FNV-1a standard vectors; hand-built `TypeInfo` to exact byte stream to shallow hash (`demo::Pt` first); derived sample types to deep hash. Properties: changing a field name, order, type, variant name/order or array length changes the hash; adding `tooltip`/`range`/`default`/`hidden`/`version` does not; `HashMap` vs `BTreeMap` equal; `Vec<Num>` equals `Vec<f64>`; `[u8; 0]` differs from `Vec<u8>`; `transparent` equals inner; `Num` equals `f64` and `Guid` equals `u128`; every length written with `u32::try_from` (a test feeds a >`u32::MAX` length through the encoder's length helper and expects `TooLarge`). **Cycles:** for self recursion and for mutual recursion `A -> B -> A`, the shallow hash of each member is identical whichever root started the walk; `deep(A)` and `deep(B)` are each stable, differ from each other, and do not change if `deep(B)` is computed before or after `deep(A)` or after registering in the other order (no memoization poisoning); a diamond of depth 20 hashes in time linear in node count (guards the exponential case).
- **Header table:** `schema_table` for a nested type lists exactly its named closure sorted by name with declared versions (`0` when undeclared); a reader-side comparison classifies equal, version-differs and hash-differs-at-same-version correctly; changing one nested type changes only that row's `shallow`.
- **Platform:** `cargo build --workspace --target wasm32-unknown-unknown` passes; no `unsafe`.
- **`strip_docs`:** with the feature, `docs` of a derived type and its fields are empty; without, they equal the `///` text (two test runs in CI). The editor and MCP feature-graph check passes.
- **Benchmarks:** one bench per row of the performance table.

## Open questions

1. **Require `Clone` for all reflected types?** Recommendation: yes in v1 (undo, prefabs and the command bus all need exact copies). A `no_clone` opt-out with structural snapshots can come later if a real type needs it.
2. **Stable names default to `module_path!()`.** Moves/renames then change the hash. Recommendation: keep the default, require `#[reflect(type_name)]` on persisted types, enforced by the `schemas.lock` check (question 5).
3. **`#[reflect(version = N)]`:** it is stored in `TypeMeta`, written to the binary header table next to each type's shallow hash, and never hashed (see "Schema hash"). Keep it in reflect, or let `exitium_serial`/`exitium_save` own versions entirely (#24, #59)? Recommendation: keep it here (the derive is where type authors already look; the hash/version decision table is defined here), with reader and migration policy owned by #24 and #59.
4. **Hash width/algorithm:** 64-bit FNV-1a by hand (this spec) vs xxhash64 (hand-written, longer) vs a 128-bit hash. 64 bits is ample for ~10^3 types and non-adversarial use; `SCHEMA_HASH_VERSION` allows an upgrade. Recommendation: FNV-1a-64.
5. **`schemas.lock` and `cargo xtask schema check`** as a CI guard against unversioned layout changes? It adds a committed file (a gated serialization artifact) and an xtask command. Recommendation: yes, as a follow-up issue after #36.
6. **Atomic `apply`.** Current design is non-atomic plus a snapshot taken by the caller. A validate-then-mutate `apply` costs a second walk. Recommendation: defer unless the command bus finds the snapshot too costly.
7. **Docs in shipping builds.** `strip_docs` (on `exitium_reflect`, enabled only through the facade feature for a game's release build) empties `docs` strings. Should shipping builds default to stripped? Recommendation: yes, via `cargo xtask package`, and keep docs in editor/MCP builds.
8. **No `usize`/`isize`, and `i128`/`u128` included** (for `Guid`). Acceptable? Recommendation: yes, explicit widths keep the binary format platform-independent.
9. **Runtime-defined types** (a data asset schema authored in text with no Rust struct) need a non-`'static` `TypeInfo` or a leak-on-load scheme. Deferred to the `exitium_data` spec; this model does not preclude it because `TypeRef` is already indirect.
10. **Sets, `VecDeque`, `Duration`.** Out of v1 (see Non-goals). `HashSet`/`BTreeSet` fit a `Set` kind or a list-with-uniqueness, `VecDeque` is a `List` impl, `Duration` is a struct `{secs: u64, nanos: u32}`. Recommendation: add them in a follow-up slice once a real game type needs one, with the owner filing the issue; nothing here precludes them. Related, **zero-copy bulk list access** (an `as_bytes` hook for `Vec<f32>` and similar). The latter is the binary-format spec's call (#24); reflect currently exposes `ListInfo.item` so a serializer can special-case with a downcast.
11. **Enum variant stable ids.** Variant `index` is declaration order, so reordering variants is a breaking change (hashed). Should `#[reflect(index = N)]` pin ids so reordering is safe? Recommendation: not in v1; revisit with the first real save-migration need.
12. **`trybuild` compile-fail tests.** A dev-dependency (gated). Recommendation: stay with `compile_fail` doc tests until the owner asks.
13. **Typed asset references and `Handle<T>`.** `Handle<T>` (#15) is a process-local arena id and gets no `Reflect` impl in v1; only `RawHandle` reflects. Persisted asset references need a stable form. Default: `AssetRef<T>` in `exitium_assets`, a `Guid`-backed wrapper reflected as a transparent `Guid`, whose stable name comes from an `AssetKind::NAME` constant on the target so `T` needs no `Reflect` bound. Alternative: reflect `Handle<T>` as `Guid` after an asset-table lookup at serialize time, which needs a context argument the serializer does not have. Recommendation: the default, decided in the assets spec. #37's `Handle<T>/RawHandle` criterion is met by `RawHandle`.
14. **Run golden vectors on wasm32 in CI.** The encoder is width-independent by construction, but CI only builds wasm. Recommendation: a follow-up CI step running the schema tests under `wasmtime` (`wasm32-wasip1`). It adds a CI tool, so it is an owner decision.
15. **`ReflectedMut` shape.** `Mut<dyn Reflect>` (needs `Mut<T: ?Sized>` in #21) versus a dedicated guard type. Either satisfies the change-detection contract; #21/#50 pick whichever keeps `Mut<T>` hot-path code simplest.
