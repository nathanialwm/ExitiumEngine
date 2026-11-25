# Spec: Serialization binary format, schema hashing and migrations

- Issue: #24
- Crates: `exitium_serial` (touchpoints: `exitium_reflect`, `exitium_save` (SP14), `exitium_net` (SP16), `exitium_command` (issue M1, the binary MCP tools), `exitium_mcp`, `exitium_editor`)
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
- **A JSON codec.** The generic JSON mapping is the command crate's (CB7 and CB8). The binary MCP tools are issue M1 below, built on the capability table in "MCP / editor surface".
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
| 10 | 2 | `flags` | Bit 0 `PACKED_INTS`: integer bulk arrays of the packable widths use tag 1 (below). All other bits are `0` in v1; any other set bit is `UnsupportedFlags` (a flag may change how the rest is read, so unknown bits are never ignored). |
| 12 | 1 | `codec` | `0` none, `1` LZ4 chunked, `2` reserved (zstd, SP14). An id the reader's `Codecs` has not registered is `UnknownCodec(id)`: ids `0` to `63` are reserved for formats Exitium specifies, `64` to `255` are free for applications and tests (the test plan registers `200`). |
| 13 | 1 | `hash_version` | The reflect spec's `SCHEMA_HASH_VERSION` the file was written with (`1`). Greater than the reader's is `HashVersionNewer`; lower is handled by "Older `hash_version`" below. Every embedded stream starts with this byte (checked). |
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

**Check order.** Every reader runs the same checks in this order, so the same bad file always gives the same error (a test per step):

1. Fewer than 64 bytes: `Truncated`. `magic`: `BadMagic`.
2. `format_version`: `0` is `BadHeader`, above the reader's is `FormatNewer`. Nothing after this step is trusted for a newer file.
3. `flags`, the reserved fields (`BadHeader`), `codec` (`UnknownCodec`), `hash_version` (`HashVersionNewer`).
4. Length arithmetic with `checked_add` and the exact file length (`Truncated`, `TrailingBytes`); `schema_len`, `strtab_len` and `raw_len` multiples of 8, `strtab_len <= raw_len` (`BadHeader`).
5. Size limits from the header alone, before any allocation: `max_file_bytes`, `max_schema_bytes`, `max_raw_bytes` and the ratio rule (`TooLarge`, `BombRatio`; see "Bomb limits").
6. `header_crc` (`HeaderCrc`), which covers the header and the schema section.
7. Parse and validate the schema section (`SchemaCorrupt`, `RecursiveSchema`).
8. `payload_crc` over the stored bytes (`PayloadCrc`), unless `verify_crc` is off.
9. Decompress (chunk checks, `CodecFailed`), then parse the string table (`BadUtf8`, `NonCanonical`, `NonZeroPadding`).
10. Match the schema against the registry and build the plan ("Verification and mismatch"), then decode the body.

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

**What a reader verifies.** `shallow_hash == FNV-1a-64(stream)` for every entry, so the streams are exactly what the hash consumed and cannot drift from it (`SchemaCorrupt` otherwise). It then parses each stream with the grammar of the reflect spec's "Shallow hash" and "Type reference" tables into a `FileSchema`: roots as `FileRef` values, and per named type its kind, field and variant names in order, and each field's `FileRef`. A `FileRef` is `Value(repr)`, `List { fixed: Option<u64>, item }`, `Map(key, value)`, `Option(inner)`, `Tuple(items)` or `Named { kind, name }`. Every `Named` ref must name an entry (`SchemaCorrupt` otherwise), nesting is limited to 32 levels, and the entry count and total bytes are limited (`BinaryLimits`). **Names and indices must be unambiguous** (`SchemaCorrupt`, hint naming the type): within one type no two fields share a name and no two variants share a name, and each variant's recorded `index` equals its position in declaration order (`0, 1, 2, ...`). Conform, migrations and the by-name `Dynamic*` builders all key on names, so a duplicate would make their input ambiguous, and the body encodes the variant's position, so a stream whose `index` differs from it describes a different layout than the one the reader would decode.

**Recursion check and minimum sizes (before any size is used).** The file is untrusted, so `struct A { a: A }` can appear. After parsing, the reader computes each named type's **minimum encoded size** as a **least fixed point**, not a walk from some starting type, so the answer cannot depend on entry order or on which type is visited first. The equations follow only edges with no indirection; `Option`, `Vec`, maps and `[T; 0]` are indirections and are **not** followed (their minimum is a constant: `Option` 1 byte, list 1, map 1, `[T; 0]` 0):

- a leaf is its fixed size (`String` 1, a varint index; `()` 0);
- a struct, tuple or transparent-erased type is the **saturating sum** of its field minima; a fixed array `[T; N]` with `N >= 1` is `N` times the item's minimum, saturating;
- an enum is the **minimum over its variants** of `1 + the sum of that variant's field minima` (the 1 is the variant varint).

Every type starts at infinity and the values are solved by Knuth's generalisation of Dijkstra's algorithm. Each variant is one "production" with a counter of its not-yet-final operand types and a running saturating sum; a struct, tuple or array is one production and an enum has one per variant. A min-heap of candidate `(value, type)` pairs is seeded with the productions that have no operands (leaves, constants, empty structs). The smallest candidate whose type is not final becomes that type's value; finalizing a type decrements the counters of the productions that use it, and a production whose counter reaches zero pushes its sum as a candidate for its own type. This is correct because every production is monotone and at least as large as each of its operands (a sum of non-negative terms, plus 1 for a variant). It runs in O((types + fields) log types) and each production fires once, so the check cannot be used to burn time. A type still not final when the heap is empty has no finite value: it is `RecursiveSchema` (a `SchemaCorrupt` subkind), found at schema validation and never at decode. When several types are infinite the error names the first one by stable name, so the same file always reports the same type. A type that merely reaches an infinite type is infinite too and is reported the same way (no value of it can exist).

So `enum List { Nil, Cons(Box<List>) }` is fine (via `Nil`, 1 byte), an enum whose every variant recurses is not, and the usual tree shape `enum Expr { Num(f64), Add(Box<Pair>) }` with `struct Pair { a: Expr, b: Expr }` is **accepted whatever the two names are**: `Expr` is 9 (via `Num`) and `Pair` is 18, in both name orders. Decode recursion of every kind, through enums, `Option` or `Vec`, is bounded by `max_depth`. The same minimum sizes feed the length-bomb rule below.

The `deep_hash` of each root is checked against the registry (not recomputed from the file), as the first step of "Verification and mismatch".

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

The reader **enforces** first-use order: it keeps a counter `next_new` (starting at 1, since entry 0 is the empty string, which may be used at any time); an index `i` with `i < next_new` is a repeat, `i == next_new` is a first use and increments the counter, and `i > next_new` is `NonCanonical` (hint: "string indices must be assigned in first-use order"). When the body ends, `next_new` must equal `count`, or the table holds a string nothing uses (`NonCanonical`, hint "unused string table entry"). Strings are read in body order whatever later adapts them (`Lenient` conform or a migration), so the check is independent of the schema policy. Because one stored string can be referenced by any number of one-byte indices, **decoded string bytes are charged to a cumulative budget** ("Robustness: limits and no panics").

#### Body

The body is the concatenation of the values the writer was asked to write, each encoded as below. The one-shot API writes one value (the root). The session API writes any sequence of values of the registered roots, with caller framing in between (varints).

| Kind | Encoding |
|---|---|
| `bool` | 1 byte, `0` or `1` (anything else is `BadBool`). |
| `char` | `u32` little-endian; must be a Unicode scalar value (`BadChar`). |
| `i8` to `i128`, `u8` to `u128` | Fixed width, little-endian, two's complement. |
| `f32`, `f64` | IEEE 754 bits, little-endian. **Every bit pattern is stored exactly**, NaN payloads and `-0.0` included. A validated Value over a float (`Num`) rejects NaN with `InvalidValue`, and a stored `-0.0` or infinity (which the constructor would normalize) is `NonCanonical` (below). |
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
zero padding            up to the next multiple of 8 (measured from the start of the raw payload);
                        zero bytes if the offset after the tag is already a multiple of 8,
                        so 0 to 7 bytes, never a whole extra 8
data
```

- **Tag 0, raw (the zero-copy form).** `len` elements in their fixed little-endian leaf encoding, back to back, starting 8-aligned. This is the same bytes the elements would have had inline; only the alignment and the tag are added. On a little-endian host, a reader hands the slice out without touching it (see "Zero-copy and soundness").
- **Tag 1, packed.** Only for the integer reprs `I16`, `I32`, `I64`, `U16`, `U32`, `U64` (the **packable** reprs). Not `I8`/`U8` (raw is already one byte), not floats, `Bool` or `Char`, and **not `I128`/`U128`**: the varint is a `u64`, and a 128-bit varint (up to 19 bytes) is not worth a second code path, so `Vec<Guid>` and other 128-bit lists are always raw. The data is `varint byte_len`, then exactly `byte_len` bytes holding `len` canonical varints, each the value (unsigned) or its **zigzag** form (`(v << 1) ^ (v >> (bits-1))`, signed). No alignment padding applies to the data: it starts right after the tag and `byte_len`, and the 8-byte padding is not written for tag 1. A varint that does not fit the element width is `OutOfRange`; `len > byte_len` is `BadBulkTag` (every varint is at least one byte). Tag 1 on a non-packable repr is `BadBulkTag`.
- **When each is written, and what the reader enforces.** Eligibility is decided by the schema alone. The writer picks the tag for **every** packable list from `WriteOptions::int_arrays` (`Raw`, default, or `Packed`) and records the choice in header flag `PACKED_INTS`. The reader requires that every packable list of 16 or more elements uses tag 1 when the flag is set and tag 0 when it is clear; any other tag is `BadBulkTag` (hint: "tag does not match the PACKED_INTS flag"). So one file never mixes the two, a reader needs no option, and a given option gives one byte string. `Raw` suits tile maps, IDs and anything LZ4 will see; `Packed` suits saves full of small counters where no compressor is available (the web build without zstd), at about 3 ns per element on decode instead of a memcpy.
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

**Canonical form.** For a fixed `(value, registry, options)` there is exactly one valid byte string (for codec 0; codec 1's compressed bytes are outside the guarantee, see "Compression container", but its raw payload is canonical). **The reader enforces every rule**, so a file it accepts is the writer's output for the value it decodes to:

| Rule | Error |
|---|---|
| varints are shortest-form | `NonCanonicalVarint` |
| every padding and reserved byte is zero | `NonZeroPadding` / `BadHeader` |
| string table entries are unique; string indices are assigned in first-use order; no entry is unused | `NonCanonical` |
| map keys are strictly ascending (duplicates and misorder) | `DuplicateKey` / `KeyOrder` |
| bool and option bytes are `0` or `1`; enum indices exist | `BadBool` / `BadOptionTag` / `BadVariant` |
| lengths and padded sizes are exact; no trailing bytes | `Truncated` / `TrailingBytes` / `NonCanonical` |
| one bulk-tag family per file, matching `PACKED_INTS`; bulk used exactly when `len >= 16` | `BadBulkTag` |
| a validated Value is stored in the form its constructor yields: `Num` with `-0.0` or an infinity, which the constructor would normalize, is rejected | `NonCanonical` (hint names the normalization) |

So `encode(decode(bytes)) == bytes` for every file the reader accepts with `Unchanged` rows and no reference hook, under the options the header records (`PACKED_INTS`, codec 0), which the round-trip test pins. A file that went through `Lenient` conform, a migration or a hook decodes to a value whose canonical bytes differ from the input by design. The guarantee is what makes golden diffs and content hashes of saves meaningful: two readers cannot disagree about whether a file is canonical. Output does not depend on pointer width, hash-map order, thread count or host endianness.

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
    /// The largest `raw_len / stored_len` this codec can honestly produce (none: 1, LZ4: 255).
    /// The container refuses a header that claims more, whatever `BinaryLimits` says.
    fn max_ratio(&self) -> u32;
    /// Append the stored form of `raw` to `out`. Deterministic for a given codec version.
    fn compress(&self, raw: &[u8], out: &mut Vec<u8>) -> Result<(), CodecError>;
    /// Fill `out` (exactly `raw_len` bytes) from `stored` and return **the number of bytes
    /// written**. The container compares the count with `out.len()` and fails with `CodecFailed`
    /// on any difference, so an implementation that under-fills cannot pass silently.
    fn decompress(&self, stored: &[u8], out: &mut [u8]) -> Result<usize, CodecError>;
}
pub struct Codecs { /* id -> Arc<dyn Codec>; register(codec) rejects a duplicate id */ }   // Codecs::default() has 0 (none) and 1 (LZ4)
```

- **Codec 0, none.** `stored == raw`.
- **Codec 1, LZ4 chunked.** The raw payload is cut into **256 KiB** chunks (the last may be shorter, none empty). Each stored chunk is `u32 raw_len`, `u32 stored_len`, then `stored_len` bytes. If LZ4 does not make a chunk smaller, the writer stores it **raw** with `stored_len == raw_len`; the reader treats `stored_len == raw_len` as raw, and the writer never emits an LZ4 block of that size, so the form is unambiguous. Each chunk decompresses into its slot of the single output buffer with `lz4_flex::block::decompress_into`. That function **returns the number of bytes it wrote** and does not itself insist on filling the slot, so the LZ4 codec compares the returned count with the chunk's declared `raw_len` and fails with `CodecFailed` on any difference (a short chunk would otherwise leave stale or zero bytes in the payload), and returns the sum of its chunk counts. The container does not trust that: for **every** codec it compares the `usize` that `decompress` returned with `raw_len` and fails with `CodecFailed` unless they are equal (a count above `out.len()` is a codec bug and fails the same way). The container can do this only because the count is part of the return type; a unit return would leave it unable to tell an under-filled, zero-initialised buffer from a full one. A registered codec is application code the reader trusts to report its own count honestly; the check catches honest under-fills and over-reports, which are the realistic bugs. Chunking bounds the working set per chunk, makes the bomb check per chunk, and keeps the door open for a parallel codec (a `Codec` impl in `exitium_save` can use tasks; this crate cannot, C1).
- **Codec 2, reserved** for zstd. Decided by SP14; an unknown id is `UnknownCodec`.
- **Determinism.** The raw payload and the CRCs of codec 0 are canonical across platforms and releases. LZ4 output depends on the `lz4_flex` version and is **not** part of the cross-version contract: the reader never depends on how the compressor behaved, and the golden tests pin the codec-0 file byte for byte and the codec-1 file by decoding a checked-in sample (so a compressor change cannot break an old file, and a security fix in `lz4_flex` can be taken by a normal dependency update).

**Bomb limits** are checked before any allocation, from the header alone, and are **codec-agnostic**: `stored_len <= max_file_bytes`; `raw_len <= max_raw_bytes`; `raw_len <= ratio * stored_len` where `ratio = min(codec.max_ratio(), limits.max_ratio)` (codec 0: 1; LZ4: 255, the structural maximum of its run-length expansion so honest data is never refused, with the 8-byte chunk headers inside `stored_len`; a registered zstd would state its own); every chunk `raw_len <= 256 KiB`; the chunk `raw_len` values sum to the header `raw_len`; and the chunk `stored_len` values (with their 8-byte headers) sum to `stored_len`. A file that claims more than that is `TooLarge` or `BombRatio` without having cost more than a read of its header. The one allocation is a buffer of `raw_len` bytes (an `AlignedBytes` for the reader, a plain `Vec<u8>` for `from_binary*`), which these checks bound by `ratio` times the bytes the file really has.

### Verification and mismatch

The reader compares the file's schema with the registry **per named type**, following the reflect spec's decision table. A row is a type name with `(version, shallow hash)`; the live side has the same from `registry.schema_table`. `SchemaPolicy` chooses what happens when rows differ.

1. **Root.** Each root is matched to a live type: by stable name when the caller gave none and the `root_ref` is a named ref, otherwise by the caller's expected `TypeId` (`from_binary_as`, needed for an anonymous root such as `Vec<Wallet>`). The live `ref(root)` bytes must equal the file's `root_ref`, or both must name the same type; otherwise `RootMismatch { file, expected }`.
2. **Fast path: equal deep hash and equal versions.** The deep hash covers every shallow hash in a root's closure but **not the declared versions** (the reflect spec never hashes them), so an equal deep hash alone proves only that the layouts are equal. A version can still have been bumped with no layout change (the author wants a migration to run), and a newer file can carry an equal layout (it must be `FileNewer`). The fast path is therefore taken for a root only when its deep hash is equal **and** every entry in the file's closure has the same declared version as the live type of that name (one merge walk over the two sorted tables, O(types), cheaper than the decode it saves). Any version difference sends the root to step 3, where the rows are classified by their versions first and by their shallow hashes second. Rejected alternative: hashing versions into the deep hash, which would change the reflect spec's hash contract and make a version bump look like a layout change in `schemas.lock`.
3. **Otherwise per row**, by the reflect spec's table:

| Row | `Strict` (default) | `Lenient` |
|---|---|---|
| version equal, shallow equal | decode directly | same |
| file version < current (the shallow hash may be equal or different: a bump with no layout change still needs its migration) | run the registered migration chain; **no chain: `NoMigration { type_name, file, current }`** | chain if registered, otherwise **conform** by name (below) with a `Conformed` warning (for an equal layout conform is the identity) |
| file version > current (whatever the shallow hash, so a newer file with an equal layout is caught) | `FileNewer { type_name, file, current }`. **Never migrated, never downgraded** | same: `FileNewer` |
| version equal, shallow differs (an unversioned schema change) | a **same-version normalizer** with a matching `from_hash` if registered (see "Migrations"), otherwise `SchemaMismatch { type_name, version, file_shallow, current_shallow }` | same normalizer, otherwise conform |
| type only in the file (removed or renamed) | error unless a migration names it (`to_name` for a rename, or one that drops it) | same, and a bare removed type that nothing reaches is ignored |
| type only in the registry (added) | not an error by itself | same |

The first error in the closure is reported, naming the type and both versions or hashes. Nothing is "guessed": a body that no rule above can decode is an error. `Lenient` exists for development and editor caches (a dev loads yesterday's file after changing a struct); `Strict` is what saves, net and shipped files use.

**Older `hash_version`.** Hashes made under different `SCHEMA_HASH_VERSION`s are never compared with each other. If the header's `hash_version` is below the reader's, the reader asks the registry for the live table **as that version would have produced it** (`TypeRegistry::schema_table_for(root, hash_version)`, an addition to #36's API; only version 1 exists today, so the call is trivially the current table) and runs the same fast-path and per-row logic against it. If reflect no longer carries that version's hasher the error is `HashVersionUnsupported { file, current }`. The deep-hash fast path is only valid between tables of the same version, which the call guarantees. A `hash_version` above the reader's is `HashVersionNewer` (the stream grammar may differ, so nothing is parsed).

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
    pub to: u32,                            // > from; or == from for a same-version normalizer (needs from_hash)
    pub from_hash: Option<SchemaHash>,      // if set, applies only to a file row with this shallow hash
    pub to_name: Option<&'static str>,      // set to rename the type (the result is then a value of this type)
    pub apply: Box<dyn Fn(&mut MigrationCx, DynamicValue) -> Result<DynamicValue, MigrationError> + Send + Sync>,
}
pub struct Migrations { /* (type_name) -> edges */ }
impl Migrations {
    pub fn register(&mut self, m: Migration) -> Result<(), MigrationError>;   // rejects to < from, to == from without from_hash, a duplicate (type_name, from, to, from_hash)
    /// `live_name` is the live type's name (the file row's own name unless a `to_name` rename maps it).
    pub fn chain(&self, type_name: &str, from: u32, shallow: SchemaHash, live_name: &str, live: u32) -> Option<Vec<&Migration>>;
}
pub struct MigrationCx<'a> { pub file_type: &'a FileTypeInfo /* name, version, fields with FileRefs */, /* warnings sink */ }
```

- **`DynamicValue`** is `Box<dyn Reflect>` holding the `Dynamic*` value the decoder built from the **file's** layout (field names from the embedded schema, so a migration can rename, split, merge or drop fields by name) or a primitive. It carries no `represented_type_info` (the old shape has no `TypeInfo`); `MigrationCx::file_type` says what it was.
- **Same-version normalizers.** An unversioned layout change leaves a file row and the live type at the **same** version with different shallow hashes (the decision table's last row). `to > from` can never express a fix for that, so an edge with `to == from` is allowed **only with `from_hash` set** (a *normalizer*): it applies to a file row whose version is `from` and whose shallow hash is `from_hash`, and returns the value in the shape the next step expects. When the file row is already at the live version the normalizer's output must be the live shape; when it is below, the normalizer's output is the shape the following version edge takes as input.
- **Chain resolution.** For a file row `(name, version = from, shallow)` and live version `live`, a chain is: an optional normalizer (an edge from `from` to `from` whose `from_hash` equals `shallow`; at most one, always first), followed by the **shortest path** in the graph whose nodes are `(type name, version)` pairs and whose edges are the registered migrations with `to > from` (breadth-first, so a skip edge `1 -> 3` beats `1 -> 2 -> 3`), ending exactly at `(live_name, live)`. If `from == live` the path part is empty, so a lone normalizer is a complete chain (and with no normalizer there is no chain: `SchemaMismatch`). **A matching `from_hash` outranks path length.** The edges leaving the first node are split into two ranks: those whose `from_hash` equals the file row's shallow hash, and those with no `from_hash` (an edge with a different `from_hash` is not a candidate). The search takes the shortest chain whose first edge is in the matching rank; only if no such chain reaches `live` does it take the shortest chain whose first edge has no `from_hash`. So a hash-matched 2-step path beats an unhashed 1-step skip edge, which is how a type that never declared a version (`from = 0`) is told apart from itself at different times. Length decides only among chains of the same rank. Later edges are matched by name and version only (the intermediate shape is whatever the previous step produced). **A rename re-keys the rest of the chain.** After an edge with `to_name`, the node is `(to_name, to)`, so every later edge is looked up under the new name, and the chain ends at the live type the final name denotes (a `to_name` that is not a registry type ends no chain). A rename also changes the shallow stream of every type that holds the renamed type in a field (its `ref` names a different type), so the **parent's own row** is classified on its own as a changed row: it needs its own migration or normalizer under `Strict`, or is conformed under `Lenient`; the child's chain does not touch it, and the parent then receives the already migrated child (bottom-up, below). Ties are broken by the lowest `to` first, then registration order, so resolution is deterministic. The result is cached per file type for the decode. A chain is never built past `live`: an edge with `to > live` is unusable, not an error at registration (the registry does not know the live versions).
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
pub struct BinaryReader<'a> { /* Arc<AlignedBytes> raw payload, FileSchema, DecodePlan, cursor, registry/refs borrows */ }
impl<'a> BinaryReader<'a> {
    /// Copies `bytes` into an `AlignedBytes` once (documented cost); use `open_owned` to avoid the copy.
    pub fn open(bytes: &[u8], registry: &'a TypeRegistry, opts: ReadOptions<'a>) -> Result<Self, BinaryError>;
    /// The caller read the file straight into an `AlignedBytes` (`AlignedBytes::read_from`), so nothing is copied.
    pub fn open_owned(bytes: AlignedBytes, registry: &'a TypeRegistry, opts: ReadOptions<'a>) -> Result<Self, BinaryError>;
    pub fn roots(&self) -> &[RootInfo];                                             // in add_root order, with the file's names
    pub fn root_slot(&self, ty: TypeId) -> Option<RootSlot>;                        // the live type's slot in this file
    pub fn read_value(&mut self, slot: RootSlot) -> Result<Decoded, BinaryError>;   // next value, decoded as that root
    pub fn read_len(&mut self) -> Result<u64, BinaryError>;
    pub fn read_bulk_view<T: Leaf>(&mut self, slot: RootSlot) -> Result<BulkView<T>, BinaryError>;   // next value as a view (below)
    pub fn finish(self) -> Result<Vec<BinaryWarning>, BinaryError>;                  // every body byte consumed, padding zero, string table fully used
}

/// A run of `Leaf` elements inside the reader's buffer. `'static`, `Clone`, `Send + Sync`:
/// it holds an `Arc` to the buffer, so it outlives the reader and may be sent to another thread.
pub struct BulkView<T: Leaf> { /* Arc<AlignedBytes> + byte range, or an owned Vec<T> on a big-endian host */ }
impl<T: Leaf> Deref for BulkView<T> { type Target = [T]; }
```

The reader's sequence of `read_*` calls must mirror the writer's; a mismatch surfaces as an ordinary decode error (a bad varint, a bad index, leftover bytes at `finish`). The slots are the schema's roots, so the file still says what it holds and the version table covers every type any value uses. `to_binary` is `new`, `add_root`, one `write_value`, `finish`.

### Zero-copy and soundness

**The decision: `bytemuck` casts, no `unsafe` in this crate.** The alternative was a designated `unsafe` module (`slice::from_raw_parts` with `// SAFETY:` comments and Miri tests). Rejected: it puts a soundness argument on the reviewer for every PR, and the safe route costs nothing at run time. The workspace is `unsafe`-denied, `bytemuck` is a small `no_std` crate the renderer already needs (so the workspace carries it once; see "Dependencies"), and it does the checks we would otherwise write ourselves (alignment, size multiple, no padding bytes).

How it is used:

- **`AlignedBytes`** owns a `Vec<u64>` and a byte length. `as_bytes()` is `bytemuck::cast_slice::<u64, u8>(&buf)[..len]`, which is safe and gives a slice whose start is 8-aligned. `AlignedBytes::read_from(impl Read, len)` fills one straight from a file with no extra copy; `from_slice` copies. File offset `64 + schema_len` is a multiple of 8, so the raw payload of codec 0 is 8-aligned **inside the file buffer**, and a decompressed payload is decompressed into a fresh `AlignedBytes` and is aligned by construction.
- **Who owns the buffer.** `BinaryReader` owns its raw payload as an `Arc<AlignedBytes>` (the whole file for codec 0, the decompressed payload for codec 1). It is never a borrow of the caller's `&[u8]`: `open(&[u8])` copies into an `AlignedBytes` once (so any caller slice, aligned or not, works, and the cost is documented), and `open_owned(AlignedBytes)` takes the buffer without a copy. The plain `from_binary*` functions need **no alignment at all**: they decode from any `&[u8]` (typed bulk arrays are copied into a destination `Vec<T>` whose own alignment is correct), and decompress into a plain `Vec<u8>`. Views are the only consumers of alignment, so only the reader pays for it.
- **`Leaf`** is a sealed trait implemented for `u8, i8, u16, i16, u32, i32, u64, i64, f32, f64` (all `bytemuck::Pod`, so any bit pattern is valid, which is exactly what makes a cast sound) with `fn from_le(self) -> Self`.
- **Read.** `bytemuck::try_cast_slice::<u8, T>(bytes)` gives a `&[T]` over the `AlignedBytes` on a little-endian host when the slice is aligned and a whole number of elements (both checked by the call; they hold by construction for the reader's buffer). `read_bulk_view` wraps that range in a `BulkView<T>` that holds the `Arc`: `Deref` re-runs the (alignment and length) cast, which cannot fail for a range it already validated, and falls back to an empty slice rather than panic if it ever did. On a **big-endian** host the `BulkView` holds an owned `Vec<T>` built by copying and `from_le` per element. So a view is zero-copy for codec 0 **and** codec 1 (the decompressed buffer is the reader's), `Owned` only on big-endian hosts, and its values are identical either way; no path returns a `Cow` that borrows from a buffer the reader owns.
- **Write.** A `Vec<T>` item list is downcast (`as_any().downcast_ref::<Vec<T>>()`, the zero-copy bulk list access that reflect's open question 10 leaves to this spec (that question's other subject, sets, `VecDeque` and `Duration`, is out of scope here)) and appended with `extend_from_slice(bytemuck::cast_slice(&v))` on little-endian hosts, element-wise `to_le_bytes` elsewhere. Lists of other types, and `[T; N]` arrays (const `N` cannot be downcast generically), take the element-wise path through `List::iter` and `value_ref` into the same bytes.
- **Non-castable items.** `Bool` and `Char` share the bulk layout and decode to a typed `Vec<bool>` / `Vec<char>` after one validating scan (every byte `0` or `1`, every `u32` a scalar value), with no `Box` per element. `I128` and `U128` decode to a typed `Vec<i128>` / `Vec<u128>` by `from_le_bytes` per element (not castable: 16-byte alignment differs across targets). Any item that is a **validated Value** (a type name that is not the primitive's, such as `Num` and `NonZeroU32`) shares the layout but decodes element by element through `set_value` and the validating constructor, then re-reads the stored leaf with `value_ref` and compares it with the wire value: a NaN in a `Vec<Num>` fails with `InvalidValue`, and a `-0.0` or an infinity (which `Num` would normalize to `0.0` / a finite value) is `NonCanonical`, because the writer can never have produced it. The layout is a function of the schema, not of how the decoder chooses to read it.
- **Decode to a typed value.** A bulk list of a castable primitive decodes straight to a concrete `Vec<T>` boxed as the field's value (one allocation and one copy of the bytes, no `Box` per element); `FromReflect` for `Vec<T>` and `[T; N]` accepts any `List` value of matching items (reflect #33, tested in B10), so the typed result costs at most one more memcpy.
- **Views.** `BinaryReader::read_bulk_view::<T>(slot)` is the real zero-copy entry: when the slot's root is `Vec<T>` or `[T; N]` of a `Leaf` item type, it returns the next value's data as a `BulkView<T>` over the reader's `Arc<AlignedBytes>` (above). This is what `exitium_save` uses for columnar snapshots and the renderer or tilemap can use for a mesh or tile slab, after the CRCs have been verified once at `open`. A view of any other item type is `TypeChanged`.

The crate has no `unsafe` and no Miri module. A test runs the little-endian cast path and the big-endian swap path (the swap function is not `cfg`-gated, so CI exercises it on x86 and on wasm builds) and asserts the same values.

### Robustness: limits and no panics

Reading untrusted bytes must return `Ok` or `Err`, never panic, never overflow the stack, and never allocate or loop beyond what the budgets below allow (each budget is charged before the work it pays for; the table of expansion factors shows how far a budget exceeds the file's own size). All arithmetic on offsets and lengths is checked.

```rust
pub struct BinaryLimits {
    pub max_file_bytes: u64,     // 512 MiB: stored size
    pub max_raw_bytes: u64,      // 1 GiB:  raw payload (wasm32 has 4 GiB of address space)
    pub max_ratio: u32,          // 1024:   upper cap on raw_len / stored_len for any codec (codec.max_ratio() may be lower)
    pub max_schema_bytes: u32,   // 16 MiB
    pub max_types: u32,          // 65_536: schema entries
    pub max_strings: u32,        // 16 Mi
    pub max_string_bytes: u32,   // 256 MiB: the string table's data
    pub max_values: u64,         // 256 Mi: values per top-level value; every leaf, struct and element decoded one by one counts 1 (time guard); a bulk element does not (see below)
    pub max_decoded_bytes: u64,  // 1 GiB: heap per top-level value (nodes and typed bulk arrays; memory guard)
    pub max_string_bytes_decoded: u64, // 1 GiB: cumulative decoded string bytes per FILE, across every value of a session
    pub max_zero_size_elements: u64,   // 16 Mi: cumulative elements of zero-byte types (`Vec<()>`, `Vec<EmptyStruct>`) per FILE
    pub max_depth: u32,          // 128
}
```

- **Four counters, charged before the work they pay for**, so a bomb is refused without having cost it. A *top-level value* is one `from_binary*` call or one `BinaryReader::read_value`; the first two counters restart for each, the last two are **cumulative per file**.
  - `values` counts every value decoded one by one: each leaf, struct, enum, option, list or map node, and each element that goes through an element-wise path (including the elements of a `Vec<Num>`, which build a validated value each). A **bulk element** decoded as a plain run (a castable primitive, `bool`, `char`, a 128-bit integer, a packed integer) is **not** charged to `values`: it is charged to `bytes` (below), which is the tighter bound for it, and its time cost is a copy or about 3 ns per packed element. This is what lets an honest 512 MiB `Vec<u8>` blob pass the default limits, since `values` (256 Mi) is smaller than `max_raw_bytes` (1 GiB) and would otherwise refuse it. `values` bounds time.
  - `bytes` bounds the memory of one top-level value and is charged the estimated heap of what it builds, using fixed per-item costs so the rule is exact and testable: every `Dynamic*` node or boxed leaf costs `NODE_COST = 64` and a typed bulk array costs `len * size_of::<T>()`. A session that reads a 1M-entity world value by value therefore pays per entity, not for the whole world at once.
  - `string_bytes` is a **cumulative per-file** counter, because strings are an amplification vector that grows across values: it is charged `24 + len` for every decoded `String`, however many top-level values the file holds.
  - `zero_size_elements` is the other **cumulative per-file** counter. An element of a zero-byte type costs no body bytes, so a one-byte length claim buys up to `max_decoded_bytes / NODE_COST` = 16M nodes per `read_value`, and because `values` and `bytes` restart for each call, a file of `N` such values would cost `N` times that. Every element of a zero-size type is therefore also charged here, before its loop, and the total over the whole file is at most `max_zero_size_elements` (16 Mi). An honest file has no use for millions of zero-byte elements, and `exitium_save` raises the default if one does.
  Exceeding them is `TooManyValues` (`values`, and `zero_size_elements` with the hint "zero-size element budget") or `DecodedTooLarge` (`bytes`, and `string_bytes` with the hint "string budget").
- **String amplification: charged, not shared.** One string-table entry can be referenced by any number of one-byte indices, and each reference decodes to its own owned `String` (the reflect `String` leaf is not shareable), so the table size alone proves nothing about the decode's memory. **Each string reference is charged `24 + len` to `string_bytes` before it is cloned**, so a 64 MiB entry referenced 16 times fails the 1 GiB default at the sixteenth reference and a million references to a 1 KiB string cost about 1 GiB at most, even when spread over a million `read_value` calls. Decoding repeated strings into a shared `Arc<str>` was rejected: it would change the reflect `String` leaf. The defaults are the same order as `max_raw_bytes`, so an honest file never comes close, and `exitium_save` raises them for a known large world.
- **Length bombs.** Every length is checked **before** it reserves memory, using each `FileRef`'s **minimum encoded size**, computed once per type by the least-fixed-point pass in "Schema section" (a `bool` is 1, a `String` 1, a struct the sum of its fields, a fixed array `N` times its item, an enum the minimum over its variants, `Option`, list and map 1, a type that can be zero bytes such as `()` or an empty struct 0). The rules:
  - A growable list or map of `len` elements needs `len * min_element_size <= remaining body bytes` (a map uses `min_key + min_value`).
  - Elements that can be 0 bytes cannot be bounded by the remaining bytes, so `len` is charged to `values`, to `bytes` (`len * NODE_COST`) and to the per-file `zero_size_elements` **before the loop** (`Vec<()>` of `2^40` is `TooManyValues` at once, and a file of many `Vec<()>` values of 16M each is stopped by the per-file counter, not only by the per-value ones).
  - A **raw** bulk array needs `len * element_size <= remaining body bytes`, and charges `len * size_of::<T>()` to `bytes`. A raw bulk array of an element-wise validated item (`Vec<Num>`) is not a plain run and also charges `len` to `values` and `len * NODE_COST` to `bytes`.
  - A **packed** bulk array needs `byte_len <= remaining`, `len <= byte_len` (each varint is at least one byte), and then charges `len * size_of::<T>()` to `bytes` **before decoding any element**, so the expansion below is bounded by the budget, not by the file.
- **Expansion factors (wire byte to decoded byte), so the defaults can be checked by hand.**

| Item | Wire bytes per element | Decoded cost per element | Worst-case factor |
|---|---|---|---|
| raw castable primitive (`f32`, `u64`, ...) | `size` | `size` | 1 |
| `Bool`, `Char` (typed `Vec`) | 1, 4 | 1, 4 | 1 |
| `I128`, `U128` (raw only) | 16 | 16 | 1 |
| packed `U16`, `U32`, `U64` and signed (varint, at least 1 byte) | 1 | 2, 4, 8 | 8 |
| validated Value item (`Num`: 8 wire bytes; `NonZeroU32`: 4) | 8, 4 | `NODE_COST` 64 | 8, 16 |
| `String` (an index, at least 1 byte) | 1 | `24 + len` | unbounded per entry, so charged to the cumulative `string_bytes` |
| `Dynamic*` node (struct, enum, option) | at least its minimum size | `NODE_COST` 64 | 64 for a 1-byte node |

  The worst realistic honest amplification is 64x, and a 1 GiB `bytes` budget therefore admits a body of at least 16 MiB of the worst kind. An honest game raises `max_decoded_bytes` for a known large file; the defaults refuse the pathological case.
- **Depth.** `max_depth` bounds recursion in the body decoder (so `Option<Box<Tree>>` chains of a million nodes are `DepthExceeded` at 129, not a stack overflow; 128 frames of the decoder fit the 1 MiB wasm stack), and the writer refuses the same depth, so it never writes a file the reader refuses. Schema `FileRef` nesting is limited to 32, and a schema with a type whose minimum encoded size is infinite (a type that recurses with no way out) is `RecursiveSchema` before decode starts.
- **Strictness is robustness.** Every canonical-form rule in "Canonical form" is an error, so there is one parse of any accepted file.
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
    HashVersionUnsupported { file: u8, current: u8 },
    BadHeader, UnsupportedFlags, HeaderCrc, PayloadCrc, UnknownCodec(u8), CodecFailed, TooLarge, BombRatio, NonZeroPadding, NonCanonical,
    // schema
    SchemaCorrupt, RecursiveSchema, RootMismatch, UnknownType, SchemaMismatch, FileNewer { type_name: String, file: u32, current: u32 },
    NoMigration, MigrationFailed, TypeChanged, MissingField, UnknownVariantName,
    // body
    NonCanonicalVarint, BadBool, BadChar, BadUtf8, BadStringIndex, BadVariant, BadOptionTag, BadBulkTag, OutOfRange,
    DuplicateKey, KeyOrder, DepthExceeded, TooManyValues, DecodedTooLarge, InvalidValue, Reference, FromReflect,
}
pub struct BinaryWarning { pub kind: BinaryWarningKind, pub path: String, pub message: String }
pub enum BinaryWarningKind { Migrated, Conformed, DefaultedField, DroppedField }
```

`Display` is one line (`save.exb: raw payload +0x1a40, at components[2].gold: expected a string index below 41, found 977`); `BinaryError` serializes to the JSON shape `{kind, offset, path, expected, found, hint, message}`. The path is kept as a small stack of segments pushed and popped as the decoder descends (an index or a field position, about 2 ns) and rendered to the reflect path syntax only when an error is built.

### Public API sketch

```rust
// exitium_serial::binary
pub const BINARY_FORMAT_VERSION: u16 = 1;

pub struct WriteOptions<'a> {
    pub codec: u8,                              // 0 none (default), 1 LZ4
    pub int_arrays: IntArrays,                  // Raw (default) | Packed; recorded in header flag PACKED_INTS
    pub codecs: &'a Codecs,
    pub refs: Option<&'a mut dyn BinaryRefs>,   // added by B13
}
pub struct ReadOptions<'a> {
    pub policy: SchemaPolicy,                   // Strict (default) | Lenient
    pub limits: BinaryLimits,
    pub migrations: Option<&'a Migrations>,
    pub codecs: &'a Codecs,
    pub refs: Option<&'a mut dyn BinaryRefs>,   // added by B13
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
pub struct AlignedBytes;                         // as_bytes, from_slice, read_from(impl Read, len), zeroed, len
pub trait Leaf: bytemuck::Pod + sealed::Sealed { fn from_le(self) -> Self; }
pub mod varint { pub fn write(v: u64, out: &mut Vec<u8>); pub fn read(buf: &[u8], pos: &mut usize) -> Result<u64, VarintError>; }
pub mod crc32 { pub fn crc32(bytes: &[u8]) -> u32; pub fn update(state: u32, bytes: &[u8]) -> u32; }
```

**Each option field lands with the issue that defines its type**, so no issue names a type that does not exist yet. `WriteOptions` is first built by B6 with `codec`, `codecs` (B2's `Codecs`) and `int_arrays` (`IntArrays`, defined in B6). `ReadOptions` is first built by B8 with `limits` (B1), `codecs` (B2), `verify_crc`, `policy` (`SchemaPolicy`, B5) and `migrations` (`Migrations`, B4). `refs` is added to **both** structs by B13, which defines `BinaryRefs`, and B13 updates the call sites of the earlier issues. Before B6 and B8 the container functions (`inspect`, `compare_schema`) take their limits, codecs and migrations as plain arguments, as the signatures above show.

`from_binary*` take one registry read guard's worth of `&TypeRegistry` from the caller (reflect spec, threading); no helper takes a second guard. They decode from any `&[u8]` and need no alignment (see "Zero-copy and soundness"). `inspect` and `compare_schema` read only headers and schema, so a 1 GB save can be classified without decoding or even decompressing its body (the schema section is in the clear before the payload, which is why the container keeps it outside the compressed bytes).

### Threading model

- Everything is a pure function of its arguments. `Codecs`, `Migrations`, `BinaryLimits`, `BinaryError`, `Decoded` and `AlignedBytes` are `Send + Sync`. `BinaryRefs` is used through `&mut dyn` by one call on one thread and needs no `Send` bound. `BinaryWriter` and `BinaryReader` are single-owner values (not `Sync`) holding `&TypeRegistry`.
- No globals, no locks, no caches shared between calls; the per-call `DecodePlan` (file type to live type, min sizes, text-form flags, migration chains) is built once per open and read-only afterwards.
- Independent files may be encoded or decoded on different threads. One file is processed on one thread in v1: the format is sequential and positional, and this crate cannot depend on `exitium_tasks` (C1). Chunked LZ4 and the CRC are the two parallelizable passes; a parallel `Codec` and a threaded CRC belong to the caller (`exitium_save`), through the `Codec` trait. wasm32 is single-threaded anyway.
- A caller that needs more than one frame of budget for a large load yields between `read_value` calls of a session (the reader's state is just a cursor).

### Memory layout

- **Encode** builds the body in one `Vec<u8>` with the string table assigned as it goes, then assembles header, schema and payload (with the compressed copy for codec 1): peak about `body + stored` bytes plus the string table. A bulk array is appended straight from the source slice.
- **Decode** holds the caller's file bytes (plus one `raw_len` buffer for codec 1: a `Vec<u8>` for `from_binary*`, an `Arc<AlignedBytes>` for the reader, which also holds an aligned copy of the file for codec 0 unless `open_owned` was used), the `FileSchema` (about 50 KB for a 100-type closure) and the result. The cumulative decoded heap is capped by `max_decoded_bytes`. Decoding is a recursive descent with O(depth) stack and builds `Dynamic*` values that `FromReflect` then turns into typed ones. A caller that decodes a snapshot should read value by value through the session API so the `Dynamic*` tree of one entity, not of the world, exists at a time.
- A file with `N` named types costs `N` rows of `16 + stream` bytes (a typical stream is 40 to 200 bytes) in the schema section; a 100-type save carries a few KB of schema.

## MCP / editor surface

This crate adds **no editor feature of its own, no command and no MCP tool**: mutation goes through the command bus (design rule 3), and a loaded save enters the world through `exitium_save` and its commands. The merged `editor-mcp-mvp.md` lists only the **text** tools (`serial_parse_text`, `serial_write_text`, built in CB12 in `exitium_command`; `exitium_mcp` forwards generically). **Nothing in it owns binary tools**, so this spec files the work itself as **issue M1** (below, in `exitium_command`, the 13th Stage 3 spine PR) rather than crediting #110 or another spec. What `exitium_serial` exposes, so an agent and a human get the same capability (agent parity):

| Capability | Exposed by `exitium_serial` | MCP tool (built by M1) | Editor use |
|---|---|---|---|
| Describe a binary file without decoding it: format and hash versions, codec, sizes and ratio, roots, the `(type, version, shallow hash)` rows | `inspect` | `serial_inspect_binary { path or base64 }`, read-only, JSON out | none filed here (see below) |
| Compare a file to the current game: each type unchanged, migratable (steps), needs a migration, unversioned change, file newer, removed, added | `compare_schema`, `RowStatus` | `serial_compare_binary { path or base64 }`, read-only, the offline form of `reflect_get_schema_hash` | none filed here |
| Validate a file: full decode under a policy, errors with offset and path, warnings (`Migrated`, `Conformed`, `DefaultedField`) | `from_binary_as`, `BinaryError`, `BinaryWarning` | `serial_validate_binary { path or base64, type_name?, policy }`, read-only, returns the error JSON shape | none filed here |
| Convert between text and binary for diffing and review | `to_binary` and `from_binary` composed with `to_text`/`from_text` (#23) | `serial_convert { direction, type_name, data }`: text to binary bytes (base64) or binary to canonical text; read-only (it writes no file) | none filed here |

**Editor surface.** A save-file panel and an "Export as text" action are plausible editor uses of the same four calls, but the editor spec has no saves panel and the saves spec (SP14) has not decided one, so **no editor issue is filed from this spec**; the tools above are the agent-parity surface and the editor can call the same functions. Recorded as an open question (12).

The error JSON is `{kind, offset, path, expected, found, hint, message}`, so an agent can locate and fix the cause. The `path` uses the reflect path grammar, so it can be handed to `SetField` after a text conversion.

## Performance

Budgets are p50, single thread, on the min-spec reference runner (4 cores, 8 GB), measured with criterion in `crates/exitium_serial/benches/binary.rs`, IDs `exitium_serial/binary/<case>/<size>` at 1k and 100k rows or at the byte size named (docs/benchmarks.md), held by the 5% gate once baselined. They are initial targets; the first benchmark PR may re-baseline with a note to the owner. The **reference row** is the text spec's entity-like struct (about 10 leaves): `id: u32`, two `Vec2`, a `Num`, a unit enum, a `String` from a small set, `flags: u8` and a 3-item `Vec<u8>`. In binary it is about **35 bytes** against about 90 in text.

The row path is bounded by the reflection walk (reflect budget: 50 M leaves/s) and by one `Box` per leaf on decode. The bulk path is bounded by the CRC, not by memory bandwidth: 32 MB at the 1 GB/s CRC floor is 32 ms, one 32 MB copy is about 6 ms, and the rest (header, schema, string table, a 10% margin) brings the total to about 50 ms, so the bulk rows are set at **600 MB/s (55 ms)** and every number in this table is derived from the same CRC floor. A CRC faster than the floor (slicing-by-8 usually reaches 1.5 to 2 GB/s on x86) is margin, not a different budget.

| Operation | Budget |
|---|---|
| Encode rows (`to_binary`, codec 0): reflect walk, string table, CRC | >= 100 MB/s of output; 100k rows (about 3.5 MB) <= 40 ms |
| Decode rows (`from_binary_into`: CRC, plan, positional decode, `FromReflect`) | >= 35 MB/s of input; 100k rows <= 100 ms (the text typed read of the same rows is budgeted at 225 ms) |
| Write one varint / read one varint | <= 3 ns each |
| Write or read one fixed leaf in the row path | <= 10 ns plus the reflect cost (about 20 ns encode, about 80 ns decode with its `Box`) |
| String table: build with 10k distinct strings; one lookup (encode) | <= 1 ms; <= 40 ns |
| CRC-32 (slicing-by-8, std only) | >= 1 GB/s |
| **Bulk encode, codec 0** (a `Vec<f32>` of 8M elements, 32 MB): one cast copy plus the payload CRC | >= 600 MB/s (<= 55 ms) |
| **Bulk decode to `Vec<T>`, codec 0**: payload CRC plus one copy | >= 600 MB/s (<= 55 ms) |
| **Bulk zero-copy view** of 32 MB: `open_owned` (the CRC pass), then `read_bulk_view` | `open_owned` <= 36 ms; the view itself <= 50 us. `open(&[u8])` adds one aligned copy of the file (<= 8 ms): <= 44 ms |
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

**1M-entity world.** The snapshot itself is `exitium_save`'s (SP14) and its 1M-entity round-trip test is #67 (a facade test, C7). This format is what makes it reachable: through the row path 1M entities x 5 components is about 50 M leaves, so encode <= 3 s and decode <= 6 s with a peak under 2 GB when the snapshot is read value by value (a whole-world `Dynamic*` tree is not built); stored as columns of bulk arrays (position and velocity slabs), the same data moves at the bulk rates above, 64 MB in about 110 ms each way. The first issue that adds the 1M benches records the numbers.

**Roadmap rows.** The roadmap's "encode and decode of a 32 MB bulk-array world at least 500 MB/s" is the codec-0 bulk row (>= 600 MB/s here, so the row has a 20% margin) and its "text round trip of the same at least 20 MB/s" is the text spec's. Its "zstd level 3 decode at least 800 MB/s" moves to SP14, since this crate ships LZ4 only; the LZ4 rows above replace it here.

Complexity: encode and decode are O(bytes) plus O(k log k) per map for the key sort; the plan is O(types in the closure); a migration chain is O(values of the migrating type).

## Test plan

- **Container.** Header fields accepted and rejected (every reserved field non-zero, flags bit 0 accepted and bit 1 rejected, magic, `format_version` 0 and 2, `hash_version` 2, an unregistered codec id); **the check order**: a file built to fail two checks at once reports the earlier one of the ten listed steps (a test per adjacent pair, including a newer-version file with a bad CRC reporting `FormatNewer`, and a wrong-length file with a bad header CRC reporting `Truncated`); an older `hash_version` goes through `schema_table_for`, and an unsupported one is `HashVersionUnsupported` (a fake reflect table stands in until a second version exists); `FormatNewer` is reported before any CRC or length check (a version-2 file with garbage after the first 10 bytes); both CRCs (flip every single bit of a small file: always an error and never a panic); `Truncated` at every prefix; `TrailingBytes`; non-zero padding in every pad position; the 176-byte worked example matches **byte for byte** and decodes to the same value; `CRC-32("123456789")` and the other standard vectors.
- **Varint.** Round trip over `0`, powers of two and their neighbours, `u64::MAX`; rejection of overlong forms (`80 00`, ten bytes ending in a value above 1, eleven bytes) and truncated varints; zigzag round trips for every signed width at its extremes.
- **Leaves and structure.** Every `ValueRepr` at min, max and a pattern; `bool` 2 and option tag 2 rejected; `char` surrogates and above `10FFFF` rejected; `f32`/`f64` with NaN payloads, `-0.0` and subnormals round trip bit for bit in plain float fields; struct, tuple struct, tuple, enum (unit, tuple, struct variants), `Option<Option<T>>`, transparent chains (`Box<Gold>` is 8 bytes), an empty struct (0 bytes), a recursive type; unknown variant index; variant reordering is a schema change (hash differs) and is caught by the table.
- **Strings.** Table order is first-use order, **enforced**: an index above `next_new` is `NonCanonical`, an entry nothing references is `NonCanonical` at `finish`, and both hold across a `Lenient` conform and a migration; duplicates in a table rejected; index out of range; invalid UTF-8; empty string is index 0 and may be used at any time; a million-string table within the limits; strings of every Unicode scalar round trip (no escaping needed in binary). **Amplification:** a table with one 64 MiB entry referenced by 20 one-byte indices fails with `DecodedTooLarge` at the 16th reference (the 1 GiB default) before the 16th clone is allocated (a counting allocator checks the peak), and a million references to a 1 KiB string are accepted exactly while the charged total stays under the budget.
- **Bulk arrays.** Lengths 0, 15, 16, 17 and 1M for every eligible repr, growable and fixed; the padding after `tag` is zero bytes up to the next multiple of 8 and **0 bytes when the tag already ends on a multiple of 8** (a case per residue of the tag's offset modulo 8, so one test per padding length 0 to 7 and no case of 8); `Vec<Num>` of 16+ with a NaN fails the whole decode with `InvalidValue`, a stored `-0.0` or an infinity is `NonCanonical`, `NonZeroU32` with a 0 fails; `Bool` and `Char` bulk validated; **packed** arrays for `I16`, `I32`, `I64`, `U16`, `U32`, `U64` at extremes, canonical varints, an element that overflows its width, `byte_len` mismatch, `len > byte_len`, packed on `f32`, `bool`, `I8`, `U8`, `I128`, `U128` and `Guid` rejected (`BadBulkTag`); a `Vec<u128>` and a `Vec<Guid>` are raw even with `PACKED_INTS` set; **tag/flag agreement**: a `PACKED_INTS` file with a tag-0 packable list (and the reverse) is `BadBulkTag`, so a mixed file cannot be accepted; the two options give different bytes for the same value and each round-trips; `Vec<f32>` raw equals the inline bytes of its elements.
- **Zero-copy and soundness.** `read_bulk_view` returns a shared `BulkView` for codec 0 **and** for a decompressed payload (the reader owns the `Arc<AlignedBytes>`; the view outlives the reader, is `Send`, and a test drops the reader and reads the view); `open(&[u8])` at a one-byte offset copies once and still yields a shared view, and `open_owned` makes no copy (a counting allocator); the view holds an owned `Vec<T>` only on a big-endian host, and the byte-swap path (not `cfg`-gated) gives the same values as the cast path; a view of a non-`Leaf` item type is `TypeChanged`; a test grep-asserts the crate contains no `unsafe` (the workspace lint already denies it).
- **Maps.** `HashMap` and `BTreeMap` give identical bytes; every key kind has an ordering test, including negative ints, `Entity`-like `{index, generation}` struct keys (by `(index, generation)`, so `10v1` after `9v1`), `Guid`, unit-only enums and strings by bytes; an out-of-order or duplicate key on read; the hook replaces keys before sorting.
- **Determinism.** The same value encodes to the same bytes across runs, thread counts, a shuffled `HashMap` insertion order, and (CI on Windows and Linux plus the wasm build running the byte-vector tests) platforms; `encode(decode(bytes)) == bytes` for every golden file whose rows are `Unchanged` (not the migrated and conformed ones) and every seeded random value, under both `PACKED_INTS` settings; and for **each canonical rule** a file that violates only that rule is rejected with the listed error (the table in "Canonical form" is the test list, one case per row).
- **Compression.** LZ4: round trip at sizes 0, 1, 255 KiB, 256 KiB, 256 KiB + 1, several MB; an incompressible chunk is stored raw and decodes; the chunk framing rejects `raw_len` 0, above 256 KiB, a sum mismatch, `stored_len` above the data, and a block that decompresses to **fewer or more bytes than its header declares** (`decompress_into` returns the count, and the codec fails on any difference: a short block must not leave zero bytes in the payload); a checked-in codec-1 sample decodes (so a future `lz4_flex` bump cannot silently break old files); **bombs**: a header claiming `raw_len` of 1 TB, a ratio above `min(codec.max_ratio(), limits.max_ratio)` (255 for LZ4, 1 for codec 0, the codec's own for a custom one), a chunk whose decompressed size differs from its header (each fails before a large allocation: a counting allocator asserts the peak stays under the bound). A custom `Codec` (a toy run-length one, id **200**, which the header rule allows because ids 64 to 255 are application-defined and it is registered in `Codecs`) round-trips, and the same file read with a `Codecs` that lacks it is `UnknownCodec(200)`; a custom codec whose `decompress` returns a count below `out.len()` (an under-fill) or above it is `CodecFailed`, and one that returns exactly `out.len()` is accepted.
- **Schema section.** Streams parse into `FileSchema` equal to the live `TypeInfo`s for derived sample types (struct, enum, recursive, mutually recursive, diamond, `Vec<Wallet>` anonymous root); `shallow != fnv(stream)` is `SchemaCorrupt`; a dangling `Named` ref; nesting above 32; duplicate entry names; entries out of order; entry and byte limits; **duplicate field names in one struct, duplicate variant names in one enum, and a variant whose recorded `index` is not its position** (each is `SchemaCorrupt`, and a stream built by hand with the correct hash for the corrupt layout still fails). **Recursion:** `struct A { a: A }`, `struct A { b: B }` with `struct B { a: A }`, `struct A { a: [A; 2] }` and an enum whose every variant recurses are `RecursiveSchema` at schema validation (no decode, no stack growth, no size used before the check); `struct T { l: Option<Box<T>> }`, `struct T { c: Vec<T> }`, `enum L { Nil, Cons(Box<L>) }` and `[A; 0]` are accepted, with the right minimum sizes. **Order independence:** `enum Expr { Num(f64), Add(Box<Pair>) }` with `struct Pair { a: Expr, b: Expr }` is accepted with minimum sizes 9 and 18 **under both name orders** (the same schema with the two types renamed so that `Pair` sorts first and again with `Expr` first, and with the entries presented in each order); a three-type cycle through enums with one exit is accepted and one with no exit is rejected under every rotation of the names; the reported type for an infinite schema is the first by name and is the same for every input order; a 10,000-type chain of structs is solved in time linear (to a log factor) in its size. `deep_hash` in the file matches the registry's for the same types and differs after changing a field.
- **Decision table and policies** (reflect's table, every row, both policies): unchanged; version lower with and without a chain; version higher is `FileNewer` even under `Lenient`; same version with a different shallow hash (`SchemaMismatch`, then a same-version normalizer, then `Lenient` conform); **the fast path needs equal versions**: a file whose deep hash equals the registry's but whose type declares a lower version is **not** decoded directly (it runs its migration, or is `NoMigration`), a file with an equal layout and a **higher** version is `FileNewer` (both under `Strict` and `Lenient`), and a file with equal deep hash and equal versions is decoded with no per-row work (a counter on the plan asserts it); file-only type (removed, renamed with `to_name`, dropped by a migration); registry-only type; a `0 -> 1` migration for a type that gains its first version; the error names the changed type and not a type that merely contains it.
- **Conform.** Renamed and reordered fields by name, an added field with and without `#[reflect(default)]` (only an explicit default fills; `ReflectDefault` alone is `MissingField`), a removed field dropped with a warning, an enum variant moved to another index, a leaf repr widening rejected as `TypeChanged`; the conformed value passes validated constructors.
- **Migrations.** A chain `1 -> 2 -> 3`; a skip edge `1 -> 3` chosen over it; two shortest paths resolved by the tie rule; `from_hash` selecting between two `0 -> 1` edges; **a same-version normalizer** (`from == to`, `from_hash` set) that repairs an unversioned layout change alone (file and live both at version 2), followed by a version chain (file at `0` with a normalizer then `0 -> 1`), and rejected at registration without `from_hash`; a `to < from` edge rejected; a nested child migrating beneath an unchanged parent (bottom-up); a parent and child both migrating (child first); a rename through `to_name`; a migration returning a wrongly shaped value (`MigrationFailed`, whole decode fails); a migration error (no partial value); `FileNewer` not migrated; registration of duplicates rejected; migration of a map value type and a list element type; `Migrated` warning counts. The embedded-schema decode is tested against a type whose fields were later renamed and reordered (the reflect spec's required test).
- **References.** With a fake text-form type standing in for `Entity` (as in the text spec): no hook (canonical structural bytes), a hook mapping ids on write and read, called exactly once per occurrence, map keys sorted by the replaced value, a hook error with offset and path, generation 0 rejected before the hook. The real `Entity` is covered in `exitium_save` and `exitium_ecs` tests.
- **Sessions.** Two roots of different types, interleaved values and a bulk root read as a view; the reader's call sequence not mirroring the writer's fails cleanly; `finish` rejects leftover bytes; one string table across values.
- **Robustness.** Every truncation and a bounded single-byte mutation sweep of each golden file, plus seeded random bytes and random token soup, return `Ok` or `Err` and never panic (std-only xorshift, fixed seed, the failing seed and input printed); length bombs (`len = 2^60` for `Vec<u8>`, `Vec<()>`, `Vec<Vec<..>>`, a map) fail before allocating; `[[[...` style recursion through `Option<Box<Tree>>` 100000 deep is `DepthExceeded` with no stack overflow (also in the wasm build); `max_values`, `max_decoded_bytes`, `max_string_bytes_decoded`, `max_zero_size_elements`, `max_strings` and `max_types` each tripped, including a **bulk array of 2^30 elements** (charged `len * size` to `bytes` before any element is read, so it fails on the budget and not by exhausting memory) and a packed `Vec<u64>` of 1 M one-byte varints expanding 8x (charged 8 MB before decoding); a `Vec<Num>` of the largest count the budget allows is accepted and the next one is not, which pins the expansion table. **Large honest blobs:** a 400 MiB `Vec<u8>` (under `max_file_bytes` and `max_raw_bytes`) decodes under the default limits (bulk elements are not charged to `values`). **Zero-size elements across a file:** a session file of 20 `Vec<()>` values of 1 M elements each (one-byte length claims, so each is within the per-value budgets) fails with `TooManyValues`, hint "zero-size element budget", in the 17th value (16 values use exactly the 16 Mi allowance), where the same values read under a per-value-only counter would all pass.
- **Platform.** `cargo build --workspace --target wasm32-unknown-unknown` passes; no `unsafe`; the codec-0 golden bytes are identical on Windows, Linux and (byte-vector tests) wasm32.
- **Benchmarks.** One per row of the performance table.

## Dependencies

**Two new crates.io dependencies, both for intake 1** (high-risk area: new dependencies get extra review; CLAUDE.md, ADR-0005). Both were built and exercised (compress and decompress, and a cast round trip) for the host and `wasm32-unknown-unknown` while writing this spec.

| Crate | Version | Features | Licence | wasm32 | Used for | Why not std |
|---|---|---|---|---|---|---|
| `lz4_flex` | `0.14` (caret; `Cargo.lock` was at 0.14.2 when this was written) | `default-features = false`, `std`, `safe-encode`, `safe-decode` | MIT | yes (pure Rust; the `safe-*` features compile out its `unsafe` paths, so it keeps the workspace `unsafe`-free) | the LZ4 codec (id 1) | std has no compression. Writing LZ4 by hand is about 600 lines of match finding and a decompressor that is a security surface; a reviewer should not have to audit our own. **Not pinned exactly:** nothing in the format depends on the compressor's bytes (codec-1 output is outside the cross-version contract and a checked-in sample proves old files still decode), so an exact pin would only block security patches. `Cargo.lock` gives reproducible builds, and a bump runs the codec-1 golden sample and the benchmark gate. |
| `bytemuck` | `1` (render-v1.md and roadmap intake 1 plan 1.25.2) | none | `Zlib OR Apache-2.0 OR MIT` | yes (`no_std`, no target code) | safe byte-to-slice casts and `Pod`/`Zeroable` for `AlignedBytes` and bulk arrays | std casts a slice only through `unsafe` `from_raw_parts`, which the workspace denies. `bytemuck` is **not in `Cargo.lock` on `main` today**: the renderer (`render-v1.md`, with its `derive` feature) and roadmap intake 1 declare it for Stage 1, so when intake 1 lands this crate shares one workspace line with them (the render crate adds the `derive` feature on its own entry); until then intake 1 is what adds it. This crate uses only the core casts and no optional feature. It is a small, widely audited, `no_std` crate with no dependencies. |

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
- **External crates for `exitium_serial`:** `lz4_flex = { workspace = true }` and `bytemuck = { workspace = true }` under `[dependencies]`. In `[workspace.dependencies]` (one line each, with the licence/wasm/why-not-std comment the roadmap asks for): `lz4_flex = { version = "0.14", default-features = false, features = ["std", "safe-encode", "safe-decode"] }` and `bytemuck = "1"` (`bytemuck` is not declared on `main` yet; intake 1 declares it once for the renderer, which adds `features = ["derive"]` on its own entry, and for this crate, which needs no feature).
- **Dev-dependencies:** `criterion` (already in the workspace) for `[[bench]] name = "binary", harness = false`, added by the first issue that adds a benchmark (B1), inside the crate.
- **Facade (`crates/exitium/`):** the `exitium::serial` re-export is the facade's wiring, not an issue below (it is a hotspot owned by the facade PRs).
- **Reflect (`crates/exitium_reflect/`):** this spec needs four small things from the schema-hash issue #36, which is **its** footprint, not an edit made by an issue below (see "Downstream issue adjustments"): the exact shallow stream on each `SchemaEntry`, a public `fnv1a_64`, the `ValueRepr` to and from code functions, and `TypeRegistry::schema_table_for(root, hash_version)` (the older-`hash_version` path). The B-issues that read the schema are blocked by #36.
- **Command (`crates/exitium_command/`):** issue M1 adds the four binary MCP tools and a base64 decoder, as the 13th Stage 3 spine PR (the roadmap freezes the crate after Stage 3). `exitium_command -> exitium_serial` already exists (roadmap C1, CB12 uses the text format), so there is no new edge.
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
11. **Enum map keys: `u32` or varint.** `reflect-model.md` ("Map keys") says a unit-only enum key is written in binary as "the `u32` variant index". This spec writes every variant index, including a key's, as a **varint** (one byte for fewer than 128 variants), and orders keys by that index. The two lines disagree. This spec cannot edit `reflect-model.md`. Recommendation: a doc-only amendment of that line to "the variant index, encoded as the binary format specifies", made with the one-line map-order amendment listed in "Downstream issue adjustments". The format here is the one to keep (a fixed `u32` would cost three extra bytes per key for nothing).
12. **An editor surface for saves.** The four binary MCP tools (M1) give agents the capability, but the editor spec has no saves panel and SP14 has not decided one, so no editor issue is filed. Recommendation: SP14 decides whether the editor shows a save-inspector built on `inspect` and `compare_schema`; the functions are public, so nothing here blocks it.
13. **Default budgets.** `max_values` (256 Mi) and `max_decoded_bytes` (1 GiB) apply per top-level value, and `max_string_bytes_decoded` (1 GiB) and `max_zero_size_elements` (16 Mi) per file, sized for wasm32's 4 GiB address space and for a 1M-entity world read entity by entity (50 M leaves, well under 256 Mi values in total, and a few hundred bytes per entity under the per-value byte budget). Recommendation: keep them and let `exitium_save` raise them for a known large file; the first benchmark PR (B1) re-checks the numbers.


## Implementation issues

Fourteen `exitium_serial` issues (B1 to B14) and one `exitium_command` issue (M1). The B-issues edit exactly one crate, so they run **one at a time in numeric order**, in the same lane as the text issues S1 to S12 (one crate has at most one issue in flight). Each is about 600 changed lines or fewer, has acceptance criteria and **one** `Blocked by` line (the only place the dependencies are stated; "Downstream issue adjustments" refers to it and never repeats it), updates the "Public API", "Invariants" and "Benchmarks" sections of `crates/exitium_serial/CLAUDE.md` for what it adds, and adds its benchmark rows to `benches/binary.rs` (design rule 4). They are filed into the full MVP milestone with `file-issue` after this spec merges (label `area:serial`, role `impl-engine-core`). **B1 to B14 are Stage 1 issues**, each `Blocked by <intake-1>` (directly or through an earlier B-issue), the Stage 1 intake PR that adds `lz4_flex` and `bytemuck` (the number is filled in when the issues are filed). **M1 is not a Stage 1 issue.** It edits `exitium_command`, which the roadmap freezes after Stage 3 (C1), and it needs CB12 and intake 2, so it runs in Stage 3 as the **13th and last spine PR**, right after CB12 (see M1). Reflect issues: #31 (model, registry), #32 (dynamic access, `Dynamic*`), #33 (containers), #36 (schema hash, with the adjustments below), #37 (core impls), #34 and #35 (derive, only for test types).

Every B-issue has this Footprint (full labelled form):

```
## Footprint
- Edits: crates/exitium_serial/
- Governs: none
- Hotspots: none
- New crate edges: none
```

The container check order (steps 1 to 10 in "Header") is built in three places: B2 (steps 1 to 6, 8 and 9, including the string-table parse of step 9), B3 (step 7) and B5 (step 10). Each issue's tests cover its own steps and the order among them.

**B1. `feat(serial): binary primitives (varint, CRC-32, aligned buffer, leaf casts, limits, errors)`** (new, about 550 lines)
- Adds `binary/{mod,error,limits,varint,crc32,aligned,leaf,view}.rs`: canonical varint, slicing-by-8 CRC-32, `AlignedBytes` (including `read_from`), the sealed `Leaf` trait with `try_cast_slice` and the byte-swap path, `BulkView<T>` (an `Arc<AlignedBytes>` plus a range, or an owned `Vec<T>` on a big-endian host), `BinaryError`, `BinaryErrorKind`, `BinaryWarning`, `BinaryLimits` and the `Budget` counters (values, bytes and the two per-file counters, string bytes and zero-size elements); the `[[bench]] binary` target and `benches/binary.rs`. Reflect-free.
- Acceptance criteria:
  - [ ] Varint canonical-form tests (every overlong and truncated case) and the CRC-32 standard vectors; the byte-swap path equals the cast path on the host.
  - [ ] `BulkView` is shared (zero-copy) for an aligned buffer, outlives the object that built it and is `Send + Sync`; an owned view only on the big-endian path; no `unsafe`.
  - [ ] `Budget` charges before it allows and refuses exactly at the limit (a test per counter).
  - [ ] No panic over every truncation of the test inputs; wasm32 build passes.
  - [ ] Benchmarks `exitium_serial/binary/{varint,crc32,bulk_copy}/*` meet the budget.
- Blocked by: <intake-1>.

**B2. `feat(serial): binary container and codecs (header, LZ4 chunks, bomb limits)`** (existing #58, rescoped: LZ4 only, zstd is SP14; about 650 lines, the string-table parse being the last 50)
- Adds `binary/{container,codec}.rs`: the 64-byte header read and write with every field rule and the **check order** steps 1 to 6, 8 and 9, the file-length and padding checks, both CRCs, the `Codec` trait (with `max_ratio`) and `Codecs`, codec 0 and the chunked LZ4 codec (`decompress` returns the byte count and the container compares it with `raw_len`), the header-only codec-agnostic bomb checks, the **string-table parse** of check step 9 (entry offsets, UTF-8 `BadUtf8`, duplicate entries `NonCanonical`, `NonZeroPadding`, the exact padded size; the first-use order and unused-entry checks need the body and are B8's), and `inspect`'s container half.
- Acceptance criteria:
  - [ ] The header rules table has an accept and a reject test per row; `FormatNewer` precedes every other check; a test per adjacent pair in the check order; every single-bit flip of a small file is an error.
  - [ ] LZ4 round trip at the chunk-boundary sizes, an incompressible chunk stored raw, a checked-in codec-1 sample decodes, a toy codec with id 200 registers, and one whose `decompress` returns a count other than `out.len()` is `CodecFailed`; an unregistered id is `UnknownCodec`.
  - [ ] String-table parse: invalid UTF-8, a duplicate entry, a non-zero pad byte, offsets that decrease or do not end at `data_len`, and `strtab_len` that is not the padded size each give the listed error, in check order.
  - [ ] Bomb tests (a 1 TB claim, a ratio above `min(codec.max_ratio(), limits.max_ratio)` for codec 0, LZ4 and a custom codec, chunk size mismatch) fail before a large allocation (counting allocator).
  - [ ] Benchmarks `exitium_serial/binary/{lz4_compress,lz4_decompress}/*` meet the budget.
  - [ ] Edit #58: drop `gate:human` and `needs-spec`, replace "zstd and LZ4" with LZ4 and the pluggable `Codec`, add the Footprint.
- Blocked by: <intake-1>, B1.

**B3. `feat(serial): binary schema section (write, parse, verify, recursion check, inspect)`** (new, about 600 lines)
- Adds `binary/schema.rs`: the schema section writer (roots and the union of closures from `registry.schema_table`, sorted), the parser into `FileSchema` and `FileRef`, the `shallow == fnv(stream)` check, nesting and count limits, the **recursion check and minimum encoded sizes** as a least fixed point (check order step 7), the name and index checks (duplicate field or variant names, a variant `index` that is not its position), and `inspect` (container plus schema, no registry).
- Acceptance criteria:
  - [ ] Streams for derived samples (struct, enum, recursive through `Option` and `Vec`, mutually recursive, diamond, `Vec<Wallet>` root) parse into a `FileSchema` equal to the live `TypeInfo`s.
  - [ ] `SchemaCorrupt` cases (bad hash, dangling `Named`, depth 33, duplicate or unsorted entries, a duplicate field name, a duplicate variant name, a variant `index` that is not its position) and the entry and byte limits.
  - [ ] `RecursiveSchema` for `struct A { a: A }`, a two-type cycle, `[A; 2]` and an all-recursive enum; the finite cases of "Test plan" are accepted with the right minimum sizes; `Expr`/`Pair` (`enum Expr { Num(f64), Add(Box<Pair>) }`, `struct Pair { a: Expr, b: Expr }`) is accepted with minimums 9 and 18 under both name orders and every entry order; the result is a least fixed point, in O((types + fields) log types), checked on a 10,000-type chain.
  - [ ] Benchmark `exitium_serial/binary/schema_parse/*` meets the budget.
- Blocked by: <intake-1>, B2, #36.

**B4. `feat(serial): migration registry and chain resolution`** (existing #59, part 1; about 400 lines)
- Adds `binary/migrate.rs`: `Migration`, `Migrations`, `MigrationCx`, `MigrationError`, `register` with its rules (`to < from` and a normalizer without `from_hash` rejected, duplicates rejected), and `chain` (optional same-version normalizer, then the shortest `(name, version)` path, a matching `from_hash` outranking path length, `to_name` renames re-keying later edges, the tie rule, never past the live version). No application to values yet.
- Acceptance criteria:
  - [ ] Chain `1 -> 2 -> 3`; a skip edge `1 -> 3` chosen over it; two shortest paths resolved by the tie rule; `from_hash` selecting between two `0 -> 1` edges; **a hash-matched 2-step path chosen over an unhashed 1-step skip edge**, falling back to the unhashed edge when the hash-matched one cannot reach the live version; a rename (`to_name`) in the middle of a chain re-keying the later edges, and a `to_name` that is not a registry type ending no chain; a lone normalizer when file and live are at the same version; a normalizer followed by `0 -> 1`; an unusable edge past the live version; deterministic across registration orders.
  - [ ] Registration rejections as listed.
  - [ ] Benchmark `exitium_serial/binary/chain/*` (resolution over 1000 registered edges) meets the budget.
- Blocked by: <intake-1>, B3, #32.

**B5. `feat(serial): decode plan and schema policy (decision table, fast path, compare_schema)`** (new, about 500 lines)
- Adds `binary/plan.rs`: `SchemaPolicy`, the registry match (root matching, `RootMismatch`), the **fast path on equal deep hash and equal versions**, the per-row decision table for both policies with errors that name the changed type and both versions or hashes, `FileNewer` in both, the older-`hash_version` path (`schema_table_for`, `HashVersionUnsupported`), `DecodePlan` (type to type map, minimum sizes, text-form flags, resolved chains from B4), `compare_schema` and `RowStatus` (check order step 10). No body decoding.
- Acceptance criteria:
  - [ ] Every row of the decision table, in both policies, has a test (including an unversioned change, a file-only type and a registry-only type), and the error points at the changed type, not at a type that contains it.
  - [ ] The fast path is taken only with equal versions: a version bump with an equal layout runs its migration or is `NoMigration`, and a newer file with an equal layout is `FileNewer` (both policies).
  - [ ] `compare_schema` returns the right `RowStatus` for every row without decoding or decompressing a body, and with a `Migrations` that resolves a chain it reports `Migratable { steps }`.
  - [ ] Benchmark `exitium_serial/binary/plan/*` meets the budget.
- Blocked by: <intake-1>, B4, #31, #36.

**B6. `feat(serial): binary encoder (leaves, structs, enums, options, lists, strings)`** (existing #56, rescoped: maps and bulk are B7; about 550 lines)
- Adds `binary/encode.rs`: the body walk for every kind except maps and bulk lists, the string table in first-use order, validated-Value reads through `value_ref`, the depth limit, the header flag `PACKED_INTS` plumbing, `WriteOptions` (`codec`, `codecs`, `int_arrays` and `IntArrays`; `refs` comes with B13), `to_binary` and `write_binary` for one value.
- Acceptance criteria:
  - [ ] The 176-byte worked example matches byte for byte; golden files for every row of the body table except maps and bulk.
  - [ ] Deterministic output across runs; transparent types erased; `hidden` fields written, `skip` fields absent.
  - [ ] Benchmark `exitium_serial/binary/encode_rows/{1000,100000}` meets the budget.
  - [ ] Edit #56: drop `gate:human` and `needs-spec`, retitle to "binary encoder", add the Footprint.
- Blocked by: <intake-1>, B3, #31, #32, #33, #37.

**B7. `feat(serial): binary encoder for maps and bulk arrays`** (new, about 550 lines)
- Adds canonical key sorting for every key kind (structural order), the bulk layout with the raw and packed tags (`WriteOptions::int_arrays`, packable widths `I16` to `U64` only), the `Vec<T>` downcast casts for the ten castable primitives and the element-wise path for the rest, and alignment and padding.
- Acceptance criteria:
  - [ ] `HashMap` and `BTreeMap` give identical bytes; every key kind has an ordering test; lengths 15, 16 and 17 for every eligible repr; padding zero bytes up to the next multiple of 8 and none when already aligned (every residue 0 to 7).
  - [ ] Packed arrays for every packable width at extremes; `Vec<u128>` and `Vec<Guid>` stay raw under `Packed`; `Vec<Num>` encodes the same bytes as `Vec<f64>`.
  - [ ] Benchmarks `exitium_serial/binary/{encode_bulk,encode_map,encode_packed}/*` meet the budget (the 32 MB bulk row at >= 600 MB/s).
- Blocked by: <intake-1>, B6.

**B8. `feat(serial): binary decoder core (budget, canonical checks, leaves, strings, structs, enums, options)`** (existing #57, part 1; about 550 lines)
- Adds `binary/{decode,cursor}.rs`: the body cursor and error path stack, canonical varints, leaves with every canonical check (bool, char, UTF-8, string first-use order and unused strings, validated Values through `ReflectFromReflect` with the re-read comparison, `InvalidValue` and `NonCanonical`), the `Budget` charges (values, bytes, string bytes and zero-size elements) and `max_depth`, positional decode into `Dynamic*` for structs, tuples, enums and options on an exact schema (the plan's fast path), `ReadOptions` (`limits`, `codecs`, `verify_crc`, `policy`, `migrations`; `refs` comes with B13), `from_binary`, `from_binary_as` and `from_binary_into` for types with none of the container kinds.
- Acceptance criteria:
  - [ ] Every `decode(encode(x)) == x` through `reflect_eq` for the B6 golden files that use these kinds; every rejection in the leaves, structure, strings and canonical-form test groups, including the string amplification test (a counting allocator).
  - [ ] Depth and value-count tests fail without allocation or stack overflow (also on wasm32); the `Num` NaN case is `InvalidValue` and `-0.0` and infinity are `NonCanonical`; `NonZeroU32` zero fails.
  - [ ] Benchmark `exitium_serial/binary/decode_struct/{1000,100000}` meets the budget.
  - [ ] Edit #57: drop `gate:human` and `needs-spec`, retitle to "binary decoder", add the Footprint.
- Blocked by: <intake-1>, B5, B6, #32.

**B9. `feat(serial): binary decoder for lists and maps`** (existing #57, part 2; about 450 lines)
- Adds list decoding (inline element by element below 16 elements and for non-bulk items, growable and fixed arrays), map decoding with strict key order, `DuplicateKey` and `KeyOrder`, the length-times-minimum-size rule, the zero-size element charge, and the full `from_binary*` for the reference row.
- Acceptance criteria:
  - [ ] The maps, lists and length-bomb test groups (including `Vec<()>` of `2^40` and a map `len = 2^60`), every key kind, an `Entity`-like struct key.
  - [ ] `encode(decode(bytes)) == bytes` for the B6 and B7 golden files that use these kinds.
  - [ ] Benchmarks `exitium_serial/binary/{decode_rows,decode_map}/*` meet the budget (`decode_rows` at 1000 and 100000).
- Blocked by: <intake-1>, B7, B8.

**B10. `feat(serial): binary decoder for bulk arrays and zero-copy views`** (new, about 500 lines)
- Adds bulk decoding for both tags (typed `Vec<T>` fast path for the ten castable primitives, validated `Vec<bool>` and `Vec<char>`, `Vec<i128>` / `Vec<u128>`, the element-wise validated path for `Num` and similar), the `PACKED_INTS` flag and tag agreement check, `len`/`byte_len` and budget charges before decoding, `BinaryReader::open`, `open_owned` and `read_bulk_view` (only the part a single bulk root needs; sessions are B13), and the `FromReflect` acceptance test for `Vec<T>` and `[T; N]` from a concrete `Vec<T>`.
- Acceptance criteria:
  - [ ] The bulk, zero-copy and canonical-form test groups: tag/flag mixing is `BadBulkTag`, packed on a non-packable repr is `BadBulkTag`, `len > byte_len`, an element overflowing its width, the `2^30`-element and 8x-expansion budget cases.
  - [ ] A view over a decompressed payload outlives the reader and is `Send`; `open` copies once and `open_owned` does not (counting allocator); the big-endian owned path equals the cast path.
  - [ ] Benchmarks `exitium_serial/binary/{decode_bulk,decode_packed,view}/*` meet the budget (the 32 MB bulk decode at >= 600 MB/s, `open_owned` <= 36 ms).
- Blocked by: <intake-1>, B7, B9.

**B11. `feat(serial): Lenient conform adaptation`** (new, about 400 lines)
- Adds the by-name adaptation of file types to live types (`Conform` plans): defaults only from an explicit `#[reflect(default)]`, dropped fields, variants by name, `TypeChanged`, and the `Conformed`, `DefaultedField` and `DroppedField` warnings; the post-order adapt pass that B12 also uses.
- Acceptance criteria:
  - [ ] The conform test group, including the explicit-default-only rule, a leaf repr widening rejected as `TypeChanged`, and the conformed value passing the validated constructors.
  - [ ] An old body is decoded with renamed and reordered fields (the embedded-schema requirement of the reflect spec); strings in dropped fields still obey first-use order.
  - [ ] Benchmark `exitium_serial/binary/conform/*` meets the budget.
- Blocked by: <intake-1>, B5, B10.

**B12. `feat(serial): migration application (bottom-up, all or nothing)`** (existing #59, part 2; about 400 lines)
- Adds post-order application of resolved chains (the chain from B4 runs as soon as a value of the migrating type is decoded), renames through `to_name`, the `Migrated` warning with counts, `MigrationFailed` carrying offset and reflect path, and all-or-nothing failure.
- Acceptance criteria:
  - [ ] The migrations test group: nested child under an unchanged parent, parent and child both migrating (child first), rename, wrong shape, error, no partial value, `FileNewer` never migrated, a migration of a map value type and of a list element type, a normalizer repairing an unversioned change.
  - [ ] A 3-step chain over 100k values meets the budget (`exitium_serial/binary/migrate/*`).
  - [ ] Edit #59: drop `gate:human` and `needs-spec`, `from_version = 0` means "undeclared", the `from == to` normalizer is allowed only with `from_hash`, add the Footprint.
- Blocked by: <intake-1>, B4, B11.

**B13. `feat(serial): binary session API and the reference hook`** (new, about 500 lines)
- Adds `BinaryWriter` and the rest of `BinaryReader` (shared string table, several roots, `write_len` and `read_len`, `finish` checks including unused strings), the `BinaryRefs` hook with the once-per-occurrence rule and replaced-key sorting, the `refs` field of `WriteOptions` and `ReadOptions` (and the updates to earlier call sites), and moves `RefSite` and `RefError` into a shared `refs` module (or re-exports them) so both formats use one definition.
- Acceptance criteria:
  - [ ] The sessions and references test groups, with a fake text-form type standing in for `Entity`; a hook error carries offset and path.
  - [ ] A two-root session (a struct root and a bulk-array root read as a view) round-trips, and a mismatched read sequence fails cleanly; the string and zero-size-element budgets are cumulative across `read_value` calls while `values` and `bytes` restart.
  - [ ] No regression in the text benchmarks after the shared `refs` move.
- Blocked by: <intake-1>, B12, S11.

**B14. `test(serial): golden files, seeded round trip and robustness for the binary format`** (existing #60, rescoped; about 550 lines)
- Adds `tests/golden_binary/*` (the worked example, one file per body-table row, a codec-1 sample, a migrated file), the std-only seeded generator of random reflected values, the truncation and mutation sweeps, the bit-flip sweep, one rejection case per canonical rule, the cross-platform byte-vector test and `exitium_serial/binary/roundtrip/{1000,100000}`. Runtime under 30 s in CI.
- Acceptance criteria:
  - [ ] `decode(encode(x)) == x` and, for `Unchanged` rows, `encode(decode(bytes)) == bytes` over seeded random values (struct, enum, container mixes, extreme `Num`, unicode strings, empty containers, recursive types) and every golden file, under both integer-array options and both codecs.
  - [ ] No panic on every truncation, every single-bit flip of the small golden files, a bounded single-byte mutation sweep of the others, and seeded random bytes; every failure prints its seed.
  - [ ] The codec-0 golden bytes are identical on Windows and Linux CI and in the wasm byte-vector test.
  - [ ] Edit #60: add the Footprint.
- Blocked by: <intake-1>, B13.

**M1. `feat(command): binary serial tools (inspect, compare, validate, convert)`** (new, about 500 lines; `exitium_command`, not `exitium_serial`)
- **Where it runs: the 13th spine PR of Stage 3.** `exitium_command` is frozen after Stage 3 (roadmap C1: "a later change is filed narrowly and runs alone at a lane boundary"), so M1 is **not** a post-Stage-3 narrow change. It is added to the Stage 3 command spine as its last PR, after CB12 (spine CB1 to CB12 becomes 13 PRs), so it runs inside the spine's own serial chain and the freeze is never broken. Every other `exitium_command` dependent (editor, MCP, script, the physics and ui surface PRs) waits for it as for any spine PR (R3, R4). Its dependencies (B14, the whole Stage 1 serial lane, and CB12) are both met when the spine reaches it, so it adds one PR to Stage 3's critical path and no wait. The roadmap amendment is listed in "Downstream issue adjustments".
- Footprint:
  - Edits: `crates/exitium_command/`
  - Governs: none
  - Hotspots: none
  - New crate edges: none
- Adds the four read-only queries of "MCP / editor surface" to the built-in tool table: `serial_inspect_binary`, `serial_compare_binary`, `serial_validate_binary` and `serial_convert` (text to binary as base64, binary to canonical text), with the binary error JSON shape and `schema_revision` handling as for the text tools. The tool table of `editor-mcp-mvp.md` gains these four rows (doc-only amendment listed below).
- **Adds the base64 decoder.** The tools take `path or base64`, but CB7 builds only a base64 **encoder** (`json.rs`, editor-mcp-mvp.md "Not added, on purpose": a 25-line encoder with RFC 4648 vectors). M1 adds the matching decoder beside it (about 40 lines, no `base64` crate): RFC 4648 section 4 alphabet only, `=` padding required, and canonical (non-zero trailing bits, whitespace, any byte outside the alphabet, padding in the middle and a length that is not a multiple of 4 are each an error naming the offset). The decoded length is computed from the input length and checked against `BinaryLimits::max_file_bytes` **before** any allocation.
- Acceptance criteria:
  - [ ] Each tool returns data equal to the Rust API's, as JSON (a test per tool); `serial_validate_binary` returns the error JSON of this spec; `register_builtin` lists them (the name-list test of CB12 is updated).
  - [ ] `serial_convert` round-trips text to binary to text on the text format's golden files; nothing is written to disk and no undo record is made.
  - [ ] The base64 decoder passes the RFC 4648 section 10 vectors (`""`, `"f"`, `"fo"`, `"foo"`, `"foob"`, `"fooba"`, `"foobar"` and their encodings) in both directions with CB7's encoder, rejects each malformed case above, and refuses an input whose decoded size passes `max_file_bytes` without allocating (counting allocator).
- Blocked by: <intake-1>, <intake-2>, B14, CB12.

**Not filed from this spec.** Per-crate specs do not file issues in other crates beyond M1, which exists because no merged spec owns the binary tools:

- The binary **world snapshot** (#61) is `exitium_save`'s (SP14, C7); it uses the session API, the bulk views and the reference hook.
- A zstd codec, the saves container (encryption, tamper detection, autosave) and a game's migration registry are SP14's.
- An editor save-inspector panel is not filed (open question 12).
- The reflect-side changes below are made in #36 and doc-only reflect amendments, owned by the reflect issues.

### Downstream issue adjustments

Applied by the architect when this spec merges (the bullets are the contract; dependencies are the `Blocked by` lines above):

- **#56, #57, #58, #59, #60:** as described under B6, B8 and B9, B2, B4 and B12, and B14. Remove `needs-spec` and `gate:human` from all five (ADR-0005; the PR Risk section names the serialization format); add the Footprint; set `Blocked by` as listed under the issue. #58's zstd scope is dropped (SP14). Add `area:serial` where missing.
- **#61:** retitle and re-scope per C7 (the binary world snapshot in `exitium_save`, SP14), and note that it builds on B13 and B10's views. Not an `exitium_serial` issue.
- **#36 (schema hash):** `SchemaEntry` gains `stream: Vec<u8>`, the exact shallow stream (version byte first), as part of the persisted-format contract with a parse test; export `fnv1a_64(bytes: &[u8]) -> u64`; export the `ValueRepr` to and from code table (`repr_code`, `repr_from_code`); add `TypeRegistry::schema_table_for(root, hash_version)` (trivially the current table while only version 1 exists). The stream grammar in the reflect spec is the contract and the golden vectors cover it.
- **#32, #33:** `FromReflect` for `Vec<T>` and `[T; N]` accepts any `List` value of matching items (including a concrete `Vec<T>` of the same element type), and the `Dynamic*` builders can create values by field name with no `represented_type_info` (used for old-schema nodes); both are tested there.
- **Reflect spec, doc-only:** three amendments to `reflect-model.md`. (1) The "Validated Values" test line (L644) expects a `Vec<Num>` **binary decode** to store `-0.0` as `0.0` and to saturate `inf` to `Num::MAX`. The binary format makes both `NonCanonical` (the writer can never have produced them; see "Canonical form"), so the line becomes: a stored `-0.0` or infinity in a `Vec<Num>` is rejected as `NonCanonical`, and a NaN element is `InvalidValue`; the other routes in that line (`set`, `apply`, `from_reflect`, text decode) keep their normalizing behaviour. (2) and (3) are in the "Map keys" paragraph, which is amended in two places. "Deterministic order when a `HashMap` is written" becomes: each serializer defines its own total order (the text serializer sorts text forms by their written text; the binary serializer sorts structurally, this spec). "Enums as the `u32` variant index" becomes: the variant index, encoded as the binary format specifies (open question 11).
- **`editor-mcp-mvp.md`, doc-only:** the "reflect, serial" row of the tool table gains the four binary tools of M1, CB12's name-list criterion covers them, the spine list notes that M1 follows CB12 as a 13th PR (filed with this spec's issues, so the spine is CB1 to CB12 plus M1), and the "Not added, on purpose" line for `base64` says the encoder is CB7's and the decoder is M1's.
- **Roadmap (`mvp-roadmap.md`):** the serial lane in Stage 1 goes from "about 17" PRs to **26** (S1 to S12 plus B1 to B14; the binary part was estimated at 5, it is 14 once split to PR size), still under the ECS chain (about 28), so the Stage 1 critical path is unchanged. **Stage 3's command spine goes from 12 to 13 PRs** (CB1 to CB12 plus M1, in the "Work" list, the "Critical path" line and the stage table), so the Stage 3 critical path goes from about 31 to **32** serial PRs (spine 13, editor lane 13, facade hook 3, tests 3); M1 is placed in the spine, not after the freeze, so C1's "`exitium_command` is frozen after Stage 3" holds as written; intake 1's dependency list names `lz4_flex` (`0.14`) and `bytemuck` as above; the "zstd level 3 decode" performance row moves to SP14; the roadmap's open question 6 is answered by "SP14 decides, id 2 reserved". The 32 MB bulk row is the codec-0 row in this spec (>= 600 MB/s against the roadmap's 500).
- **SP14 (saves) and SP16 (net):** build on `BinaryWriter`/`BinaryReader`, `BinaryRefs`, the bulk views and the `Codec` trait; both may use `compare_schema` for pre-flight checks. SP14 owns codec id 2, encryption and a game's migration registry, and reconciles its "saves row" budgets with the rates above.
- **#23 (text spec):** no change. The two formats share `RefSite` and `RefError` after B13.
- **Whole-crate footprints.** The text issues and the B-issues all declare `Edits: crates/exitium_serial/`, so the factory runs them one after another (26 PRs). If the lane ever becomes the critical path, the S- and B-issues can be re-footprinted to `src/text/`, `src/binary/` and their benches and tests, plus a hotspot on `src/lib.rs` and the crate `CLAUDE.md`; that is a filing-time choice, not a design change.
