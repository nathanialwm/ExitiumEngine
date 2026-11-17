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
- Sets (`HashSet`, `BTreeSet`), `usize`/`isize`, trait-object fields, borrowed fields (`&'a T`). None are reflectable in v1.
- Compile-fail tests with `trybuild` (a dependency; #34 defers it).

## Design

### Crate layout and dependency direction

```
exitium_core  <-  exitium_reflect  <-  exitium_ecs, exitium_serial, exitium_data, exitium_script, exitium_mcp, exitium_editor
                       ^
              exitium_reflect_derive (proc-macro, re-exported by exitium_reflect)
```

- `exitium_reflect` depends on `exitium_core` only (#37: `reflect -> core`). Core types (`Num`, `Guid`, `Handle<T>`, glam math) get their `Reflect` impls inside `exitium_reflect`, which satisfies the orphan rule because the trait is local.
- `exitium_reflect` has **no external dependencies**. It does not use `serde`; serializers walk reflection directly.
- `exitium_reflect_derive` is the only crate using `syn`, `quote`, `proc-macro2` (see Dependencies).
- Everything is safe Rust (no `unsafe`, no Miri module). Downcasting uses `std::any::Any`; leaked `TypeInfo` for generics uses `Box::leak`.

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
    pub docs: &'static str,         // joined `///` lines; empty with the `strip_docs` feature
    pub version: Option<u32>,       // #[reflect(version = N)]: stored, NOT hashed (open question 3)
    pub attrs: Attributes,
    pub is_dynamic: bool,           // true only for the Dynamic* types
}

pub struct StructInfo      { pub meta: TypeMeta, pub fields: Box<[FieldInfo]>, pub transparent: bool }
pub struct TupleStructInfo { pub meta: TypeMeta, pub fields: Box<[FieldInfo]>, pub transparent: bool } // field names "0", "1", ...
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
- **`transparent`** (`#[reflect(transparent)]`, only legal on a single-field struct): the type is layout-identical to its field. `Num(f64)`, `Guid(u128)`, `Entity(u64)` and `Box<T>` use it. Consumers treat it as its inner field (the inspector shows one number, text format writes `3.5` not `Num(3.5)`). Its schema hash equals its inner field's hash.
- **Tuples** `(A, B)` up to arity 8 are `TupleStruct` kind with the synthetic name `(A,B)`. No ninth kind.
- **`Box<T>`** is a transparent `TupleStruct` named `Box<T>`.
- **Value** covers only primitives and `String`. Domain types such as `Num`, `Guid`, `Handle<T>`, `Rect`, `Color`, and glam types are `transparent` wrappers or ordinary structs (`Vec2` is a struct `{x, y}`), never opaque. Consequence: every reflected value bottoms out in the 16 `ValueRepr` leaves, so serializers need exactly 16 leaf encoders and no per-type codecs.
- `usize`/`isize` have no `Reflect` impl (width differs between wasm32 and 64-bit; it would break hash and binary stability). Use explicit widths.
- Field lookup by name is a linear scan of the `fields` slice. Components have few fields; a scan of 16 short names beats hashing.

### Stable type names

`std::any::type_name` is explicitly unspecified across compiler versions, so it is never used. Instead:

| Type | Stable name |
|---|---|
| primitives, `String` | `u32`, `f64`, `bool`, `String`, `()` |
| user type | `module_path!()` + `::` + identifier, e.g. `glutton::economy::Wallet`; override with `#[reflect(type_name = "glutton::Wallet")]` |
| generics | base name + stable names of the arguments, no spaces: `Vec<f32>`, `Option<glutton::Wallet>`, `HashMap<String,u32>`, `[f32;4]`, `(f32,f32)`, `Box<T>`, `glutton::Pair<u8,String>` |

Moving or renaming a type changes its name, and the name is part of the hash, so it is treated as a schema change. Types that are persisted should pin their name with `type_name`. Generic names are composed at runtime in the `GenericTypeCell` (not in `const`), then cached.

### Reflect trait and kind traits (API sketch)

```rust
/// Everything reflected implements this. Components and resources are `Send + Sync` already.
pub trait Reflect: Any + Send + Sync {
    fn type_info(&self) -> &'static TypeInfo;
    /// Concrete types: `Some(own info)`. Dynamic* types: the type they were built to represent, if known.
    fn represented_type(&self) -> Option<&'static TypeInfo>;

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

/// Static side: `T::type_info()` needs no instance.
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
- **Map keys** are limited to `Value` kinds and `transparent` wrappers of them (ints, `bool`, `char`, `String`, `Entity`, `Guid`). Lookup downcasts the key to `K`, so no reflected hashing is needed.
- **`Dynamic*` types** (`DynamicStruct`, `DynamicTupleStruct`, `DynamicEnum`, `DynamicList`, `DynamicMap`, `DynamicOption`) implement `Reflect` and the matching kind trait with `is_dynamic = true`. They are what the text and binary readers produce when a value has no concrete type yet or does not match the current schema (unknown fields are kept by name). `DynamicValue` in the migration API (#59) is `Box<dyn Reflect>` holding one of these or a primitive. A reader sets `represented_type` from the registry when it knows the target type.

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
| `range(min = a, max = b [, step = s])` | field | Numeric bounds, inspector slider, JSON Schema `minimum`/`maximum`. Applied to a non-numeric `Value` field it is rejected at registration (`RegistryError::InvalidAttribute`). | no |
| `tooltip = "text"` | type, field, variant | Human text. Falls back to `///` docs when absent. | no |
| `hidden` | field, variant | Hide from the **inspector** only. Still serialized, still in MCP schemas (marked hidden) so agents keep parity. | no |
| `skip` | field | Not reflected, not serialized, not in `TypeInfo` or the hash. Rebuilt with `Default::default()` or its `default` expression in `from_reflect`. | no (absent) |
| `default` / `default = "path::to_fn"` | field | Value used when the field is missing in dynamic/serialized data. Stored as `fn() -> Box<dyn Reflect>` so schemas can show defaults. | no |
| `custom(key = literal, ...)` | type, field, variant | Extensible key/value store (`bool`, integer, float, string), e.g. `custom(widget = "color")`. | no |
| `type_name = "..."` | type | Override the stable name. | yes (name) |
| `transparent` | single-field struct | See "TypeInfo". | yes |
| `version = N` | type | Declared type version for migrations. Stored only. | no |
| `crate = path` | type | Override where generated code finds `exitium_reflect`. | no |
| `Default`, `Component`, ... | type | Register type data `ReflectDefault`, `ReflectComponent`, ... (see Registry). | no |

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

With the `strip_docs` Cargo feature (off by default, on for shipping builds) `docs` strings are empty to keep wasm small.

### Type registry

```rust
pub struct TypeRegistry {
    by_id:   HashMap<TypeId, TypeRegistration>,
    by_name: HashMap<&'static str, TypeId>,
}
pub struct TypeRegistration {
    info: &'static TypeInfo,
    data: Vec<(TypeId, Box<dyn TypeData>)>,   // small; linear scan
    hash: OnceLock<SchemaHash>,               // computed on first request, then cached
}

impl TypeRegistry {
    pub fn register<T: GetTypeRegistration>(&mut self) -> Result<(), RegistryError>;
    pub fn register_type_data<T: Reflect, D: TypeData + FromType<T>>(&mut self);
    pub fn get(&self, id: TypeId) -> Option<&TypeRegistration>;
    pub fn get_by_name(&self, stable_name: &str) -> Option<&TypeRegistration>;
    pub fn type_data<D: TypeData>(&self, id: TypeId) -> Option<&D>;
    pub fn schema_hash(&self, id: TypeId) -> Option<SchemaHash>;
    pub fn iter(&self) -> impl Iterator<Item = &TypeRegistration>;
}
```

- **Registration is explicit** (`registry.register::<Wallet>()`, normally via `app.register_type::<Wallet>()` in `exitium_app`). No `inventory`/`ctor` link-time auto-registration: it fails on wasm, depends on link order and is hidden magic.
- `register::<T>` also registers every type reachable from `T`'s fields and items (`GetTypeRegistration::register_type_dependencies`), so `Vec<Wallet>` registers `Wallet`.
- Same `TypeId` twice is a no-op. A different `TypeId` with an already-used stable name is `RegistryError::DuplicateName { name }` (#31). Registration stops at the first error; earlier registrations stay.
- **Type data** is how other crates attach behavior to a type without `exitium_reflect` knowing them: `TypeData` is a `Send + Sync + 'static` trait, `FromType<T>` builds it. `#[reflect(Default)]` in the derive is sugar for `register_type_data::<T, ReflectDefault>()`; the `ReflectDefault` ident must be in scope at the derive site.

  | Type data | Defined in | Purpose |
  |---|---|---|
  | `ReflectFromReflect` | `exitium_reflect`, always registered by the derive | build `T` from a dynamic value |
  | `ReflectDefault` | `exitium_reflect`, on `#[reflect(Default)]` | `fn() -> Box<dyn Reflect>` |
  | `ReflectComponent` | `exitium_ecs` (#50), on `#[reflect(Component)]` | see "ECS integration" |
  | `ReflectResource` | `exitium_ecs`, on `#[reflect(Resource)]` | same for resources |

- **Generics and containers:** the registry is keyed by concrete instantiation (`Vec<f32>` and `Vec<u8>` are separate entries). Derived generic types need `T: Reflectable`, a blanket-implemented alias for `Reflect + Typed + FromReflect + GetTypeRegistration`. A static in a generic function is shared by all instantiations, so generic `TypeInfo` is cached in `GenericTypeCell`: `RwLock<HashMap<TypeId, &'static TypeInfo>>`, write-once per instantiation, entries `Box::leak`ed (bounded by the number of distinct instantiations in the program).
- Built-in impls (#31, #33): `bool`, `char`, all fixed-width ints incl. `i128`/`u128`, `f32`, `f64`, `String`, `()`, `Vec<T>`, `[T; N]`, `Option<T>`, `Box<T>`, `HashMap<K,V>`, `BTreeMap<K,V>`, tuples up to arity 8 (small private `macro_rules!`, not public API). Core types come with #37.

### Derive macro

`#[derive(Reflect)]` on a struct or enum generates plain `impl` blocks, nothing else:

1. `Reflect` and the matching kind trait (`Struct`, `TupleStruct` or `Enum`)
2. `Typed` (the `TypeInfo`, behind a `OnceLock`, or `GenericTypeCell` if generic)
3. `FromReflect`
4. `GetTypeRegistration` (registers the type and its field types, plus any `#[reflect(Default, ...)]` type data)

**Path to `exitium_reflect`.** Games depend only on the `exitium` facade. Generated code uses `::exitium_reflect` when the compiling crate's `CARGO_PKG_NAME` starts with `exitium_`, otherwise `::exitium::reflect` (the facade re-exports `exitium_reflect as reflect`). `#[reflect(crate = path)]` overrides it. `exitium_reflect` itself declares `extern crate self as exitium_reflect;`. This needs no manifest-parsing dependency.

**Macro-magic limits (CLAUDE.md rule 6).** The derive must stay readable to a basic-level Rust reviewer:

- It only parses a struct/enum, its generics, and `#[reflect(...)]` / `#[doc]` attributes. No function-like or attribute proc macros, no `macro_rules!` DSL in public API.
- It emits only the four impl groups above, no new public items, hidden fields or helper modules. All paths are fully qualified. No global state.
- Each generated impl method is a direct match/field access. A hand-written equivalent for a sample struct and enum lives in `exitium_reflect/tests/manual_impl.rs`, with a test that it behaves like the derive output. The derive module docs show the same example.
- Derive crate: at most 800 lines excluding tests, no `unsafe`, no `unwrap`/`panic` (errors are `compile_error!` with a span).
- Unsupported input is a clear compile error: unions, lifetime parameters, borrowed fields, `usize`/`isize` fields.
- Attribute parsing is hand-written on `syn`'s `parse_nested_meta` (about 150 lines), no `darling`.

### Schema hash

Purpose: a 64-bit identifier of a type's **binary layout**, written into binary headers and compared on load. It catches "layout changed but nobody wrote a migration". It is not a security feature (tamper detection belongs to `exitium_save`).

```rust
pub struct SchemaHash(pub u64);                       // Display: 16 lowercase hex digits
pub const SCHEMA_HASH_VERSION: u8 = 1;
pub fn schema_hash(info: &TypeInfo) -> SchemaHash;    // exitium_reflect::schema
```

**Algorithm.** FNV-1a, 64-bit (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`), written by hand (about 10 lines, no dependency, `const`-capable) over a canonical byte stream. `std::hash::DefaultHasher` is not used because its algorithm is unspecified across Rust versions.

**Canonical encoding.** Integers are fixed-width little-endian. Strings are a `u32` byte length then UTF-8. There is no `usize`, pointer width, `TypeId`, `type_name` or map iteration order anywhere in the input, so the result is identical on every compiler, OS, architecture and on wasm32. The stream starts with `SCHEMA_HASH_VERSION`.

| Type | Bytes hashed (after the tag) |
|---|---|
| Value | tag `0x01`, repr code (`Unit`=0, `Bool`=1, `Char`=2, `I8`..`I128`=3..7, `U8`..`U128`=8..12, `F32`=13, `F64`=14, `String`=15) |
| Struct / TupleStruct | tag `0x02`/`0x03`, stable name, field count, then per field in declaration order: field name, **hash of the field's type** (8 bytes) |
| Enum | tag `0x04`, stable name, variant count, then per variant in declaration order: index (`u32`), name, kind (0 unit, 1 tuple, 2 struct), field count, fields as above |
| List | tag `0x05`, item hash, fixed length (`u64`, `0` = growable) |
| Map | tag `0x06`, key hash, value hash |
| Option | tag `0x07`, inner hash |
| transparent wrapper | no tag: the hash of its single field (so `Num` hashes like `f64`, `Box<T>` like `T`) |

**Cycles.** Types can refer to themselves (`Tree { kids: Vec<Tree> }`). The walk keeps a stack of types in progress; reaching one again emits tag `0xFF` plus the distance back up the stack (`u32`) instead of recursing. Distance, not name, keeps the value independent of where the walk started. Only the root result is cached (in the registry), so no cycle-dependent intermediate is memoized.

**What is hashed:** type kind and stable name, field names and order, field types (recursively), enum variant names, order, kinds and fields, list fixed length, `transparent`.
**What is not:** `range`, `tooltip`, `hidden`, `default`, `custom`, `docs`, `version`, `skip`ped fields, the Rust container used (`HashMap` and `BTreeMap` hash the same; serializers must order keys deterministically), Rust-level details (`TypeId`, module path beyond the stable name, generic parameter names).

Consequences by design: renaming or reordering a field, changing a field type, renaming or reordering a variant, renaming/moving a type (without `type_name`), and changing array length **change the hash**. Adding a tooltip or range does not.

**Stability contract.** Golden vectors (fixed byte streams and their expected `u64`) are checked in with #36. They are the compatibility contract: changing one is a breaking binary-format change and needs a new `SCHEMA_HASH_VERSION` and an owner-approved PR. The vectors are fixed byte arrays, so the same tests prove native/wasm32 equality without running on wasm.

**Roles** (details in #24, #59):

- *Binary header:* stores the root type's stable name and `SchemaHash`; the reader recomputes the hash from the registry and compares. On mismatch it rejects, runs a registered migration chain, or falls back to dynamic decode (policy is #24's).
- *Text format:* has no hash. It is self-describing by names, so additive changes migrate through `default` and unknown-field reporting.
- *Migrations:* the registry gives `(stable name, SchemaHash, declared version)`. Migrations are registered by `(type_name, from_version, to_version)` (#59). The hash is the guard that detects an unversioned change.
- *CI guard (proposed, open question 5):* `cargo xtask schema export` dumps `(name, hash)` for every registered persisted type and `schema check` compares to a committed `schemas.lock`. Layout changes then show up as a visible, gate-labelled diff. Not part of #36.

### ECS integration hook

`exitium_ecs` depends on `exitium_reflect` (#50), never the reverse. The reflection side defines only `TypeData`/`FromType`. `ReflectComponent` lives in `exitium_ecs` and is a struct of function pointers built by `FromType<T: Component + Reflect + FromReflect>`:

```rust
pub struct ReflectComponent {
    contains:    fn(&World, Entity) -> bool,
    get:         fn(&World, Entity) -> Option<&dyn Reflect>,
    get_mut:     fn(&mut World, Entity) -> Option<&mut dyn Reflect>,   // marks the component changed (see #21)
    insert:      fn(&mut World, Entity, &dyn Reflect) -> Result<(), ApplyError>, // FromReflect, or apply if present
    remove:      fn(&mut World, Entity) -> bool,
}
```

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
| Core impls | #37 | `Num`, `Guid`, `Handle<T>`, glam |
| ECS hook | #50 | `ReflectComponent`, `register_reflected` |

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

| Capability | MCP tool (in `exitium_mcp`) | Editor | Built from |
|---|---|---|---|
| List registered types (name, kind, `SchemaHash`, version, docs) | `reflect.list_types` | type picker | `TypeRegistry::iter` |
| JSON Schema for one type or all (offline) | `reflect.get_type_schema`, `schema_dump` (PLAN phase 4) | inspector layout | `TypeInfo` + `Attributes` |
| Read a component or resource field | `entity.get_field` (entity, type name, path) | inspector read | `ReflectComponent::get` + `reflect_path` |
| Set a field | `entity.set_field` (command `SetField { target, type_name, path, value }`) | inspector edit, same command | `reflect_path_mut` / `apply`, undo from `clone_boxed` |
| Add / remove a component | `entity.add_component`, `entity.remove_component` | inspector "+" / delete | `ReflectComponent::insert/remove` + `ReflectDefault` |
| Compare schema versions | `reflect.get_schema_hash` | migration warnings | `schema_hash` |

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
| `schema_hash` of a 100-field nested type (uncached) | <= 20 µs; cached read <= 5 ns |

Memory: `TypeInfo` <= 160 B per type plus <= 96 B per field; registry <= 1 KiB per registered type (so 2000 types <= 2 MiB). Compile time: derive expansion <= 2 ms per type; a clean build of the derive crate with `syn`/`quote`/`proc-macro2` is a one-time cost, not rebuilt per game build with a warm cache. No `unsafe`, so no Miri module. Complexity: lookups O(1) average (registry), O(f) for field-by-name with f fields, O(s) for a path of s segments.

## Test plan

- **Unit tests per kind** (struct, tuple struct, enum with all three variant kinds, list, array, map, option, value): metadata queries, dynamic get/set, error cases for every access method.
- **Registry:** register/lookup by `TypeId` and name, idempotent re-register, duplicate name error, dependency registration, type data, invalid attribute rejected.
- **Paths:** every grammar production, error `segment_index`, enum and option traversal, a randomized no-panic test on arbitrary strings (fixed seed, std only).
- **`apply`/`clone_boxed`/`reflect_eq`:** per row of the apply table, including variant switching and map key removal; round-trip "build, mutate dynamically, compare".
- **Derive:** nested structs, generics (`Pair<A, B>`), all attributes, `skip`+`default` through `from_reflect`, `transparent`, recursive type via `Box`, `crate = ...` path override. `compile_fail` doc tests (no `trybuild`) cover unknown attribute and unsupported input.
- **Manual-impl parity:** `tests/manual_impl.rs` hand-implements the sample types and asserts identical `TypeInfo` and behavior to the derive.
- **Schema hash:** golden vectors (bytes to `u64`); changing a field name, order, type, variant name/order or array length changes the hash; adding `tooltip`/`range`/`default`/`hidden` does not; `HashMap` vs `BTreeMap` equal; `transparent` equals inner; cyclic type hashes stably and independent of walk start; `Num` hashes as `f64`.
- **Platform:** `cargo build --workspace --target wasm32-unknown-unknown` passes; no `unsafe`.
- **Benchmarks:** one bench per row of the performance table.

## Open questions

1. **Require `Clone` for all reflected types?** Recommendation: yes in v1 (undo, prefabs and the command bus all need exact copies). A `no_clone` opt-out with structural snapshots can come later if a real type needs it.
2. **Stable names default to `module_path!()`.** Moves/renames then change the hash. Recommendation: keep the default, require `#[reflect(type_name)]` on persisted types, enforced by the `schemas.lock` check (question 5).
3. **`#[reflect(version = N)]`:** keep it in reflect as stored metadata, or let `exitium_serial`/`exitium_save` own versions entirely (#24, #59)? Recommendation: keep it here (the derive is where type authors already look), semantics owned by #24.
4. **Hash width/algorithm:** 64-bit FNV-1a by hand (this spec) vs xxhash64 (hand-written, longer) vs a 128-bit hash. 64 bits is ample for ~10^3 types and non-adversarial use; `SCHEMA_HASH_VERSION` allows an upgrade. Recommendation: FNV-1a-64.
5. **`schemas.lock` and `cargo xtask schema check`** as a CI guard against unversioned layout changes? It adds a committed file (a gated serialization artifact) and an xtask command. Recommendation: yes, as a follow-up issue after #36.
6. **Atomic `apply`.** Current design is non-atomic plus a snapshot taken by the caller. A validate-then-mutate `apply` costs a second walk. Recommendation: defer unless the command bus finds the snapshot too costly.
7. **Docs in shipping builds.** `strip_docs` feature empties `docs` strings. Should shipping builds of games default to stripped? Recommendation: yes, and keep docs in editor/MCP builds.
8. **No `usize`/`isize`, and `i128`/`u128` included** (for `Guid`). Acceptable? Recommendation: yes, explicit widths keep the binary format platform-independent.
9. **Runtime-defined types** (a data asset schema authored in text with no Rust struct) need a non-`'static` `TypeInfo` or a leak-on-load scheme. Deferred to the `exitium_data` spec; this model does not preclude it because `TypeRef` is already indirect.
10. **Sets** (`HashSet`/`BTreeSet`) and **zero-copy bulk list access** (an `as_bytes` hook for `Vec<f32>` and similar). The latter is the binary-format spec's call (#24); reflect currently exposes `ListInfo.item` so a serializer can special-case with a downcast.
11. **Enum variant stable ids.** Variant `index` is declaration order, so reordering variants is a breaking change (hashed). Should `#[reflect(index = N)]` pin ids so reordering is safe? Recommendation: not in v1; revisit with the first real save-migration need.
12. **`trybuild` compile-fail tests.** A dev-dependency (gated). Recommendation: stay with `compile_fail` doc tests until the owner asks.
