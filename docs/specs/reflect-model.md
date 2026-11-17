# Spec: Reflection model, type registry, derive macro and schema hash

- Issue: #20
- Crates: `exitium_reflect`, `exitium_reflect_derive` (touchpoints: `exitium_core`, `exitium_ecs`, `exitium_serial`, `exitium_mcp`, `exitium_script`, `exitium_editor`)
- Status: draft
- Amended by #78: validated `Num` as a Value kind with a leaf accessor, hand-written `Entity` and the engine-crate name rule, `NonZeroU32` generations, glam as a core dependency with SIMD-backed types, header-table and migration decode rules, text-form MCP schemas and non-string map path keys.

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

- `exitium_reflect` depends on `exitium_core` only (#37: `reflect -> core`). Core types (`Num`, `Guid`, `RawHandle`, `Rect`, `Color`, glam math) get their `Reflect` impls inside `exitium_reflect`, which satisfies the orphan rule because the trait is local. These impls are **hand-written**: `#[derive(Reflect)]` cannot run on a type declared in another crate, and several of these types have private fields or an invariant (see "Value kinds with an invariant"). `Entity` is declared in `exitium_ecs`, so its impl lives there (see "Entity").
- `exitium_reflect` has **no direct external dependency**. It does not use `serde`; serializers walk reflection directly. `exitium_core` itself depends on `glam` (#88), so `glam` is in every build that reflects math types, but `exitium_reflect` reaches it only through the `exitium_core::math` re-exports and never names the `glam` crate (one glam version per build, one place to change it).
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
    Value(&'a dyn Reflect),      // bool, ints, floats, char, String, (), and validated newtypes (Num, Guid, NonZeroU32); read the leaf with value_ref()
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
    pub version: Option<NonZeroU32>, // #[reflect(version = N)], N >= 1: stored, NOT hashed, written to binary headers (see Schema hash)
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
- **`transparent`** (`#[reflect(transparent)]`, only legal on a single-field struct): the type is layout-identical to its field **and the field is freely reachable**, so the derive can hand out `&T` and `&mut T` to it. `Box<T>` and user newtypes such as `struct Gold(f64)` use it. Consumers treat it as its inner field (the inspector shows one number, text format writes `3.5` not `Gold(3.5)`). In the schema hash a transparent type is invisible: it resolves to its inner field's type (see Schema hash). Chains of transparent types resolve to the first non-transparent type. **`transparent` is not legal for a type with a private field or an invariant** (`Num`, `Guid`): a reflected `&mut f64` would let any caller store a value the wrapper forbids. Those types are Value kinds (next rules). A transparent chain that loops back to itself (`#[reflect(transparent)] struct L(Box<L>)`, since `Box<T>` is transparent too) has no first non-transparent type: resolution keeps a visited set and returns `SchemaError::TransparentCycle { name }`, `register` reports it as `RegistryError::Schema`, and the derive rejects the direct self-reference with a compile error (the registry check is the authority; the derive check only gives an earlier message).
- **Tuples** `(A, B)` up to arity 8 are `TupleStruct` kind with the synthetic name `(A,B)`. No ninth kind.
- **`Box<T>`** is a transparent `TupleStruct` named `Box<T>`.
- **Value** covers primitives, `String`, and **hand-written validated newtypes** (below). Everything else (`RawHandle`, `Entity`, `Rect`, `Color`, and the glam types) is an ordinary struct (`Vec2` is a struct `{x, y}`), never opaque. Consequence: every reflected value bottoms out in the 16 `ValueRepr` leaves, so the **binary** format needs exactly 16 leaf encoders and no per-type codecs.
- **Value kinds with an invariant.** An engine crate may implement `Reflect` by hand for a newtype that has a private field and an invariant, as a `Value` kind: `ValueInfo { repr }` where `repr` is the representation the type has on the wire. The type keeps its own stable name (`Num`, `Guid`, `NonZeroU32`). The schema `ref` of a Value uses **only the repr code** (see Schema hash), so `Num` hashes exactly as `f64`, `Guid` as `u128`, `NonZeroU32` as `u32`, and `Vec<Num>` equals `Vec<f64>`. The derive never emits a Value kind; user newtypes use `transparent`. Rules every such impl follows:
  1. **Read** goes through `value_ref()`, which returns the leaf by value (`ValueRef::F64(n.as_f64())`). `Struct::field_at`-style borrowing of the inner field is impossible and not needed.
  2. **Every write routes through the validating constructor.** `set_value`, `set`, `apply`, `FromReflect::from_reflect` and the binary and text decoders all end in the same function, so a raw inner value is never stored. For `Num` that function is `Num::from_f64`, which maps `-0.0` to `0.0` and saturates `inf` to `Num::MAX`/`MIN`. The reflect impl additionally **rejects NaN** with `ValueError::Invalid` instead of storing the `ZERO` that `from_f64` would produce, so a corrupt file or a bad MCP call is an error rather than a silent zero. For `NonZeroU32` the function is `NonZeroU32::new`, and `0` is `ValueError::Invalid`. `Guid` accepts every `u128`.
  3. **The stable name and repr are part of the persisted-format contract** (name table below).
  4. A validation failure leaves `self` unchanged (a Value write is atomic).

  `range` on a `Num` field resolves through the Value kind: legal because `repr` is `F64` (see Attributes).
- **Text form hook.** A few types have a canonical text spelling that is not the structural one: `Guid` text must equal `Guid::Display` (the 36-character UUID spelling, #37), and `Entity` is `"42v1"`, never a bare integer (#21). They register the type data `ReflectTextForm` (below). The **text and JSON/MCP** serializers (#23) consult it before walking structure; the **binary** format never does, so binary layout stays exactly what the schema hash describes. The hook changes nothing about `TypeInfo` or the hash. `Num` has **no** text form: it writes as an `f64` (the shortest spelling that round-trips). Display notations (`Notation`, #89) are for presentation only.
- **`Guid`** is a hand-written Value kind (`repr: U128`, stable name `Guid`) with `ReflectTextForm` = `Guid::Display`/`FromStr` (already in `exitium_core`). Decision: Value, not `transparent`, because `Guid(u128)` has a private field. Hash equals `u128`; binary is the 16-byte little-endian leaf; every `u128` is valid, so `set_value` never fails except on a wrong repr. As a map key it is accepted by rule (b) below.
- **`NonZeroU32`** is a hand-written Value kind (`repr: U32`, stable name `NonZeroU32`, hash equals `u32`) whose `set_value` rejects `0`. It exists so `RawHandle` and `Entity` can reflect their generation as a `u32` field that can never become 0 through reflection (`field_at_mut` on it returns a `&mut dyn Reflect` that enforces the rule in `set_value`). It is registered like any other engine type; games do not use it directly.
- **`Entity`** is *not* a transparent `u64` and is **hand-written**, not derived. See "Entity" below.
- **`RawHandle`/`Handle<T>`.** `RawHandle { index: u32, generation: NonZeroU32 }` (#15, merged in #81) is reflected by a hand-written impl in `exitium_reflect` as the struct `{ index: u32, generation: u32 }`; `generation` is the `NonZeroU32` Value, so `from_reflect`/`set`/`apply` and any decode reject `0`, the same rule as `Entity`. Note `RawHandle::to_bits` packs the index in the **high** half (it is built for ordering) and is therefore *not* the reflected byte layout; no serializer uses `to_bits` for `RawHandle`. `RawHandle` is for debug and inspector display and is persisted only through a remap (a saved world maps old handles to new ones on load), never as a stable id. `Handle<T>` gets **no** `Reflect` impl in v1: it is a process-local arena id with no meaning after a reload, so it must not be persisted or hashed as if it did, and giving it a stable name would force every handle target `T` to be `Reflectable`. Consequence: a component with a `Handle<Image>` field **cannot derive `Reflect` unless that field is `#[reflect(skip)]`**, and a skipped field is dropped from scenes and saves. Sprites and other persisted references use `AssetRef<T>` (open question 13). The derive detects a field type whose last path segment is `Handle` and emits a targeted `compile_error!` ("`Handle<T>` is a process-local id and is not reflectable; persist an `AssetRef<T>`, or mark the field `#[reflect(skip)]`") instead of the generic missing-trait error.

#### Entity

`Entity` (declared in `exitium_ecs`, `entity.rs`, private fields `index: u32` and `generation: NonZeroU32`) gets a **hand-written `Reflect` + `ReflectTextForm` impl** in `entity.rs`, safe code that depends on `exitium_reflect`. It is not derived because the derive's generated `from_reflect` builds `Self { .. }` field-wise, whereas `Entity` must be rebuilt through its own checked constructor, and keeping the impl next to the private fields keeps that invariant in one file a reviewer reads. It reflects as the **struct `{ index: u32, generation: u32 }`** named `Entity`: `field_at(0)` is `&self.index` and `field_at(1)` is `&self.generation` seen as the `NonZeroU32` Value. `from_reflect`, `set` and `apply` reject generation 0 (through that Value's `set_value`, and `Entity` is rebuilt only through the checked constructor). `ReflectTextForm` is `"{index}v{generation}"`.

Binary layout is the reflected structure: `index` as 4 little-endian bytes, then `generation`. `Entity::to_bits` is pinned to `(generation << 32) | index` so that `to_bits().to_le_bytes()` equals those 8 bytes; #24 may write either form and the bytes are identical (the schema hash describes the structure only). The ECS spec carries the same pin.
- `usize`/`isize` have no `Reflect` impl (width differs between wasm32 and 64-bit; it would break hash and binary stability). Use explicit widths.
- Field lookup by name is a linear scan of the `fields` slice. Components have few fields; a scan of 16 short names beats hashing.

### Stable type names

`std::any::type_name` is explicitly unspecified across compiler versions, so it is never used. Instead:

| Type | Stable name |
|---|---|
| primitives, `String` | `u32`, `f64`, `bool`, `String`, `()` |
| engine core types (pinned, unqualified) | `Num`, `Guid`, `NonZeroU32`, `RawHandle`, `Entity`, `Rect`, `Color`, and the glam math types that `exitium_core::math` re-exports, under their glam names: `Vec2`, `Vec3`, `Vec4`, `DVec2`, `IVec2`, `IVec3`, `IVec4`, `UVec2`, `UVec3`, `UVec4`, `Quat`, `Mat2`, `Mat3`, `Mat4`, `Affine2` |
| user type | `module_path!()` + `::` + identifier, e.g. `glutton::economy::Wallet`; override with `#[reflect(type_name = "glutton::Wallet")]` |
| generics | base name + stable names of the arguments, no spaces: `Vec<f32>`, `Option<glutton::Wallet>`, `HashMap<String,u32>`, `[f32;4]`, `(f32,f32)`, `Box<glutton::Wallet>`, `glutton::Pair<u8,String>` |

Rules: a user type name always contains `::` (it starts with the crate name). The derive accepts a `type_name` override **without `::` only when `CARGO_PKG_NAME` is in `ENGINE_CRATES`** (the same allow-list the paths rule uses, see Derive macro); in any other crate an override without `::` is a compile error, so no user type can collide with an engine core name. Engine core names are fixed by this table and are part of the persisted-format contract (changing one needs a new `SCHEMA_HASH_VERSION` and a review that checks the format change; serialization formats are a high-risk area, CLAUDE.md). The table is also a constant, `PINNED_TYPE_NAMES`, in `exitium_reflect`. A test registers every core type and asserts that the registered names equal this table (the expected list is spelled out in the test, so adding a re-export to `exitium_core::math` without a table row fails it), and a test in `exitium_ecs` (#50) asserts `Entity`'s registered name is in the table. Two engine types that pick the same unqualified name are caught by the registry's `DuplicateName` check.

Glam types (`glam` is a dependency of `exitium_core` since #88, re-exported through `exitium_core::math`) are reflected as structs of their **components**, the same schema on every target: `Vec2 { x, y }`, `Vec4 { x, y, z, w }`, `Quat { x, y, z, w }` (all `f32`; `DVec2` is `f64`, `IVec*` `i32`, `UVec*` `u32`), `Mat2 { x_axis, y_axis }`, `Mat3 { x_axis, y_axis, z_axis }`, `Mat4 { x_axis, y_axis, z_axis, w_axis }` (columns), and `Affine2 { matrix2: Mat2, translation: Vec2 }`. The impls are written with a small private `macro_rules!` in `exitium_reflect`. **Field access is not uniform across targets.** On SSE2 (x86, x86_64), NEON (aarch64) and `simd128` (wasm32 with the feature), glam's `Vec4`, `Quat` and `Mat2` are tuple structs around a SIMD register (`__m128`, `float32x4_t`, `v128`) with private fields; elsewhere they are plain structs. So the reflect impls must **never use struct literals, destructuring patterns or layout assumptions** on those three types. They read and write a lane with the plain expressions `v.x` / `&mut v.x`, which compile on every target because glam implements `Deref`/`DerefMut` to a plain `{ x, y, z, w }` (or two-column) struct, and they build values with `Vec4::new`, `Quat::from_xyzw` and `Mat2::from_cols`. `Mat3`, `Mat4`, `Affine2` and the `IVec*`/`UVec*`/`Vec3`/`Vec2`/`DVec2` types have public fields; `Mat4` holds `Vec4` columns and `Affine2` a `Mat2`, so the same rule applies one level down. A `Quat` is any four `f32`, not necessarily unit length, so it needs no validation. Test (in #37): reflect round trips for `Vec4`, `Quat` and `Mat2` run in CI on the host (SSE2) and the wasm build checks that the same source compiles on the scalar layout.

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

    /// Leaf accessor for `Value` kinds (default: `None`). Returns the leaf by value, so a validated
    /// newtype whose inner field cannot be borrowed (`Num`, `Guid`) still has one. The 16 leaf
    /// encoders and the text and MCP writers read through this and never downcast to the bare
    /// primitive: a `Num` does not downcast to `f64`.
    fn value_ref(&self) -> Option<ValueRef<'_>> { None }
    /// Validated write of a leaf (default: `Err(ValueError::NotAValue)`). A `Value` impl checks the
    /// repr, runs its validating constructor and stores nothing on error (see "Value kinds with an
    /// invariant"). `set`, `apply`, `from_reflect` and the decoders reach a Value only through this.
    fn set_value(&mut self, v: ValueRef<'_>) -> Result<(), ValueError> { Err(ValueError::NotAValue) }
}

/// One variant per `ValueRepr`; `Copy`, scalars by value, text borrowed.
pub enum ValueRef<'a> {
    Unit, Bool(bool), Char(char), I8(i8), I16(i16), I32(i32), I64(i64), I128(i128),
    U8(u8), U16(u16), U32(u32), U64(u64), U128(u128), F32(f32), F64(f64), String(&'a str),
}
impl ValueRef<'_> { pub fn repr(&self) -> ValueRepr; }
pub enum ValueError {
    NotAValue,
    WrongRepr { expected: ValueRepr, found: ValueRepr },
    Invalid { type_name: &'static str, reason: &'static str },   // NaN into Num, 0 into NonZeroU32
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
- **Map keys** are limited to (a) `Value` kinds except floats and `()` (ints, `bool`, `char`, `String`, and so `Num` is excluded), (b) `transparent` wrappers of those and `Value` newtypes over them (`Guid`), (c) **enums whose variants are all unit variants** (`HashMap<ResourceKind, Num>` is the idiomatic incremental-game state and must work), and (d) any type registered with `ReflectTextForm` (`Entity`). Lookup downcasts the key to `K` (which supplies `Hash + Eq` or `Ord`), so no reflected hashing is needed. Key spelling in text/JSON: variant name for enums, the text form for (d), decimal for ints. Binary: leaf encoding, enums as the `u32` variant index. Deterministic order when a `HashMap` is written: ints numeric, strings and text forms by UTF-8 bytes, enums by variant index. Registration rejects any other key type with `RegistryError::InvalidMapKey`.
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
| Value | Read `other.value_ref()` and call `self.set_value(..)`. A different repr is `ApplyError::TypeMismatch { expected, found }`; a failed validation (NaN into a `Num`, `0` into a `NonZeroU32`) is `ApplyError::Invalid(ValueError)` and leaves `self` unchanged. So a plain `f64` value applies onto a `Num` field, and never stores a raw `f64`. |

`apply` is **not atomic**: on error `self` may be partly modified. The command bus takes a `clone_boxed()` snapshot first and restores it on failure (also the undo record). Atomic apply is open question 6.

### Path access

```
path     = segment { segment }
segment  = field | index | key
field    = [ "." ] ident              ; leading "." optional on the first segment
index    = "[" [ "-" ] digits "]"  |  "." digits   ; list/array element, tuple-struct field, or integer map key ("-" only for signed integer keys)
key      = "[" '"' chars '"' "]"          ; map key spelled as text, \" and \\ escapes
```

Examples: `position.x`, `items[3].name`, `pair.0`, `stats["hp"]`, `inventory[12].count`, `resources["Gold"]`, `owners["42v1"]`.
Enum paths walk the *active* variant's fields. An `Option` is traversed transparently: `a.b` where `a: Option<S>` reads `S.b`, and `None` yields `PathError::NoneValue`.

**Map key segments.** The parser keeps a segment as either an integer token (`[12]`, `[-3]`) or a quoted string (`["Gold"]`) and does not know the key type. The walker resolves the token against `MapInfo.key` (`TypeRef`) of the map it is standing on, then calls `Map::get` with the resulting key value:

| `MapInfo.key` | Accepted segment | Resolution |
|---|---|---|
| integer `Value` (`u8` to `u128`, `i8` to `i128`) | `[digits]`, and `[-digits]` for signed | parsed to the key's width; out of range or a quoted string is `PathErrorKind::BadKey` |
| `String` | `["text"]` | used as is |
| `bool`, `char` | `["true"]` / `["a"]` | `char` needs exactly one scalar value |
| unit-only enum (`HashMap<ResourceKind, Num>`, the headline case) | `["Gold"]` | the variant name; the walker builds a `DynamicEnum` unit variant and converts it with the key type's `ReflectFromReflect`; an unknown name is `BadKey` |
| type with `ReflectTextForm` (`Guid`, `Entity`) | `["42v1"]` | `from_text`; digits are **not** accepted for these, even for `Guid`, whose repr is `u128` |

A key type that is none of these cannot exist (registration rejects it, `InvalidMapKey`). A segment `[3]` on a list is an index; on a map it is a key, and the walker decides by the container kind it stands on. A negative index on a list or tuple is `OutOfBounds`. `PathErrorKind` gains `BadKey { key_type: &'static str }`.

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
pub enum PathErrorKind { Syntax { offset: usize }, NotFound, OutOfBounds { len: usize }, NoneValue, BadKey { key_type: &'static str }, KindMismatch { expected: &'static str, found: &'static str } }
```

Parsing never panics on arbitrary input (tested by a randomized test).

### Attributes

Written as `#[reflect(...)]` on types, fields and variants. Unknown attributes are a compile error that lists the valid set for that position.

| Attribute | On | Meaning | Hashed? |
|---|---|---|---|
| `range(min = a, max = b [, step = s])` | field | Numeric bounds, inspector slider, JSON Schema `minimum`/`maximum`. Legal when the field's type, **after resolving `transparent` wrappers**, is a `Value` whose `repr` is an integer or float, so `range` works on a `Num` field (a Value kind with `repr: F64`; bounds are in the unit of that `f64`). Rejected at registration (`RegistryError::InvalidAttribute`) on `bool`, `char`, `String`, containers, structs and enums, including `Option<f32>` and `Vec<f32>` (v1 has no per-element range). Bounds and `step` are stored as `f64`; for `i64`/`u64`/`i128`/`u128` beyond 2^53 they are inspector hints only, not exact limits. `range` is a UI and schema hint: `set`/`apply` do not clamp. | no |
| `tooltip = "text"` | type, field, variant | Human text. Falls back to `///` docs when absent. | no |
| `hidden` | field, variant | Hide from the **inspector** only. Still serialized, still in MCP schemas (marked hidden) so agents keep parity. | no |
| `skip` | field | Not reflected, not serialized, not in `TypeInfo` or the hash. Rebuilt with `Default::default()` or its `default` expression in `from_reflect`. | no (absent) |
| `default` / `default = "path::to_fn"` | field | Value used when the field is missing in dynamic/serialized data. Stored as `fn() -> Box<dyn Reflect>` so schemas can show defaults. | no |
| `custom(key = literal, ...)` | type, field, variant | Extensible key/value store (`bool`, integer, float, string), e.g. `custom(widget = "color")`. | no |
| `type_name = "..."` | type | Override the stable name. Must contain `::`, except in an `ENGINE_CRATES` crate (see "Stable type names"). | yes (name) |
| `transparent` | single-field struct | See "TypeInfo". Only for a freely reachable inner field; a transparent cycle is rejected. | yes |
| `version = N` | type | Declared type version, `N >= 1`, for migrations (`0` is a compile error: it means "undeclared" in headers). Stored in `TypeMeta` and in binary headers, not hashed (see "Schema hash"). A compile error together with `transparent`: a transparent type has no header row, so its version would be lost. | no |
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

- **`ReflectTextForm`** is built by `FromType<T: Reflect + TextForm>`, where `trait TextForm: Sized { fn to_text(&self) -> String; fn from_text(s: &str) -> Result<Self, TextFormError>; }` is implemented by hand on the type. `exitium_reflect` implements it for `Guid` (`to_text` is exactly `Guid::Display`, `from_text` is `Guid::from_str`, both in `exitium_core` already). `exitium_ecs` implements it for `Entity` by hand in `entity.rs` (`"{index}v{generation}"`, never a bare integer). The trait also carries `const PATTERN: Option<&'static str> = None`, a regular expression the type's text always matches, used only by the MCP schema generator (`Guid`: `^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$`; `Entity`: `^[0-9]+v[1-9][0-9]*$`). A `to_text`/`from_text` round trip is a required test for every implementor. Only the text/JSON/MCP serializers consult it; map keys of such types are written as the text string. It is not hashed: a change to a text form is a text-format change reviewed with the type, and golden tests for `Guid` and `Entity` text pin it.
- **Generics and containers:** the registry is keyed by concrete instantiation (`Vec<f32>` and `Vec<u8>` are separate entries). Derived generic types need `T: Reflectable`, a blanket-implemented alias for `Reflect + Typed + FromReflect + GetTypeRegistration`. A static in a generic function is shared by all instantiations, so generic `TypeInfo` is cached in `GenericTypeCell`: write-once per instantiation, entries `Box::leak`ed (bounded by the number of distinct instantiations in the program). `HashMap::new()` is not `const`, so the cell is `static CELL: OnceLock<RwLock<HashMap<TypeId, &'static TypeInfo>>>` (a `RwLock<BTreeMap<..>>` would be const-constructible, `TypeId` being `Ord`, but costs a tree walk on every hit against the 40 ns budget).

  **No user code and no re-entry under the lock.** Building the name and info of `Vec<Vec<f32>>`, `Pair<Pair<u8,u8>,u8>` or `Option<Option<T>>` calls `type_info()` of the argument types, which lands in the *same* cell (one `static` per generic function). So the lookup is:
  1. read lock, look up `TypeId::of::<Self>()`, release; return on hit;
  2. on a miss, **build a `PendingInfo` with no lock held** (this may recursively fill the cell for the argument types). `PendingInfo` is the `TypeInfo` under construction with its composed name still an owned `String` (`Vec<f32>`, `glutton::Pair<u8,String>`); the field and variant lists are owned and every other string is already a `&'static str` literal or an argument's `&'static` name. Nothing is leaked yet;
  3. take the write lock and `entry(id)`: `Occupied` returns the existing `&'static` and drops the `PendingInfo` (its `String` and boxes are freed, **nothing is leaked on a lost race**); `Vacant` calls `PendingInfo::finish()`, which leaks the name (`Box::leak(name.into_boxed_str())`) and the `TypeInfo` once, and inserts. The leak therefore happens exactly once per instantiation, and only on the vacant branch.

  Two threads racing on a miss may both build; one wins and both return the same reference. No `Mutex` poisoning is possible (no user code runs under a guard); the cell uses `PoisonError::into_inner` anyway. Non-generic types use `OnceLock`, which cannot re-enter either: building a `TypeInfo` only stores `TypeRef` function pointers for field types, it never calls their `type_info()` (this is also what makes recursive types such as `Tree { kids: Vec<Tree> }` buildable). Test: `Vec<Vec<f32>>`, `Pair<Pair<u8,u8>,u8>` and `Option<Option<u32>>` first-use from 8 threads, no deadlock, equal `&'static` pointers.
- Built-in impls (#31, #33): `bool`, `char`, all fixed-width ints incl. `i128`/`u128`, `f32`, `f64`, `String`, `()`, `Vec<T>`, `[T; N]`, `Option<T>`, `Box<T>`, `HashMap<K,V>`, `BTreeMap<K,V>`, tuples up to arity 8 (small private `macro_rules!`, not public API). Core types come with #37.

### Threading model

- **Everything static is shareable.** `TypeInfo` and its parts are immutable `'static` data; `Reflect` and `TypeData` are `Send + Sync` (supertraits), so reflected types, their type data and `TypeInfo` can cross threads freely. A type that is not `Send + Sync` cannot be reflected (consistent with the ECS non-goal on non-`Send` components, #21). No reflected value contains interior locking; `&mut dyn Reflect` is exclusive by Rust's rules, and concurrent access to *world* data is governed by the ECS access rules, not by reflection.
- **First-use caches are thread-safe and idempotent.** `OnceLock` (non-generic `TypeInfo`, hash caches) and `GenericTypeCell` give the same result no matter which thread initializes first.
- **Registry ownership.** `TypeRegistry` is an ordinary struct with no interior mutability besides the `OnceLock` hash caches, hence `Send + Sync`. The shared handle is `AppTypeRegistry(Arc<RwLock<TypeRegistry>>)`, **defined in `exitium_reflect`** (so `exitium_ecs` can name it: `exitium_app` is downstream of ecs, and `World::register_reflected` needs the registry). `exitium_ecs` makes it a resource with `impl Resource for AppTypeRegistry` (the trait is local there, so the orphan rule holds), and `World::register_reflected` initializes it on first use if absent, so a bare `World` works in tests. `exitium_app` only inserts it at build time and hands out clones; it defines nothing. MCP, editor, script and serializer code clone the `Arc` and read through it.
- **Read-mostly.** All game types are registered during app build, before the first update, through `app.register_type::<T>()`, which takes the write lock. After startup the registry is only read. A reader takes **one** read guard per operation (one serialize call, one MCP request, one inspector frame), never one per field, and never holds a guard across a system run or command application. **Code that holds a read guard must not take a second read guard** (not even through a helper that "just looks something up"): `std::sync::RwLock` may block a new reader while a writer is queued, so a nested read can deadlock against a late registration. Helpers take `&TypeRegistry` and the guard is taken once at the entry point. Cost of that guard is covered by the "registry lookup" budgets. `&'static TypeInfo` values outlive the guard; `&TypeRegistration` borrows do not.
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

- *Engine crates* get `::exitium_reflect` / `::exitium_ecs`. The decision is an **explicit allow-list inside the derive** (`ENGINE_CRATES`, matched against `CARGO_PKG_NAME`), not a name-prefix rule. The same allow-list decides who may use an unqualified `type_name` override (see "Stable type names"): it contains every `exitium_*` workspace crate that depends on `exitium_reflect` directly, plus the `exitium` facade itself (which depends on `exitium_reflect`, so `::exitium_reflect` resolves inside its own source and no `extern crate self as exitium;` is needed). `exitium_reflect` declares `extern crate self as exitium_reflect;` for its own derive uses and tests.
- *Everything else* (games, examples, user crates, **including a user crate that happens to be named `exitium_something`**) gets `::exitium::reflect` / `::exitium::ecs`, which the facade re-exports. This `reflect` (and `ecs`) re-export is part of the facade's public API, a high-risk area that gets extra reviewer scrutiny (CLAUDE.md, ADR-0005); this spec requires it, and the PR body lists it under Risk.
- `#[reflect(crate = path)]` and `#[reflect(ecs_crate = path)]` override either root, for a crate that renames its dependency or re-exports through its own prelude.
- **Drift guard:** `exitium_reflect_derive/tests/crate_paths.rs` lists the crate directories under `crates/` using `std::fs` and asserts each is either in `ENGINE_CRATES` or in a short `NOT_DERIVE_USERS` list in the same test (for example `exitium_core`, which cannot depend on reflect, and `exitium_cli`). A newly added crate fails that test until someone classifies it, so the allow-list cannot silently rot. No manifest-parsing dependency is needed.

**Macro-magic limits (CLAUDE.md rule 6).** The derive must stay readable to a basic-level Rust reviewer:

- It only parses a struct/enum, its generics, and `#[reflect(...)]` / `#[doc]` attributes. No function-like or attribute proc macros, no `macro_rules!` DSL in public API.
- It emits only the four impl groups above, no new public items, hidden fields or helper modules. All paths are fully qualified (see "Paths"). No global state.
- Each generated impl method is a direct match/field access. A hand-written equivalent for a sample struct and enum lives in `exitium_reflect/tests/manual_impl.rs`, with a test that it behaves like the derive output. The derive module docs show the same example.
- Derive crate: at most 800 lines excluding tests, no `unsafe`, no `unwrap`/`panic` (errors are `compile_error!` with a span).
- Unsupported input is a clear compile error: unions, lifetime parameters, borrowed fields, `usize`/`isize` fields, a `Handle<T>` field that is not `skip` (targeted message, see "TypeInfo"), `version = 0`, `version` on a `transparent` type, an unqualified `type_name` outside `ENGINE_CRATES`, and a `transparent` type whose field is the type itself.
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

**Named and anonymous types.** A *named* type is a struct, tuple struct or enum declared by a user or by the engine (`Wallet`, `Entity`, `Vec2`; `Num`, `Guid` and `NonZeroU32` are Value kinds, so they are not nodes). Everything else is *anonymous*: `Value` (including the validated newtypes, which are identified by their repr alone, never by their own name), `Vec<T>`, `[T; N]`, `HashMap`/`BTreeMap`, `Option<T>` and the built-in tuples `(A, B)` (`TupleStructInfo.is_tuple = true`). Anonymous types are identified by their structure alone, never by their composed name. Consequences: `HashMap` and `BTreeMap` hash the same, `Vec<Num>` hashes like `Vec<f64>`, and the closure and the table contain only named types. (User *generic* structs such as `Pair<u8,String>` are named types, and their name embeds the written argument names, so `Pair<Num,u8>` and `Pair<f64,u8>` differ.)

**Transparent resolution.** A `transparent` type is not a node. Wherever a type would be referenced, or hashed as a root, it is replaced by its inner field's type (repeatedly; a cycle is `SchemaError::TransparentCycle`). So a field of type `Box<Wallet>` is a reference to `Wallet`. A validated Value newtype needs no resolution: `ref(Value)` is `0x01` plus the repr code, so a field of type `Num` is a reference to `f64`, a `Guid` to `u128`, and `deep_hash(Num) == deep_hash(f64)`.

**Type reference** `ref(T)` (after transparent resolution) is how one type appears inside another's stream. It is a pure function of `T`'s `TypeInfo` and contains no hash and no walk state:

| `T` | `ref(T)` |
|---|---|
| Value (only the repr; a validated newtype's own name is never written) | `0x01`, repr code (`Unit`=0, `Bool`=1, `Char`=2, `I8`=3, `I16`=4, `I32`=5, `I64`=6, `I128`=7, `U8`=8, `U16`=9, `U32`=10, `U64`=11, `U128`=12, `F32`=13, `F64`=14, `String`=15) |
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
pub struct SchemaTable { pub root: String, pub root_ref: Vec<u8>, pub deep: SchemaHash, pub entries: Vec<SchemaEntry> }
pub struct SchemaEntry { pub name: String, pub version: u32, pub shallow: SchemaHash }  // sorted by name
```

`entries` has one row per named type in the root's closure, in the same sorted order as the deep hash. `root` is the root's registry name (informational). `root_ref` is the canonical bytes of `ref(root)`: for a named root a few bytes (kind and name), for an anonymous root such as `Vec<Wallet>` the structural encoding (`05 00 02 "Wallet"`). `version` is the declared `#[reflect(version = N)]` (`N >= 1`), or `0` when undeclared; because a declared version can never be `0`, the two never collide. With `root_ref` stored, `deep` **can** be recomputed from the table alone (the deep stream is the version byte, `ref(root)`, the closure size and the rows), so a reader checks the whole file in one comparison and falls back to row-by-row comparison to find what changed.

*Relation between `version` and the hash.* The hash is computed from the schema; the version is **declared by the author and never hashed**. The author raises `version` whenever the type's shallow hash changes. A type that gains its first `version = 1` migrates from `0`. A reader compares every row, in the file or in the registry, and acts as follows:

| Row is in | Version | Shallow hash | Meaning | Reader action |
|---|---|---|---|---|
| both | equal | equal | unchanged | decode directly |
| both | file < current | any | declared change | run the registered migration chain `from_version -> current` (#59); no chain: error naming the type and both versions |
| both | file > current | any | **file written by a newer build** | error `FileNewer { type, file_version, current }`; never attempt a downgrade or a migration |
| both | equal | differs | **unversioned schema change** (the bug this hash exists to catch) | reject with an error naming the type, or decode dynamically; policy is #24's. A migration may declare `from_hash` to cover types that never had a version. |
| file only | any | any | type removed (or renamed without a pinned `type_name`) | error unless a registered migration names it (a migration may drop or rename the type); then its rows feed the chain |
| registry only | n/a | n/a | type added since the file was written | not an error by itself: a type that *contains* the new one has a changed shallow hash and is classified on its own row; a new root type with no row is simply absent from old files |

Only the row of the type that changed differs in its shallow hash; types that merely *contain* it keep their rows unless their own field list changed, so the error points at the changed type. A `version` declared on a `transparent` type has no row (a transparent type is not a node), so the derive rejects that combination (see Attributes).

**Decoding an old file into a `DynamicValue` (migrations).** The table has `(name, version, shallow hash)` per type. It does **not** have the old field names, order or types, and the binary body is positional, so a table plus the *current* registry cannot decode an old body: the old layout is gone. A migration (#59) takes a `DynamicValue`, so #24 must pick one of these, and this spec fixes which are legal:

1. **Keep the old Rust types registered** under their old names (`glutton::WalletV1`, with a `type_name` pin) for as long as saves from that version must load. Decode with the old type, convert with reflection (`clone_boxed`/`DynamicStruct` from the old value), then run the chain. Simple, costs code and registry entries that the game carries.
2. **Embed a full schema in the header**: per row, the bytes of the shallow stream (kind, name, field names and `ref` encodings, which is exactly the data the shallow hash already consumes) plus `root_ref`. The reader parses those bytes into a throwaway `TypeInfo` and decodes the positional body into a `DynamicStruct` with field names, with no old Rust type. Header cost is the shallow stream length per named type (tens of bytes each). **Recommended**, because it needs no old code and is deterministic.
3. **Migrate through the text format**: the old build exports text (self-describing by names), the new build imports it with defaults for missing fields. Only works when the old build is still available to export, so it suits editor upgrades and assets, not saves already in players' hands.

Whichever is chosen, a body that cannot be decoded by any of them is reported with the type name and both versions, never guessed. CI enforcement of "hash changed implies version raised" is the `schemas.lock` proposal (open question 5), which records `(name, version, shallow hash)` per persisted type.

**Stability contract and golden vectors.** Vectors are checked in with #36 and are the compatibility contract: changing one is a breaking binary-format change that needs a new `SCHEMA_HASH_VERSION` and a PR whose Risk section names the format change (a high-risk area, so the reviewer checks it closely). They test **three layers**, because testing only the hash function would miss the place native and wasm could diverge (the `TypeInfo` to bytes walk, `usize` casts, ordering):

1. *FNV-1a only:* `""` -> `0xcbf29ce484222325`, `"a"` -> `0xaf63dc4c8601ec8c`, `"foobar"` -> `0x85944171f73967e8`.
2. *Encoder:* `TypeInfo` values built **by hand** (no derive, so the encoder is tested in isolation) with the exact expected **byte stream** (hex) and shallow hash. Required samples: a struct with fields of several kinds, a zero-field struct, a tuple struct, an enum with unit, tuple and struct variants, and `ref` encodings for `Vec<u8>`, `[u8; 0]` (**must differ from `Vec<u8>`**), `[u8; 4]`, `HashMap<String,u32>` (equal to the `BTreeMap` encoding), `Option<f32>`, a built-in tuple, and a field whose type is a transparent wrapper (resolves to the inner type). The `demo::Pt` example above is the first of these.
3. *Deep hash of derived sample types:* struct, enum, nested struct, **self-recursive** (`Tree { kids: Vec<Tree> }`), **mutually recursive** (`A { b: Option<Box<B>> }`, `B { a: Vec<A> }`), diamond (`Top { l: Mid1, r: Mid2 }`, both holding `Leaf`), `Num` (equals `f64`) and `Guid` (equals `u128`). A hand-built `TypeInfo` for the struct `Entity { index: u32, generation: u32 }` gets a pinned shallow hash too (`exitium_reflect` cannot depend on `exitium_ecs`; the `exitium_ecs` test checks its real impl against that vector). Expected `u64`s are fixed in the test source.

The encoder uses only `u8`/`u32`/`u64`/UTF-8 and `u32::try_from` for every length, so there is nothing width-dependent for wasm32 to disagree on. CI only *builds* wasm; running the golden tests on wasm32 (for example under `wasmtime`) is a recommended follow-up (open question 14).

**Roles** (details in #24, #59):

- *Binary header:* stores the `SchemaTable`. The reader recomputes the table from the registry and applies the version/hash decision table.
- *Text format:* has no hash. It is self-describing by names, so additive changes migrate through `default` and unknown-field reporting.
- *Migrations:* registered by `(type_name, from_version, to_version)` (#59); the hash is the guard that detects an unversioned change.
- *CI guard (proposed, open question 5):* `cargo xtask schema export` dumps `(name, version, shallow hash)` for every registered persisted type and `schema check` compares to a committed `schemas.lock`. A hash change without a version bump shows up as a visible diff in the PR, which the reviewer checks (serialization formats are a high-risk area). Not part of #36.

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

**Change detection contract for `get_mut`.** It must not flag the component changed just because it was called: the inspector and MCP read through it, and a flag on every call would raise false `Changed<T>` for UI bindings. It returns a guard, **`ReflectedMut<'w>`**, with the ECS `Mut<T>` semantics defined in #21 (the ECS spec uses the same name; `ReflectMut<'a>` in this spec is only the kind-view enum returned by `Reflect::reflect_mut`, a different type). **The stamp happens in an accessor, not in `DerefMut`**: `as_reflect(&self) -> &dyn Reflect` is free and stamps nothing; `as_reflect_mut(&mut self) -> &mut dyn Reflect` stores the current tick into the row's `changed` slot at that call and returns the reference; a guard whose `as_reflect_mut` is never called flags nothing; `bypass_change_detection()` exists for callers that knowingly do not change data. `ReflectedMut` is a small dedicated type that wraps the typed `Mut<T>` (built by the `FromType<T>` closure, which knows `T`); it has no `Deref`/`DerefMut`, so an auto-deref in a method call cannot stamp by accident, and the `SetField` command can validate the path and value first and call `as_reflect_mut` only to perform the write. `insert` onto a component the entity already has applies through the same guard, so it flags `changed` (not `added`), matching the ECS rule for overwriting an existing component; `insert` of a new component stamps `added = changed`. Test (in #50): `get` and an unused `get_mut` leave `Changed<T>` false; a field set through `get_mut` makes it true.

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
| Core impls | #37 | hand-written impls: `Num`, `Guid` (Value, `U128`), `NonZeroU32` (Value, `U32`), `RawHandle`, `Rect`, `Color`, glam (`Vec2`..`Mat4`, `DVec2`, `Mat2`, `Affine2`), `ReflectTextForm` for `Guid`, `PINNED_TYPE_NAMES` (no `Handle<T>` impl, open question 13) |
| ECS hook | #50 | `ReflectComponent`, `ReflectedMut`, `register_reflected`, `impl Resource for AppTypeRegistry`, hand-written `Entity` reflection and its `TextForm` (`"42v1"`) in `entity.rs` |

### Downstream issue adjustments

Applied when this amendment merges (the architect edits each issue body; the bullets are the contract):

- **#31 (model, registry):** `value_ref`/`set_value`, `ValueRef`, `ValueError`; `TypeMeta.version: Option<NonZeroU32>`; `SchemaError::TransparentCycle` and the registry check; `AppTypeRegistry` lives here; `GenericTypeCell` uses `PendingInfo` and `OnceLock<RwLock<HashMap>>`.
- **#33 (containers):** `GenericTypeCell` leak-on-vacant rule and its counted-leak test.
- **#34 (derive):** unqualified `type_name` only in `ENGINE_CRATES`; compile errors for `version = 0`, `version` plus `transparent`, a self-referential `transparent`, and a non-`skip` `Handle<T>` field.
- **#36 (schema hash):** Value `ref` is the repr only; `Num` equals `f64`, `Guid` equals `u128`, `NonZeroU32` equals `u32`; `SchemaTable.root_ref`.
- **#37 (core impls):** the criterion "`Handle<T>`/`RawHandle`" is edited to "`RawHandle`" (there is no `Handle<T>` impl); `Num` is a Value kind, not `transparent`; add `NonZeroU32`, `Mat2`, `Affine2`, `DVec2` to the list; glam SIMD rule; the pinned-name test.
- **#50 (ECS hook):** `ReflectedMut` stamps in `as_reflect_mut`; hand-written `Entity` impl; `Entity::to_bits` pin (the ECS spec carries it too); `AppTypeRegistry` as a resource.
- **#23/#24 (text, binary):** read leaves through `value_ref`, write them through `set_value`; the text format consults `ReflectTextForm`, the binary format never does; #24 chooses one of the three migration-decode options in "Header table and versions".
- **#59 (migrations):** `from_version = 0` means "undeclared"; `FileNewer` is never migrated.

### Dependencies (high-risk area: extra review)

New dependencies are a high-risk area (CLAUDE.md, ADR-0005): there is no human gate, the PR body justifies them and the reviewer checks them. Added to `[workspace.dependencies]` once, used only by `exitium_reflect_derive` (#34):

| Crate | Version req | Features | Why |
|---|---|---|---|
| `syn` | `2` | `default-features = false`, `derive`, `parsing`, `printing`, `proc-macro` | Parses struct/enum/generics/attribute syntax with correct spans and error messages. |
| `quote` | `1` | default | Builds the generated impls as readable token templates. |
| `proc-macro2` | `1` | default | Token types for `syn`/`quote`; lets the macro logic be unit-tested outside a proc-macro context. |

All three are `MIT OR Apache-2.0`, already allowed by `deny.toml`. They run only at compile time: they are not linked into games or wasm output. The ecosystem crates this engine will use anyway (`serde_derive`, `mlua`, `wgpu`, `egui`) already pull them in transitively, so the practical new supply-chain surface is near zero. Exact patch versions come from `Cargo.lock` at implementation time. `exitium_reflect` itself takes no direct external dependency (it sees `glam` only through `exitium_core`, which added it in #88).

Alternatives rejected: raw `proc_macro` token parsing (zero deps but roughly 1500 lines of fragile parsing, poor error spans, harder to review); `macro_rules!` (cannot read struct attributes or handle generics/enums reliably); `darling`/`venial`/`proc-macro-crate` (extra dependencies for savings of about 150 lines); adopting `bevy_reflect`/`facet` (large dependency trees, outside control of our hash stability and `Num`/`Guid` data model).

## MCP / editor surface

Agent parity (design rule 1): reflection is read-only metadata plus mutation primitives. Mutation reaches users only through command-bus commands (design rule 3). Reflection is also what lets those commands be typed without a hand-written schema (design rule 2). No `exitium_reflect` code ships an editor-only feature.

Tool names are snake_case `<domain>_<verb>` (dots are not valid in many MCP clients). This spec owns only the `reflect_*` tools. **Entity, component and resource access is owned by #21** (`ecs_get_entity`, `ecs_get_component`, `ecs_query_entities`, and the `SpawnEntity`/`InsertComponent`/`RemoveComponent`/`SetField { entity, type_name, path, value }` commands with their undo records); this spec defines none of them and does not re-state their payloads. It only guarantees the primitives they are built on.

| Capability | MCP tool (in `exitium_mcp`) | Editor | Built from |
|---|---|---|---|
| List registered types (name, kind, `SchemaHash`, version, docs) | `reflect_list_types` | type picker | `TypeRegistry::iter` |
| JSON Schema of one type, or all types (offline dump, PLAN phase 4) | `reflect_get_type`, `reflect_dump_schema` | inspector layout | `TypeInfo` + `Attributes` |
| Schema hash and header table of a type, for migration warnings | `reflect_get_schema_hash` (renamed from `reflect_get_schema` so it is not mistaken for `reflect_get_type`, which returns the JSON Schema) | migration warnings | `deep_hash`, `schema_table` |

Primitives provided for #21's tools (named so the parity test can map them): `ReflectComponent::get`/`get_mut`/`insert`/`remove`, `GetPath::reflect_path`/`reflect_path_mut`, `Reflect::apply`/`clone_boxed` (undo snapshot), `ReflectDefault` (add a component with defaults), `ReflectTextForm` and the text format (#23) for values in and out.

Tool input and output schemas are generated from `TypeInfo`, never hand-written. Values cross MCP in the text format (#23). **Text-form types are strings on the wire**, so the generator consults `ReflectTextForm` before the structure: a type with a text form emits `{"type": "string"}` (plus `"pattern"` from `ReflectTextForm::PATTERN` where one exists), never the integer that `Guid`'s `u128` repr or the object that `Entity`'s `{index, generation}` structure would give. Examples: `Guid` is a string matching the 36-character UUID pattern, `Entity` is a string matching `^[0-9]+v[1-9][0-9]*$`, and a `HashMap<Entity, _>` is an object with those strings as keys. A validated Value without a text form follows its repr: `Num` is `{"type": "number"}` (`range` becomes `minimum`/`maximum`), `NonZeroU32` is an integer with `minimum: 1`. A test in `exitium_mcp` checks that the schema type of every registered text-form type is `string`. The `exitium_mcp` parity test (PLAN phase 4 item 6) must see an MCP equivalent for every inspector command. Inspector widgets read `range`, `hidden`, `tooltip`/`docs` and `custom("widget")`. `hidden` fields are not shown in the inspector but remain visible and editable through MCP (marked `x-exitium-hidden`). This spec adds no tools itself; the tool list above is the contract the Phase 4 issues implement.

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
- **Validated Values (`Num`, `NonZeroU32`).** NaN and `-0.0` written into a `Num` through each route: `set_field`/`reflect_path_mut` then `set_value`, `Reflect::apply` from an `f64` value and from a `DynamicStruct` field, `FromReflect::from_reflect`, **text decode**, and **binary decode of a `Vec<Num>`** (a NaN element is an error and the whole decode fails; `-0.0` is stored as `0.0`; `inf` saturates to `Num::MAX`). In every case no `Num` ever holds NaN or `-0.0` (assert with `to_bits`), a rejected write leaves the value unchanged, and `Num == Num` and `Hash` stay consistent afterwards. `range` on a `Num` field is accepted and a `Num` does not downcast to `f64` but encodes identically through `value_ref`. Likewise `0` into a `NonZeroU32`, a `RawHandle.generation` and an `Entity.generation` is rejected by `set`, `apply`, `from_reflect`, text and binary decode, and `field_at_mut("generation")` cannot be used to write it.
- **Threading:** `GenericTypeCell` first use of `Vec<Vec<f32>>`, `Pair<Pair<u8,u8>,u8>` and `Option<Option<u32>>` from 8 threads: no deadlock, equal `&'static` references, only one leaked value per instantiation (counted, **including the composed name**: a forced lost race leaks nothing); `TypeRegistry`, `TypeInfo`, `ReflectComponent` and every `TypeData` in the crate are `Send + Sync` (compile-time assertion); concurrent readers with one writer registering a late type.
- **Method names:** a test with `Reflect` and `Typed` both imported calls `Wallet::type_info()` and `wallet.reflect_type_info()` and compiles (guards against the E0034 ambiguity coming back).
- **Paths:** every grammar production, error `segment_index`, enum and option traversal, a randomized no-panic test on arbitrary strings (fixed seed, std only). Map key segments for every row of the key table: `HashMap<ResourceKind, Num>` with `resources["Gold"]` (and an unknown variant is `BadKey`), `HashMap<u32, _>` with `[12]`, `HashMap<i32, _>` with `[-3]`, `HashMap<String, _>`, `BTreeMap<Guid, _>` and `HashMap<Entity, _>` with `["42v1"]` (digits rejected), and `[3]` on a list versus on a map.
- **Transparent cycle:** `#[reflect(transparent)] struct L(Box<L>)` is a derive error, and the hand-built `TypeInfo` equivalent registered directly is `SchemaError::TransparentCycle`, not a hang or stack overflow.
- **`apply`/`clone_boxed`/`reflect_eq`:** per row of the apply table, including variant switching and map key removal; round-trip "build, mutate dynamically, compare".
- **Derive:** nested structs, generics (`Pair<A, B>`), all attributes, `skip`+`default` through `from_reflect`, `transparent`, recursive type via `Box`, `crate = ...` and `ecs_crate = ...` overrides, `#[reflect(Default, TextForm)]` compiling in a crate that imports nothing, `#[reflect(Component)]` on a type that is also `derive(Component)` (test in `exitium_ecs`), and the same attribute on a type that is *not* a `Component` failing with a trait-bound error. Crate-path resolution: engine crate names resolve to `::exitium_reflect`, any other name (including `exitium_mygame`) to `::exitium::reflect`, and the `crate_paths.rs` drift test over `crates/`. `compile_fail` doc tests (no `trybuild`) cover unknown attribute, `type_name` without `::` in a user crate (and that the same override is accepted in an `ENGINE_CRATES` crate) and unsupported input (including a non-`skip` `Handle<T>` field, `version = 0` and `version` on a `transparent` type). **Pinned names:** `PINNED_TYPE_NAMES` equals the names actually registered for the core types (spelled out in the test), and `Entity`'s registered name is in it (test in `exitium_ecs`). **Core impls (#37):** `RawHandle`, `Rect`, `Color` and every glam type round-trip through `reflect_path`/`set`/`from_reflect`; `Vec4`, `Quat` and `Mat2` are exercised on the host SIMD target, and the same source builds for wasm32.
- **Text form:** `Guid` `to_text` equals `Display` and round-trips; `Entity` is `"42v1"` and `"0v0"`/missing generation is an error; map keys of these types; binary output ignores the hook. `Entity::to_bits() == (generation << 32) | index` and `to_bits().to_le_bytes()` equals the binary encoding of the reflected `{index, generation}` structure (test in `exitium_ecs`). The MCP schema of `Guid` and `Entity` is `string` with the pattern.
- **Manual-impl parity:** `tests/manual_impl.rs` hand-implements the sample types and asserts identical `TypeInfo` and behavior to the derive.
- **Schema hash** (three vector layers, see "Schema hash"): FNV-1a standard vectors; hand-built `TypeInfo` to exact byte stream to shallow hash (`demo::Pt` first); derived sample types to deep hash. Properties: changing a field name, order, type, variant name/order or array length changes the hash; adding `tooltip`/`range`/`default`/`hidden`/`version` does not; `HashMap` vs `BTreeMap` equal; `Vec<Num>` equals `Vec<f64>`; `[u8; 0]` differs from `Vec<u8>`; `transparent` equals inner; `Num` equals `f64`, `Guid` equals `u128` and `NonZeroU32` equals `u32` (each a Value kind with its own name that the hash never sees); every length written with `u32::try_from` (a test feeds a >`u32::MAX` length through the encoder's length helper and expects `TooLarge`). **Cycles:** for self recursion and for mutual recursion `A -> B -> A`, the shallow hash of each member is identical whichever root started the walk; `deep(A)` and `deep(B)` are each stable, differ from each other, and do not change if `deep(B)` is computed before or after `deep(A)` or after registering in the other order (no memoization poisoning); a diamond of depth 20 hashes in time linear in node count (guards the exponential case).
- **Header table:** `schema_table` for a nested type lists exactly its named closure sorted by name with declared versions (`0` when undeclared); a reader-side comparison classifies equal, version-differs and hash-differs-at-same-version correctly; changing one nested type changes only that row's `shallow`. The deep hash recomputed from `root_ref` and the rows equals `deep` for a named root and for the anonymous root `Vec<Wallet>`. Classification also covers a row only in the file (removed type), a row only in the registry (added type), file version greater than current (`FileNewer`), and `0 -> 1` for a type that gains its first version. If #24 picks the embedded-schema option, decoding an old positional body into a `DynamicStruct` with field names is tested against a type whose fields were later renamed and reordered.
- **Platform:** `cargo build --workspace --target wasm32-unknown-unknown` passes; no `unsafe`.
- **`strip_docs`:** with the feature, `docs` of a derived type and its fields are empty; without, they equal the `///` text (two test runs in CI). The editor and MCP feature-graph check passes.
- **Benchmarks:** one bench per row of the performance table.

## Open questions

1. **Require `Clone` for all reflected types?** Recommendation: yes in v1 (undo, prefabs and the command bus all need exact copies). A `no_clone` opt-out with structural snapshots can come later if a real type needs it.
2. **Stable names default to `module_path!()`.** Moves/renames then change the hash. Recommendation: keep the default, require `#[reflect(type_name)]` on persisted types, enforced by the `schemas.lock` check (question 5).
3. **`#[reflect(version = N)]`:** it is stored in `TypeMeta`, written to the binary header table next to each type's shallow hash, and never hashed (see "Schema hash"). Keep it in reflect, or let `exitium_serial`/`exitium_save` own versions entirely (#24, #59)? Recommendation: keep it here (the derive is where type authors already look; the hash/version decision table is defined here), with reader and migration policy owned by #24 and #59.
4. **Hash width/algorithm:** 64-bit FNV-1a by hand (this spec) vs xxhash64 (hand-written, longer) vs a 128-bit hash. 64 bits is ample for ~10^3 types and non-adversarial use; `SCHEMA_HASH_VERSION` allows an upgrade. Recommendation: FNV-1a-64.
5. **`schemas.lock` and `cargo xtask schema check`** as a CI guard against unversioned layout changes? It adds a committed file (a serialization artifact, a high-risk area) and an xtask command. Recommendation: yes, as a follow-up issue after #36.
6. **Atomic `apply`.** Current design is non-atomic plus a snapshot taken by the caller. A validate-then-mutate `apply` costs a second walk. Recommendation: defer unless the command bus finds the snapshot too costly.
7. **Docs in shipping builds.** `strip_docs` (on `exitium_reflect`, enabled only through the facade feature for a game's release build) empties `docs` strings. Should shipping builds default to stripped? Recommendation: yes, via `cargo xtask package`, and keep docs in editor/MCP builds.
8. **No `usize`/`isize`, and `i128`/`u128` included** (for `Guid`). Acceptable? Recommendation: yes, explicit widths keep the binary format platform-independent.
9. **Runtime-defined types** (a data asset schema authored in text with no Rust struct) need a non-`'static` `TypeInfo` or a leak-on-load scheme. Deferred to the `exitium_data` spec; this model does not preclude it because `TypeRef` is already indirect.
10. **Sets, `VecDeque`, `Duration`.** Out of v1 (see Non-goals). `HashSet`/`BTreeSet` fit a `Set` kind or a list-with-uniqueness, `VecDeque` is a `List` impl, `Duration` is a struct `{secs: u64, nanos: u32}`. Recommendation: add them in a follow-up slice once a real game type needs one, with the owner filing the issue; nothing here precludes them. Related, **zero-copy bulk list access** (an `as_bytes` hook for `Vec<f32>` and similar). The latter is the binary-format spec's call (#24); reflect currently exposes `ListInfo.item` so a serializer can special-case with a downcast.
11. **Enum variant stable ids.** Variant `index` is declaration order, so reordering variants is a breaking change (hashed). Should `#[reflect(index = N)]` pin ids so reordering is safe? Recommendation: not in v1; revisit with the first real save-migration need.
12. **`trybuild` compile-fail tests.** A dev-dependency (justified in the PR body, extra review). Recommendation: stay with `compile_fail` doc tests until the owner asks.
13. **Typed asset references and `Handle<T>`.** `Handle<T>` (#15) is a process-local arena id and gets no `Reflect` impl in v1; only `RawHandle` reflects. Persisted asset references need a stable form. Default: `AssetRef<T>` in `exitium_assets`, a `Guid`-backed wrapper reflected as a transparent `Guid`, whose stable name comes from an `AssetKind::NAME` constant on the target so `T` needs no `Reflect` bound. Alternative: reflect `Handle<T>` as `Guid` after an asset-table lookup at serialize time, which needs a context argument the serializer does not have. Recommendation: the default, decided in the assets spec. #37's `Handle<T>/RawHandle` criterion is edited to `RawHandle` at merge of #78 (see "Downstream issue adjustments"). `RawHandle` itself is for debug and inspector display and persists only through a remap. `AssetRef<T>` can be `transparent` because its single field, a `Guid`, is freely reachable and is itself a Value.
14. **Run golden vectors on wasm32 in CI.** The encoder is width-independent by construction, but CI only builds wasm. Recommendation: a follow-up CI step running the schema tests under `wasmtime` (`wasm32-wasip1`). It adds a CI tool (CI config is a high-risk area, and the factory App cannot push workflow changes, so the owner pushes that part).
15. **`ReflectedMut` shape.** Settled by #78 to align with the ECS spec: a dedicated guard type named `ReflectedMut` (not `ReflectMut`, which is the kind-view enum) wrapping the typed `Mut<T>`, stamping in `as_reflect_mut`, with no `DerefMut`. Still open for #21/#50: whether to also offer `Deref<Target = dyn Reflect>` for reads (recommendation: no, `as_reflect()` only, one spelling).
