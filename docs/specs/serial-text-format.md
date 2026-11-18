# Spec: Serialization text format

- Issue: #23
- Crates: `exitium_serial` (touchpoints: `exitium_reflect`, `exitium_scene` (#111), `exitium_ui`, `exitium_mcp`, `exitium_editor`)
- Status: draft
- Scope: Jam MVP. The text format only. The binary format (#24), compression and migrations are deferred, but the format carries a version marker so they can be added.

## Goal

Define the human-editable, diff-friendly text format that every authored file uses: scenes, UI markup and, in the MVP, game saves. An agent writes it through MCP and with its own file tools, a human reads and edits it in a text editor, and `git diff` shows exactly what changed. One grammar, driven entirely by `exitium_reflect` (design rule 2): the writer walks a reflected value, the reader binds text to a reflected type, and nothing in the format is hand-written per type.

This spec fixes the grammar, the mapping from `TypeInfo` to text, the exact spelling of every leaf, the canonical layout the writer produces, the forward-compatibility rules, the error contract, the public API, and the **reference story** (how `Entity` and `Guid` references are written, read and remapped) that the scene format (#111) builds on. The Jam MVP implementation issues are listed at the end.

## Non-goals

- The binary format, compression, schema tables and migrations (#24, #56 to #60; cut from the MVP). The text format has no schema hash and is self-describing by names (reflect spec, "Roles"). Only a format version marker is kept, so migrations can be added without a new format.
- The scene envelope: how entities, components and labels are arranged in a file, and spawning a scene into a world (#111). This spec gives #111 the primitives it needs (syntax tree, tagged values, reference hook) and nothing more.
- ECS world snapshots (#61). `exitium_serial` never depends on `exitium_ecs` (see "References"). #61 as written is cut from the MVP and its scope moves to the scene spec.
- Preserving comments and layout across a read-modify-write (open question 1). The writer is canonical and drops comments.
- A JSON codec. `exitium_mcp` converts JSON to and from reflected values (reflect stays JSON-free). This spec fixes the mapping rules it must follow ("JSON mapping"), and `exitium_serial` exports the leaf helpers both sides share.
- Data assets, prefab variants and runtime-defined types (cut from the MVP; reflect spec open question 9).
- Schema-less (untyped) tooling beyond what the syntax tree already gives (diff tools, linters).

## Design

### Crate and dependencies

`exitium_serial` depends on `exitium_reflect` only (which depends on `exitium_core`). It has **no external dependency**, no `unsafe`, and builds for wasm32 (it is not in `NATIVE_ONLY`). It does not name `Num`, `Guid` or `Entity`: it sees them as reflected `Value` leaves and as types registered with `ReflectTextForm`, so it stays below `exitium_ecs` and `exitium_scene` in the crate graph (Stage 1 can build it in parallel with the ECS).

### Decision: hand-written format and parser, not RON or another existing format

| Option | Verdict | Reasons |
|---|---|---|
| **Hand-written, RON-shaped grammar and a recursive-descent parser** | **Chosen** | No dependency, wasm-safe, exact control of spans, defaults, unknown fields, `Num`, text forms and canonical output. About 1500 lines for lexer, syntax tree, binder and writer, each readable on its own (design rule 6). |
| `ron` crate | Rejected | Built on `serde`; the reflect spec rejects `serde` (serializers walk reflection directly), so we would write a serde `Serializer`/`Deserializer` bridge for `dyn Reflect` and still lose: integer width in `ron::Value`, unknown-field retention, our default and missing-field rules, `ReflectTextForm` hooks, the `@label` reference token, and error spans for binder errors. Adds `serde` and others to the wasm build. |
| JSON / JSON5 | Rejected | No comments (JSON), no trailing commas, no enums, and unreadable diffs for large documents. Used only on the MCP wire (see "JSON mapping"). |
| TOML | Rejected | Poor for deep nesting and lists of structs; inline tables are clumsy for components. |
| YAML | Rejected | Indentation-significant and implicit typing (`no` is a bool). A mistake an agent makes silently changes meaning. |

The syntax resembles RON (and Rust struct literals) on purpose: agents and reviewers read it without learning anything. It is **not** RON and is not parsed by a RON parser. Differences: no implicit `Some`, no `#![enable(..)]` extensions, no struct-name requirement, no raw or byte strings, a `%exitium-text` header, the `@label` token, and the number rules below.

### Layers

```
text --lexer--> tokens --parser--> Syntax (untyped tree, spans)        needs no registry
                                      |
                                      +--bind(TypeInfo, TextRefs)--> DynamicValue / typed T   (needs &TypeRegistry)

&dyn Reflect --writer(TextRefs)--> text                                 (needs &TypeRegistry)
```

1. **Syntax tree.** The parser turns text into a flat arena of nodes with byte spans and does not know any type. Numbers stay as source slices. This is what lets the scene crate scan a file for `@label` tokens before binding anything, lets tools diff or lint without a registry, and keeps one error path for syntax mistakes.
2. **Binder.** Walks a syntax node and a `TypeInfo` together and produces a value: concrete primitives for leaves, `Dynamic*` values for structure (reflect spec, "`Dynamic*` types"), then `FromReflect` for a typed `T`. Type-directed: the same literal `5` is a `u8`, an `f32` or a `Num` depending on the target. Every binder error carries the span of the offending node.
3. **Writer.** Walks `&dyn Reflect` through `reflect_ref()` and writes canonical text straight to a `fmt::Write`. It builds no tree.

### Lexical rules

- **Encoding.** UTF-8. A leading BOM is skipped. Line endings `\n` and `\r\n` are both accepted and treated identically; the writer always writes `\n`. Tabs and spaces are whitespace. Indentation is not significant.
- **Comments.** `// to end of line` and `/* block */`. Block comments nest. Comments are trivia: they are never part of a value, are not preserved by the writer, and are allowed anywhere whitespace is.
- **Identifiers.** ASCII `[A-Za-z_][A-Za-z0-9_]*`, or a raw identifier `r#name` (the `r#` is dropped). Reserved words in value position are `true`, `false`, `inf`, `nan`; a variant or tag with such a name is written `r#true`. In field-name position (`name:`) the reserved words are ordinary identifiers. Type, field and variant names are ASCII in v1: the writer reports `UnsupportedName` for any other name, and the derive and registry reject them with the next reflect amendment (open question 8).
- **Paths.** `ident { "::" ident }`: a stable type name such as `glutton::economy::Wallet`. A stable name that is not a plain path (`glutton::Pair<u8,String>`, `Vec<f32>`) is written as a string literal in the places a type name appears (`"glutton::Pair<u8,String>"(first: 1, second: "x")`).
- **Keywords.** `true`, `false`, `inf`, `nan`.
- **Reserved characters.** `%` only starts the header. `@` only starts a reference token. Any other character outside the productions is a syntax error.

### Grammar (EBNF)

```ebnf
document   = [ BOM ] , trivia , header , trivia , value , trivia , EOF ;
fragment   = trivia , value , trivia , EOF ;

header     = "%exitium-text" , hs , uint , [ hs , "root" , hs , type_ref ] ;   (* one line *)
type_ref   = path | string ;

trivia     = { ws | line_comment | block_comment } ;
line_comment  = "//" , { any - LF } ;
block_comment = "/*" , { block_comment | any } , "*/" ;                          (* nests *)

value      = number | string | char | bool | ref | tagged | named | group | list | map ;
bool       = "true" | "false" ;
tagged     = string , group ;                  (* a type name that is not a path, then its group *)
named      = path , [ group ] ;                (* unit variant, Some(..), None, Variant(..), or a value written with its type name *)
group      = "(" , [ members ] , ")" ;
members    = fields | items ;
fields     = field , { "," , field } , [ "," ] ;
items      = value , { "," , value } , [ "," ] ;
field      = ident , ":" , value ;
list       = "[" , [ items ] , "]" ;
map        = "{" , [ entries ] , "}" ;
entries    = entry , { "," , entry } , [ "," ] ;
entry      = value , ":" , value ;

number     = [ "-" ] , ( int | float | "inf" ) | "nan" ;
int        = dec | "0x" , hex | "0o" , oct | "0b" , bin ;                      (* "_" allowed between digits *)
dec        = digit , { digit | "_" } ;
float      = dec , "." , dec , [ exp ] | dec , exp ;
exp        = ( "e" | "E" ) , [ "+" | "-" ] , dec ;

string     = '"' , { strchar | escape } , '"' ;
char       = "'" , ( strchar | escape ) , "'" ;
escape     = "\" , ( "n" | "r" | "t" | "\" | '"' | "'" | "0" | "u{" , hex{1,6} , "}" ) ;
ref        = "@" , refchar , { refchar } ;     (* refchar = A-Z a-z 0-9 _ - . *)
```

`hs` is spaces and tabs, `uint` a decimal integer, `any` any character, `ws` whitespace including newlines.

Disambiguation, all decided by one token of lookahead:

- In a `group`, a member that starts with an identifier followed by a single `:` is a `field`; anything else is an `item`. A group is all fields or all items, never a mix.
- `string group` is a `tagged` value. A string followed by anything else is a plain string.
- A `named` path of one identifier followed by `:` inside a map is a key (`{ Gold: 5 }`).
- `_` is only legal between two digits. `1.`, `.5`, `1e`, `+1`, `-nan`, `0x` and `1__0` are errors.
- Raw newlines and other control characters (except tab) inside a string or char literal are errors; write `\n`. This keeps files stable under `git` line-ending conversion.

The syntax tree is a function of this grammar alone. Everything type-specific is decided by the binder.

### Mapping from `TypeInfo`

Read through `reflect_ref()` / `TypeInfo`. "Written" is what the writer emits; "Accepted" is the extra the reader tolerates.

| Kind | Written | Accepted | Example |
|---|---|---|---|
| Struct (`StructInfo`) | `( name: value, .. )` in declaration order | fields in any order; the type name as a prefix (`glutton::Wallet(..)`, must equal the target's stable name) | `(gold: 5, name: "x")` |
| Tuple struct, built-in tuple | `( v0, v1 )` | name prefix | `(1.0, 2.0)` |
| `transparent` struct or newtype (including `Box<T>`) | the inner value, no wrapper | none | `Gold(3.5)` is written `3.5` |
| Enum, unit variant | `Variant` | `Enum::Variant`, `path::Enum::Variant` | `Idle` |
| Enum, tuple variant | `Variant(a, b)` | same | `Move(1.0, 2.0)` |
| Enum, struct variant | `Variant(x: 1, y: 2)` | same | `Attack(target: @boss, power: 7)` |
| `Option<T>` | `None`, `Some(v)` | none (no implicit `Some`) | `Some(3)`, `None` |
| `Vec<T>`, `[T; N]` | `[a, b]` | trailing comma; an array needs exactly N items | `[1, 2, 3]` |
| `HashMap<K,V>`, `BTreeMap<K,V>` | `{ k: v }` sorted by key (below) | any order; duplicate keys are an error | `{Gold: 12.5, Wood: 3.0}` |
| Value: ints, floats, `bool`, `char`, `String`, `()` | the leaf spellings below | see below | |
| Value with a text form (`Guid`, and any `ReflectTextForm` type) | string literal holding `to_text()`, or the `@label` token (see "References") | a hook may rewrite the text | `"67e55044-10b1-426f-9247-bb680e5fe0c8"` |
| `Entity` (struct, with a text form) | as a text-form value: `"42v1"` without a hook, `@label` with one | same | `@boss` |

Rules:

- **The text form wins.** A type registered with `ReflectTextForm` is written and read as its text (a string literal or `@label`), never structurally. `Entity` is a struct `{ index, generation }` in reflection and a string in text. The binary format ignores the hook (reflect spec); this format consults it before walking structure, as the reflect spec requires.
- **Variants are named, never numbered.** Reordering variants does not change a file; renaming one does (an "unknown variant" error with a suggestion).
- **Names are optional on read, never written** (except where a tag is asked for, see "Tagged values"). A name that is present must equal the target type's stable name; otherwise `TypeMismatch`.
- **Unit-like structs and empty groups.** A struct with no fields, the unit type `()` and an empty tuple are all `()`.
- **Positional values for a named struct are an error** (`(1.0, 2.0)` for `Vec2`), with a hint that spells out the fields.
- **Map keys** follow the reflect spec: ints in decimal, `String` as a string literal, `bool`/`char` as literals, unit-only enums as the bare variant name, text-form types as the text-form string or `@label`. Floats and structs are not keys (registration rejects them).

### Leaf spellings

All of these are pinned by golden tests. `r` is the leaf's `ValueRepr`.

**Integers** (`I8` to `I128`, `U8` to `U128`). Written: decimal, `-` only for signed, no separators (`-17`, `340282366920938463463374607431768211455`). Accepted: decimal, `0x`, `0o`, `0b` (so `0xff`, `0b1010`, `-0x10` for signed), and `_` between digits (`1_000_000`). A literal outside the type's range is `OutOfRange` and names the range (`expected 0..=255`). A float literal (`1.0`, `1e3`) for an integer is an error with the hint "write 1000". `u128` and `i128` use their full range; no 64-bit intermediate is allowed.

**Floats** (`F32`, `F64`).

- Written: the shortest decimal that reads back to the same value, using std's float formatting (`{}`/`{:e}`), which does not depend on locale or platform. If `1e-5 <= |x| < 1e16` the plain form is written, with `.0` appended when the digits have no `.` (`5.0`, `0.25`, `1000000000000000.0`); otherwise the scientific form `{:e}` is written (`1e16`, `1.5e-7`, `1.7976931348623157e308`, `5e-324`). `0.0` and `-0.0` are written `0.0` and `-0.0` (the sign of zero survives). `nan`, `inf` and `-inf` are keywords. An `f32` is formatted as an `f32`, never widened to `f64` first (`0.1f32` is `0.1`, not `0.10000000149011612`).
- Accepted: any `float` or `int` literal. An integer literal is converted to the float exactly or, when it has more digits than the float holds, to the nearest value (`16777217` into an `f32` is `16777216.0`). A float literal is parsed by std directly into the target width (an `f32` literal is parsed as `f32`, never through `f64`, which would round twice). A literal too large for the type (`1e999`) reads as `inf`. NaN payload bits are not preserved: every NaN reads as the canonical NaN, so tests compare NaNs by `is_nan`.

**`Num`** (reflect spec: a Value kind with `repr: F64` and no text form). It uses the `F64` spellings, so `Num::MAX` is `1.7976931348623157e308` and a large incremental-game value is written `1.5e300`, which reads back exactly (`Num::parse` in `exitium_core` reads it). Because the binder runs every Value through its validating constructor (see "Binding"), reading into a `Num` has three outcomes: `nan` is an error (`value nan is not valid for type Num`); `inf`, `-inf` and `1e999` saturate to `Num::MAX`/`MIN` and emit the warning `ValueNormalized`; `-0.0` is stored as `0.0` with the same warning. A `Num` is never written in `inf`/`nan`/`-0.0` form because it cannot hold one.

**Strings** (`String`). Written between double quotes. Escapes written: `\\`, `\"`, `\n`, `\r`, `\t`, and `\u{..}` for every other control character (`U+0000`-`U+001F`, `U+007F`), for the bidirectional controls (`U+202A`-`U+202E`, `U+2066`-`U+2069`) and for `U+FEFF`, so a reviewer cannot be fooled by text that reads differently from what it contains. Everything else is written as raw UTF-8 (no normalization). Accepted: those escapes plus `\'` and `\0`, and `\u{1}` to `\u{10FFFF}` that is a Unicode scalar value (surrogates are an error). A single-quoted `char` uses the same rules and writes `'\''` for the quote.

**`bool`**: `true`, `false`. **`()`**: `()`.

**Text-form leaves** (`Guid`, `Entity`, any `ReflectTextForm` type): a string literal holding exactly `to_text()`, so `Guid` is the 36-character lowercase UUID, `Entity` is `"42v1"`. `from_text` failures become a `TextForm` error with the type's `PATTERN` in the `expected` field (`^[0-9a-f]{8}-..`).

### Writer: canonical output

The writer is **deterministic**: the same value gives the same bytes on every OS, architecture and wasm32. Nothing depends on locale, hash-map order, pointer width or float printing of the host.

**Order.** Struct and variant fields in declaration order. Map entries sorted by key, **always**, whatever the Rust container is (a `BTreeMap` with a custom `Ord` and a `HashMap` write the same text): ints numerically, `bool` false before true, `char` by scalar value, strings and text forms by UTF-8 bytes (so `"10v1"` sorts before `"9v1"`), unit-only enums by variant index. This is the reflect spec's order made total. No field is omitted: `hidden` fields are written; `skip` fields are not in `TypeInfo`; default-valued fields are written too (a changed code default must not silently change a saved file; open question 2).

**Layout.** Layout is a function of the **shape** of the value, not of how wide its text happens to be, so editing a number does not reflow its neighbours and a diff shows one changed line. A value is *leaf-like* if it is a number, string, char, bool, `()`, text-form value or `@label`, a unit variant, `None`, or `Some(leaf-like)` or a transparent wrapper of a leaf-like value.

- A struct, tuple, tuple or struct variant, or map whose members are all leaf-like and that has **at most 4 members** is written **flat** on one line: `(x: 1.0, y: 2.0)`, `Attack(target: @boss, power: 7)`, `{Gold: 12.5, Wood: 3.0}`. A list or array of leaf-like items with **at most 8 items** is flat: `[1, 2, 3]`. An empty container is `()`, `[]` or `{}`.
- Everything else is **broken**: the opening bracket, then one member per line indented by 2 spaces (never tabs), each followed by a comma (including the last: the **trailing comma**), then the closing bracket on its own line at the parent's indent. Flat containers have no trailing comma and no padding inside the brackets: `[1, 2]`, not `[ 1, 2, ]`.
- A flat container whose one-line form would pass **column 100** (counted in characters, from the start of the line, including the trailing comma) is broken instead. This width cap is a safety valve for long strings, not the normal rule. Only that container breaks; its children are re-evaluated.
- A long list of numbers is one item per line. That is the diff-friendly choice; a packed layout for bulk numeric arrays is open question 4.
- `Some(x)` with a broken `x` is written with the inner group directly inside: `Some((\n  a: 1,\n))`.
- One space after `:` in a field or map entry; none before it or before a comma. No trailing whitespace. The document ends with exactly one `\n`.
- A reference hook may attach a **comment** to a text-form value (`WrittenRef::comment`). It is written after the item's comma on the same line (`icon: "67e5..", // sprites/coin.png`), only inside a broken container; a container with an annotated member is broken. Comments are decoration: the reader ignores them and the next write regenerates them from the hook.
- **Document header.** `write_document` writes `%exitium-text 1 root <type_ref>` then `\n` then the value. `write_value` (a fragment) writes no header. The version is `TEXT_FORMAT_VERSION`.
- **Recursion limit.** The writer refuses to descend more than 128 levels (a deeply nested recursive value), with `DepthExceeded` and the path. This keeps the wasm stack safe.

Canonical form is a fixed point: `write(read(write(x))) == write(x)`.

### Reader: binding text to a type

`bind` walks one syntax node and one `TypeInfo`.

- **Leaves** are parsed for the target `ValueRepr` and wrapped as the matching primitive. When the target is a **validated Value** (`Num`, `NonZeroU32`, or any hand-written Value kind: its `TypeInfo` name is not the primitive's), the binder then runs the type's registered `ReflectFromReflect` on that primitive, so the validating constructor runs at the literal's span. A rejection is `InvalidValue` (`value 0 is not valid for type NonZeroU32`, `value nan is not valid for type Num`). If the validated result differs bit-for-bit from the literal (saturation, `-0.0`), the binder adds a `ValueNormalized` warning. No unvalidated value is ever stored, and the binder never downcasts a `Num` to `f64` (reflect spec).
- **Text-form leaves**: the string (or `@label`) goes through the `TextRefs` hook if one is installed (see "References"), then `ReflectTextForm::from_text`.
- **Structs and variants.** Each `field: value` is matched by name. See "Forward compatibility" for missing, unknown and duplicate fields.
- **Containers.** Lists and maps bind each element with the item/key/value type. A fixed array needs exactly N items (`expected 4 items, found 3`). A duplicate map key is `DuplicateKey`, with the first occurrence in `related`.
- **Option.** `None` and `Some(v)` only. A bare value for an `Option` field is an error with the hint "write Some(..)". The identifiers `None` and `Some` are the option syntax only where an `Option` is expected; in an enum they are ordinary variant names.
- **Tagged values.** A group written with its type name (`glutton::Wallet(gold: 5)`, `"glutton::Pair<u8,String>"(..)`) binds against the registry when the target type is not known in advance (`bind_tagged`), and is checked against the target otherwise. An unknown tag is `UnknownType` with up to 3 close names. This is how #111 stores a heterogeneous list of components: each element is a tagged group. A tag is a path (or a string when the stable name is not a path). An enum value is tagged `path::Variant(..)`, and `bind_tagged` resolves the longest registered type name that is a prefix of the path; `write_tagged` of an enum whose name is not a plain path is `UnsupportedName`.
- **Result.** `Bound { value: Box<dyn Reflect>, warnings }`. Structure is `DynamicStruct`/`DynamicEnum`/... with `represented_type_info` set; `FromReflect` then builds the typed value. A `partial` mode fills nothing in, so `Reflect::apply` leaves absent fields untouched (the "restricted to what `b` contains" rule); `Complete` mode fills defaults and is what files use.
- **Registry use.** The caller passes `&TypeRegistry` and takes one read guard for the whole call (reflect spec, threading). The binder follows `TypeRef` function pointers for field types and uses the registry only for the root name, `ReflectTextForm`, `ReflectDefault` and `ReflectFromReflect`, with a small per-call `TypeId` cache so these cost one linear scan, not a hash lookup per leaf.
- **Limits.** `Limits { max_bytes: 64 MiB, max_nodes: 8 Mi, max_depth: 128 }` by default. Exceeding one is an error, never a crash. 128 levels of recursive descent fit comfortably in the 1 MiB wasm stack (the parser frame is under 200 bytes, the binder frame under 500). Callers lower the limits for untrusted input (the MCP server uses 1 MiB) and raise them for a big world.

### Forward compatibility: missing, unknown and duplicate fields

The text format has no schema hash. It stays loadable across code changes by name, which gives these rules (the reflect spec's "additive changes migrate through `default` and unknown-field reporting"):

| Situation | Behaviour |
|---|---|
| **Field missing** in the text | In `Complete` mode: the field's `#[reflect(default)]`, else the field type's `ReflectDefault`, else error `MissingField` at the struct's span (the same order as the reflect spec's apply table, so an enum variant switch and a file agree). In `partial` mode: left absent. |
| **Field unknown** to the type | Controlled by `UnknownFields`: `Error` (an error with the valid field names and a did-you-mean suggestion, edit distance at most 2), `Warn` (skip the value, add a `UnknownField` warning with its span and path), `Keep` (store the value by name in the `DynamicStruct`, so a tool that rewrites the file does not lose it; `apply` ignores fields the target lacks). Defaults: **`Error`** for `from_text_into`, MCP and the editor's paste (an agent's typo must be an error it can fix), **`Warn`** for loading files (an older build reads a newer file). |
| **Field duplicated** | Always an error (`DuplicateField`, with the first span in `related`). |
| **Unknown variant** | Always an error (a value cannot be skipped): `UnknownVariant` with the valid variants and a suggestion. |
| **Unknown type tag** | Always an error (`UnknownType`), with suggestions. |
| **Type changed shape** | An error at the offending node (`expected X, found Y`). Renames and reshapes need a migration (deferred); until then they fail loudly with a span. |
| **Field order** | Irrelevant on read; the writer normalizes it. |

Kept unknown values are untyped: integer literals become `i128` (or `u128` above `i128::MAX`), floats `f64`, groups `DynamicStruct`/`DynamicTupleStruct` keyed by field name or position, `[..]` a `DynamicList`, `{..}` a `DynamicMap`, a bare identifier a unit `DynamicEnum`, `@x` the string `"@x"`. A type tag on a kept value is dropped with a warning. The writer prints Dynamic values with no `TypeInfo` from their kind alone, so unknown data round-trips.

### Versioning marker

The first thing in a document is the header `%exitium-text <n> [root <type>]`. `n` is `TEXT_FORMAT_VERSION`, currently `1`.

- A document with no header is `MissingHeader` (with the hint "start the file with `%exitium-text 1`"). `n = 0` is invalid. `n` greater than the reader's supported version is `FormatNewer { file, supported }` and nothing is bound. `n` lower than current is read by the rules of version `n` (only version 1 exists).
- `root <type>` is the stable name of the root value (a path, or a string for names like `Vec<f32>`). The writer always writes it, so a file says what it holds. A reader that is given an expected type checks it (`file holds glutton::Wallet, expected glutton::Mode`); `from_text` uses it to pick the type when the caller has none.
- The header is the only place the format version lives, and it is only a **grammar** version. Any change to the grammar, the leaf spellings or the reference rules in this spec after v1 ships bumps it; layout and ordering changes of the writer do not (old readers accept any layout) but they change golden files and are reviewed as format changes (high-risk area: save and serialization formats).
- Schema versions of individual types (`#[reflect(version = N)]`) are not written in v1 (no migrations in the MVP). The header grammar is closed in v1 (`%exitium-text`, `root`); a later version may add directives (for example `%types` rows with per-type schema versions). A v1 reader rejects a newer file with `FormatNewer` before it looks at any directive, and a newer reader reads v1 files unchanged.

### Errors

Every error names a place and says expected versus found, in words an agent can act on. One struct serves the editor, the CLI and MCP.

```rust
pub struct Span { pub start: u32, pub end: u32 }            // byte offsets, half-open

pub struct TextError {
    pub kind: ErrorKind,
    pub span: Span,
    pub start: LineCol, pub end: LineCol,                  // line 1-based, column 1-based in Unicode scalar values (tab = 1)
    pub path: String,                                       // reflect path syntax to the value ("components[2].gold"); "" for syntax errors
    pub expected: String,                                   // "an integer 0..=255 (u8)"
    pub found: String,                                      // `string "ten"`
    pub hint: Option<String>,                               // "write 1000", "did you mean `gold`?"
    pub related: Option<Span>,                              // the first of two duplicates
}
pub enum ErrorKind {
    // syntax (from the lexer and parser): the first one stops parsing
    UnexpectedChar, UnexpectedToken, UnexpectedEof, UnterminatedString, UnterminatedComment, BadEscape,
    BadNumber, MissingHeader, BadHeader, FormatNewer { file: u32, supported: u32 }, TooLarge, DepthExceeded,
    // binding (and `UnsupportedName`, from the writer): collected, up to 16 per call
    TypeMismatch, OutOfRange, InvalidValue, TextForm, UnknownField, MissingField, DuplicateField, DuplicateKey,
    UnknownVariant, UnknownType, WrongArity, Reference, UnsupportedName,
}
pub struct TextErrors { pub errors: Vec<TextError>, pub truncated: bool }   // never empty
pub struct TextWarning { pub kind: WarningKind, pub span: Span, pub path: String, pub message: String }
pub enum WarningKind { UnknownField, ValueNormalized, DroppedTag }
```

Requirements:

- Syntax errors stop at the first one (the parser does not recover). **Binding errors are independent**, so the binder continues past them and reports up to 16, which lets an agent fix a whole file in one round trip. `truncated` says more existed.
- Line and column come from a `LineIndex` built lazily (binary search over line starts); the hot path stores only the byte span. Columns count Unicode scalar values after normalizing `\r\n`; a BOM does not count.
- `Display` is one line: `scene.txt:12:11: expected an integer 0..=255 (u8) for `health`, found string "ten"`. `TextError::render(source, name)` returns the source line with a caret under the span, as `rustc` does. `TextError` serializes to the JSON shape `{kind, line, column, end_line, end_column, path, expected, found, hint, message}` in `exitium_mcp`.
- The path uses the reflect path grammar (`items[3].name`, `stats["hp"]`), so an error path can be handed to `SetField`.
- A hook failure is `Reference` with the hook's message and the node's span.
- No error path may panic. Parsing arbitrary bytes, including every truncation of a valid document, returns `Ok` or `Err` (tested).

### References: entities and GUIDs

A saved file refers to other things in two ways, and they need different treatment.

**Entity references** (`Parent(Entity)`, `Children(Vec<Entity>)`, `Attack { target: Entity }`, an `Entity` map key). An `Entity` is `{ index, generation }`, meaningful only inside the world that allocated it. A persisted file must not contain raw ids: the entities a scene spawns get fresh ids, and every reference must be remapped to them.

**GUID references** (`AssetRef<T>`, a transparent `Guid` per reflect open question 13; any `Guid` field). A `Guid` names the same thing across runs, files and machines, so it needs no remapping.

The mechanism, and why it is in this crate:

1. **`Entity` and `Guid` are text-form types.** In text a reference is a string literal holding the type's canonical text, or the `@label` token. The writer and binder find them through `ReflectTextForm`, so `exitium_serial` never names `Entity` and does not depend on `exitium_ecs`. The remap lives in a hook the caller installs. Rejected: a `MapEntities` trait in ECS and a serial-to-ecs dependency (it would force the format below the ECS in the crate graph and make UI markup depend on the world).
2. **`@label` is the written form of a local reference.** `@boss` is lexical sugar for the string `"@boss"`: the lexer produces a `Ref` node, and the binder treats it exactly like a string literal in a text-form position (a `String` field rejects it: `expected a string, found a reference`). The writer emits `@name` instead of `"@name"` when the text a hook returns matches the `ref` token rules. This costs nothing in the grammar, makes references unmistakable to a human and an agent, and lets the scene crate find every reference and label in a file from the syntax tree **without a schema** (`Syntax::refs()`).
3. **`TextRefs` is the hook.** One trait covers both directions:

```rust
pub trait TextRefs {
    /// Writing. `canonical` is `ReflectTextForm::to_text()`. Return how to spell the value.
    fn write_ref(&mut self, site: &RefSite<'_>, canonical: &str) -> Result<WrittenRef, RefError> {
        Ok(WrittenRef::canonical())
    }
    /// Reading. `written` is the string-literal content, or the whole `@label` token including the `@`.
    /// Return the canonical text to hand to `from_text`, or `None` to use `written` unchanged.
    fn read_ref(&mut self, site: &RefSite<'_>, written: &str) -> Result<Option<String>, RefError> {
        Ok(None)
    }
}
pub struct RefSite<'a> {
    pub type_name: &'static str,         // the text-form type: "Entity", "Guid"
    pub owner: Option<&'static str>,     // stable name of the struct or enum that declares the field
    pub field: Option<&'a str>,          // the field name (None for a list item or a map key)
    pub is_map_key: bool,
}
pub struct WrittenRef { pub text: Option<String>, pub comment: Option<String> }   // text None = canonical
pub struct RefError { pub message: String }
```

   With no hook installed, a text-form value is read and written canonically (`"42v1"`, the UUID) and `@label` is an error (`reference @boss needs a reference resolver`). That is the right behaviour for MCP and debug dumps in a live session, where raw ids are valid. `RefSite` carries the owner and field so a policy can depend on the field (`AssetRef` fields are `Guid`, but a hook may annotate only the field `image`) without the serializer knowing asset types.

**How the scene crate (#111) uses it** (the contract, not the scene spec):

- **Write.** The scene writer assigns each entity a label (`@boss`, `@e3`) and installs a `TextRefs` whose `write_ref` maps an `Entity`'s canonical text to its label. An entity that is not in the file is an error (`RefError`), not a raw id, so a saved scene never contains `"42v1"`. For an `AssetRef`/`Guid` field the hook returns `WrittenRef { text: None, comment: Some(path) }`, producing `icon: "67e5..", // sprites/coin.png`. The UUID is the data; the comment is for humans.
- **Read.** The scene loader parses the syntax tree first, collects the labels with `Syntax::refs()`, **reserves** one entity per label (ECS `reserve_entity`, so forward references work and no entity needs to exist when the reference is parsed), installs a `TextRefs` whose `read_ref` maps `@boss` to the reserved entity's canonical text, and only then binds each entity's components. `Parent` and `Children` are ordinary `Entity` fields: they need no special case.
- **Author-time convenience.** A `read_ref` for a `Guid` field may accept a path (`"sprites/coin.png"`) and return the UUID. The next write normalizes the field to the UUID. Whether this is allowed is the assets spec's decision (#111); the hook point is here.
- **Where labels are declared** (an entity header, a map key such as `{ @boss: (..) }`, or a field of a scene-crate text-form `Label` type) is the scene spec's decision. The grammar already allows `@label` as a value, a field value and a map key.

**`RawHandle`** is not a reference in text: reflect persists it only through a remap, so a component with a `RawHandle` field marks it `skip`. The writer does not special-case it.

The binary format (deferred) uses the same hook contract (entity remap on load), so a scene can later ship in binary without changing the scene crate.

### Public API sketch

```rust
// exitium_serial::text
pub const TEXT_FORMAT_VERSION: u32 = 1;

// ---- writing ----
pub struct WriteOptions<'a> { pub refs: Option<&'a mut dyn TextRefs> }          // Default: no hook
pub fn to_text(value: &dyn Reflect, registry: &TypeRegistry) -> Result<String, TextError>;          // a fragment
pub fn to_document(value: &dyn Reflect, registry: &TypeRegistry) -> Result<String, TextError>;      // header + root
pub fn write_value(value: &dyn Reflect, registry: &TypeRegistry, opts: WriteOptions<'_>, out: &mut dyn fmt::Write) -> Result<(), TextError>;
pub fn write_document(value: &dyn Reflect, registry: &TypeRegistry, opts: WriteOptions<'_>, out: &mut dyn fmt::Write) -> Result<(), TextError>;
pub fn write_tagged(value: &dyn Reflect, registry: &TypeRegistry, opts: WriteOptions<'_>, out: &mut dyn fmt::Write) -> Result<(), TextError>;  // "glutton::Wallet(..)"

// ---- syntax tree (no registry) ----
pub enum Mode { Document, Fragment }
pub struct Syntax<'src> { /* source, Vec<Node>, header, root */ }
pub fn parse_syntax<'s>(source: &'s str, mode: Mode, limits: &Limits) -> Result<Syntax<'s>, TextError>;
impl<'s> Syntax<'s> {
    pub fn header(&self) -> Option<&Header<'s>>;      // version, root type_ref
    pub fn root(&self) -> NodeId;
    pub fn node(&self, id: NodeId) -> &Node;          // kind, span
    pub fn children(&self, id: NodeId) -> Children<'_>;
    pub fn text(&self, id: NodeId) -> &'s str;        // the source slice
    pub fn tag(&self, id: NodeId) -> Option<&'s str>; // type name of a tagged/named group
    pub fn refs(&self) -> impl Iterator<Item = (NodeId, &'s str)> + '_;   // every @label, in order
}
pub struct Node { pub kind: NodeKind, pub span: Span /* + private child links */ }     // 24 bytes
pub enum NodeKind { Int, Float, Str, Char, Bool, Ref, Named, Tagged, Group, Field, List, Map, Entry }

// ---- binding (needs the registry) ----
pub enum UnknownFields { Error, Warn, Keep }
pub struct ReadOptions<'a> {
    pub unknown_fields: UnknownFields,      // default Error
    pub partial: bool,                      // default false (Complete)
    pub refs: Option<&'a mut dyn TextRefs>,
    pub limits: Limits,
}
pub struct Bound { pub value: Box<dyn Reflect>, pub warnings: Vec<TextWarning> }
pub fn bind(syntax: &Syntax<'_>, node: NodeId, ty: &'static TypeInfo, registry: &TypeRegistry, opts: &mut ReadOptions<'_>) -> Result<Bound, TextErrors>;
pub fn bind_tagged(syntax: &Syntax<'_>, node: NodeId, registry: &TypeRegistry, opts: &mut ReadOptions<'_>) -> Result<Bound, TextErrors>;

// ---- one-call conveniences ----
pub fn from_text(source: &str, registry: &TypeRegistry, opts: &mut ReadOptions<'_>) -> Result<Bound, TextErrors>;            // a document; type from its header
pub fn from_text_as(source: &str, mode: Mode, ty: TypeId, registry: &TypeRegistry, opts: &mut ReadOptions<'_>) -> Result<Bound, TextErrors>;
pub fn from_text_into<T: Reflectable>(source: &str, mode: Mode, registry: &TypeRegistry, opts: &mut ReadOptions<'_>) -> Result<(T, Vec<TextWarning>), TextErrors>;
pub fn apply_text(target: &mut dyn Reflect, source: &str, registry: &TypeRegistry, opts: &mut ReadOptions<'_>) -> Result<Vec<TextWarning>, TextErrors>;  // partial apply

// ---- shared leaf helpers (also used by exitium_mcp's JSON mapping) ----
pub mod leaf { pub fn write_float_f64(v: f64, out: &mut dyn fmt::Write); pub fn write_float_f32(..); pub fn write_string(s: &str, out: ..);
               pub fn parse_int(text: &str, repr: ValueRepr) -> Result<Leaf, LeafError>; pub fn parse_float(..); pub fn map_key_order(a: &dyn Reflect, b: &dyn Reflect) -> Ordering; }
// `Leaf` is the owned form of `ValueRef`: the same 16 variants, with `String(String)`.
```

`from_text_into` is `bind` then `T::from_reflect`; `apply_text` is `bind` in partial mode then `Reflect::apply` (snapshot and rollback are the command bus's job, as for any apply). The exact names inside `leaf` are the implementer's; what this spec requires is that the writer, the binder and `exitium_mcp` call the same spelling and key-order functions, so the three can never disagree.

### Threading model

- Everything is a pure function of its arguments. `Syntax`, `Bound`, `TextError` and the options other than the hook are `Send + Sync`; `TextRefs` is used through `&mut dyn`, held by one call on one thread, and needs no `Send` bound.
- No global state, no locks, no allocation caches shared between calls. The registry guard is the caller's (one `RwLock` read guard per call; helpers take `&TypeRegistry`, never a second guard).
- Independent documents may be parsed on different threads. A single document is parsed on one thread; wasm32 is single-threaded and the MVP executor is single-threaded, so there is no parallel parse. The design leaves one door open: a caller can bind one top-level item at a time and drop it (the binder works on a subtree), and the writer streams to a `fmt::Write` and needs memory proportional to depth, so a 1M-entity file does not have to exist in memory twice. Parsing is synchronous CPU work: on the web the 10k-entity budget below keeps a scene load inside one frame budget of a loading screen, and a caller that needs more yields between top-level items.

### Memory layout

`Node` is 24 bytes: a 1-byte kind, a `Span` (two `u32`), and two `u32` links (first child, next sibling) in a flat `Vec<Node>`. The tree borrows the source `&str` (no copy of strings; escapes are decoded at bind time), so it is about 3 times the source size for typical text (one node per roughly 8 bytes) and at worst 12 times (`[1,1,1,..]`), bounded by `max_nodes`. Byte offsets are `u32`, so a document over `u32::MAX - 1` bytes is `TooLarge` before parsing, regardless of `max_bytes`.

### Worked example

Types (the first line of each is the reflect derive; attributes as in the reflect spec):

```rust
#[derive(Component, Reflect, Clone)]
#[reflect(Component, type_name = "glutton::Wallet")]
struct Wallet {
    gold: Num, owner: Option<Entity>, tags: Vec<String>,
    income: HashMap<ResourceKind, Num>,        // unit-only enum key
    icon: AssetRef<Image>,                     // transparent Guid
    pos: Vec2, mode: Mode,
}
#[derive(Reflect, Clone)] enum Mode { Idle, Move(f32, f32), Attack { target: Entity, power: u32 } }
```

A document the writer produces (with a scene-style `TextRefs` that labels entities and annotates the asset):

```
%exitium-text 1 root glutton::Wallet
(
  gold: 1.5e300,
  owner: Some(@boss),
  tags: ["a", "b\n"],
  income: {Gold: 12.5, Wood: 3.0},
  icon: "67e55044-10b1-426f-9247-bb680e5fe0c8", // sprites/coin.png
  pos: (x: 12.5, y: -3.25),
  mode: Attack(target: @player, power: 7),
)
```

Things the reader also accepts for the same value: fields in another order, a trailing comma or none, `gold: 15e299`, comments anywhere, the type name as a prefix (`glutton::Wallet( ... )`), `CRLF` line endings, `power: 0x7`, and (as written) `mode: glutton::Mode::Attack(...)`. Things it rejects, each with a span: `gold: nan` (`value nan is not valid for type Num`), `gold: "5"` (expected a number, found a string), `power: -1` (`expected 0..=4294967295 (u32)`), `tags: ["a" "b"]` (expected `,` or `]`), `mode: Teleport` (unknown variant `Teleport`; valid: Idle, Move, Attack), `income: {Gold: 1, Gold: 2}` (`DuplicateKey`), `owner: @boss` with no hook (`reference @boss needs a reference resolver`) and `owner: 5` (expected `Some(..)` or `None`).

### JSON mapping (contract for `exitium_mcp`)

MCP carries values as JSON, with schemas generated from `TypeInfo` by `exitium_mcp` (reflect spec). These rules are normative so JSON and text mean the same thing; `exitium_mcp` implements them by walking reflection and uses the `leaf` helpers for spellings, and `exitium_serial` itself takes no JSON dependency.

| Reflected | JSON |
|---|---|
| struct | object, keys in declaration order |
| tuple struct, tuple | array (a `transparent` wrapper is its inner value) |
| enum, unit variant | string `"Variant"` |
| enum, tuple / struct variant | `{"Variant": [..]}` / `{"Variant": {..}}` (externally tagged) |
| `Option<T>` | `null` for `None`, the value for `Some`; when `T` is itself an `Option` or `()`, `{"Some": v}` (so nesting is unambiguous) |
| list, array | array |
| map | object; key spelling as in text (a unit-only enum key is its variant name, a text-form key its text, an int key decimal) |
| `bool`, `String` | boolean, string; `char`: a one-character string; `()`: `null` |
| ints up to 32 bits | number |
| `i64`, `u64`, `i128`, `u128` | number when `\|v\| <= 2^53`, otherwise a decimal string; a reader accepts either spelling |
| `f32`, `f64`, `Num` | number; `nan`, `inf`, `-inf` of a plain float as the strings `"nan"`, `"inf"`, `"-inf"` |
| text-form type (`Guid`, `Entity`) | string with the canonical text; never a number or object |

Unknown fields and missing fields follow the same `UnknownFields` and default rules. JSON has no `@label`: a live MCP session uses raw `"42v1"` entity strings, which is the no-hook behaviour.

## MCP / editor surface

This crate adds **no editor feature of its own and no command**: mutation always goes through the command bus (design rule 3), and a value in text enters a command the same way a value in JSON does (`SetField`, `InsertComponent`; owned by the ECS and command specs). What the format adds is a way to move values as text, and parity requires the same for the agent and the human.

| Capability | MCP tool (in `exitium_mcp`) | Editor | Built from |
|---|---|---|---|
| Validate text against a type and get the value back as JSON, with errors and warnings (spans, expected and found) | `serial_parse_text { text, type_name, unknown_fields? }` | text pane and paste show errors with line, column and a squiggle | `from_text_as` |
| Write a value as canonical text (a fragment or a document) | `serial_write_text { type_name, value, document? }` | "Copy as text" on any component or field | `to_text`, `to_document` |

Both tools are read-only. They are specified here and implemented in an `exitium_mcp` issue owned by the MCP and command spec (the contract is the API above and the error JSON in "Errors"); they are not filed from this spec because they edit another crate. The editor's text view of a component and "paste from text" use the same functions with `UnknownFields::Error` and, for pasting part of a struct, `partial` plus `apply_text`. An agent that edits files directly with its own file tools gets the same checks by calling `serial_parse_text` on the new text, and canonical formatting by round-tripping it (comments are lost, open question 1). The MCP error payload is the `TextError` JSON above, so an agent can place a fix at a line and column.

## Performance

Budgets are p50, single thread, on the min-spec reference runner, measured with criterion in `crates/exitium_serial/benches/text.rs`, IDs `exitium_serial/text/<case>/<size>` at 1k and 100k rows (docs/benchmarks.md), held by the 5% gate once baselined. They are initial targets; the first benchmark PR may re-baseline with a note to the owner. The reference "row" is an entity-like struct of about 90 bytes and 10 leaves: ints, `f32` pairs, a short string, an `@label`, a small enum list.

| Operation | Budget |
|---|---|
| Lex and build the syntax tree (`parse_syntax`) | >= 250 MB/s; 100k rows (9 MB) <= 40 ms |
| Bind to `Dynamic*` values (`bind`), including `Box` per leaf | >= 60 MB/s (about 140 ns per leaf); 100k rows <= 150 ms |
| Typed read, `from_text_into` (bind plus `from_reflect`) | >= 40 MB/s |
| Write (`write_value`), ints, strings, enums | >= 100 MB/s; 100k rows <= 90 ms |
| Write one float (shortest form) | <= 120 ns |
| Write one integer | <= 25 ns |
| Escape a string, ASCII | >= 500 MB/s |
| Parse one number literal (lex plus convert) | <= 40 ns |
| Sort a 10-entry map by canonical key order | <= 1 µs |
| Build the line index for a 1 MB source | <= 1 ms; one error line/column lookup <= 1 µs |
| **A 10k-entity scene (about 3 MB, about 500k nodes): read and bind** | **<= 100 ms**; write <= 60 ms |
| Syntax tree memory | <= 24 B per node; <= 3x source typical |
| Writer memory | O(depth); no `String` per value |
| wasm32 code size added by the crate (release, before `wasm-opt`) | <= 120 KiB (initial target; recorded by the first benchmark PR) |

Complexity: lex and parse O(n) in source bytes; bind O(nodes) with O(f) field-by-name scans over a struct's fields (reflect spec: a scan of 16 short names beats hashing); write O(leaves) plus O(k log k) per map for the canonical sort.

**1M-entity round trip.** The Jam MVP cuts the 1M-entity benches (the contract), so no such test is in the MVP issues. The design keeps it open: with 1M entities x 5 components (about 450 MB of text) the targets are write <= 5 s and read <= 8 s with peak memory under 2 GB, which needs the per-item streaming described in "Threading model", not a single tree over the whole file. The first issue that adds 1M-entity benches also adds that test and these numbers. At 100k rows the round trip (write, parse, bind, compare through reflection) must stay under 2 s in CI.

## Test plan

- **Grammar.** One test per production, accepted and rejected; every escape; nested block comments; CRLF and BOM; `r#` identifiers; the reserved words as variant names; `@label` characters; trailing comma on and off; an empty `()`, `[]`, `{}`.
- **Numbers.** Integers at every width's min and max (including `i128`/`u128` extremes, hex, binary, `_`), out-of-range at each, a float for an int, an int for a float, `16777217` into `f32`. Floats: a vector table pinning the shortest spelling and the plain/scientific cut (`1e-5`, `9.999e-6`, `1e15`, `1e16`, `0.1f32`, `5e-324`, `f64::MAX`, `-0.0`), `nan`/`inf` for plain floats. **`Num`**: `Num::MAX` and `MIN`, `1.5e300`, `5e-324`, `1e999` saturates with a warning, `-inf` saturates, `nan` is an error, `-0.0` reads as `0.0` with a warning and a `Num` never holds `-0.0` or NaN afterwards (assert with `to_bits`), `Num == Num` and hashing stay consistent. `0` into `NonZeroU32`, a `RawHandle.generation` and an `Entity.generation` is rejected.
- **Strings.** All escapes both ways, raw UTF-8, bidirectional controls written as `\u{..}`, lone surrogate escape rejected, raw newline rejected, an emoji and a combining mark round-trip byte-exact.
- **Containers and kinds.** Every row of the mapping table, including `Option<Option<T>>`, a fixed array of the wrong length, tuples of wrong arity, transparent newtypes (`Gold(3.5)` is `3.5`), a recursive type through `Box`/`Vec`, maps with every key kind (int, `String`, `bool`, `char`, unit-only enum, `Guid`, `Entity`), and the canonical key order for each (including `"10v1"` before `"9v1"`). `HashMap` and `BTreeMap` give identical text.
- **Forward compatibility.** Missing field with `default`, with `ReflectDefault`, and with neither (error). Unknown field under `Error` (valid names and a suggestion), `Warn`, `Keep` (kept value round-trips through the writer). Duplicate field and key. Unknown variant and tag. `partial` mode leaves absent fields untouched under `apply_text`. A struct written by an "older" type and read by a "newer" one that gained a defaulted field, and the reverse.
- **Header and version.** Missing header, `0`, `2` (`FormatNewer`), a `root` mismatch, a fragment containing a header, a header after a value.
- **Errors.** Golden messages for each `ErrorKind`: line and column (with CRLF, tabs, a BOM and a multi-byte character before the error), the span, `path` in reflect path syntax, `expected`/`found`, the hint, and `related`. Up to 16 binding errors are collected, `truncated` is set beyond that, and a syntax error stops at one.
- **Limits and robustness.** `[[[[...` 100000 deep returns `DepthExceeded` without overflowing the stack (also in the wasm build); `max_nodes` and `max_bytes`. **No panic on any input**: every truncation and a bounded sweep of single-byte mutations of each golden file, plus seeded random bytes and random token soup. The generator is a small std-only xorshift with a fixed seed (no new dependency; a failure prints the seed and the shrunken input).
- **References.** With a fake text-form type standing in for `Entity` (`exitium_serial` cannot name the real one): no hook (canonical text), a hook that maps to `@label` and back, forward references through a pre-scan with `Syntax::refs()`, a hook error surfacing as a `Reference` error with the right span, `@label` in a `String` field rejected, an annotated `Guid` writes the comment and reads back equal, map keys and list items as references, `RefSite` contents (owner, field, `is_map_key`). A test in `exitium_scene` (#111) covers the real `Entity`.
- **Round trip.** `bind(parse(write(x))) == x` through `reflect_eq` over seeded random reflected values (struct, enum and container mixes, extreme `Num`, unicode strings, empty containers, recursive types) and over every golden file; and `write(read(write(x))) == write(x)` (canonical form is a fixed point).
- **Golden files** under `crates/exitium_serial/tests/golden/`, one per documented construct, with the expected output checked in. The test normalizes `\r\n` to `\n` in the expected file before comparing, so a Windows checkout with `core.autocrlf` still passes. A failing golden prints a readable line diff.
- **Determinism and layout.** The layout rules above (flat/broken thresholds, the 100-column cap, trailing commas, `Some((..))`, annotations forcing a break), one layout test per rule. A test that changing one number's digit count never changes the layout of its siblings.
- **Platform.** `cargo build --workspace --target wasm32-unknown-unknown` passes; no `unsafe`; the golden tests produce the same bytes on Windows and Linux CI.
- **Benchmarks.** One per row of the performance table.

## Dependencies

**No new crates.io dependencies.** `exitium_serial` uses std only. The earlier plan for #55 (`proptest` as a dev-dependency, with `gate:human`) is dropped: the randomized tests use a seeded std PRNG, so `Cargo.lock` does not change for this spec. (Shrinking is lost; the failing seed and input are printed instead.) Compression crates (`zstd`, `lz4`) belong to the deferred binary format (#58).

**Crate edge for the intake PR.** `exitium_serial -> exitium_reflect` (path dependency, added to `crates/exitium_serial/Cargo.toml`). `exitium_core` is not needed (the crate reaches `Guid` and `Num` only through reflection). The edge is needed by the writer and binder issues, not by the lexer and syntax tree issues, which use std alone. `crates/exitium_serial/CLAUDE.md` currently lists `exitium_core`; the first implementation issue corrects it.

## Open questions

1. **Comment and layout preservation.** The writer is canonical, so an editor save drops a human's comments. Recommendation: defer. The syntax tree keeps spans, so a later "patch" mode can replace one value's byte range and leave the rest of the file alone, which is the right fix for scenes a human annotated. Until then, `//` comments are for hand-written files nobody round-trips, and hook annotations are regenerated.
2. **Omitting default-valued fields** to shrink diffs. Recommendation: no in v1. A saved value that equals today's default would change meaning when the code default changes. Revisit with an explicit `#[reflect(skip_if_default)]`.
3. **Multi-line and raw string literals.** Raw newlines in strings are errors so files survive line-ending conversion. Recommendation: keep, and add a raw string `r#".."#` (still no raw newlines) only when a real asset needs it.
4. **Packed layout for bulk numeric arrays** (`Vec<f32>` of hundreds of items). One item per line is diff-friendly and long. Recommendation: keep one per line until tilemaps return, then add a fixed-width row layout (for example 16 per row) for lists of numbers longer than 64, a writer-only change.
5. **Compact text forms for math and colour types** (`Vec2` as `(1.0, 2.0)`, `Color` as `"#ff8800"`), through `ReflectTextForm`. They would shorten scenes a lot but change how core types read. Recommendation: not in v1; structural spellings are unambiguous and agent-friendly, and a text form can be added later as an accepted-on-read alias first.
6. **`@label` as lexical sugar** for a quoted string. Recommendation: yes (this spec). The alternative is plain strings only, which loses the schema-free label scan and the visual cue.
7. **Default `UnknownFields` for file loads is `Warn`** (forward compatibility) while MCP and paste use `Error`. The risk is a typo in a hand-edited file loading with a warning. Recommendation: keep, and make the editor show load warnings in its log panel.
8. **Non-ASCII type, field and variant names.** The format is ASCII-only in v1 and the writer errors on others. Recommendation: have the derive and registry reject non-ASCII names in a reflect amendment (the PR for #34 or a small follow-up), so the error appears at compile time.
9. **Integer and float precision on the JSON wire.** Beyond 2^53 a 64-bit integer is a string. Recommendation: keep; the MCP spec may refine.
10. **File extensions.** The serializer reads text, not paths. Recommendation: the scene and UI specs choose their own (`.scene`, `.ui`) and share the header's `root` field to say what a file holds.

## Implementation issues

All issues edit exactly one crate, `exitium_serial`, so they run one at a time in the order shown (one crate has at most one issue in flight). Each is about 600 changed lines or fewer, updates the "Public API", "Invariants" and "Benchmarks" sections of `crates/exitium_serial/CLAUDE.md` for what it adds, adds its benchmark rows (design rule 4), and has `Blocked by` lines. To be filed into the Jam MVP milestone with `file-issue` after this spec merges (label `area:serial`, role `impl-engine-core`); the reflect issues are #31 (model, registry, primitives), #32 (dynamic access, `Dynamic*`, `apply`), #33 (containers), #34 and #35 (derive), #37 (core impls and `ReflectTextForm` for `Guid`).

Every issue has the same Footprint block (the edge to `exitium_reflect` is wired by the Stage 1 intake PR, see "Dependencies"):

```
## Footprint
- Edits: crates/exitium_serial/
- Governs: none
- Hotspots: none
- New crate edges: none
```

**S1. `feat(serial): text spans, errors and lexer`** (new)
- Footprint: the block above.
- Adds `text/{mod,span,error,lexer}.rs`: `Span`, `LineIndex`, `LineCol`, `TextError`, `ErrorKind`, `TextWarning`, `render`, `Display`; the lexer for every token in "Lexical rules" (comments, nesting, escapes, numbers incl. `_`, hex, exponent, `@label`, raw identifiers, keywords, header token) with `Limits`; corrects the crate CLAUDE.md.
- Acceptance: every lexical rule has an accept and a reject test; error columns count scalar values (CRLF, tab, BOM, multi-byte cases); `render` golden test; no panic over all truncations of the test inputs; no dependency on `exitium_reflect`; wasm32 build passes.
- Blocked by: none (Stage 1, std only).

**S2. `feat(serial): text syntax tree and parser`** (new)
- Footprint: the block above.
- Adds `text/{syntax,parser}.rs`: `Syntax`, `Node` (24 bytes), `NodeKind`, `parse_syntax` for `document` and `fragment`, the header and version marker (`MissingHeader`, `BadHeader`, `FormatNewer`), tagged values, the depth and node limits, `Syntax::refs()`, `tag()`, `children()`, `text()`.
- Acceptance: one test per EBNF production; the one-token-lookahead rules (`field` vs `item`, `tagged` vs string) tested; `DepthExceeded` at 129 levels and no stack overflow at 100000; truncation sweep never panics; parse benchmark `exitium_serial/text/parse_syntax/{1000,100000}` meets the budget.
- Blocked by: S1.

**S3. `feat(serial): text leaf codecs (numbers, strings, escapes)`** (new)
- Footprint: the block above.
- Adds `text/leaf.rs`: shortest float writing with the plain/scientific cut for `f32` and `f64`, integer writing, string and char escaping (control and bidi characters), `parse_int` for every width with range errors, `parse_float` into the target width, canonical map key order. Public for `exitium_mcp` (the "shared leaf helpers").
- Acceptance: the float vector table and integer extremes from the test plan; `f32` formatted as `f32`; escapes round trip over all `char` boundaries and a seeded random string sample; leaf benchmarks `exitium_serial/text/leaf/*` meet the budget; std only.
- Blocked by: S1. (Does not need reflect: pure functions over `&str` and primitives.)

**S4. `feat(serial): text format writer from reflection`** (existing #53, rescoped)
- Footprint: the block above.
- `to_text`, `to_document`, `write_value`, `write_document`, `write_tagged`: kind walk, the transparent and text-form rules, the layout algorithm (flat/broken, thresholds, 100-column cap, trailing commas, `Some((..))`), canonical key order, the header with `root`, the depth limit, `UnsupportedName`. Dynamic values with no `TypeInfo` are written from their kind. No `TextRefs` yet (canonical text forms only).
- Acceptance: the layout tests and golden files for every row of the mapping table; deterministic output across runs; `HashMap` and `BTreeMap` give identical text; hidden fields written, `skip` fields absent; writer benchmarks `exitium_serial/text/write/{1000,100000}` meet the budget. Edit #53: drop `gate:human` and `needs-spec`, replace "Blocked by #23, #34" with S3, #31, #32, #33, #37, add the Footprint, change "`BTreeMap`-style sorted keys" to "always sorted by canonical key order".
- Blocked by: S3, #31, #32, #33, #37 (derive #34/#35 only for test types; hand-written types may be used until they land).

**S5. `feat(serial): bind text to reflected types`** (existing #54, rescoped: the parser half moves to S2)
- Footprint: the block above.
- Adds `text/bind.rs`: `bind`, `from_text`, `from_text_as`, `from_text_into`, `Bound`, `ReadOptions`, `Limits`; leaves, validated Values through `ReflectFromReflect` (`InvalidValue`, `ValueNormalized`), text forms, lists, arrays, maps with duplicate-key errors, `Option`, tuples, transparent types; the binding-error collection (up to 16) with paths and `expected`/`found`. Structs and enums here handle fields by name with `Error` for anything unknown; the policies come in S6.
- Acceptance: every accept/reject example in the "Worked example" and "Numbers" test groups; `Num` outcomes (nan error, saturation and `-0.0` warnings); `NonZeroU32` zero rejected; 16-error cap and `truncated`; `bind(parse(write(x))) == x` over the S4 golden files; bind benchmarks `exitium_serial/text/bind/{1000,100000}` meet the budget. Edit #54: drop `gate:human`, replace its `from_text` and `from_text_into` signatures with the API above, replace "Blocked by #53" with S2 and S4, add the Footprint.
- Blocked by: S2, S4.

**S6. `feat(serial): text forward compatibility (defaults, unknown fields, tags, partial apply)`** (new)
- Footprint: the block above.
- Adds `UnknownFields::{Error, Warn, Keep}`, missing-field filling (`default`, then `ReflectDefault`, else `MissingField`), did-you-mean suggestions (edit distance at most 2, at most 3 names), `DuplicateField`, `UnknownVariant`, `UnknownType`, `bind_tagged`, the optional name prefix and `Enum::Variant` forms, kept untyped values and their writer path, `partial` mode, `apply_text`.
- Acceptance: the whole "Forward compatibility" test group, including old-to-new and new-to-old struct versions and a kept unknown field that round-trips through the writer.
- Blocked by: S5, #34, #35.

**S7. `feat(serial): text reference hook (TextRefs, @label)`** (new)
- Footprint: the block above.
- Adds `TextRefs`, `RefSite`, `WrittenRef`, `RefError` to the writer and binder; `@label` spelling on write when the hook's text matches the token rules; comment annotations and the forced break; `Reference` errors with spans; `Syntax::refs()` used by the tests for forward references.
- Acceptance: the whole "References" test group using a fake text-form type; no hook means canonical text and `@label` is an error; a hook error carries the span; `RefSite` fields verified.
- Blocked by: S4, S6.

**S8. `test(serial): golden files, seeded round-trip and robustness for the text format`** (existing #55, rescoped)
- Footprint: the block above.
- Adds `tests/golden/*` (one file per documented construct), the std-only seeded generator of random reflected values, the truncation and mutation sweeps, the 100k-row round trip under 2 s, and the end-to-end benchmark `exitium_serial/text/roundtrip/{1000,100000}`. Runtime under 30 s in CI. Edit #55: remove the `proptest` dev-dependency and `gate:human`, replace "Blocked by #54" with S6 and S7, add the Footprint.
- Blocked by: S6, S7.

**Issues in other crates, not filed from this spec** (listed so the owners of those specs pick them up):

- `exitium_mcp`: `serial_parse_text`, `serial_write_text` and the JSON mapping above, with the parity test entry. Owned by the MCP and command spec.
- `exitium_scene` (#111): the scene envelope, label assignment, the `TextRefs` implementations for `Entity` and asset `Guid`s, the pre-scan and entity reservation on load.
- `exitium_reflect` (small amendment): reject non-ASCII type, field and variant names (open question 8).
- Intake PRs: `exitium_serial -> exitium_reflect` edge; no `Cargo.lock` change for external crates.

### Downstream issue adjustments

Applied by the architect when this spec merges (the bullets are the contract):

- **#53, #54, #55:** as described under S4, S5 and S8. Remove the `needs-spec` label from all three. The `gate:human` criterion is obsolete (ADR-0005); the PR Risk section names the serialization format.
- **#61 (world snapshot):** not an `exitium_serial` issue. Its text half is covered by the scene spec (#111) through `TextRefs`; its binary half is deferred with #24. `exitium_serial` does not depend on `exitium_ecs`.
- **#24 (binary format):** deferred; when it returns it reuses the `TextRefs` contract for entity remap and follows the reflect spec's decision table for headers.
- **#111 (scene spec):** builds on "References", "Tagged values" and `Syntax::refs()`; decides label syntax placement, the asset path convenience and the scene envelope types.
