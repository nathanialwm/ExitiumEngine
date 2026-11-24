# Spec: Serialization binary format, schema hashing and migrations

- Issue: #24
- Crates: `exitium_serial` (touchpoints: `exitium_reflect`, `exitium_save` (SP14), `exitium_net` (SP16), `exitium_mcp`, `exitium_editor`)
- Status: draft
- Scope: Full MVP. The binary container, body encoding, schema embedding and verification, migrations, the LZ4 codec and the zero-copy read path. The ECS world snapshot is **not** here: it lives in `exitium_save` (C7, SP14).

## Goal

Define the compact shipping format for saves, shipped assets and net snapshots: byte-exact, deterministic, endian-fixed, schema-checked, safe to read from untrusted bytes, and fast enough that a 32 MB bulk-array world moves at memory speed. Like the text format (#23), it is derived entirely from `exitium_reflect` (design rule 2): the encoder walks a reflected value, the decoder walks bytes and a `TypeInfo` together, and nothing in the format is hand-written per type.

This spec fixes the byte layout, the canonical-form rules, how the reflect spec's schema table is embedded and verified, what a reader does on every mismatch (reject, migrate or conform), the migration model, the compression container and its dependencies, the zero-copy story and why it needs no `unsafe`, the robustness limits, the public API (including the multi-value session API that `exitium_save` builds the world snapshot on), and the implementation issues.

## Non-goals

- **The ECS world snapshot** (#61). `exitium_serial` has no `exitium_ecs` edge (C7). The binary snapshot of a world is built in `exitium_save` from the session API in this spec, with the `Entity` remap hook in "References".
- **Encryption, authentication and tamper detection.** `exitium_save` (SP14) wraps this container. The CRCs here detect accidental corruption only and are not a security feature.
- **A zstd codec.** The container reserves codec id 2 and a pluggable `Codec` trait; SP14 decides whether and how zstd ships, including on the web (the roadmap's open question 6). This crate ships LZ4 only.
- **The text format and its leaf spellings** (#23, `serial-text-format.md`). The binary format never consults `ReflectTextForm` (reflect spec) and shares only the `RefSite` hook shape.
- **Streaming or seekable access** to a file larger than memory, random access into a body, and in-place patching. A file is verified and decoded whole.
- **Cross-version forward reading.** A file from a newer format version or a newer type version is an error, never a guess (`FormatNewer`, `FileNewer`).
- **A JSON codec and MCP tools.** The generic tools (#110) build on the capability table in "MCP / editor surface".
- **Layouts for structs of fixed leaves** (`Vec<Vec2>` as one flat slab). Only lists of primitive leaves are bulk in v1 (open question 3).

## Design

### Crate and dependencies

`exitium_serial` already depends on `exitium_reflect` and, for tests, on `exitium_core` (text spec). The binary format adds **two external dependencies** and no crate edge: `lz4_flex` (the LZ4 codec) and `bytemuck` (safe byte casts for zero-copy), both for intake 1 (see "Dependencies"). The crate stays free of `unsafe` and builds for wasm32. Library code reads leaves through `Reflect::value_ref` and writes them through `set_value`, never naming `Num`, `Guid` or `Entity`.

### Decisions

| Question | Chosen | Why |
|---|---|---|
| Body layout | **Positional, type-directed, no tags.** The schema says what comes next. | Smallest and fastest. The embedded schema makes it self-describing (below). |
| Integers and floats | **Fixed-width little-endian leaves.** Varints only for structure (lengths, variant indices, string indices) and for the optional packed integer arrays. | Fixed width is branch-free, casts to slices for bulk data, and has an exact minimum size per type for the bounds checks. LZ4 recovers most of what varint leaves would save. |
| Schema in the file | **Embedded shallow streams** (reflect spec, migration decode option 2). | Decodes an old body with no old Rust type, deterministically; costs tens of bytes per named type. Rejected: keeping old types registered (option 1: code the game carries forever) and text-form migration (option 3: needs the old build). |
| Zero-copy | **`bytemuck` casts on primitives, an 8-byte-aligned buffer, no `unsafe`.** | See "Zero-copy and soundness". |
| Compression | **LZ4 block, chunked at 256 KiB, through a pluggable `Codec`.** Codec id 2 reserved for zstd. | `lz4_flex` is pure Rust and safe, so it builds on wasm32 with no toolchain trouble. |
| Strings | **One string table per file**, a varint index per `String` value. | A save repeats the same names, tags and asset paths thousands of times. |
| Canonical form | **One valid encoding per value** (checked on read). | Deterministic bytes make golden tests, diffs of saves and content hashes meaningful, and rule out a class of parser-differential bugs. |

### Layers

```
&dyn Reflect --encode(registry, refs)--> body bytes + string table
                                          + schema section (from registry.schema_table)
                                          --container(codec)--> file bytes

file bytes --container(verify CRCs, decompress, limits)--> header + FileSchema + payload
   payload + FileSchema --plan(registry, policy, migrations)--> DecodePlan
   body --decode(plan)--> Dynamic* tree (post-order adapt: migrate or conform) --FromReflect--> typed value
```

`inspect` (header and schema, no registry) and `compare_schema` (per-type status against a registry) sit beside this and never decode a body.

### File layout

A file is: a **64-byte header**, a **schema section**, and a **payload**. Everything is little-endian. Every section starts at a multiple of 8 bytes from the start of the file, and every padding byte is zero (a non-zero pad byte is `NonZeroPadding`).

```
offset 0                    header (64 bytes)
offset 64                   schema section (schema_len bytes, multiple of 8)
offset 64 + schema_len      payload (stored_len bytes)   = codec(raw payload)
```

The **raw payload** (after decompression; equal to the stored payload for codec 0) is: the **string table** (`strtab_len` bytes, multiple of 8), then the **body**, then zero padding up to a multiple of 8 bytes. Alignment inside the body is measured from the start of the raw payload, which is itself 8-aligned in memory (see "Zero-copy and soundness"), so an offset that is a multiple of 8 in the raw payload is 8-aligned in memory.

#### Header (64 bytes)

| Offset | Size | Field | Rule |
|---|---|---|---|
| 0 | 8 | `magic` | `89 45 58 42 0D 0A 1A 0A` (`\x89EXB\r\n\x1a\n`): a non-ASCII first byte so text tools do not treat the file as text, and the line-ending pair and `1A` catch text-mode and newline conversions (the PNG scheme). Any other value is `BadMagic`. |
| 8 | 2 | `format_version` | `u16`, `BINARY_FORMAT_VERSION = 1`. `0` is `BadHeader`. Greater than the reader's is `FormatNewer { file, supported }`, **decided right after the magic, before any other field is trusted** (so a newer layout reports `FormatNewer`, never a CRC or length error). |
| 10 | 2 | `flags` | `0` in v1. Any set bit is `UnsupportedFlags` (a flag may change how the rest is read, so unknown bits are never ignored). |
| 12 | 1 | `codec` | `0` none, `1` LZ4 chunked, `2` reserved (zstd). Any other, or one the reader's `Codecs` lacks, is `UnknownCodec(id)`. |
| 13 | 1 | `hash_version` | The reflect spec's `SCHEMA_HASH_VERSION` the file was written with (`1`). Greater than the reader's is `HashVersionNewer`. Every embedded stream starts with this byte (checked). |
| 14 | 2 | reserved | `0`. |
| 16 | 4 | `schema_len` | Bytes of the schema section including its padding. |
| 20 | 4 | reserved | `0`. |
| 24 | 8 | `stored_len` | Bytes of the stored payload (the compressed bytes for codec 1). |
| 32 | 8 | `raw_len` | Bytes of the raw payload, a multiple of 8. For codec 0 equal to `stored_len`. |
| 40 | 4 | `strtab_len` | Bytes of the string table at the start of the raw payload, a multiple of 8. |
| 44 | 4 | reserved | `0`. |
| 48 | 4 | `header_crc` | CRC-32 (IEEE, the zlib polynomial) over bytes `0..48` of the header followed by the whole schema section. |
| 52 | 4 | `payload_crc` | CRC-32 over the stored payload bytes (the compressed bytes for codec 1, so corruption is caught before decompression). |
| 56 | 8 | reserved | `0`. |

The file length must equal `64 + schema_len + stored_len` exactly: a shorter file is `Truncated`, extra bytes are `TrailingBytes`. A wrapping container (SP14) hands this crate exactly the file's bytes. The reserved fields are checked to be zero, so a later version can give them meaning without older readers misreading them.

#### Schema section

The reflect spec's schema table, embedded so an old body can be decoded without the old Rust types. It holds one or more **roots** and the union of their closures:

```
u32 root_count                                  (>= 1)
root_count x { u64 deep_hash
               u32 root_ref_len
               root_ref bytes }                 (the canonical ref(root) of the reflect spec)
u32 entry_count
entry_count x { u32 version                     (declared #[reflect(version = N)], 0 if undeclared)
                u32 stream_len
                u64 shallow_hash
                stream bytes }                  (the exact shallow stream, version byte first)
zero padding to a multiple of 8
```

Entries are sorted bytewise by stable name (the order of the deep hash), with no duplicate name. A single-root file has the same rows as `SchemaTable` for that root. For several roots (the session API) the rows are the union, sorted; two roots that reach the same name with different shallow hashes cannot both be written (`AmbiguousName`, impossible for types from one registry).

**What a reader verifies.** `shallow_hash == FNV-1a-64(stream)` for every entry, so the streams are exactly what the hash consumed and cannot drift from it (`SchemaCorrupt` otherwise). It then parses each stream with the grammar of the reflect spec's "Shallow hash" and "Type reference" tables into a `FileSchema`: roots as `FileRef` values, and per named type its kind, field and variant names in order, and each field's `FileRef`. A `FileRef` is `Value(repr)`, `List { fixed: Option<u64>, item }`, `Map(key, value)`, `Option(inner)`, `Tuple(items)` or `Named { kind, name }`. Every `Named` ref must name an entry (`SchemaCorrupt` otherwise), nesting is limited to 32 levels, and the entry count and total bytes are limited (`BinaryLimits`). The `deep_hash` of each root is checked against the registry (not recomputed from the file), as the first step of "Verification and mismatch".

The schema section is covered by `header_crc`, so a bit flip in a stream is caught before it is parsed.

#### String table

The first part of the raw payload.

```
u32 count                       (>= 1; entry 0 is always the empty string)
u32 data_len
count x u32 end                 (cumulative end offsets into data; end[0] = 0; non-decreasing; last = data_len)
data bytes                      (data_len bytes, UTF-8)
zero padding to a multiple of 8
```

Entry `i` is `data[end[i-1]..end[i]]` with `end[-1] = 0`. Every entry must be valid UTF-8 (`BadUtf8`), entries are **unique** (a duplicate is `NonCanonical`), and `strtab_len` must equal the padded size. The encoder assigns indices in **first-use order** over the canonical walk, so the table is deterministic (maps are walked in canonical key order, below). A `String` value in the body is a varint index. Type, field and variant names are **not** in this table; they live only in the schema section.

#### Body

The body is the concatenation of the values the writer was asked to write, each encoded as below. The one-shot API writes one value (the root). The session API writes any sequence of values of the registered roots, with caller framing in between (varints).

| Kind | Encoding |
|---|---|
| `bool` | 1 byte, `0` or `1` (anything else is `BadBool`). |
| `char` | `u32` little-endian; must be a Unicode scalar value (`BadChar`). |
| `i8` to `i128`, `u8` to `u128` | Fixed width, little-endian, two's complement. |
| `f32`, `f64` | IEEE 754 bits, little-endian. **Every bit pattern is stored exactly**, NaN payloads and `-0.0` included. A validated Value that rejects them (`Num`) does so on decode (below). |
| `()` | 0 bytes. |
| `String` | varint string-table index; `>= count` is `BadStringIndex`. |
| Struct, tuple struct, tuple | The fields in declaration order, back to back. No count, no names, no padding. A `transparent` type has no encoding of its own (it is its inner field), matching the hash, which erases it. |
| Enum | varint variant index (declaration order), then that variant's fields as a struct. An index that is not a variant of the schema is `BadVariant`. |
| `Option<T>` | 1 byte, `0` for `None`, `1` for `Some`, then the inner value if `1` (anything else is `BadOptionTag`). |
| `Vec<T>` | varint length, then the elements (**bulk layout** when eligible, below). |
| `[T; N]` | The elements, no length (it is in the schema). Bulk layout when eligible. |
| `HashMap<K, V>`, `BTreeMap<K, V>` | varint length, then `key, value` pairs in **canonical key order, strictly ascending** (below). The Rust container is not recorded: it is not in the hash either. |

**Varint** is unsigned LEB128 for a `u64`: 1 to 10 bytes, seven bits per byte, high bit means "more". It is **canonical**: no redundant trailing zero group (the last byte is non-zero unless the value is the single byte `0`) and the tenth byte is at most `1`. A non-canonical or overlong varint is `NonCanonicalVarint`. Lengths and indices are `u64` on the wire but checked against `u32::MAX` and the limits before use, so `usize` width never matters (wasm32 and 64-bit read the same file).

**Bulk arrays.** A list or array whose **item**, as the schema hash sees it (after transparent resolution; a validated Value counts as its repr, so `Vec<Num>` is a list of `F64`), is a fixed-size primitive leaf (`Bool`, `Char`, `I8` to `I128`, `U8` to `U128`, `F32`, `F64`; not `Unit`, not `String`) and whose length is **at least 16** uses the bulk layout instead of per-element encoding:

```
[varint len]            only for Vec<T>; absent for [T; N]
u8 tag                  0 raw, 1 packed
zero padding            to the next multiple of 8 (measured from the start of the raw payload)
data
```

- **Tag 0, raw (the zero-copy form).** `len` elements in their fixed little-endian leaf encoding, back to back, starting 8-aligned. This is the same bytes the elements would have had inline; only the alignment and the tag are added. On a little-endian host, a reader hands the slice out without touching it (see "Zero-copy and soundness").
- **Tag 1, packed.** Only for the integer reprs `I16`, `I32`, `I64`, `I128`, `U16`, `U32`, `U64`, `U128` (not `I8`/`U8`, where raw is already one byte, and never floats, `Bool` or `Char`). The data is `varint byte_len`, then exactly `byte_len` bytes holding `len` canonical varints, each the value (unsigned) or its **zigzag** form (`(v << 1) ^ (v >> (bits-1))`, signed). No alignment padding applies to the data: it starts right after the tag and `byte_len`, and the 8-byte padding is not written for tag 1. A varint that does not fit the element width is `OutOfRange`. Tag 1 on an ineligible repr is `BadBulkTag`.
- **When each is written.** Eligibility is decided by the schema alone. Within eligible integer lists the writer picks the tag from `WriteOptions::int_arrays`: `Raw` (default) or `Packed`. A given option gives one byte string, so output stays deterministic; a reader accepts either tag for any eligible list, so no reader needs the option. `Raw` suits tile maps, IDs and anything LZ4 will see; `Packed` suits saves full of small counters where no compressor is available (the web build without zstd), at about 3 ns per element on decode instead of a memcpy.
- Fewer than 16 elements are written inline, element by element, with no tag or padding. The threshold is part of the format; the decoder learns the layout from `len` (growable) or from the schema (`N`).

**Maps and canonical key order.** Reflect already restricts keys to leaf Values (not floats), their transparent wrappers and Value newtypes (`Guid`), unit-only enums, and `ReflectTextForm` types (`Entity`). The writer sorts entries by key before writing, whatever the Rust container (a `HashMap` and a `BTreeMap` with a custom `Ord` give the same bytes):

| Key | Order |
|---|---|
| ints | numeric (signed numerically, so `-1` before `0`) |
| `bool` | `false` before `true` |
| `char` | by scalar value |
| `String` | by the UTF-8 bytes of the content (not by string-table index) |
| unit-only enum | by variant index |
| `Guid` (a `u128` Value) | numeric |
| a struct key such as `Entity { index, generation }` | field by field in declaration order, each compared as above (index, then generation) |

This is **structural, not textual**: the binary format never consults `ReflectTextForm`, so `Entity` keys sort by `(index, generation)` and not by their `"42v1"` text (the text format sorts by text, `"10v1"` before `"9v1"`). The reflect spec's one-line map-order note is amended to say so (see "Downstream issue adjustments"). The decoder requires **strictly ascending** keys: equal keys are `DuplicateKey`, a smaller key is `KeyOrder`. This is what makes the encoding canonical and a duplicate-key bomb impossible.

**Canonical form.** For a fixed `(value, registry, options)` there is exactly one valid byte string (for codec 0; see "Compression container" for codec 1). The reader enforces it: canonical varints, zero padding, unique strings, ascending map keys, `0`/`1` bools and option tags, exact lengths, reserved fields zero. Hence `encode(decode(bytes)) == bytes` for every file the reader accepts under the writer's options, which the round-trip test pins. Output does not depend on pointer width, hash-map order, thread count or host endianness.

#### Worked example

`struct demo::Pt { x: f32, y: f32 }` with `Pt { x: 1.0, y: 2.0 }`, codec 0. The schema numbers are the reflect spec's vectors (shallow `7085092b080ffcd2`, deep `2a7a8a41e74c164b`); the file is 176 bytes:

```
0000  89 45 58 42 0d 0a 1a 0a 01 00 00 00 00 01 00 00    magic | version 1, flags 0, codec 0, hash_version 1, reserved
0010  58 00 00 00 00 00 00 00 18 00 00 00 00 00 00 00    schema_len 88, reserved | stored_len 24
0020  18 00 00 00 00 00 00 00 10 00 00 00 00 00 00 00    raw_len 24 | strtab_len 16, reserved
0030  06 9d 66 61 18 d5 f9 69 00 00 00 00 00 00 00 00    header_crc 61669d06, payload_crc 69f9d518, reserved
0040  01 00 00 00 4b 16 4c e7 41 8a 7a 2a 0d 00 00 00    schema: root_count 1, deep 2a7a8a41e74c164b, root_ref_len 13
0050  02 08 00 00 00 64 65 6d 6f 3a 3a 50 74 01 00 00    root_ref = 02 "demo::Pt" | entry_count 1
0060  00 00 00 00 00 20 00 00 00 d2 fc 0f 08 2b 09 85    version 0, stream_len 32, shallow 7085092b080ffcd2
0070  70 01 02 08 00 00 00 64 65 6d 6f 3a 3a 50 74 02    stream: 01 02 "demo::Pt" 02000000
0080  00 00 00 01 00 00 00 78 01 0d 01 00 00 00 79 01              "x" 01 0d  "y" 01
0090  0d 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00    0d | 7 pad bytes | strtab: count 1, data_len 0
00a0  00 00 00 00 00 00 00 00 00 00 80 3f 00 00 00 40    end[0] 0, 4 pad bytes | body: 1.0f32, 2.0f32
```

These bytes are the first golden file (computed with the layout above and the CRC of the zlib standard, whose check value `CRC-32("123456789") = cbf43926` is the first CRC vector).

### Compression container

`codec` selects how the raw payload is stored. The trait is the only thing the container knows:

```rust
pub trait Codec: Send + Sync {
    fn id(&self) -> u8;
    /// Append the stored form of `raw` to `out`. Deterministic for a given codec version.
    fn compress(&self, raw: &[u8], out: &mut Vec<u8>) -> Result<(), CodecError>;
    /// Fill `out` (exactly `raw_len` bytes) from `stored`. Any shortfall or excess is an error.
    fn decompress(&self, stored: &[u8], out: &mut [u8]) -> Result<(), CodecError>;
}
pub struct Codecs { /* id -> Arc<dyn Codec> */ }   // Codecs::default() has 0 (none) and 1 (LZ4)
```

- **Codec 0, none.** `stored == raw`.
- **Codec 1, LZ4 chunked.** The raw payload is cut into **256 KiB** chunks (the last may be shorter, none empty). Each stored chunk is `u32 raw_len`, `u32 stored_len`, then `stored_len` bytes. If LZ4 does not make a chunk smaller, the writer stores it **raw** with `stored_len == raw_len`; the reader treats `stored_len == raw_len` as raw, and the writer never emits an LZ4 block of that size, so the form is unambiguous. Each chunk decompresses into its slot of the single output buffer with `lz4_flex::block::decompress_into`, which needs the exact output size and fails on any mismatch. Chunking bounds the working set per chunk, makes the bomb check per chunk, and keeps the door open for a parallel codec (a `Codec` impl in `exitium_save` can use tasks; this crate cannot, C1).
- **Codec 2, reserved** for zstd. Decided by SP14; an unknown id is `UnknownCodec`.
- **Determinism.** The raw payload and the CRCs of codec 0 are canonical across platforms and releases. LZ4 output is deterministic for a pinned `lz4_flex` (the version is pinned exactly, see "Dependencies") but is **not** part of the cross-version contract: the reader never depends on how the compressor behaved, and the golden tests pin the codec-0 file byte for byte and the codec-1 file by decoding a checked-in sample.

**Bomb limits** are checked before any allocation, from the header alone: `stored_len <= max_file_bytes`; `raw_len <= max_raw_bytes`; `raw_len <= max_ratio * stored_len` (default ratio 256, which is the structural maximum of LZ4's per-byte expansion, so honest data is never refused); every chunk `raw_len <= 256 KiB`; the chunk `raw_len` values sum to the header `raw_len`; and the chunk `stored_len` values (with their 8-byte headers) sum to `stored_len`. A file that claims more than that is `TooLarge` or `BombRatio` without having cost more than a read of its header. Allocation is one `AlignedBytes` of `raw_len`, which these checks bound by 256 times the bytes the file really has.

### Verification and mismatch

The reader compares the file's schema with the registry **per named type**, following the reflect spec's decision table. A row is a type name with `(version, shallow hash)`; the live side has the same from `registry.schema_table`. `SchemaPolicy` chooses what happens when rows differ.

1. **Root.** Each root is matched to a live type: by stable name when the caller gave none and the `root_ref` is a named ref, otherwise by the caller's expected `TypeId` (`from_binary_as`, needed for an anonymous root such as `Vec<Wallet>`). The live `ref(root)` bytes must equal the file's `root_ref`, or both must name the same type; otherwise `RootMismatch { file, expected }`.
2. **Fast path.** If a root's `deep_hash` equals the registry's deep hash for that root, every row in its closure is unchanged (the deep hash covers all shallow hashes), so the body decodes directly with no per-row work.
3. **Otherwise per row**, by the reflect spec's table:

| Row | `Strict` (default) | `Lenient` |
|---|---|---|
| version equal, shallow equal | decode directly | same |
| file version < current | run the registered migration chain; **no chain: `NoMigration { type_name, file, current }`** | chain if registered, otherwise **conform** by name (below) with a `Conformed` warning |
| file version > current | `FileNewer { type_name, file, current }`. **Never migrated, never downgraded** | same: `FileNewer` |
| version equal, shallow differs (an unversioned schema change) | a migration with a matching `from_hash` if registered, otherwise `SchemaMismatch { type_name, version, file_shallow, current_shallow }` | same migration, otherwise conform |
| type only in the file (removed or renamed) | error unless a migration names it (`to_name` for a rename, or one that drops it) | same, and a bare removed type that nothing reaches is ignored |
| type only in the registry (added) | not an error by itself | same |

The first error in the closure is reported, naming the type and both versions or hashes. Nothing is "guessed": a body that no rule above can decode is an error. `Lenient` exists for development and editor caches (a dev loads yesterday's file after changing a struct); `Strict` is what saves, net and shipped files use.

**Conform (Lenient only).** The body is read with the **file's** schema (it is positional, so it must be), then each file type is adapted to the live type **by name**, the same rules as the text format's forward compatibility:

- Struct: each live field is matched to a file field by name; the type of a matched field must conform recursively (a leaf repr must match exactly, so `U32` to `U64` is `TypeChanged`, because widening is a migration, not a guess). A live field **missing from the file** is filled **only** from its explicit `#[reflect(default)]` (warning `DefaultedField`); otherwise `MissingField`. A file field **missing from the live type** is dropped (warning `DroppedField`); its bytes were still consumed positionally.
- Enum: variants match by **name**, not index. A file variant the live type lacks is `UnknownVariantName`. Fields conform as for a struct.
- Lists, arrays (same length), maps, options and tuples conform element-wise. A change of container kind is `TypeChanged`.

A conformed value is as valid as any other: it still goes through `FromReflect` and every validated Value constructor.

### Migrations

A migration upgrades the decoded value of **one named type** from one declared version to a later one. It is the same model the saves crate uses (SP14 owns the registry of a game's migrations; this crate owns the mechanism).

```rust
pub struct Migration {
    pub type_name: &'static str,            // the type's name in the file (its old name for a rename)
    pub from: u32,                          // 0 = the type had no declared version
    pub to: u32,                            // > from, <= the live version
    pub from_hash: Option<SchemaHash>,      // if set, applies only to a file row with this shallow hash
    pub to_name: Option<&'static str>,      // set to rename the type (the result is then a value of this type)
    pub apply: Box<dyn Fn(&mut MigrationCx, DynamicValue) -> Result<DynamicValue, MigrationError> + Send + Sync>,
}
pub struct Migrations { /* (type_name) -> edges */ }
impl Migrations {
    pub fn register(&mut self, m: Migration) -> Result<(), MigrationError>;   // rejects to <= from, a duplicate (type_name, from, to, from_hash)
    pub fn chain(&self, type_name: &str, from: u32, shallow: SchemaHash, to: u32) -> Option<Vec<&Migration>>;
}
pub struct MigrationCx<'a> { pub file_type: &'a FileTypeInfo /* name, version, fields with FileRefs */, /* warnings sink */ }
```

- **`DynamicValue`** is `Box<dyn Reflect>` holding the `Dynamic*` value the decoder built from the **file's** layout (field names from the embedded schema, so a migration can rename, split, merge or drop fields by name) or a primitive. It carries no `represented_type_info` (the old shape has no `TypeInfo`); `MigrationCx::file_type` says what it was.
- **Chain resolution.** For a file row `(name, version = from, shallow)` and live version `to`, the chain is the **shortest path** in the graph whose nodes are version numbers and whose edges are the registered migrations for that name (breadth-first, so a skip edge `1 -> 3` beats `1 -> 2 -> 3`). Edges leaving the *first* node are filtered by `from_hash` when the edge sets it (an edge with a matching hash wins over one without), which is how a type that never declared a version (`from = 0`) is told apart from itself at different times. Later edges are matched by version only (the intermediate shape is whatever the previous migration produced). The chain must end exactly at `to`; ties are broken by the lowest `to` first, then registration order, so resolution is deterministic. The result is cached per file type for the decode.
- **Order of application: bottom-up.** The body is decoded in post-order. When a value of a migrating type has been decoded from its old layout, its chain runs immediately, and the parent then sees the migrated child. A parent whose own row is unchanged is decoded by the same positional layout and needs nothing; a parent that also changed version runs its own chain after its children. A migration never has to know about nested types it does not own.
- **Failure policy: all or nothing.** A migration error, a missing chain (Strict), or a migrated value that `FromReflect` cannot turn into the live type (for example a field the migration forgot to add) fails the **whole** decode with `MigrationFailed { type_name, from, to, source }` (or `NoMigration`) and an offset and reflect path. No partial value is returned. `FileNewer` is never migrated, in either policy. There is no "skip this value and continue".
- **Warnings.** Each applied chain adds a `Migrated { type_name, from, to, count }` warning (one per type and chain, with the number of values), so the editor log and MCP show that a file was upgraded. The caller (saves) usually rewrites the file in the current version afterwards.
- **Cost.** Files whose `deep_hash` matches never touch the migration code. A migrating type pays one closure call per value.

### References and the session API

**Entity remap.** A saved world refers to entities by id, which are meaningless in another world. The binary format cannot name `Entity` (no `exitium_ecs` edge, C7), so it reuses the text format's hook idea. `RefSite` and `RefError` are the text spec's types (`serial-text-format.md`, "References") and live in a shared `refs` module once S11 has landed:

```rust
pub trait BinaryRefs {
    /// Writing. Called once per occurrence of a value whose registered type has `ReflectTextForm`
    /// (`Entity`, `Guid`). Return a replacement to encode instead, or None to encode `value` as is.
    fn map_on_write(&mut self, site: &RefSite<'_>, value: &dyn Reflect) -> Result<Option<Box<dyn Reflect>>, RefError> { Ok(None) }
    /// Reading. The decoder has already built the concrete value with `ReflectFromReflect`
    /// (so generation 0 is rejected before the hook). Return the value to store.
    fn map_on_read(&mut self, site: &RefSite<'_>, value: Box<dyn Reflect>) -> Result<Box<dyn Reflect>, RefError> { Ok(value) }
}
```

The hook sees only types with `ReflectTextForm` registered (a flag computed once per type in the plan, so a decode with no hook, or a type without one, pays nothing per value). It must be **pure** in the same sense as the text hook: called exactly once per occurrence, with the same answer for the same input, and the writer buffers a map's replaced keys so they are sorted by the replacement, never the original. `exitium_save` implements it to map live `Entity` values to stable save ids on write and fresh entities on read. A hook error is `Reference` with the offset and path.

**Session API (several values in one file).** One call writes one value (the world snapshot is thousands of values of different types, plus bulk columns). The session shares one string table and one schema section:

```rust
pub struct BinaryWriter<'r> { /* registry, options, roots, string table, body buffer */ }
impl<'r> BinaryWriter<'r> {
    pub fn new(registry: &'r TypeRegistry, opts: WriteOptions<'r>) -> Self;
    pub fn add_root(&mut self, ty: TypeId) -> Result<RootSlot, BinaryError>;       // lists the type in the schema section
    pub fn write_value(&mut self, slot: RootSlot, value: &dyn Reflect) -> Result<(), BinaryError>;   // appends one value
    pub fn write_len(&mut self, v: u64);                                            // caller framing: a canonical varint
    pub fn finish(self) -> Result<Vec<u8>, BinaryError>;
}
pub struct BinaryReader<'a> { /* container, FileSchema, DecodePlan, cursor */ }
impl<'a> BinaryReader<'a> {
    pub fn open(bytes: &'a [u8], registry: &'a TypeRegistry, opts: ReadOptions<'a>) -> Result<Self, BinaryError>;
    pub fn roots(&self) -> &[RootInfo];                                             // in add_root order, with the file's names
    pub fn root_slot(&self, ty: TypeId) -> Option<RootSlot>;                        // the live type's slot in this file
    pub fn read_value(&mut self, slot: RootSlot) -> Result<Decoded, BinaryError>;   // next value, decoded as that root
    pub fn read_len(&mut self) -> Result<u64, BinaryError>;
    pub fn read_bulk_view<T: Leaf>(&mut self, slot: RootSlot) -> Result<Cow<'a, [T]>, BinaryError>;   // next value as a borrowed slice (below)
    pub fn finish(self) -> Result<Vec<BinaryWarning>, BinaryError>;                  // every body byte consumed, padding zero
}
```

The reader's sequence of `read_*` calls must mirror the writer's; a mismatch surfaces as an ordinary decode error (a bad varint, a bad index, leftover bytes at `finish`). The slots are the schema's roots, so the file still says what it holds and the version table covers every type any value uses. `to_binary` is `new`, `add_root`, one `write_value`, `finish`.

### Zero-copy and soundness

**The decision: `bytemuck` casts, no `unsafe` in this crate.** The alternative was a designated `unsafe` module (`slice::from_raw_parts` with `// SAFETY:` comments and Miri tests). Rejected: it puts a soundness argument on the reviewer for every PR, and the safe route costs nothing at run time. The workspace is `unsafe`-denied, `bytemuck` is already in the graph for `wgpu` and the renderer (so it adds no supply-chain surface), and it does the checks we would otherwise write ourselves (alignment, size multiple, no padding bytes).

How it is used:

- **`AlignedBytes`** owns a `Vec<u64>` and a byte length. `as_bytes()` is `bytemuck::cast_slice::<u64, u8>(&buf)[..len]`, which is safe and gives a slice whose start is 8-aligned. The whole file is read into one (a caller that already holds an 8-aligned `&[u8]`, checked with the safe `<*const u8>::align_offset`, skips the copy; otherwise `from_binary` copies once). A decompressed payload is decompressed into a fresh `AlignedBytes`. File offset `64 + schema_len` is a multiple of 8, so the raw payload of codec 0 is 8-aligned inside the file buffer, and a decompressed payload is aligned by construction.
- **`Leaf`** is a sealed trait implemented for `u8, i8, u16, i16, u32, i32, u64, i64, f32, f64` (all `bytemuck::Pod`, so any bit pattern is valid, which is exactly what makes a cast sound) with `fn from_le(self) -> Self`.
- **Read.** `bytemuck::try_cast_slice::<u8, T>(bytes)` gives `Cow::Borrowed(&[T])` on a little-endian host when the slice is aligned and a whole number of elements (both checked by the call). Anything else falls back to `Cow::Owned`: a copy through `vec![T::zeroed(); n]` and `copy_from_slice` on the byte view, then `from_le` per element on a big-endian host. The fallback produces identical values; a view is purely an optimization and its absence is never an error.
- **Write.** A `Vec<T>` item list is downcast (`as_any().downcast_ref::<Vec<T>>()`, which reflect open question 10 left to this spec) and appended with `extend_from_slice(bytemuck::cast_slice(&v))` on little-endian hosts, element-wise `to_le_bytes` elsewhere. Lists of other types, and `[T; N]` arrays (const `N` cannot be downcast generically), take the element-wise path through `List::iter` and `value_ref` into the same bytes.
- **Validated and non-castable items** (`Bool`, `Char`, `I128`, `U128`, and any Value whose type name is not the primitive's, such as `Num` and `NonZeroU32`) share the **bulk layout** but never the cast: they decode element by element, with bool and char checks and `set_value` through the validating constructor (so a NaN in a `Vec<Num>` fails the decode, and `-0.0` is stored as `0.0` with a `ValueNormalized` warning). The layout is a function of the schema, not of how the decoder chooses to read it.
- **Decode to a typed value.** A bulk list of a castable primitive decodes straight to a concrete `Vec<T>` boxed as the field's value (one allocation and one copy of the bytes, no `Box` per element); `FromReflect` for `Vec<T>` and `[T; N]` accepts any `List` value of matching items (reflect #33, tested in B7), so the typed result costs one more memcpy.
- **Views.** `BinaryReader::read_bulk_view::<T>(slot)` is the real zero-copy entry: when the slot's root is `Vec<T>` or `[T; N]` of a `Leaf` item type, it returns the next value's data as `Cow::Borrowed` over the reader's buffer. This is what `exitium_save` uses for columnar snapshots and the renderer or tilemap can use for a mesh or tile slab, after the CRCs have been verified once at `open`.

The crate has no `unsafe` and no Miri module. A test runs the little-endian cast path and the big-endian swap path (the swap function is not `cfg`-gated, so CI exercises it on x86 and on wasm builds) and asserts the same values.

### Robustness: limits and no panics

Reading untrusted bytes must return `Ok` or `Err`, never panic, never overflow the stack, never allocate more than a bounded multiple of the file. All arithmetic on offsets and lengths is checked.

```rust
pub struct BinaryLimits {
    pub max_file_bytes: u64,     // 512 MiB: stored size
    pub max_raw_bytes: u64,      // 1 GiB:  raw payload (wasm32 has 4 GiB of address space)
    pub max_ratio: u32,          // 256:    raw_len <= max_ratio * stored_len
    pub max_schema_bytes: u32,   // 16 MiB
    pub max_types: u32,          // 65_536: schema entries
    pub max_strings: u32,        // 16 Mi
    pub max_string_bytes: u32,   // 256 MiB
    pub max_values: u64,         // 64 Mi: decoded nodes (a leaf, a struct, an element; a bulk array counts as 1)
    pub max_depth: u32,          // 128
}
```

- **Length bombs.** Every length is checked **before** it reserves memory: `len * min_element_size <= remaining body bytes`, where `min_element_size` is computed once per `FileRef` (a `bool` is 1, a `String` 1, a struct the sum of its fields, an enum 1, an `Option` 1, a list 1, a map 1; a type that can be zero bytes, such as `()` or a struct of no fields, is 0). Elements that can be 0 bytes cannot be bounded by the remaining bytes, so they are bounded by `max_values` instead (`Vec<()>` of `2^40` is `TooManyValues`). A bulk array's `len * element_size` must fit in the remaining bytes. Maps check `len * (min_key + min_value)`.
- **Depth.** `max_depth` bounds recursion in the body decoder (so `Option<Box<Tree>>` chains of a million nodes are `DepthExceeded` at 129, not a stack overflow; 128 frames of the decoder fit the 1 MiB wasm stack), and the writer refuses the same depth, so it never writes a file the reader refuses. Schema `FileRef` nesting is limited to 32.
- **Strictness is robustness.** Non-canonical varints, non-zero padding, duplicate strings, out-of-order keys, bad bool or option tags, trailing bytes and non-zero reserved fields are all errors, so there is one parse of any accepted file.
- **Corruption.** Both CRCs are checked before the section they cover is parsed (`HeaderCrc`, `PayloadCrc`). A CRC pass costs about a gigabyte per second (see "Performance"); `ReadOptions::verify_crc` defaults to `true` and only `exitium_save`, which authenticates the file itself, may turn it off.
- **Fuzzing without a dependency.** The tests mutate and truncate golden files with a std-only seeded generator (as in the text spec); there is no `cargo fuzz` or `proptest` dependency.

### Errors

One struct for the editor, the CLI and MCP, like the text format's `TextError`:

```rust
pub struct BinaryError {
    pub kind: BinaryErrorKind,
    pub offset: u64,                 // byte offset in the raw payload (body errors) or in the file (container errors)
    pub path: String,                // reflect path to the value being decoded ("components[2].gold"); "" for container errors
    pub expected: String,
    pub found: String,
    pub hint: Option<String>,
}
pub enum BinaryErrorKind {
    // container
    BadMagic, Truncated, TrailingBytes, FormatNewer { file: u16, supported: u16 }, HashVersionNewer { file: u8, supported: u8 },
    BadHeader, UnsupportedFlags, HeaderCrc, PayloadCrc, UnknownCodec(u8), CodecFailed, TooLarge, BombRatio, NonZeroPadding, NonCanonical,
    // schema
    SchemaCorrupt, RootMismatch, UnknownType, SchemaMismatch, FileNewer { type_name: String, file: u32, current: u32 },
    NoMigration, MigrationFailed, TypeChanged, MissingField, UnknownVariantName,
    // body
    NonCanonicalVarint, BadBool, BadChar, BadUtf8, BadStringIndex, BadVariant, BadOptionTag, BadBulkTag, OutOfRange,
    DuplicateKey, KeyOrder, DepthExceeded, TooManyValues, InvalidValue, Reference, FromReflect,
}
pub struct BinaryWarning { pub kind: BinaryWarningKind, pub path: String, pub message: String }
pub enum BinaryWarningKind { Migrated, Conformed, DefaultedField, DroppedField, ValueNormalized }
```

`Display` is one line (`save.exb: raw payload +0x1a40, at components[2].gold: expected a string index below 41, found 977`); `BinaryError` serializes to the JSON shape `{kind, offset, path, expected, found, hint, message}`. The path is kept as a small stack of segments pushed and popped as the decoder descends (an index or a field position, about 2 ns) and rendered to the reflect path syntax only when an error is built.

### Public API sketch

```rust
// exitium_serial::binary
pub const BINARY_FORMAT_VERSION: u16 = 1;

pub struct WriteOptions<'a> {
    pub codec: u8,                              // 0 none (default), 1 LZ4
    pub int_arrays: IntArrays,                  // Raw (default) | Packed
    pub codecs: &'a Codecs,
    pub refs: Option<&'a mut dyn BinaryRefs>,
}
pub struct ReadOptions<'a> {
    pub policy: SchemaPolicy,                   // Strict (default) | Lenient
    pub limits: BinaryLimits,
    pub migrations: Option<&'a Migrations>,
    pub codecs: &'a Codecs,
    pub refs: Option<&'a mut dyn BinaryRefs>,
    pub verify_crc: bool,                       // default true
}
pub struct Decoded { pub value: Box<dyn Reflect>, pub warnings: Vec<BinaryWarning> }

// one value
pub fn to_binary(value: &dyn Reflect, registry: &TypeRegistry, opts: WriteOptions<'_>) -> Result<Vec<u8>, BinaryError>;
pub fn write_binary(value: &dyn Reflect, registry: &TypeRegistry, opts: WriteOptions<'_>, out: &mut dyn io::Write) -> Result<(), BinaryError>;
pub fn from_binary(bytes: &[u8], registry: &TypeRegistry, opts: ReadOptions<'_>) -> Result<Decoded, BinaryError>;                 // named root
pub fn from_binary_as(bytes: &[u8], ty: TypeId, registry: &TypeRegistry, opts: ReadOptions<'_>) -> Result<Decoded, BinaryError>;  // any root
pub fn from_binary_into<T: Reflectable>(bytes: &[u8], registry: &TypeRegistry, opts: ReadOptions<'_>) -> Result<(T, Vec<BinaryWarning>), BinaryError>;

// no registry needed
pub fn inspect(bytes: &[u8], limits: &BinaryLimits, codecs: &Codecs) -> Result<BinaryInfo, BinaryError>;   // header, roots, per-type (name, version, shallow)
// with a registry, no body decode
pub fn compare_schema(bytes: &[u8], registry: &TypeRegistry, migrations: Option<&Migrations>, limits: &BinaryLimits, codecs: &Codecs) -> Result<SchemaReport, BinaryError>;
pub enum RowStatus { Unchanged, Migratable { steps: u32 }, NeedsMigration, UnversionedChange, FileNewer, RemovedFromRegistry, AddedInRegistry }

// sessions: BinaryWriter / BinaryReader (see "References and the session API")
// building blocks, public for exitium_save and tests
pub struct AlignedBytes;                         // as_bytes, from_slice, zeroed, len
pub trait Leaf: bytemuck::Pod + sealed::Sealed { fn from_le(self) -> Self; }
pub mod varint { pub fn write(v: u64, out: &mut Vec<u8>); pub fn read(buf: &[u8], pos: &mut usize) -> Result<u64, VarintError>; }
pub mod crc32 { pub fn crc32(bytes: &[u8]) -> u32; pub fn update(state: u32, bytes: &[u8]) -> u32; }
```

`from_binary` takes one registry read guard's worth of `&TypeRegistry` from the caller (reflect spec, threading); no helper takes a second guard. `inspect` and `compare_schema` read only headers and schema, so a 1 GB save can be classified without decoding or even decompressing its body (the schema section is in the clear before the payload, which is why the container keeps it outside the compressed bytes).

### Threading model

- Everything is a pure function of its arguments. `Codecs`, `Migrations`, `BinaryLimits`, `BinaryError`, `Decoded` and `AlignedBytes` are `Send + Sync`. `BinaryRefs` is used through `&mut dyn` by one call on one thread and needs no `Send` bound. `BinaryWriter` and `BinaryReader` are single-owner values (not `Sync`) holding `&TypeRegistry`.
- No globals, no locks, no caches shared between calls; the per-call `DecodePlan` (file type to live type, min sizes, text-form flags, migration chains) is built once per open and read-only afterwards.
- Independent files may be encoded or decoded on different threads. One file is processed on one thread in v1: the format is sequential and positional, and this crate cannot depend on `exitium_tasks` (C1). Chunked LZ4 and the CRC are the two parallelizable passes; a parallel `Codec` and a threaded CRC belong to the caller (`exitium_save`), through the `Codec` trait. wasm32 is single-threaded anyway.
- A caller that needs more than one frame of budget for a large load yields between `read_value` calls of a session (the reader's state is just a cursor).

### Memory layout

- **Encode** builds the body in one `Vec<u8>` with the string table assigned as it goes, then assembles header, schema and payload (with the compressed copy for codec 1): peak about `body + stored` bytes plus the string table. A bulk array is appended straight from the source slice.
- **Decode** holds the file (or the compressed bytes and one `AlignedBytes` of `raw_len`), the `FileSchema` (about 50 KB for a 100-type closure) and the result. Decoding is a recursive descent with O(depth) stack and builds `Dynamic*` values that `FromReflect` then turns into typed ones. A caller that decodes a snapshot should read value by value through the session API so the `Dynamic*` tree of one entity, not of the world, exists at a time.
- A file with `N` named types costs `N` rows of `16 + stream` bytes (a typical stream is 40 to 200 bytes) in the schema section; a 100-type save carries a few KB of schema.

## MCP / editor surface

This crate adds **no editor feature of its own, no command and no MCP tool**: mutation goes through the command bus (design rule 3), and a loaded save enters the world through `exitium_save` and its commands. Per-crate specs do not file issues in `exitium_mcp` or `exitium_editor`; #110 and the saves spec build tools generically from reflection. What this spec owes them is the list of what `exitium_serial` exposes, so an agent and a human get the same capability (agent parity):

| Capability | Exposed by `exitium_serial` | Editor use | MCP use (built by #110 and SP14) |
|---|---|---|---|
| Describe a binary file without decoding it: format and hash versions, codec, sizes and ratio, roots, the `(type, version, shallow hash)` rows | `inspect` | save-file panel | a read-only inspect tool: bytes or path in, JSON out |
| Compare a file to the current game: each type unchanged, migratable (steps), needs a migration, unversioned change, file newer, removed, added | `compare_schema`, `RowStatus` | the same panel marks each type; an "unversioned change" is the bug the schema hash exists to catch | a read-only schema-check tool, the offline form of `reflect_get_schema_hash` |
| Validate a file: full decode under a policy, errors with offset and path, warnings (`Migrated`, `Conformed`, `DefaultedField`) | `from_binary_as`, `BinaryError`, `BinaryWarning` | load log and error squiggles in the save viewer | a read-only validate tool returning the error JSON shape |
| Convert between text and binary for diffing and review | `to_binary` and `from_binary` composed with `to_text`/`from_text` (#23) | "Export as text" for a save or a scene | a convert tool: type name and bytes in, canonical text out, and the reverse |

The error JSON is `{kind, offset, path, expected, found, hint, message}`, so an agent can locate and fix the cause. The `path` uses the reflect path grammar, so it can be handed to `SetField` after a text conversion.

## Performance

Budgets are p50, single thread, on the min-spec reference runner (4 cores, 8 GB), measured with criterion in `crates/exitium_serial/benches/binary.rs`, IDs `exitium_serial/binary/<case>/<size>` at 1k and 100k rows or at the byte size named (docs/benchmarks.md), held by the 5% gate once baselined. They are initial targets; the first benchmark PR may re-baseline with a note to the owner. The **reference row** is the text spec's entity-like struct (about 10 leaves): `id: u32`, two `Vec2`, a `Num`, a unit enum, a `String` from a small set, `flags: u8` and a 3-item `Vec<u8>`. In binary it is about **35 bytes** against about 90 in text.

The row path is bounded by the reflection walk (reflect budget: 50 M leaves/s) and by one `Box` per leaf on decode. The bulk path is bounded by memory bandwidth.

| Operation | Budget |
|---|---|
| Encode rows (`to_binary`, codec 0): reflect walk, string table, CRC | >= 100 MB/s of output; 100k rows (about 3.5 MB) <= 40 ms |
| Decode rows (`from_binary_into`: CRC, plan, positional decode, `FromReflect`) | >= 35 MB/s of input; 100k rows <= 100 ms (the text typed read of the same rows is budgeted at 225 ms) |
| Write one varint / read one varint | <= 3 ns each |
| Write or read one fixed leaf in the row path | <= 10 ns plus the reflect cost (about 20 ns encode, about 80 ns decode with its `Box`) |
| String table: build with 10k distinct strings; one lookup (encode) | <= 1 ms; <= 40 ns |
| CRC-32 (slicing-by-8, std only) | >= 1 GB/s |
| **Bulk encode, codec 0** (a `Vec<f32>` of 8M elements, 32 MB): cast copy plus both CRCs | >= 800 MB/s (<= 40 ms) |
| **Bulk decode to `Vec<T>`, codec 0**: CRC verify plus one copy | >= 800 MB/s (<= 40 ms) |
| **Bulk zero-copy view**, `open` then `read_bulk_view` of 32 MB, after the CRC | view <= 50 us; `open` is the CRC pass, <= 35 ms |
| Packed integer array (`Vec<u32>`, values below 128) | encode >= 150 M elements/s, decode >= 200 M elements/s |
| `Vec<Num>` bulk decode (element-wise validation, `Box` per element) | >= 12 M elements/s |
| Map: sort and write a 10-entry map | <= 1 us; 100k entries <= 40 ms |
| LZ4 compress, 256 KiB chunks (typical row data) | >= 150 MB/s of raw input |
| LZ4 decompress | >= 500 MB/s of raw output |
| Header read plus schema parse for a 100-type, 8-field closure; building the `DecodePlan` | <= 150 us; <= 100 us |
| `compare_schema` of a 100 MB compressed save | <= 1 ms plus the CRC of the schema section (the payload is not read) |
| A migration chain of 3 steps over a 100k-value type | <= 120 ms (dominated by the closure calls) |
| Uncompressed size, reference row | <= 40 bytes; 1M entities x 5 components (about 12 bytes per component) <= 64 MB; LZ4 <= 40 MB |
| wasm32 code added by the crate (release, before `wasm-opt`), including `lz4_flex` | <= 150 KiB (initial target, recorded by the first benchmark PR) |

**1M-entity world.** The snapshot itself is `exitium_save`'s (SP14) and its 1M-entity round-trip test is #67 (a facade test, C7). This format is what makes it reachable: through the row path 1M entities x 5 components is about 50 M leaves, so encode <= 3 s and decode <= 6 s with a peak under 2 GB when the snapshot is read value by value (a whole-world `Dynamic*` tree is not built); stored as columns of bulk arrays (position and velocity slabs), the same data moves at the bulk rates above, 64 MB in about 100 ms each way. The first issue that adds the 1M benches records the numbers.

**Roadmap rows.** The roadmap's "encode and decode of a 32 MB bulk-array world at least 500 MB/s" is the codec-0 bulk row (>= 800 MB/s here, so the row has margin) and its "text round trip of the same at least 20 MB/s" is the text spec's. Its "zstd level 3 decode at least 800 MB/s" moves to SP14, since this crate ships LZ4 only; the LZ4 rows above replace it here.

Complexity: encode and decode are O(bytes) plus O(k log k) per map for the key sort; the plan is O(types in the closure); a migration chain is O(values of the migrating type).

## Test plan

- **Container.** Header fields accepted and rejected (every reserved field non-zero, flags, magic, `format_version` 0 and 2, `hash_version` 2, unknown codec); `FormatNewer` is reported before any CRC or length check (a version-2 file with garbage after the first 10 bytes); both CRCs (flip every single bit of a small file: always an error and never a panic); `Truncated` at every prefix; `TrailingBytes`; non-zero padding in every pad position; the 176-byte worked example matches **byte for byte** and decodes to the same value; `CRC-32("123456789")` and the other standard vectors.
- **Varint.** Round trip over `0`, powers of two and their neighbours, `u64::MAX`; rejection of overlong forms (`80 00`, ten bytes ending in a value above 1, eleven bytes) and truncated varints; zigzag round trips for every signed width at its extremes.
- **Leaves and structure.** Every `ValueRepr` at min, max and a pattern; `bool` 2 and option tag 2 rejected; `char` surrogates and above `10FFFF` rejected; `f32`/`f64` with NaN payloads, `-0.0` and subnormals round trip bit for bit in plain float fields; struct, tuple struct, tuple, enum (unit, tuple, struct variants), `Option<Option<T>>`, transparent chains (`Box<Gold>` is 8 bytes), an empty struct (0 bytes), a recursive type; unknown variant index; variant reordering is a schema change (hash differs) and is caught by the table.
- **Strings.** Table order is first-use order; duplicates in a table rejected; index out of range; invalid UTF-8; empty string is index 0; a million-string table within the limits; strings of every Unicode scalar round trip (no escaping needed in binary).
- **Bulk arrays.** Lengths 0, 15, 16, 17 and 1M for every eligible repr, growable and fixed; the padding after `tag` is exactly to a multiple of 8 and zero; `Vec<Num>` of 16+ with a NaN fails the whole decode, `-0.0` becomes `0.0` with a warning, `inf` saturates (also `NonZeroU32` with a 0); `Bool` and `Char` bulk validated; **packed** arrays for every integer width at extremes, canonical varints, an element that overflows its width, `byte_len` mismatch, packed on `f32` or `bool` rejected; both tags decode to equal values; `Vec<f32>` raw equals the inline bytes of its elements.
- **Zero-copy and soundness.** The view is `Cow::Borrowed` for an aligned buffer on a little-endian host and `Cow::Owned` with equal contents for a misaligned input (a one-byte offset) and for a decompressed payload; the byte-swap path (not `cfg`-gated) gives the same values as the cast path; a test grep-asserts the crate contains no `unsafe` (the workspace lint already denies it).
- **Maps.** `HashMap` and `BTreeMap` give identical bytes; every key kind has an ordering test, including negative ints, `Entity`-like `{index, generation}` struct keys (by `(index, generation)`, so `10v1` after `9v1`), `Guid`, unit-only enums and strings by bytes; an out-of-order or duplicate key on read; the hook replaces keys before sorting.
- **Determinism.** The same value encodes to the same bytes across runs, thread counts, a shuffled `HashMap` insertion order, and (CI on Windows and Linux plus the wasm build running the byte-vector tests) platforms; `encode(decode(bytes)) == bytes` for every golden file and every seeded random value.
- **Compression.** LZ4: round trip at sizes 0, 1, 255 KiB, 256 KiB, 256 KiB + 1, several MB; an incompressible chunk is stored raw and decodes; the chunk framing rejects `raw_len` 0, above 256 KiB, a sum mismatch, `stored_len` above the data and a block that decompresses to the wrong size; a checked-in codec-1 sample decodes (so a future `lz4_flex` bump cannot silently break old files); **bombs**: a header claiming `raw_len` of 1 TB, a ratio of 1000, a chunk whose decompressed size differs from its header (each fails before a large allocation: a counting allocator asserts the peak stays under the bound). A custom `Codec` (a toy run-length one, id 200) registered through `Codecs` round-trips, which tests the trait without zstd.
- **Schema section.** Streams parse into `FileSchema` equal to the live `TypeInfo`s for derived sample types (struct, enum, recursive, mutually recursive, diamond, `Vec<Wallet>` anonymous root); `shallow != fnv(stream)` is `SchemaCorrupt`; a dangling `Named` ref; nesting above 32; duplicate entry names; entries out of order; entry and byte limits. `deep_hash` in the file matches the registry's for the same types and differs after changing a field.
- **Decision table and policies** (reflect's table, every row, both policies): unchanged; version lower with and without a chain; version higher is `FileNewer` even under `Lenient`; same version with a different shallow hash (`SchemaMismatch`, then a `from_hash` migration, then `Lenient` conform); file-only type (removed, renamed with `to_name`, dropped by a migration); registry-only type; a `0 -> 1` migration for a type that gains its first version; the error names the changed type and not a type that merely contains it.
- **Conform.** Renamed and reordered fields by name, an added field with and without `#[reflect(default)]` (only an explicit default fills; `ReflectDefault` alone is `MissingField`), a removed field dropped with a warning, an enum variant moved to another index, a leaf repr widening rejected as `TypeChanged`; the conformed value passes validated constructors.
- **Migrations.** A chain `1 -> 2 -> 3`; a skip edge `1 -> 3` chosen over it; two shortest paths resolved by the tie rule; `from_hash` selecting between two `0 -> 1` edges; a nested child migrating beneath an unchanged parent (bottom-up); a parent and child both migrating (child first); a rename through `to_name`; a migration returning a wrongly shaped value (`MigrationFailed`, whole decode fails); a migration error (no partial value); `FileNewer` not migrated; registration of `to <= from` and duplicates rejected; migration of a map value type and a list element type; `Migrated` warning counts. The embedded-schema decode is tested against a type whose fields were later renamed and reordered (the reflect spec's required test).
- **References.** With a fake text-form type standing in for `Entity` (as in the text spec): no hook (canonical structural bytes), a hook mapping ids on write and read, called exactly once per occurrence, map keys sorted by the replaced value, a hook error with offset and path, generation 0 rejected before the hook. The real `Entity` is covered in `exitium_save` and `exitium_ecs` tests.
- **Sessions.** Two roots of different types, interleaved values and a bulk root read as a view; the reader's call sequence not mirroring the writer's fails cleanly; `finish` rejects leftover bytes; one string table across values.
- **Robustness.** Every truncation and a bounded single-byte mutation sweep of each golden file, plus seeded random bytes and random token soup, return `Ok` or `Err` and never panic (std-only xorshift, fixed seed, the failing seed and input printed); length bombs (`len = 2^60` for `Vec<u8>`, `Vec<()>`, `Vec<Vec<..>>`, a map) fail before allocating; `[[[...` style recursion through `Option<Box<Tree>>` 100000 deep is `DepthExceeded` with no stack overflow (also in the wasm build); `max_values`, `max_strings` and `max_types` each tripped.
- **Platform.** `cargo build --workspace --target wasm32-unknown-unknown` passes; no `unsafe`; the codec-0 golden bytes are identical on Windows, Linux and (byte-vector tests) wasm32.
- **Benchmarks.** One per row of the performance table.

## Dependencies

**Two new crates.io dependencies, both for intake 1** (high-risk area: new dependencies get extra review; CLAUDE.md, ADR-0005). Both were built and exercised (compress and decompress, and a cast round trip) for the host and `wasm32-unknown-unknown` while writing this spec.

| Crate | Version | Features | Licence | wasm32 | Used for | Why not std |
|---|---|---|---|---|---|---|
| `lz4_flex` | `=0.14.2` | `default-features = false`, `std`, `safe-encode`, `safe-decode` | MIT | yes (pure Rust; the `safe-*` features compile out its `unsafe` paths, so it keeps the workspace `unsafe`-free) | the LZ4 codec (id 1) | std has no compression. Writing LZ4 by hand is about 600 lines of match finding and a decompressor that is a security surface; a reviewer should not have to audit our own. Pinned **exactly** because the encoder's output is deterministic only for one version, which the golden tests and cached files rely on; a bump is its own reviewed PR. |
| `bytemuck` | `1` (`Cargo.lock` has 1.25.2) | none | `Zlib OR Apache-2.0 OR MIT` | yes (`no_std`, no target code) | safe byte-to-slice casts and `Pod`/`Zeroable` for `AlignedBytes` and bulk arrays | std casts a slice only through `unsafe` `from_raw_parts`, which the workspace denies. `bytemuck` is already pulled in by `wgpu` and the renderer (intake 1 declares it for those crates), so this adds no supply-chain surface; this crate uses only the core casts and needs none of its optional features. |

Both licences are in `deny.toml`'s allow list (`MIT`, `Zlib`, `Apache-2.0`). **No other dependency.** Rejected, with the reason:

- **`zstd` (C bindings)**: needs a C toolchain for `wasm32-unknown-unknown`, the same trouble as the Luau spike (R5). Left to SP14.
- **`ruzstd`** (pure-Rust zstd): the decoder is complete but the encoder is limited and decode is several times slower than LZ4. Left to SP14.
- **`flate2`/`miniz_oxide`, `lzma`**: slower than LZ4 at the ratios a save needs, no advantage on game data that LZ4 does not give at a fraction of the CPU.
- **`crc32fast`**: about 30 lines of std-only slicing-by-8 give the speed we need and the crate would be a dependency for one function.
- **`byteorder`**: `to_le_bytes` and `from_le_bytes` are in std.
- **A designated `unsafe` module** instead of `bytemuck`: see "Zero-copy and soundness".
- **`proptest` / `cargo fuzz`**: the randomized tests use a seeded std PRNG (as in the text spec); shrinking is traded for zero dependencies.

## Wiring for the intake PRs

Everything the implementation issues below need from `Cargo.toml` and `Cargo.lock`. Intake 1 adds the union of the merged specs' lists, so no issue below touches `Cargo.lock`.

- **Crate edges:** none new. `exitium_serial -> exitium_reflect` and the `exitium_core` dev-dependency come from the text spec's wiring; `exitium_serial` has **no** edge to `exitium_ecs` (C7) or any other runtime crate.
- **External crates for `exitium_serial`:** `lz4_flex = { workspace = true }` and `bytemuck = { workspace = true }` under `[dependencies]`. In `[workspace.dependencies]` (one line each, with the licence/wasm/why-not-std comment the roadmap asks for): `lz4_flex = { version = "=0.14.2", default-features = false, features = ["std", "safe-encode", "safe-decode"] }` and `bytemuck = "1"` (if another merged spec's wiring already declares `bytemuck`, intake 1 reuses that line; this crate needs no feature).
- **Dev-dependencies:** `criterion` (already in the workspace) for `[[bench]] name = "binary", harness = false`, added by the first issue that adds a benchmark (B1), inside the crate.
- **Facade (`crates/exitium/`):** the `exitium::serial` re-export is the facade's wiring, not an issue below (it is a hotspot owned by the facade PRs).
- **Reflect (`crates/exitium_reflect/`):** this spec needs three small things from the schema-hash issue #36, which is **its** footprint, not an edit made by an issue below (see "Downstream issue adjustments"): the exact shallow stream on each `SchemaEntry`, a public `fnv1a_64`, and the `ValueRepr` to and from code functions. The B-issues are blocked by #36.
- `crates/exitium_serial/CLAUDE.md` gets the binary "Public API", "Invariants" and "Benchmarks" sections from the issues below, as the text issues do.

## Open questions

1. **Zstd (the roadmap's open question 6).** Codec id 2 and the `Codec` trait are reserved and ready; this crate ships LZ4 only. Recommendation: SP14 decides between `zstd` (C, native only), `ruzstd` (pure Rust decoder, limited encoder) and LZ4 on the web. Cloud saves are where the better ratio matters, and they are native.
2. **Raw or packed integer arrays by default.** `Raw` is the default (zero-copy, and LZ4 recovers most of the size). `Packed` is smaller without a compressor. Recommendation: keep `Raw` as the library default and let `exitium_save` choose `Packed` for web saves if SP14 ships no zstd there. A later layout change is a format version bump.
3. **Bulk layout for structs of fixed leaves** (`Vec<Vec2>`, `Vec<Color>`, a tile struct). Today they take the per-element row path (still contiguous and fixed-size, but one reflection walk per element). A flat bulk layout is a large win for meshes and tile data and changes bytes for existing types, so it needs `format_version = 2`. Recommendation: defer until a real asset needs it; columnar snapshots in `exitium_save` (a slab per component field) already cover the world case.
4. **Direct typed decode.** Decode builds `Dynamic*` values, then `FromReflect`. A `ReflectBinaryDecode` type data (decode straight into `T` in place) would cut the row path's allocations but needs `Default` or `MaybeUninit`-style construction, which reflect lacks. Recommendation: not in v1; the budgets above are set for the `Dynamic*` path, and a bench miss is the trigger.
5. **Scan-and-skip validation for `Vec<Num>`.** Element-wise `set_value` costs a `Box` per element. A scan for NaN, infinities and `-0.0` (and zeros for `NonZeroU32`), taking the typed `Vec<f64>` path when none exist, would make big number arrays near-bulk speed. Recommendation: add it if a bench shows `Vec<Num>` dominating a save.
6. **Map key order for text-form keys.** This spec sorts them structurally (`Entity` by index, then generation); the reflect spec's map note says "text forms by UTF-8 bytes", which is the text format's rule. They are different formats with different goals (structural is cheap and numeric; textual is what a human reads). Recommendation: amend the reflect line to say each serializer defines its own total order (this spec, "Downstream issue adjustments").
7. **Cross-process content hashes.** Canonical codec-0 bytes make a content hash of a save meaningful (dedup, cloud sync). Should the header carry one? Recommendation: no; it belongs to SP14's container, which also authenticates.
8. **Seeking and partial loads.** A file is verified and decoded whole. A save with a large thumbnail or log section might want to skip it. Recommendation: put such sections in separate roots a reader may skip with the session API (it already frames values), rather than adding an index to the format.
9. **File extension and MIME type.** The container does not care. Recommendation: `.exb` for a standalone value and whatever the consumer picks for a save, decided by the saves and assets specs.
10. **`schemas.lock`** (reflect open question 5): a committed `(name, version, shallow hash)` list that makes "hash changed without a version bump" a visible diff in the PR. It is the author-side half of what this spec enforces at load time. Recommendation: do it after #36, as reflect proposes.

## Implementation issues

All issues edit exactly one crate, `exitium_serial`, so they run one at a time, in the order shown, in the same lane as the text issues S1 to S12 (one crate has at most one issue in flight; when the S-issues and these have the same whole-crate footprint, the factory runs them one after another). Each is about 600 changed lines or fewer, has acceptance criteria and `Blocked by` lines, updates the "Public API", "Invariants" and "Benchmarks" sections of `crates/exitium_serial/CLAUDE.md` for what it adds, and adds its benchmark rows to `benches/binary.rs` (design rule 4). They are filed into the full MVP milestone with `file-issue` after this spec merges (label `area:serial`, role `impl-engine-core`). Every issue is a Stage 1 issue and is `Blocked by <intake-1>`, the Stage 1 intake PR that adds `lz4_flex` and `bytemuck` (the number is filled in when the issues are filed). Reflect issues: #31 (model, registry), #32 (dynamic access, `Dynamic*`), #33 (containers), #36 (schema hash, with the adjustments below), #37 (core impls), #34 and #35 (derive, only for test types).

Every issue has this Footprint (full labelled form):

```
## Footprint
- Edits: crates/exitium_serial/
- Governs: none
- Hotspots: none
- New crate edges: none
```

**B1. `feat(serial): binary primitives (varint, CRC-32, aligned buffer, leaf casts, limits, errors)`** (new)
- Adds `binary/{mod,error,limits,varint,crc32,aligned,leaf}.rs`: canonical varint, slicing-by-8 CRC-32, `AlignedBytes`, the sealed `Leaf` trait with `try_cast_slice` views and the byte-swap path, `BinaryError`, `BinaryErrorKind`, `BinaryWarning`, `BinaryLimits`; the `[[bench]] binary` target and `benches/binary.rs`. Reflect-free.
- Acceptance criteria:
  - [ ] Varint canonical-form tests (every overlong and truncated case) and the CRC-32 standard vectors; the byte-swap path equals the cast path on the host.
  - [ ] `Cow::Borrowed` for an aligned slice, `Cow::Owned` with equal contents for a misaligned one; no `unsafe`.
  - [ ] No panic over every truncation of the test inputs; wasm32 build passes.
  - [ ] Benchmarks `exitium_serial/binary/{varint,crc32,bulk_copy}/*` meet the budget.
- Blocked by: <intake-1>.

**B2. `feat(serial): binary container and codecs (header, LZ4 chunks, bomb limits)`** (existing #58, rescoped: LZ4 only, zstd is SP14)
- Adds `binary/{container,codec}.rs`: the 64-byte header read and write with every field rule, the file-length and padding checks, both CRCs, the `Codec` and `Codecs` types, codec 0 and the chunked LZ4 codec, the header-only bomb checks, and `inspect`'s container half.
- Acceptance criteria:
  - [ ] The header rules table has an accept and a reject test per row; `FormatNewer` precedes every other check; every single-bit flip of a small file is an error.
  - [ ] LZ4 round trip at the chunk-boundary sizes, an incompressible chunk stored raw, a checked-in codec-1 sample decodes, a toy custom codec registers.
  - [ ] Bomb tests (a 1 TB claim, ratio 1000, chunk size mismatch) fail before a large allocation (counting allocator).
  - [ ] Benchmarks `exitium_serial/binary/{lz4_compress,lz4_decompress}/*` meet the budget.
  - [ ] Edit #58: drop `gate:human` and `needs-spec`, replace "zstd and LZ4" with LZ4 and the pluggable `Codec`, add the Footprint.
- Blocked by: <intake-1>, B1.

**B3. `feat(serial): binary schema section (write, parse, verify, inspect)`** (new)
- Adds `binary/schema.rs`: the schema section writer (roots and the union of closures from `registry.schema_table`, sorted), the parser into `FileSchema` and `FileRef`, the `shallow == fnv(stream)` check, nesting and count limits, `inspect` and `compare_schema` with `RowStatus`.
- Acceptance criteria:
  - [ ] Streams for derived samples (struct, enum, recursive, mutually recursive, diamond, `Vec<Wallet>` root) parse into a `FileSchema` equal to the live `TypeInfo`s.
  - [ ] `SchemaCorrupt` cases (bad hash, dangling `Named`, depth 33, duplicate or unsorted entries) and the entry and byte limits.
  - [ ] `compare_schema` returns the right `RowStatus` for every row of the decision table without decoding a body.
  - [ ] Benchmark `exitium_serial/binary/schema_parse/*` meets the budget.
- Blocked by: <intake-1>, B2, #36.

**B4. `feat(serial): binary encoder (leaves, structs, enums, options, lists, strings)`** (existing #56, rescoped: maps and bulk are B5)
- Adds `binary/encode.rs`: the body walk for every kind except maps and bulk lists, the string table in first-use order, validated-Value reads through `value_ref`, the depth limit, `to_binary` and `write_binary` for one value.
- Acceptance criteria:
  - [ ] The 176-byte worked example matches byte for byte; golden files for every row of the body table except maps and bulk.
  - [ ] Deterministic output across runs; transparent types erased; `hidden` fields written, `skip` fields absent.
  - [ ] Benchmark `exitium_serial/binary/encode_rows/{1000,100000}` meets the budget.
  - [ ] Edit #56: drop `gate:human` and `needs-spec`, retitle to "binary encoder", add the Footprint, replace "Blocked by" with <intake-1>, B3, #31, #32, #33, #37.
- Blocked by: <intake-1>, B3, #31, #32, #33, #37.

**B5. `feat(serial): binary encoder for maps and bulk arrays`** (new)
- Adds canonical key sorting for every key kind (structural order), the bulk layout with the raw and packed tags (`WriteOptions::int_arrays`), the `Vec<T>` downcast casts for the ten castable primitives and the element-wise path for the rest, and alignment and padding.
- Acceptance criteria:
  - [ ] `HashMap` and `BTreeMap` give identical bytes; every key kind has an ordering test; lengths 15, 16 and 17 for every eligible repr; padding exactly to 8 and zero.
  - [ ] Packed arrays for every integer width at extremes; `Vec<Num>` encodes the same bytes as `Vec<f64>`.
  - [ ] Benchmarks `exitium_serial/binary/{encode_bulk,encode_map,encode_packed}/*` meet the budget (the 32 MB bulk row at >= 800 MB/s).
- Blocked by: <intake-1>, B4.

**B6. `feat(serial): binary decoder for exact schemas (non-map, non-bulk kinds)`** (existing #57, rescoped: maps, bulk and views are B7)
- Adds `binary/{decode,plan}.rs`: the `DecodePlan` (file type to live type, minimum sizes, text-form flags), positional decode into `Dynamic*` with the canonical checks, validated Values through `ReflectFromReflect` (`InvalidValue`, `ValueNormalized`), the error path stack, every limit (`max_values`, `max_depth`, length-times-minimum-size), the fast path on a matching `deep_hash`, and `from_binary`, `from_binary_as`, `from_binary_into` for files whose schema equals the registry's.
- Acceptance criteria:
  - [ ] Every `decode(encode(x)) == x` through `reflect_eq` for the B4 golden files; every rejection in the leaves and structure test group.
  - [ ] Length-bomb, depth and value-count tests fail without allocation or stack overflow (also on wasm32); the `Num` NaN and `-0.0` cases and `NonZeroU32` zero are covered.
  - [ ] Benchmark `exitium_serial/binary/decode_rows/{1000,100000}` meets the budget.
  - [ ] Edit #57: drop `gate:human` and `needs-spec`, retitle to "binary decoder", add the Footprint, replace "Blocked by" with <intake-1>, B4, #31, #32, #33, #37.
- Blocked by: <intake-1>, B4, #32.

**B7. `feat(serial): binary decoder for maps and bulk arrays, zero-copy views`** (new)
- Adds map decoding with strict key order and `DuplicateKey`, bulk decoding for both tags (typed `Vec<T>` fast path, element-wise validated path), `BinaryReader::read_bulk_view`, and the `FromReflect` acceptance test for `Vec<T>` and `[T; N]` from a concrete `Vec<T>`.
- Acceptance criteria:
  - [ ] The maps, bulk, zero-copy and canonical-form test groups, including the borrowed and owned views and an `Entity`-like struct key.
  - [ ] `encode(decode(bytes)) == bytes` for every golden file and seeded random values.
  - [ ] Benchmarks `exitium_serial/binary/{decode_bulk,decode_map,decode_packed,view}/*` meet the budget (the 32 MB bulk decode at >= 800 MB/s).
- Blocked by: <intake-1>, B5, B6.

**B8. `feat(serial): schema policy (decision table, FileNewer, Lenient conform)`** (new)
- Adds `SchemaPolicy`, the per-row decision table for both policies with the errors that name the changed type and both versions or hashes, `FileNewer` in both, the conform adaptation by name (defaults, dropped fields, variants by name, `TypeChanged`), and the warnings.
- Acceptance criteria:
  - [ ] Every row of the decision table, in both policies, has a test (including an unversioned change and a file-only type), and the error points at the changed type, not at a type that contains it.
  - [ ] The conform test group, including the explicit-default-only rule.
  - [ ] An old body is decoded with renamed and reordered fields (the embedded-schema requirement of the reflect spec).
  - [ ] Benchmark `exitium_serial/binary/plan/*` meets the budget.
- Blocked by: <intake-1>, B7.

**B9. `feat(serial): versioned migrations (registry, chain resolution, bottom-up apply)`** (existing #59, rescoped)
- Adds `Migration`, `Migrations`, `MigrationCx`, shortest-chain resolution with the `from_hash` filter and the tie rule, post-order application, renames through `to_name`, all-or-nothing failure and the `Migrated` warning.
- Acceptance criteria:
  - [ ] The migrations test group: chain, skip edge, tie, `from_hash`, nested bottom-up, parent and child both migrating, rename, wrong shape, error, `FileNewer` never migrated, registration rejections.
  - [ ] A migration of a map value type and of a list element type; a 3-step chain over 100k values meets the budget (`exitium_serial/binary/migrate/*`).
  - [ ] Edit #59: drop `gate:human` and `needs-spec`, replace "Blocked by" with B8, `from_version = 0` means "undeclared", add the Footprint.
- Blocked by: <intake-1>, B8.

**B10. `feat(serial): binary session API and the reference hook`** (new)
- Adds `BinaryWriter` and `BinaryReader` (shared string table, several roots, `write_len` and `read_len`, `finish` checks), the `BinaryRefs` hook with the once-per-occurrence rule and replaced-key sorting, and moves `RefSite` and `RefError` into a shared `refs` module (or re-exports them) so both formats use one definition.
- Acceptance criteria:
  - [ ] The sessions and references test groups, with a fake text-form type standing in for `Entity`.
  - [ ] A two-root session (a struct root and a bulk-array root read as a view) round-trips, and a mismatched read sequence fails cleanly.
  - [ ] No regression in the text benchmarks after the shared `refs` move.
- Blocked by: <intake-1>, B9, S11.

**B11. `test(serial): golden files, seeded round trip and robustness for the binary format`** (existing #60, rescoped)
- Adds `tests/golden_binary/*` (the worked example, one file per body-table row, a codec-1 sample, a migrated file), the std-only seeded generator of random reflected values, the truncation and mutation sweeps, the bit-flip sweep, the cross-platform byte-vector test and `exitium_serial/binary/roundtrip/{1000,100000}`. Runtime under 30 s in CI.
- Acceptance criteria:
  - [ ] `decode(encode(x)) == x` and `encode(decode(bytes)) == bytes` over seeded random values (struct, enum, container mixes, extreme `Num`, unicode strings, empty containers, recursive types) and every golden file, under both integer-array options and both codecs.
  - [ ] No panic on every truncation, every single-bit flip of the small golden files, a bounded single-byte mutation sweep of the others, and seeded random bytes; every failure prints its seed.
  - [ ] The codec-0 golden bytes are identical on Windows and Linux CI and in the wasm byte-vector test.
  - [ ] Edit #60: replace "Blocked by" with B10 and add the Footprint.
- Blocked by: <intake-1>, B10.

**Not filed from this spec.** Per-crate specs do not file issues in other crates:

- The binary **world snapshot** (#61) is `exitium_save`'s (SP14, C7); it uses the session API, the bulk views and the reference hook.
- A zstd codec, the saves container (encryption, tamper detection, autosave) and a game's migration registry are SP14's.
- The MCP and editor tools in "MCP / editor surface" are built by #110 and SP14.
- The reflect-side changes below are made in #36 and a one-line reflect amendment, owned by the reflect issues.

### Downstream issue adjustments

Applied by the architect when this spec merges (the bullets are the contract):

- **#56, #57, #58, #59, #60:** as described under B4, B6, B2, B9 and B11. Remove `needs-spec` and `gate:human` from all five (ADR-0005; the PR Risk section names the serialization format); add the Footprint; set `Blocked by` as listed. #58's zstd scope is dropped (SP14). Add `area:serial` where missing.
- **#61:** retitle and re-scope per C7 (the binary world snapshot in `exitium_save`, SP14), and note that it builds on B10 and B7's views. Not an `exitium_serial` issue.
- **#36 (schema hash):** `SchemaEntry` gains `stream: Vec<u8>`, the exact shallow stream (version byte first), as part of the persisted-format contract with a parse test; export `fnv1a_64(bytes: &[u8]) -> u64`; export the `ValueRepr` to and from code table (`repr_code`, `repr_from_code`). The stream grammar in the reflect spec is the contract and the golden vectors cover it.
- **#32, #33:** `FromReflect` for `Vec<T>` and `[T; N]` accepts any `List` value of matching items (including a concrete `Vec<T>` of the same element type), and the `Dynamic*` builders can create values by field name with no `represented_type_info` (used for old-schema nodes); both are tested there.
- **Reflect spec, one line:** "Deterministic order when a `HashMap` is written" is amended: each serializer defines its own total order. The text serializer sorts text forms by their written text; the binary serializer sorts structurally (this spec). Doc-only.
- **Roadmap (`mvp-roadmap.md`):** the serial lane in Stage 1 goes from "about 17" PRs to **23** (S1 to S12 plus B1 to B11; the binary part was estimated at 5, it is 11 once split to PR size), still under the ECS chain (about 28), so the Stage 1 critical path is unchanged; intake 1's dependency list names `lz4_flex` and `bytemuck` as above; the "zstd level 3 decode" performance row moves to SP14; the roadmap's open question 6 is answered by "SP14 decides, id 2 reserved". The 32 MB bulk row is the codec-0 row in this spec.
- **SP14 (saves) and SP16 (net):** build on `BinaryWriter`/`BinaryReader`, `BinaryRefs`, the bulk views and the `Codec` trait; both may use `compare_schema` for pre-flight checks. SP14 owns codec id 2, encryption and a game's migration registry, and reconciles its "saves row" budgets with the rates above.
- **#23 (text spec):** no change. The two formats share `RefSite` and `RefError` after B10.
- **Whole-crate footprints.** The text issues and these all declare `Edits: crates/exitium_serial/`, so the factory runs them one after another (23 PRs). If the lane ever becomes the critical path, the S- and B-issues can be re-footprinted to `src/text/`, `src/binary/` and their benches and tests, plus a hotspot on `src/lib.rs` and the crate `CLAUDE.md`; that is a filing-time choice, not a design change.
