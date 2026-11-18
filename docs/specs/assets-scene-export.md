# Spec: Assets, scenes, audio and export (Windows exe and web)

- Issue: #111
- Crates: `exitium_assets`, `exitium_scene`, `exitium_audio`, `exitium_cli` (touchpoints: `exitium_serial`, `exitium_reflect`, `exitium_ecs`, the `exitium` facade, `exitium_render`, `exitium_ui`, and `exitium_editor` and `exitium_mcp` through #110)
- Status: draft. Owner-approved: the contract in `docs/specs/jam-mvp.md` (Rust only, WebGPU only on the web, kira for audio, the editor as a feature of the game binary, Windows and Chrome as the only tested targets). Everything else here is the author's proposal. The owner has decided one licensing point: a narrow `deny.toml` exception for kira's MPL-2.0 crate `triple_buffer`, recorded in an ADR in the intake 1 PR (see "Licence decision").

## Goal

Specify everything a game needs between "a file on disk" and "a running game that can be shipped": how assets are named, loaded and decoded on Windows and in the browser; how a scene is saved to and loaded from a text file through reflection; how sound and music play, including the browser's autoplay rule; and how `exitium new` and `exitium export` create a game project and package it as a zipped Windows exe or an itch.io-ready web build.

Four crates are involved. `exitium_assets`, `exitium_audio` and `exitium_scene` are runtime crates (they build for wasm32). `exitium_cli` is native Windows only (already in `NATIVE_ONLY`). By cross-spec ruling (a) and rule C1, none of the three runtime crates has an edge to another runtime crate. Loaded data reaches render, audio and UI through glue in the `exitium` facade, which this spec describes as a contract and files as facade issues.

## Non-goals

- Luau, binary scene format, prefab variants, data assets, asset hot reload, streaming audio, MP3 and other codecs, JPEG and WebP, texture compression, atlas packing, a pack file, a content hash cache. All are deferred (contract, or proposed in "Scope" below) and the design leaves their path open.
- `AssetRef<T>`. The MVP uses plain `Guid` fields (decision D1). The typed wrapper is a later, additive change.
- Building the MCP tools, editor panels and commands. #110 builds them. This spec lists the plain Rust functions they call and the behaviour they must expose ("MCP / editor surface"). It files no issue in `exitium_mcp` or `exitium_editor`.
- The default engine font, the facade re-export lists, `DefaultPlugins` entries and the sample game. They are facade work (render-v1.md open question 7, app-platform.md F3).
- Publishing to itch.io (`butler`), Steam, release workflows, macOS and Linux export, installers, code signing.
- Spatial audio, audio effects (reverb, filters), audio ducking, voice stealing. kira supports them and the API leaves room.

## Design

### Decisions at a glance

| Id | Decision | Why |
|---|---|---|
| D1 | An asset reference in a component is a plain `Guid`. No `AssetRef<T>` in the MVP. | Core is frozen and `AssetRef<T>` in `exitium_assets` would need an edge from render and ui (C1). Render's `Sprite.image` and the UI's image and font fields are already `Option<Guid>`. This settles reflect-model.md open question 13 and render-v1.md open question 1. |
| D2 | One manifest file, `assets/assets.manifest`, in the text format, maps `Guid` to path, kind and import settings. | The web cannot list a directory. One sorted text file diffs well and is the allow-list for loading and for export. |
| D3 | The loader decodes and **hands the payload to the consumer**; it keeps no copy. | Render keeps the GPU copy, audio keeps the frames. A retained CPU copy would double memory for every texture. |
| D4 | Native: two worker threads read and decode. Web: `fetch`, then decode on the main thread under a per-frame time budget. | The web has no threads in the MVP. Native decode must not stall a frame. |
| D5 | Formats: PNG; WAV and Ogg Vorbis; TTF and OTF; UTF-8 text. | Permissive licences, pure Rust, wasm32. Everything else is an open question. |
| D6 | A scene is a reflected text file: a list of entity records with `@label` ids and tagged components. Loading is atomic. | Uses the serial hooks exactly as designed (serial-text-format.md, "References"). Agents and humans both edit it. |
| D7 | `exitium_scene` has **no edge to `exitium_app`**. Plugins and resources are registered by the facade glue. | The scene API is functions over `&World` and `&TypeRegistry`. Dropping the edge lets the scene lane run beside the `exitium_app` spine (R3). This refines the roadmap's known-wiring line. |
| D8 | Audio is kira with default features off. No kira type is public. On the web the audio device is created at the first user gesture. | kira is the owner's choice. Hiding it keeps the engine's audio API independent of the library. Browsers keep an `AudioContext` created before a gesture suspended. |
| D9 | The CLI uses no `clap` and no TOML or JSON crate. The project file is text format read through `exitium_serial`'s syntax tree. Archives are written with the `zip` crate. | Two subcommands do not need a parser crate. One grammar for every authored file. `Compress-Archive` on Windows PowerShell 5.1 writes backslash paths that break on the web. |
| D10 | Web export needs `wasm-bindgen-cli` at exactly the locked `wasm-bindgen` version, **0.2.130**. No CI change is required. | The `wasm-bindgen` schema must match the library in the wasm file. |

### Crate map

```
exitium_assets/ (paths relative to src/ except examples/)       Stage 1 unless marked
  lib.rs
  path.rs        AssetPath, AssetKind, extension table
  error.rs       AssetError
  registry.rs    AssetInfo, ImageSettings, AssetRegistry, Guid minting
  image.rs       PNG decode, edge bleed, PNG encode
  audio.rs       WAV and Ogg Vorbis decode
  font.rs        font validation, UTF-8 text
  source.rs      AssetSource trait, FsSource, MemorySource           (native)
  server.rs      AssetServer, states, Loaded, Payload, limits        (native threads)
  web.rs         fetch source and budgeted decode                    (wasm32)
  native.rs      scan, copy-in, downscale, atomic write, trash       (not wasm32)
  manifest.rs    Manifest, read and write on the text format         Stage 2, feature `manifest`
  edit.rs        import, rename, remove, settings, undo records      Stage 2, feature `manifest`
  plugin/        AssetPlugin, AssetStatus, events, poll system       Stage 2, feature `plugin`
  examples/      assets_demo.rs, web/index.html, fixtures

exitium_audio/ (paths relative to src/ except examples/)        Stage 1 unless marked
  lib.rs   clip.rs (ClipData, ClipId)   engine.rs (AudioEngine, buses, voices, music, state)
  plugin/  AudioPlugin, ClipStore, AudioSettings, AudioStats, events    Stage 2, feature `plugin`
  examples/audio_demo.rs, examples/web/index.html

exitium_scene/ (src/)                                           Stage 2 only
  lib.rs   components.rs (Name, SceneMember, Scenes)   label.rs   write.rs   parse.rs   instantiate.rs   refs.rs (TextRefs)   error.rs

exitium_cli/ (src/, and template/ beside it)                     Stage 4, native only
  main.rs  args.rs  project.rs  new.rs  export/{mod,stage,zip,windows,web,tools}.rs  serve.rs      (template/ holds the embedded files)
```

### Assets

#### Identity

- **`Guid`** (from `exitium_core`) names an asset for good. It survives a rename, a move and a re-import. A saved scene stores the `Guid`, never the path.
- **`AssetPath`** is a validated `String`, relative to the asset root, `/`-separated. Rules: ASCII letters, digits, `_`, `-`, `.` and `/` only; no empty, `.` or `..` segment; no segment that starts with `.`; at most 200 bytes; no two paths in one manifest that differ only by case (Windows is case-insensitive and the web is not, so the exporter would otherwise ship a game that works only on one of them); the name `assets.manifest` is reserved. `AssetPath::sanitize(file_name)` returns a legal suggestion (`Hero Portrait (1).png` becomes `hero_portrait_1.png`), used by the import error message. The restriction keeps URLs, zip entries and Windows names safe without escaping.
- **`AssetKind`** is chosen by extension (case-insensitive): `png` is `Image`; `ttf`, `otf` are `Font`; `wav`, `ogg` are `Audio`; `scene` is `Scene`; `txt`, `ui` are `Text`. `Scene` and `Text` are both UTF-8 text payloads. They are separate kinds so tools can list scenes. A `.ui` file holds the text form of a `UiNodeDesc` (ui-mvp.md open question 8); the UI plugin, not this crate, parses it.
- **Minting.** `exitium_core` has no random source (its `guid.rs` says the asset database mints them). `AssetRegistry::mint(path)` returns a UUID-shaped value (version nibble 8, variant bits `10`) from FNV-1a 128 over `path`, a zero byte and a `u32` attempt counter, and increments the counter while the value is already in the registry. It is deterministic, needs no crate, and is pinned by golden vectors. The same path in two projects mints the same value, which is harmless: a `Guid` is only meaningful inside one project.

```rust
pub enum AssetKind { Image, Font, Audio, Scene, Text }
pub enum ImageFilter { Linear, Nearest }
pub struct ImageSettings { pub filter: ImageFilter, pub mips: bool, pub edge_bleed: bool }   // Linear, true, true; pixel art: Nearest, false, false
pub struct AssetInfo { pub guid: Guid, pub path: AssetPath, pub kind: AssetKind, pub image: Option<ImageSettings> }   // `image` is Some only for Image

pub struct AssetRegistry { /* BTreeMap<AssetPath, Guid>, HashMap<Guid, AssetInfo> */ }
impl AssetRegistry {
    pub fn get(&self, guid: Guid) -> Option<&AssetInfo>;
    pub fn guid_of(&self, path: &AssetPath) -> Option<Guid>;
    pub fn iter(&self) -> impl Iterator<Item = &AssetInfo>;                       // sorted by path
    pub fn mint(&self, path: &AssetPath) -> Guid;                                  // see Minting; does not insert
    pub fn register(&mut self, path: AssetPath, kind: AssetKind) -> Result<AssetInfo, AssetError>;   // mint, then insert
    pub fn insert(&mut self, info: AssetInfo) -> Result<(), AssetError>;          // duplicate guid, path, case-clash, kind/extension mismatch
    pub fn rename(&mut self, guid: Guid, to: AssetPath) -> Result<(), AssetError>; // the Guid stays
    pub fn remove(&mut self, guid: Guid) -> Option<AssetInfo>;
    pub fn set_image_settings(&mut self, guid: Guid, s: ImageSettings) -> Result<ImageSettings, AssetError>;  // returns the old value
}
```

`ImageSettings` maps to render's `TextureDesc` in the facade glue (`filter`, address mode `Clamp`, `mips`). `edge_bleed` is applied by the decoder (below).

#### The manifest (Stage 2, feature `manifest`)

```
%exitium-text 1 root exitium_assets::Manifest
(
  assets: [
    (
      guid: "0b1c2d3e-4f50-8061-a273-8495a6b7c8d9",
      path: "scenes/main.scene",
      kind: Scene,
      image: None,
    ),
    (
      guid: "67e55044-10b1-826f-9247-bb680e5fe0c8",
      path: "sprites/hero.png",
      kind: Image,
      image: Some((filter: Nearest, mips: false, edge_bleed: false)),
    ),
  ],
)
```

This is the writer's exact output (serial-text-format.md "Writer: canonical output"): every field is written, a record whose members are not all leaf-like is broken (`image: Some(..)` is not leaf-like) and a flat record would pass column 100 here anyway, entries are sorted by path (`scenes/main.scene` before `sprites/hero.png`), and the trailing commas are the writer's.

`Manifest { assets: Vec<AssetInfo> }` derives `Reflect`, so the text, the schema and the MCP view come from reflection (design rule 2). **How `AssetPath` reflects:** it is a hand-written `Value` kind with an invariant (reflect-model.md, "Value kinds with an invariant"), stable name `exitium_assets::AssetPath`, `repr: String`, no text form, implemented in the `manifest` feature. Every write routes through `AssetPath::new`, so a path that breaks the rules can never be stored: `set_value`, `from_reflect`, `apply` and the text binder all fail with `ValueError::Invalid`, which serial reports as `InvalidValue` at the literal's span (`value "../x.png" is not valid for type AssetPath`). A transparent `String` wrapper would have skipped the rules on manifest load. Because the type cannot hold a bad path, `AssetRegistry::insert` has no path error; it reports duplicates, case clashes and a kind that does not match the extension. The schema hash sees `String` only. Entries are written sorted by path (UTF-8 byte order), so a diff shows one changed line per change. A manifest is read with `UnknownFields::Warn` (an older build reads a newer file). Loading validates every `AssetInfo` through `AssetRegistry::insert` and reports all failures at once. On native a missing manifest is an empty registry (a fresh project). On the web a missing manifest is `ManifestState::Failed` and the game shows it. The CLI reads the same file through the syntax tree only (see "CLI").

#### Loading

```rust
pub struct AssetConfig {
    pub root: PathBuf,                    // native. discover(): $EXITIUM_ASSET_DIR, else <exe dir>/assets if it exists, else <cwd>/assets
    pub web_base: String,                 // web, "assets", relative to the page
    pub threads: u8,                      // native, default 2
    pub limits: AssetLimits,
    pub auto_register: bool,              // default false. true: an unknown path under `root` is registered on first request (examples, tests)
    pub max_undelivered_bytes: usize,     // default 128 MiB: workers pause above this
    pub max_fetches: u8,                  // web, default 6 in flight
    pub decode_budget_ms: f32,            // web, default 4.0: decode at least one asset per frame, more while under budget
}
pub struct AssetLimits { pub max_file_bytes: usize /*64 MiB*/, pub max_image_dim: u32 /*8192*/, pub max_audio_frames: u32 /*16 Mi*/, pub max_text_bytes: usize /*16 MiB*/ }

pub enum AssetState { Unloaded, Loading, Ready, Failed(AssetError) }
pub enum Payload { Image(DecodedImage), Font(Vec<u8>), Audio(DecodedAudio), Text(String) }
pub struct DecodedImage { pub width: u32, pub height: u32, pub rgba8: Vec<u8> }   // straight alpha, sRGB-encoded, row-major, top-left origin: render's ImageData
pub struct DecodedAudio { pub sample_rate: u32, pub frames: Vec<[f32; 2]> }      // mono is duplicated
pub struct Loaded { pub guid: Guid, pub kind: AssetKind, pub payload: Payload }

impl AssetServer {
    pub fn new(config: AssetConfig, registry: AssetRegistry) -> AssetServer;
    pub fn registry(&self) -> &AssetRegistry;      pub fn registry_mut(&mut self) -> &mut AssetRegistry;
    pub fn request(&mut self, guid: Guid) -> AssetState;                 // idempotent: starts a load when Unloaded
    pub fn request_path(&mut self, path: &str) -> Result<Guid, AssetError>;
    pub fn state(&self, guid: Guid) -> AssetState;
    pub fn retry(&mut self, guid: Guid);                                 // Failed to Loading
    pub fn forget(&mut self, guid: Guid);                                // Ready to Unloaded; the consumer drops its own copy
    pub fn poll(&mut self);                                              // once per frame, main thread: collect finished jobs
    pub fn take_loaded(&mut self, kind: AssetKind) -> impl Iterator<Item = Loaded> + '_;   // each payload is delivered exactly once
    pub fn pending(&self) -> usize;                                      // Loading plus undelivered
    pub fn stats(&self) -> AssetStats;
}
```

State machine: `Unloaded --request--> Loading --> Ready | Failed(e)`; `Failed --retry--> Loading`; `Ready --forget--> Unloaded`. `Ready` means the payload has been decoded and queued for delivery. After `take_loaded` hands it out the server holds only the state. Unloading is manual in the MVP (no reference counts): the facade glue calls `forget` and destroys the texture or clip when a game asks.

`AssetError` is `Clone` and `#[non_exhaustive]`: `NotRegistered`, `BadPath(String)`, `NotFound { path }`, `Io(String)`, `Http { status }`, `Network(String)`, `TooLarge { what, size, max }`, `Decode { kind, message }`, `Unsupported { what }`, `Utf8`. Every message says what failed and what to do (`image is 9000 x 4000, the limit is 8192; downscale it, import does this for you`).

Requesting an asset needs `&mut AssetServer`. A scene load returns the `Guid`s it references (below) and the glue requests them. For assets that components reference later (an inspector edit), render's and UI's glue request any `Guid` that is not in their store, once per `Guid` until it fails.

#### Native source and the worker threads

`AssetSource` is a blocking trait (`fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError>`, `Send + Sync`) with `FsSource { root }` and `MemorySource` (tests). `read` enforces `max_file_bytes` before reading (file length). A worker takes a job (guid, path, kind, settings), reads, decodes and sends `Done { guid, result }` over an `mpsc` channel. `poll` drains the channel on the main thread. `Drop` stops and joins the workers. Back-pressure: a worker waits while undelivered payload bytes exceed `max_undelivered_bytes`. The only shared state is the job queue (`Mutex` plus `Condvar`), an atomic byte counter and the channel. Workers never see the world, the registry or a consumer.

#### Web source

On `wasm32` there is no `AssetSource` and no thread. `request` calls `window.fetch(url)` from `wasm_bindgen_futures::spawn_local`, where `url = web_base + "/" + path`. Up to `max_fetches` run at once. A response that is not `ok` is `Http { status }`; a thrown error is `Network`. The body becomes a `Vec<u8>` and is pushed into a `Mutex<VecDeque>` (uncontended: one thread). `poll` decodes from that queue: at least one asset per frame, then more while `decode_budget_ms` has not elapsed (`web_time::Instant`). A single large decode can still exceed the budget. Chrome budget: a 2048 x 2048 PNG decodes in at most 400 ms (see "Performance"). Web workers for decoding need threads and are out of the MVP.

The server type holds only `Send + Sync` data on both targets (`Mutex` wrappers, no `Rc`), so it can be an ECS resource (`Resource: Send + Sync`).

#### Decoders

All decoders are pure functions on `&[u8]` plus limits, so tests and benchmarks need no server.

| Kind | Crate | Accepts | Output | Errors |
|---|---|---|---|---|
| Image | `png` 0.18 | PNG, any colour type and bit depth, interlaced, tRNS. Only the default image of an APNG. Gamma and ICC chunks are ignored: pixels are treated as sRGB. | `DecodedImage` | dimension over `max_image_dim` is `TooLarge`; corrupt data is `Decode` |
| Audio | `hound` 3.5, `lewton` 0.10 | WAV PCM 8, 16, 24, 32 bit and float 32; Ogg Vorbis. 1 or 2 channels. Sample rate 8,000 to 192,000 Hz. | `DecodedAudio` (`i16` and others scaled to `[-1, 1]`) | more than 2 channels is `Unsupported`; over `max_audio_frames` is `TooLarge` (16 Mi frames, about 6 minutes at 44.1 kHz) |
| Font | `ttf-parser` 0.25 | TrueType and OpenType, face 0. Not `.ttc`. | The original bytes, after `Face::parse` succeeds | invalid is `Decode`; over 16 MiB is `TooLarge` |
| Scene, Text | std | UTF-8, a leading BOM is dropped | `String` | invalid UTF-8 is `Utf8` |

`image::decode_png` writes straight into the final `Vec<u8>` (one allocation), expands palettes, grey and 16-bit data to RGBA8, and then, when `edge_bleed` is set, **bleeds colour into fully transparent pixels**: it repeats 8 passes, each copying the colour of a neighbouring pixel with non-zero alpha into a transparent pixel (alpha stays 0). This removes the dark fringe that linear filtering and mips show at sprite edges, which resolves render-v1.md open question 5 without a premultiplied pipeline. It is a no-op for an image with no transparent pixel. Pixel art turns it off together with `Nearest` filtering. `image::encode_png(width, height, rgba8)` is the one encoder, used for screenshots (render returns RGBA8 and does not encode, render-v1.md).

Decode is the whole cost of "decoding fonts": render's `FontStore::insert_font(guid, bytes)` rebuilds the face itself. The assets crate only guarantees the bytes are a font, so a bad file fails at load with a path and not later inside the shaper.

#### Hand-off to consumers

`take_loaded(kind)` is how each consumer gets its data, so each facade glue system takes its own kind and nothing is cloned:

| Consumer | Takes | Does |
|---|---|---|
| Render glue | `Image`, `Font` | `TextureStore::insert(guid, ImageData { size, rgba8 }, TextureDesc)` and `FontStore::insert_font(guid, bytes)` (render-v1.md D3) |
| Audio glue | `Audio` | `ClipStore::insert(guid, ClipData { sample_rate, frames })` |
| Scene glue | `Scene` | parse and instantiate (below) |
| Whoever requested it | `Text` | the UI plugin parses `.ui` files |

A payload nobody takes stays queued. The plugin warns once per kind after 120 frames and `AssetStats.undelivered_bytes` shows it.

#### Native-only helpers (`native.rs`, not wasm32)

Pure file functions used by import and save (composed in Stage 2): `scan(root, &registry) -> Vec<Unregistered { path, kind }>` (files under `root` not in the registry, skipping names that fail `AssetPath`); `copy_in(src, root, dest)`; `downscale_png(rgba, w, h, max_dimension)` (box filter by an integer factor, so no resampling blur; render's `TextureTooLarge` contract in render-v1.md); `write_atomic(root, path, bytes)` (write a temp file in the same directory, then rename); `move_to_trash(root, path) -> PathBuf` (into `<root>/.trash/`, which `scan` skips, so a removal can be undone). Nothing here touches the world.

### Scenes

#### File format

A scene file is text with the header `%exitium-text 1 root exitium_scene::SceneFile` and extension `.scene`. `SceneFile` is **not** a registered reflected type: a generic `from_text` of a scene file returns `UnknownType`, and tools use the scene API instead. The envelope is read and written by this crate; every component is written and read by `exitium_serial` (`write_tagged`, `bind_tagged`).

```
%exitium-text 1 root exitium_scene::SceneFile
(
  entities: [
    (
      id: @main_camera,
      parent: None,
      components: [
        exitium_scene::Name("Main camera"),
        exitium_app::Transform2d(
          translation: (x: 0.0, y: 0.0),
          rotation: 0.0,
          scale: (x: 1.0, y: 1.0),
        ),
        exitium_render::Camera2d(
          scaling: PixelsPerUnit(1.0),
          zoom: 1.0,
          viewport: (
            min: (x: 0.0, y: 0.0),
            max: (x: 1.0, y: 1.0),
          ),
          pixel_perfect: false,
          clear: Some((r: 0.05, g: 0.05, b: 0.08, a: 1.0)),
          layers: 1,
          order: 0,
          active: true,
        ),
      ],
    ),
    (
      id: @hero,
      parent: None,
      components: [
        exitium_scene::Name("Hero"),
        exitium_app::Transform2d(
          translation: (x: 12.5, y: -3.0),
          rotation: 0.0,
          scale: (x: 1.0, y: 1.0),
        ),
        exitium_render::Sprite(
          image: Some("67e55044-10b1-826f-9247-bb680e5fe0c8"), // sprites/hero.png
          rect: None,
          size: None,
          anchor: (x: 0.5, y: 0.5),
          tint: (r: 1.0, g: 1.0, b: 1.0, a: 1.0),
          flip_x: false,
          flip_y: false,
          layer: 2,
          z: 0.0,
          mask: 1,
          visible: true,
        ),
      ],
    ),
    (
      id: @hero_shadow,
      parent: Some(@hero),
      components: [
        exitium_scene::Name("Hero shadow"),
      ],
    ),
  ],
)
```

The example is the writer's exact output for those components (field lists from render-v1.md and app-platform.md as of this spec), so it also loads. The layout rules that produce it, from serial-text-format.md "Layout": a struct is **flat** only when every member is leaf-like and it has at most 4 members (`(x: 12.5, y: -3.0)`, `Some((r: .., g: .., b: .., a: ..))`); anything with a non-leaf member (`Transform2d`, `Camera2d`'s `viewport`) or more than 4 members is **broken**, one member per line with a trailing comma. The envelope's own layout is fixed by this crate: `entities`, each record and each `components` list are always broken, so one component edit is one hunk in a diff. Because every field is written (serial writes defaults too), an agent that writes a scene by hand must also write every field, **unless** the component marks the field `#[reflect(default)]`; the separate issue RA1 does that for render's components (see "Downstream adjustments"), so a hand-written `Sprite(image: Some(".."))` loads, with a `DefaultedField` warning per filled field.

Rules:

- **Envelope.** The root group has a required field `entities`, a list of entity records. An unknown envelope field is a warning (forward compatibility). An entity record has `id` (required, an `@label`), `parent` (`None` or `Some(@label)`, default `None`) and `components` (a list of tagged groups, default empty). The writer always writes all three.
- **Labels.** `id: @label` **declares** a label. Every other `@label` in the file is a use and must be declared (`UnknownLabel`, with the closest declared names). Declared labels are unique (`DuplicateLabel`, with the first span). Forward references are fine. The writer derives a label from the entity's `Name`: lowercase, every character outside `[a-z0-9_]` becomes `_`, runs of `_` collapse, empty becomes `e`; a collision appends `_2`, `_3` and so on. An entity without a `Name` is `@e<n>`, where `n` is its 1-based position in the file. Labels are derived on every write, so renaming an entity renames its label (a one-line diff plus its uses).
- **Components** are tagged groups, `exitium_app::Transform2d(..)`. Each type must be registered with `ReflectComponent`. A type appears at most once per entity (`DuplicateComponent`). A component is written in this order: `exitium_scene::Name` first, then the rest by stable type name (UTF-8 bytes). `Name` is a plain tuple struct (`Name(pub String)`, not `transparent`), so its tagged form is unambiguous.
- **Order.** Entities are written in pre-order of the hierarchy: roots in `Entity` index order, children in their parent's `Children` order. Loading spawns in file order and re-parents in file order, so sibling order survives a round trip. Index order of roots is a diff-noise heuristic only (open question 6).
- **Skipped silently on save** (by design, nothing is lost): a component whose `TypeInfo` carries `custom(derived = true)` (`GlobalTransform2d`, app-platform.md); `SceneMember`; and `Parent` and `Children` (matched by the stable names `exitium_ecs::Parent` and `exitium_ecs::Children`, checked by a test against the registry). The parent relation is the record's `parent` field, and `Children` is rebuilt by #49's `Commands` helpers on load.
- **Reported, never silent.** A component on a member entity whose type has **no `ReflectComponent`** cannot be written. `write_scene` returns one `SceneWarning::UnsavedComponent { type_name, entities }` per such type (the ECS-registered type name and the number of entities; an agent that forgot `#[reflect(Component)]` sees it in the `scene_save` result). `SaveOptions::strict` turns it into an error (`SceneErrorKind::UnsavedComponent`). Known internal state that is rebuilt by its own systems (physics `BodyLink` and `ColliderLink`, "the physics world is derived state and is never saved", physics-2d.md) is listed in `SaveOptions::ignore_unreflected` by name, so the facade passes a constant `INTERNAL_COMPONENTS` and a save of a physics scene stays quiet while any other unreflected type is reported.
- **Parent outside the scene.** A saved root whose `Parent` is not a member is written `parent: None` and becomes a root on reload. In `SaveScope::Scene` this also gives `SceneWarning::ParentOutsideScene { entity }` (a member authored under a runtime entity loses that link, and so does an instance that was attached with `InstantiateOptions::parent`). In `SaveScope::Entities` the listed entities are the roots by definition, so there is no warning.
- **Entity references** in components (`Attack(target: @boss)`) use the serial `TextRefs` hook: the writer maps an entity's canonical text to its label and refuses an entity that is not in the file (`ReferenceOutsideScene`, naming the component and field), so a saved scene never contains `"42v1"`.
- **Asset references** are `Guid` fields, written as the UUID string with the asset's path in a comment (`// sprites/hero.png`), from the `asset_path` callback below. The comment is regenerated on every write and ignored on read.
- **Author-time convenience (decided here, serial-text-format.md leaves it to #111).** When the caller passes `resolve_asset`, a `Guid` field may be written as a path (`image: Some("sprites/hero.png")`). The reader maps it to the registered `Guid`; an unknown path is a `Reference` error. The next write normalizes it to the UUID. Without a resolver a path is a `TextForm` error. An agent that hand-writes a scene can therefore name assets by path.
- **Unknown fields** in a component use the serial file default `Warn`. An unknown component type is always an error with up to 3 suggestions: dropping it silently would lose data on the next save.

#### Components and resources (reflected)

```rust
#[derive(Component, Reflect, Clone, Default)] #[reflect(Component, Default, type_name = "exitium_scene::Name")]
pub struct Name(pub String);
#[derive(Component, Reflect, Clone, Copy)] #[reflect(Component, type_name = "exitium_scene::SceneMember", custom(derived = true))]
pub struct SceneMember(pub SceneId);                  // which scene instance an entity belongs to. Not saved.
#[derive(Reflect, Clone, Copy, PartialEq, Eq, Hash)] #[reflect(transparent, type_name = "exitium_scene::SceneId")]
pub struct SceneId(pub u32);
#[derive(Resource, Reflect, Default)] #[reflect(Resource, Default, type_name = "exitium_scene::Scenes")]
pub struct Scenes { pub next: u32, pub scenes: Vec<SceneInfo>, pub active: Option<SceneId> }   // SceneInfo { id, source: Option<String> asset path }
```

**Membership is the save scope.** Only entities with `SceneMember(id)` are written for scene `id`. Instantiating a scene tags every entity it spawns. Entities that game code spawns at runtime (bullets, particles, editor helpers) carry no `SceneMember` and are never saved by accident. The command that spawns an entity for authoring (#110) must call `mark_member`. This is the one rule #110 has to honour (see "MCP / editor surface").

#### API

```rust
pub struct ParsedScene<'a> { /* Syntax<'a>, header, entity table: label, parent label, component nodes */ }
impl<'a> ParsedScene<'a> {
    pub fn parse(text: &'a str, limits: &Limits) -> Result<ParsedScene<'a>, SceneErrors>;     // syntax, envelope and label checks; no registry, no world
    pub fn entity_count(&self) -> usize;
    pub fn instantiate(&self, world: &mut World, registry: &TypeRegistry, opts: &InstantiateOptions<'_>)
        -> Result<SceneInstance, SceneErrors>;
}
pub struct InstantiateOptions<'a> {
    pub scene: Option<SceneId>,                                // None: allocate a new SceneId in the `Scenes` resource (inserted if absent)
    pub parent: Option<Entity>,                                // attach the scene's roots to this entity (prefab-like use)
    pub resolve_asset: Option<&'a dyn Fn(&str) -> Option<Guid>>,
    pub unknown_fields: UnknownFields,                         // default Warn
}
pub struct SceneInstance {
    pub scene: SceneId, pub entities: Vec<Entity> /* file order */, pub roots: Vec<Entity>,
    pub labels: Vec<(String, Entity)>, pub asset_refs: Vec<Guid> /* deduplicated, first-use order */,
    pub warnings: Vec<SceneWarning>,
}

pub enum SaveScope<'a> { Scene(SceneId), Entities(&'a [Entity]) }          // Entities: these and their descendants
pub struct SaveOptions<'a> {
    pub asset_path: Option<&'a dyn Fn(Guid) -> Option<String>>,
    pub strict: bool,                                          // default false: unreflected components are warnings, true: errors
    pub ignore_unreflected: &'a [&'a str],                     // ECS type names that are internal state (see "Reported, never silent")
}
pub struct SavedScene { pub text: String, pub warnings: Vec<SceneWarning> }
pub enum SceneWarning { UnsavedComponent { type_name: String, entities: u32 }, ParentOutsideScene { entity: String }, Text(TextWarning) /* load side: binder warnings */ }
pub fn write_scene(world: &World, registry: &TypeRegistry, scope: SaveScope<'_>, opts: &SaveOptions<'_>) -> Result<SavedScene, SceneErrors>;
pub fn validate_scene(text: &str, registry: &TypeRegistry, resolve_asset: Option<&dyn Fn(&str) -> Option<Guid>>) -> ValidationReport;  // instantiates into a scratch World
pub fn despawn_scene(world: &mut World, scene: SceneId) -> usize;         // #49's despawn_recursive for every member root, applied through World::run_system_once before returning; returns the count
pub fn mark_member(world: &mut World, entity: Entity, scene: SceneId);
pub fn register_types(world: &mut World, registry: &mut TypeRegistry);    // Name, SceneMember, SceneId, Scenes (the facade glue calls it; no Plugin here)

pub struct SceneErrors { pub errors: Vec<SceneError>, pub truncated: bool }       // up to 16, never empty
pub struct SceneError { pub kind: SceneErrorKind, pub entity: Option<String> /* label */, pub component: Option<String>, pub text: Option<TextError>, pub message: String }
pub enum SceneErrorKind { Syntax, Envelope, InvalidParent, DuplicateLabel, UnknownLabel, UnknownComponentType, DuplicateComponent, Bind, ReferenceOutsideScene, UnsavedComponent, ParentCycle, TooLarge, Insert }
```

`ParsedScene::parse` and `instantiate` take no `Plugin` or `App`. The caller holds the registry read guard for the whole call (serial threading rule).

#### Load algorithm (atomic)

1. `parse`: `exitium_serial::parse_syntax` (limits per caller), then the envelope check: record shape, declared labels (unique), every used `@label` declared (`Syntax::refs()`), no parent cycle.
2. `instantiate`, phase one. First, if `opts.parent` is `Some(p)`, check `world.contains(p)` (ecs-storage-queries.md); a dead `p` is a caller mistake and returns `InvalidParent` ("`opts.parent` is not a live entity") before anything is spawned or bound. Then spawn **one empty entity per declared label** with `world.spawn(())`, the empty bundle (the entity is placed in the empty archetype, ecs-storage-queries.md). The storage spec says "tuple impls up to arity 8" without stating arity 0, so this is recorded as a downstream note for #41 ("Downstream adjustments"). Reserving without spawning would leave unflushed ids that `flush_entities` turns into phantom entities if the load fails (ecs-scheduler.md, abandonment), so the scene spawns real empty entities and despawns them on failure. Build `SceneRefs` (below) over the label table.
3. Phase two: for every component node in file order, `bind_tagged` with `ReadOptions { unknown_fields, refs: Some(&mut SceneRefs), partial: false, limits }`. Errors are collected (up to 16 across the file). No component has been inserted yet.
4. If there was any error: despawn the entities of phase one and return. The world is unchanged except for entity index and generation churn.
5. Phase three: `ReflectComponent::insert` for each bound component (a failure here is `Insert`; no parent link exists yet, so the loader rolls back with `world.despawn` on every entity it created and returns). Then `SceneMember` on every entity.
6. Hierarchy. #49 defines `set_parent`, `remove_child` and `despawn_recursive` as `Commands` helpers (ecs-scheduler.md: `despawn_recursive` "applies through `Commands`"), not as `World` methods. The scene applies them with **`World::run_system_once`** (ecs-scheduler.md, "`Schedules` and running by label": "initialize, run, apply deferred"), which already exists and needs no ECS change. It runs one closure that takes `Commands`: `let report = world.run_system_once(move |mut cmds: Commands| { for (child, parent) in &links { cmds.entity(*child).set_parent(*parent); } });` (the exact call shape is #49's; the closure is `Send + 'static` and owns its `links` list: `(child, parent)` for every record with a `parent` in file order, plus `(root, p)` for every root when `opts.parent` is `Some(p)`). The deferred commands are applied inside the call, so nothing outlives it and the caller never sees a half-parented scene. The returned `ApplyReport` has only `applied` and `skipped_dead_entity`, so **the only failure the loader can observe is a skipped command** (`skipped_dead_entity > 0`; the entities were just created and `opts.parent` was checked alive in step 2, so a skip means the world changed underneath the call or a bug, and is reported as `Insert`). On a skip the loader rolls back: one more `run_system_once` closure first calls `remove_child(p, root)` for every root it attached to `opts.parent` (so the parent's `Children` list holds no dead id; `World::despawn` never touches `Children`, ecs-storage-queries.md) and then `despawn_recursive` on every root it created; entities that never got a parent are removed with `world.despawn`. `despawn_scene` uses the same pattern for `despawn_recursive`. No change to #45 or #49 is needed.
7. Return `SceneInstance`.

`SceneRefs` implements `TextRefs`. `read_ref`: for `Entity`, an `@label` maps to the spawned entity's canonical text (`ReflectTextForm::to_text` through the registry); a plain `"42v1"` string is rejected in a scene (raw ids have no meaning in a file); for `Guid`, a UUID passes through and is recorded in `asset_refs`, a path goes through `resolve_asset`. `write_ref` is a pure function of `(site, canonical text)` (serial-text-format.md, "Hook calls"): labels are assigned before writing starts, so it never numbers by call order. The labels of the scene being saved live in a `HashMap<String, String>` from canonical entity text to label.

**Why not reserve entities.** The serial spec suggests `reserve_entity` for forward references. Spawn-empty costs one extra archetype move per entity, which the instantiate budget below includes, and removes a whole class of leak on failure. If the ECS later adds a safe `release_reserved`, the loader may switch.

**Writing** collects members with a query over `SceneMember`, orders them (pre-order), assigns labels, then for each entity writes the record text itself and each component with `write_tagged` into a scratch `String`, re-indented by 8 columns (entities 2, record 4, record fields 6, component items 8). Serial's layout (flat or broken, the 100-column cap) is computed from the component's own first column, so a long flat component may reach column 108. This is deterministic and only a safety valve (serial-text-format.md "Layout"); a base-indent option in serial would remove it and is not required.

**Validate.** `validate_scene` instantiates into a scratch `World` with a copy of no resources (components are inserted by type data, so no resource is needed) and drops it. It is how a read-only MCP tool checks a file or a pasted text without touching the live world.

**Large scenes on the web.** Parse, bind and instantiate run on the main thread inside one call. Budget: 10,000 entities of 5 components load in at most 150 ms native and 300 ms in Chrome, so a scene load happens behind a loading screen. An incremental loader (`instantiate_step(n)` over the entity table) is additive and is open question 7.

### Audio

#### Engine

`AudioEngine` wraps kira's `AudioManager`. **No kira type is public.** The API speaks `f32` volumes in `[0, 1]` (linear amplitude; the crate converts to decibels, and `0` is silence) and seconds.

```rust
pub enum AudioState { Locked, Ready, Unavailable(String) }
pub enum Bus { Music, Sfx, Ui }                          // each is a kira sub-track of the main (master) track
pub struct ClipData { pub sample_rate: u32, pub frames: Vec<[f32; 2]> }
pub struct ClipId(/* core Handle<Clip> */);
pub struct Voice(/* generational */);
pub struct PlayOptions  { pub volume: f32 /*1*/, pub pitch: f32 /*1, 0.25..=4*/, pub pan: f32 /*0, -1..=1*/, pub looped: bool, pub bus: Bus /*Sfx*/, pub fade_in: f32 /*0*/ }
pub struct MusicOptions { pub volume: f32 /*1*/, pub looped: bool /*true*/, pub fade_in: f32 /*1.0*/, pub crossfade: f32 /*1.0: the old track fades out over this*/ }
pub enum AudioOutput { Device, Null { sample_rate: u32 } }   // Null: no device; the mixer runs but plays nowhere (tests, CI, headless runs)
pub struct AudioConfig  { pub max_voices: u16 /*64, global*/, pub output: AudioOutput /*Device*/, pub start_locked: Option<bool> /*None: true only for Device on wasm32*/ }
pub struct AudioStats   { pub state: AudioState, pub voices: u16, pub clips: u32, pub dropped_locked: u64, pub dropped_limit: u64, pub dropped_missing: u64 }

impl AudioEngine {
    pub fn new(config: AudioConfig) -> AudioEngine;       // Device on native: opens the default output now. Device on the web: starts Locked, no device. Null: Ready at once unless `start_locked`
    pub fn unlock(&mut self) -> AudioState;               // web: creates the device, idempotent. native: no-op
    pub fn state(&self) -> &AudioState;
    pub fn insert_clip(&mut self, data: ClipData) -> Result<ClipId, AudioError>;   // 1..=16 Mi frames, 8,000..=192,000 Hz
    pub fn remove_clip(&mut self, id: ClipId) -> bool;                              // stops its voices
    pub fn play(&mut self, clip: ClipId, opts: &PlayOptions) -> Option<Voice>;      // None: locked, unavailable, limit reached or missing clip (counted)
    pub fn stop(&mut self, voice: Voice, fade: f32);        pub fn stop_all(&mut self, fade: f32);    pub fn is_playing(&self, voice: Voice) -> bool;
    pub fn play_music(&mut self, clip: ClipId, opts: &MusicOptions);   // the same clip already playing is left alone
    pub fn stop_music(&mut self, fade: f32);
    pub fn set_master_volume(&mut self, v: f32);   pub fn set_bus_volume(&mut self, bus: Bus, v: f32);
    pub fn update(&mut self);                                           // once per frame: reap finished voices, start pending music, refresh stats
    pub fn advance_null(&mut self, frames: u32) -> f32;                 // test helper: runs the Null mixer for `frames` and returns the peak absolute sample; 0.0 for a Device output
    pub fn stats(&self) -> AudioStats;
}
```

- **Mixer.** The main track is master; `Music`, `Sfx` and `Ui` are sub-tracks. A sound plays into its bus's track. **Voice limit.** kira's `sound_capacity` is **per track** (128 by default on the main track and on every sub-track), so it cannot express one global cap. The engine therefore counts voices itself: one counter over every bus, incremented by `play` and decremented in `update` when a voice has stopped. A `play` at `max_voices` is dropped and counted in `dropped_limit` (no stealing in the MVP). `play_music` ignores the cap, so music never fails, but its voices (at most two during a crossfade) are counted. The engine sets each track's `sound_capacity` to `max_voices + 2`, so kira's own limit is never the first one to trigger, and `AudioStats.voices` is the global counter.
- **Music** is one slot. `play_music` with a different clip fades the old one out over `crossfade` and the new one in over `fade_in`. The same clip does nothing. While `Locked`, the latest `play_music` request is **remembered** and started by `unlock`, so title music begins at the first click. `play` (a sound effect) while `Locked` is dropped and counted in `dropped_locked`.
- **No device is not a crash.** On Windows a missing output device, or in CI, gives `Unavailable(reason)` and every call is a counted no-op. **The test and headless path is public and kira-free:** `AudioOutput::Null { sample_rate }` runs the mixer on kira's `MockBackend` (which builds for wasm32 too), so engine logic and the facade's headless tests run with no sound card, and `advance_null(frames)` renders audio into nowhere and returns the peak sample, so a test can assert that a sound was audible without naming a kira type. `AudioPlugin` carries an `AudioConfig`, so a headless app builds `AudioPlugin { config: AudioConfig { output: AudioOutput::Null { sample_rate: 48_000 }, ..Default::default() } }`. `start_locked: Some(true)` with `Null` reproduces the web's `Locked` start on the host.
- **Decoded clips are shared.** `insert_clip` converts `Vec<[f32; 2]>` into kira's `Arc<[Frame]>` in one pass and frees the source. Peak memory for that clip is 2x for the duration of the call.

#### The web autoplay rule

A browser keeps an `AudioContext` that was created before any user gesture in the `suspended` state. kira's web backend (cpal) creates the context inside `AudioManager::new` and calls `resume()` once, so a manager created at startup may stay silent forever. The design therefore **creates the manager at the first gesture**: `AudioEngine::new` on wasm32 returns `Locked` and makes no cpal call, and `unlock()` creates the manager. The facade audio glue (FA2) calls `unlock()` from a system that runs in `First`, after `INPUT`, whenever `InputState` reports any key, mouse button or touch press edge in the frame, and until the state is no longer `Locked`. Chrome's autoplay policy lets a page start audio once the user has interacted with it (the activation is sticky), so the delay of one frame between the event and the frame callback is expected to be harmless. This is the part of the design that cannot be proved by a unit test: the audio example (AU3) must run in Chrome, press a key or click, and hear the tone, and its PR records that. The risk and the fallback are open question 4. Audio on the web is mixed on the main thread (cpal's WebAudio backend schedules buffers from timers), so its cost counts against the frame (budget below).

#### Plugin (Stage 2, feature `plugin`)

`AudioPlugin` inserts `Audio` (a resource wrapping the engine; kira's `AudioManager` is `Send + Sync`) and `ClipStore` (`Guid -> ClipId`, `insert(guid, ClipData)`, `get(guid)`), and registers the reflected types below. Systems: `apply_audio_settings` in `Update` (pushes `AudioSettings` to the engine when it `is_changed`), `audio_events` (reads `PlayAudio`, `StopAllAudio`, `PlayMusic`, `StopMusic`), and `audio_update` in `Last` (`update()` and the `AudioStats` copy). Game code uses `ResMut<Audio>`: `audio.play_guid(guid, &PlayOptions::default())` looks the clip up in the `ClipStore` (a clip that is not loaded is `dropped_missing`).

| Reflected type | Kind | Fields |
|---|---|---|
| `AudioSettings` | resource, `Default` | `master`, `music`, `sfx`, `ui`: `f32`, `range(min = 0.0, max = 1.0)`, default `1.0`. The source of truth for volumes. |
| `AudioStats` | resource, read-only | the fields of `AudioStats` above |
| `PlayAudio` | event | `clip: Guid`, `bus: Bus`, `volume: f32`, `pitch: f32`, `pan: f32`, `looped: bool` |
| `StopAllAudio`, `PlayMusic { clip: Guid, volume, fade_in, crossfade }`, `StopMusic { fade }` | events | |
| `Bus` | enum | `Music`, `Sfx`, `Ui` |

### Facade glue (contract)

By ruling (a) the facade holds every cross-crate system. These are issues FA1 to FA4 below, each listing `crates/exitium/` under `Hotspots`. **FA1 is render-v1.md's G1b, not a second issue:** the architect files one issue (FA1, cross-linked as G1b), and adds this spec's blockers to it. The same holds for any other overlap with a peer spec's facade issue (the architect files each system once).

| Glue | Runs | Behaviour |
|---|---|---|
| Asset to render and UI (FA1, `AssetGluePlugin`) | `Update`, after the plugin's `ASSETS` set | `take_loaded(Image)` becomes `TextureStore::insert(guid, ImageData, TextureDesc { filter, address: Clamp, mips })` built from the registry's `ImageSettings`; `take_loaded(Font)` becomes `FontStore::insert_font`. **Requests are change-driven, never a per-frame scan:** `Query<&Sprite, Changed<Sprite>>` and `Query<&Text, Changed<Text>>` (an `Added` row counts as `Changed`, so a spawn is seen) feed a `Local<HashSet<Guid>>` of `Guid`s already requested; the UI part walks the `UiDrawList`'s image and font `Guid`s only when the UI tree's `paint_revision` is newer than the glue's last seen value (ui-mvp.md). A `Guid` that is not in a store and not yet requested is requested once. **Failures are retried by a separate, bounded rule**, because a change-driven scan never revisits an unchanged row: when a `Guid` reaches `Failed` with a transient error (`NotRegistered`, `NotFound`, `Io`, `Http`, `Network`), the glue records it in a `Local<HashMap<Guid, Retry { attempts: u8, due: f64 }>>` and calls `AssetServer::retry(guid)` when `Time` passes `due`; the delays are 1, 2, 4, 8 and 16 seconds (5 attempts), after which the `Guid` stays `Failed`, is logged once, and only an editor `retry` or a fresh change of the referencing component (which removes it from the `requested` set on the final failure) requests it again. A permanent error (`Decode`, `TooLarge`, `Unsupported`, `Utf8`, `BadPath`) is never retried automatically. The retry map is empty in the normal case, so the system costs one `is_empty` check per frame. After a `forget` the glue drops the `Guid` from its `requested` set, so a later change of a sprite that names it requests it again; nothing else does, which is intended (unloading is a deliberate act in the MVP). Cost is proportional to the rows that changed, and 20,000 unchanged sprites cost one filter check per archetype chunk (budget below). A `Failed` state logs once with the path. |
| Asset to audio (FA2, `AudioGluePlugin`) | `Update`, after `ASSETS` | `take_loaded(Audio)` becomes `ClipStore::insert(guid, ClipData)`. In `First`, after `INPUT`: the autoplay unlock above. |
| Scene loading (FA3, `ScenePlugin`) | `Update` | An event `LoadScene { path: String, mode: LoadMode (Replace or Additive), parent: Option<Entity> }` requests the `.scene` asset. When its `Scene` payload is taken, the glue parses and instantiates it with `resolve_asset` from the registry, requests `SceneInstance.asset_refs`, and sends `SceneLoaded { scene, path }` or `SceneLoadFailed { path, errors: String }` (reflected events). `Replace` calls `despawn_scene` for the active scene first. The glue lives in a facade `ScenePlugin`, which inserts `Scenes` and calls `scene::register_types` (D7: `exitium_scene` has no `Plugin`). |
| Save to disk (#110, not facade) | editor | `write_scene` then `native::write_atomic` then `AssetRegistry::register` for a new file. The editor depends on all three crates, so #110 composes it; no facade code is needed. |

### CLI

`exitium` (`exitium_cli`, native Windows only, already in `NATIVE_ONLY`) has three subcommands in the MVP.

```
exitium new <name> [--dir <parent>] [--engine-path <dir> | --engine-git <url> [--engine-rev <rev>]]
exitium export --target windows|web [--project <dir>] [--out <dir>] [--check] [--json]
exitium serve <dir> [--port 8080]
```

Arguments are parsed by hand (about 150 lines): no `clap`. Exit codes: 0 ok, 1 failure (a message on stderr that names the cause and the fix), 2 usage. With `--json` the command prints exactly one JSON object on stdout at the end (hand-written, flat, about 30 lines) so that the MCP export tool can parse it.

#### `exitium new <name>`

`name` is `[a-z][a-z0-9_]*` (snake case; one name for the package, the binary and the wasm file, so no hyphen ambiguity). The command refuses to overwrite an existing directory. Layout (the files are embedded in the CLI with `include_str!` from `crates/exitium_cli/template/`):

```
<name>/
  Cargo.toml            [package] name, edition 2024; [dependencies] exitium (path or git, see below);
                        [features] editor = ["exitium/editor"]; target.'cfg(target_arch = "wasm32")'.dependencies wasm-bindgen = "=0.2.130"
  rust-toolchain.toml   channel = the engine's pinned toolchain; targets = ["wasm32-unknown-unknown"]
  exitium.project       the project file (below)
  .gitignore            /target, /dist
  CLAUDE.md             a short guide for an agent: build, run with the editor, MCP, export commands, the rules of the engine's parity design
  src/main.rs           App::new().add_plugins(DefaultPlugins).add_systems(Startup, load_main_scene).run();
                        load_main_scene sends LoadScene { path: "scenes/main.scene", mode: Replace, parent: None }
  assets/
    assets.manifest     the manifest with one entry, scenes/main.scene (a minted Guid, written in the canonical layout)
    scenes/main.scene   one entity, the `@main_camera` record of the example above with its full
                        Name, Transform2d and Camera2d field lists, so it loads with no defaults involved
```

`cargo run --features editor` starts the game with the editor and the live-control server (the `editor` feature lives in the facade, C2). `cargo run` starts the plain game. The `wasm-bindgen` pin makes the game's lock file resolve the same library version as the engine's workspace, so the one CLI version in "Export tools" is the right one. The engine reference defaults to a **path dependency on the checkout the CLI was built from** (`CARGO_MANIFEST_DIR/../..` baked in at compile time), because nothing is published (jam-mvp.md open question 5). `--engine-git <url> [--engine-rev <rev>]` writes a git dependency instead (the repository is public); `--engine-path` writes another path. The generated project must pass `cargo check` and `cargo check --target wasm32-unknown-unknown` (a CLI test does this when the toolchain is present).

The **project file** is `%exitium-text 1 root exitium_cli::Project`:

```
%exitium-text 1 root exitium_cli::Project
(
  name: "my_game",        // package and binary name
  title: "My Game",       // the exported page's title
  width: 1280,            // the exported page's canvas size
  height: 720,
  background: "#101018",  // the exported page's background colour
  export_features: [],    // extra cargo features for `exitium export`, never "editor"
)
```

**What reads it, and when.** Only `exitium` itself reads the project file, and only in `exitium export` (step 1, before anything is built). `exitium new` writes it. Nothing reads it at run time: the game binary never parses it, and the editor does not. The window title and size of the running game come from the game's own `WindowConfig` in `src/main.rs`; `exitium new` writes the same title, width and height into both files, so they agree at creation and are independent afterwards (an edit to one is not picked up by the other). `export_features` is a list of cargo feature names passed as `--features` together with `--no-default-features`; the exporter refuses a list that contains `editor`. Its default is the empty list.

It is read through `exitium_serial::parse_syntax` and a walk over the syntax tree (fields by name, `leaf::unescape_string` for strings). The CLI derives no `Reflect` (`exitium_cli` is in the reflect derive's `NOT_DERIVE_USERS`, reflect-model.md), and the root type name is a label, not a registered type. The export reads each manifest entry's `path` from `assets/assets.manifest` the same way. Unknown fields are ignored with a warning.

#### `exitium export --target windows`

1. Read `exitium.project`; locate the crate (`--project`, default the current directory). Refuse a non-Windows host: `export --target windows` is Windows only in the MVP.
2. `cargo build --release --bin <name> --no-default-features` (plus the project's optional `export_features`, default none). **`editor` is never enabled**, so the exe contains no editor and no live-control server.
3. Stage `dist/<name>-windows/`: `<name>.exe`, and `assets/` holding `assets.manifest` and only the files the manifest lists, copied with their relative paths. A manifest entry whose file is missing is an error that lists all of them. A file under `assets/` that the manifest does not list is a warning and is not shipped.
4. Write `dist/<name>-windows.zip`: a deterministic archive (entries sorted by path, forward slashes, fixed timestamp 1980-01-01, deflate through `zip` with the `deflate-flate2` feature, one top folder `<name>-windows/`). Two exports of the same inputs give the same bytes.
5. Report: file count, total bytes, zip bytes, warnings.

The exe finds its assets next to itself (`AssetConfig::discover`: `<exe dir>/assets`).

#### `exitium export --target web`

1. **Preflight** (also `--check`, which stops here): the `wasm32-unknown-unknown` standard library exists (`rustc --print target-libdir --target wasm32-unknown-unknown`); `wasm-bindgen --version` is on `PATH` and equals the `wasm-bindgen` version in the game's `Cargo.lock`; `wasm-opt` is optional. Each failure prints the exact command to fix it (below).
2. `cargo build --release --target wasm32-unknown-unknown --bin <name> --no-default-features`.
3. `wasm-bindgen --target web --no-typescript --out-dir <stage> --out-name <name> target/wasm32-unknown-unknown/release/<name>.wasm`. This writes `<name>.js` and `<name>_bg.wasm`. The platform and render examples already use this recipe with a `main` entry (app-platform.md PL5, render-v1.md I12), and C4 confirms it for the template. `main` returns after `run()` on the web (app-platform.md, "Runner contract").
4. If `wasm-opt` is on `PATH`: `wasm-opt -Oz` on `<name>_bg.wasm` (the implementer verifies the feature flags on a real run). Otherwise the step is skipped with a note.
5. Write `index.html` from an embedded template: the title, a `<canvas id="exitium-canvas">` (the platform's default canvas id, app-platform.md) sized from the project file, the background colour, a "Loading" text that the module removes, and a module script `import init from "./<name>.js"; await init();`. If `navigator.gpu` is missing, the page shows "This game needs WebGPU (Chrome or Edge 113 or later on Windows)" and does not load the module. Any error from `init` is shown in the page and in the console.
6. Copy the manifest and its files to `<stage>/assets/` as in the Windows export.
7. Write `dist/<name>-web.zip` with `index.html` at the **root of the zip** (itch.io requires it), the same deterministic rules, and report raw and compressed wasm sizes (the entry's deflate size stands for the gzip size; budget 10 MB, jam-mvp.md) and the file count. The implementer checks itch.io's current HTML5 limits on file count and size and puts them in a constant that produces a warning.

The page must be served over HTTP (browsers do not run wasm modules from `file://`). `exitium serve <dir>` is a std-only static server (one thread per connection, `GET` only, loopback only, `Content-Type: application/wasm` for `.wasm`, `text/javascript` for `.js`, no cache) for testing in Chrome without Python or Node. It is the last CLI issue and is optional for the MVP exit.

#### Export tools

| Tool | For | Version | How to get it | How the CLI checks |
|---|---|---|---|---|
| Rust toolchain | both | the game's `rust-toolchain.toml` (same as the engine, 1.100.0) | rustup | cargo itself |
| MSVC build tools | windows | any VS 2022 Build Tools with the "Desktop development with C++" workload | Microsoft | cargo's link error is passed through |
| `wasm32-unknown-unknown` target | web | from `rust-toolchain.toml` (`targets`) | rustup installs it with the toolchain | `rustc --print target-libdir --target wasm32-unknown-unknown` exists |
| **`wasm-bindgen-cli`** | web | **exactly 0.2.130** (the workspace `wasm-bindgen`; it must equal the library version in the game's `Cargo.lock`) | `cargo install wasm-bindgen-cli --version 0.2.130 --locked` (builds from source, a few minutes) | `wasm-bindgen --version` against the lock |
| `wasm-opt` (Binaryen) | web, optional (smaller wasm) | any recent release | a Binaryen release on `PATH` | `wasm-opt --version` |
| Chrome 113 or later with WebGPU | to run the web build | | | not checked |

Not needed: Node, npm, `trunk`, `wasm-pack`, a system `zip`, Python. `exitium_cli` has a constant `WASM_BINDGEN_VERSION = "0.2.130"`, and a test fails when it differs from the `wasm-bindgen` version in the workspace `Cargo.lock` and in the template, so a version bump in the engine cannot leave the CLI behind. If the game's lock names another version, the error says: `this game locks wasm-bindgen X; install the CLI with the same version: cargo install wasm-bindgen-cli --version X --locked` and, for a game created by `exitium new`, `cargo update -p wasm-bindgen --precise 0.2.130`.

#### Does CI have to change?

**No `.github/workflows/` change is required for the MVP.**

- `cargo xtask ci wasm` already builds every non-`NATIVE_ONLY` crate for wasm32, and `exitium_cli` is already `NATIVE_ONLY`. `exitium_assets`, `exitium_audio` and `exitium_scene` build for wasm32 and are covered.
- The Linux runner image already has `libasound2-dev` (`ci/runner/linux/Dockerfile`), which kira's `cpal` needs on Linux. Audio tests use `AudioOutput::Null`, so no sound card is needed.
- `wasm-bindgen-cli` is not on the runners. The export tests use a fake `wasm-bindgen` script and a fake `cargo` shim (a plan test: the CLI builds a list of commands and the test checks it), and the real web export is the Stage 4 rehearsal, run by hand on Windows. A real export test is `#[ignore]`d.
- The `deny` job runs automatically on the new dependencies. It needs the `deny.toml` exception for `triple_buffer` (below). That is a hotspot file in intake 1, not a workflow.
- If the owner later wants a CI web-export smoke job, that is a change to `.github/workflows/ci.yml` (the owner pushes it) plus `cargo install wasm-bindgen-cli --version 0.2.130 --locked` in `ci/runner/linux/Dockerfile` (not a workflow, but the runner image must be rebuilt by the owner). It is not proposed for the MVP.
- Intake should extend the xtask wasm step to `cargo build --examples --target wasm32-unknown-unknown` for `exitium_assets` and `exitium_audio` (as app-platform.md proposes for platform), so the two web examples are checked in CI. That is an `xtask/` edit, which intake holds.

### Threading

- **Main thread.** Every public function of `exitium_assets`, `exitium_audio` and `exitium_scene` is called from the main thread, inside a system. ECS execution is single-threaded in the MVP.
- **Assets, native.** Two worker threads (`exitium-assets-0`, `-1`) read and decode. They share the job queue, an atomic byte counter and the result channel with the main thread, and nothing else. They never touch the world, a registry or a consumer. The roadmap says the live-control server is the only extra thread (jam-mvp.md, Threading); that line did not count these workers or the audio device thread (cpal's). Both are listed here. Workers exit on drop. The web has no worker.
- **Audio.** The device callback runs on the audio thread (native) or on the main thread from timers (web). The engine communicates with it through kira's lock-free command queues. Game code never blocks on audio.
- **Scene.** Pure synchronous functions. Parsing may run on any thread (serial's `Syntax` is `Send + Sync`), but binding and instantiation need `&mut World`, so a scene load is a main-thread step.
- **CLI.** A single thread. It spawns `cargo`, `wasm-bindgen` and `wasm-opt` as child processes and waits. `serve` uses one thread per connection.

## MCP / editor surface

Per cross-spec rule C4, this section is the list of what the three runtime crates expose. #110 builds every tool and panel generically from reflection and commands. A capability is parity-complete when the editor action and the MCP tool call the same function.

**Reflected surface.** `AssetKind`, `ImageFilter`, `ImageSettings`, `AssetInfo`, `Manifest`; resource `AssetStatus` (`manifest: ManifestState`, `requested`, `loading`, `ready`, `failed` counts, `failed_list: Vec<AssetFailure { guid, path, message }>`); events `AssetLoaded { guid, kind }`, `AssetFailed { guid, kind, message }`. `Name`, `SceneMember` (`derived`), `SceneId`, resource `Scenes`; events `LoadScene`, `SceneLoaded`, `SceneLoadFailed` (FA3). `AudioSettings`, `AudioStats`, `Bus`, events `PlayAudio`, `StopAllAudio`, `PlayMusic`, `StopMusic`. Inspector fields: `Name` (text), `ImageSettings` (enum and two checkboxes), `AudioSettings` (four sliders). Components that hold a `Guid` asset reference (`Sprite.image`, `Text.font`, UI images) show an **asset picker**: the field carries `custom(widget = "asset", kind = "image")` (or `"font"`), which render and UI add to their derives; the picker lists `asset_list` for that kind. **#110 builds the picker generically**: it reads the field's `custom("widget")` and `custom("kind")` from `TypeInfo`, shows the registered assets of that kind (with a thumbnail for images), and writes the chosen `Guid` through `SetField`; a field of type `Guid` without the widget stays a text box. The attribute is added by the issues RA1 (render) and UA1 (ui), see "Downstream adjustments".

**Commands and tools #110 builds.** Names follow `<domain>_<verb>`. "Undo" says what the bus records.

| Capability | Plain Rust function (this spec) | Tool | Kind and undo |
|---|---|---|---|
| List assets with state, error, kind, settings, and optionally the unregistered files | `AssetRegistry::iter`, `AssetServer::state`, `native::scan` | `asset_list` (filters: kind, state, `include_unregistered`) | read |
| One asset in detail (info, state, error, decoded size and duration, which scenes reference it) | `AssetRegistry::get`, `edit::find_references` | `asset_get` | read |
| Import a file (copy into `assets/`, or register in place when it is already inside), set kind and settings, downscale an image to `max_dimension` (default 4096) | `edit::import_asset` | `asset_import` | command. Undo removes the registration and deletes the copy it made. |
| Register a file that is already under `assets/` | `edit::register_asset` | `asset_register` | command. Undo unregisters. |
| Change image settings | `edit::set_image_settings` | `asset_set_settings` | command. Undo restores the old value. |
| Rename or move (the `Guid` stays, so no scene changes) | `edit::rename_asset` | `asset_rename` | command. Undo renames back. |
| Remove (the file goes to `<root>/.trash/`); the result lists the scenes that still reference the `Guid` | `edit::remove_asset`, `edit::find_references` | `asset_remove` | command. Undo moves the file back and re-registers. |
| New, open (load), save, save as; list; validate text | `Scenes`, `write_scene`, `ParsedScene`, `validate_scene`, `despawn_scene` | `scene_new`, `scene_load` (`mode`, `parent`), `scene_save` (`path?`, `scope?`), `scene_list`, `scene_validate` (`text` or `path`) | `scene_load` and `scene_new` are commands: additive undo despawns the instance; replace keeps the previous scene's text and undo reloads it. `scene_save` writes a file, not the world, and has no undo; its result carries the `SceneWarning` list (unsaved components, out-of-scope parents), so an agent that forgot `#[reflect(Component)]` is told. `scene_list`, `scene_validate` are read-only. |
| Create an entity for authoring | the generic spawn command of #110 plus `mark_member(world, entity, active_scene)` | the entity tools of #110 | command. **Must call `mark_member`**, or the entity is not saved. |
| Play a sound or music, stop, set volume | events and the `AudioSettings` resource | `audio_play`, `audio_play_music`, `audio_stop`, `audio_set_volume`, `audio_status` | `audio_play`, `audio_stop` are runtime actions with no undo (the bus must let a command declare "no undo", as physics-2d.md already requires). `audio_set_volume` is a resource edit with undo. `audio_status` reads `AudioStats`. The generic `ecs_send_event`, `ecs_set_resource_field` and `ecs_get_resource` already cover all of it; the named tools exist for discovery. |
| Create a project, export | the CLI (`exitium new`, `exitium export --json`, `--check`) | `project_new`, `project_export_check`, `project_export` (`target`, `out?`) | ops tools: start the `exitium` binary as a process and return its JSON (no `exitium_mcp` to `exitium_cli` edge, jam-mvp.md Stage 4). Export is long: the tool reports progress lines and the final JSON. No undo. |

Parity on the editor side: an asset browser panel (list, import path field, settings inspector, rename, remove, a play button for audio), a scene menu (new, open, save, save as), an audio panel (the four sliders and stop all) and an Export menu that runs the same CLI. Each is a front end of the tools above.

## Performance

All numbers are initial targets on the min-spec reference runner (4 cores, 8 GB), native release, single thread unless stated. Benchmarks use criterion, IDs `<crate>/<area>/<case>/<size>` (docs/benchmarks.md), held by the 5% gate once baselined; the first benchmark PR may re-baseline with a note to the owner. Chrome numbers are recorded by the examples and are not CI-gated.

| Operation | Budget |
|---|---|
| `decode_png`, 2048 x 2048 RGBA (16 MiB), `exitium_assets/image/decode_png/2048` | <= 120 ms native; <= 400 ms Chrome |
| `edge_bleed`, 2048 x 2048, 30% transparent | <= 30 ms |
| `decode_wav`, 60 s stereo 44.1 kHz 16 bit, `exitium_assets/audio/decode_wav/60s` | <= 60 ms |
| `decode_ogg`, 60 s stereo 44.1 kHz, `exitium_assets/audio/decode_ogg/60s` | <= 1.0 s native; <= 3 s Chrome |
| Font validation, 500 KB | <= 2 ms |
| `AssetServer::poll`, nothing pending | <= 300 ns |
| `poll` handing out one payload | <= 3 µs (the payload moves, never copies) |
| Registry lookup by `Guid` / by path / `mint` | <= 50 ns / <= 80 ns / <= 200 ns |
| Manifest, 5,000 entries (about 600 KB): read and bind / write | <= 25 ms / <= 15 ms |
| Native load latency, 64 KB file, request to ready, excluding decode | <= 3 ms |
| Memory: server idle | <= 64 KB plus 120 B per registered asset |
| Memory: peak per load | image: compressed bytes plus 1x decoded; audio: compressed bytes plus 2x decoded (one conversion) |
| Web: concurrent fetches | at most `max_fetches` = 6 |
| Scene, 10,000 entities x 5 components (about 3 MB), `exitium_scene/parse/10000` read and bind | <= 100 ms (serial's budget) |
| Scene `instantiate`, 10,000 entities x 5 components, `exitium_scene/instantiate/10000` | <= 50 ms (<= 5 µs per entity) |
| Scene load, parse to instantiate | <= 150 ms native; <= 300 ms Chrome |
| Scene `write_scene`, 10,000 entities, `exitium_scene/write/10000` | <= 100 ms |
| `validate_scene`, 10,000 entities | <= 200 ms |
| Facade request scan (FA1), 20,000 unchanged sprites and 100 changed, `exitium/glue/request_scan/20000` | <= 1 µs unchanged; <= 150 ns per changed sprite |
| Audio `play` | <= 3 µs |
| Audio mix, 32 voices x 1,024 frames (`AudioOutput::Null`), `exitium_audio/mix/32voices` | <= 0.4 ms (about 2% of one core at 48 kHz); on the web it runs on the main thread, so <= 1 ms per chunk in Chrome |
| `insert_clip`, 1 Mi frames, `exitium_audio/insert_clip/1m_frames` | <= 12 ms |
| Audio clip memory | 8 bytes per frame (a 60 s stereo 44.1 kHz clip is about 21 MiB) |
| `exitium new` | <= 200 ms |
| Export packaging (stage and zip) of 1,000 files, 100 MB | <= 4 s, byte-identical on a second run |
| Cold export end to end | <= 10 min Windows, <= 15 min web on 4 cores (jam-mvp.md); `wasm-bindgen` <= 20 s, `wasm-opt` <= 60 s |

Complexity: scene parse and write are O(source bytes); instantiate is O(components); label lookup is a hash map; registry operations are O(log n) (path index) or O(1) (guid). Nothing is O(n²).

## Test plan

- **Paths and ids.** Every `AssetPath` rule, accept and reject (non-ASCII, `..`, leading dot, 201 bytes, case-only clash, the reserved name); `sanitize` table; the extension table; `mint` golden vectors, version and variant bits, collision bump, rename keeps the `Guid`.
- **Decoders.** Generated PNGs (every colour type and bit depth, interlaced, tRNS, 1 x 1, 8192 x 1 and 8193 x 1 limit cases, truncated and corrupt files); edge bleed (colour only under alpha 0, alpha untouched, opaque image unchanged, 8-pixel reach); `encode_png` then `decode_png` equals the input. WAV through `hound`'s writer for every depth and float, mono duplicated, 3 channels unsupported, over-limit; an Ogg Vorbis fixture (a small CC0 file with its licence beside it) decodes to the expected frame count and sample rate; font fixture (an OFL face under 100 KB with its licence); bad bytes for each kind give `Decode` and never panic. A truncation sweep and a seeded random-bytes sweep for every decoder (std xorshift, as in serial).
- **Server.** With `MemorySource`: state machine for every transition; `request` is idempotent; `retry`; `forget`; delivery exactly once per payload; `take_loaded` by kind; back-pressure pauses workers above `max_undelivered_bytes`; 1,000 requests across 2 workers all complete; `Drop` joins the workers; `auto_register`; `NotRegistered`. A web test module compiles for wasm32 (run by hand in Chrome).
- **Manifest.** `AssetPath` reflects as a validated `String` Value: a manifest with `path: "../x.png"` reads as `InvalidValue` at the literal and an `apply` of a bad string leaves the value unchanged. Round trip and fixed point (`write(read(write(x))) == write(x)`); sorted output; duplicate guid, duplicate path, case clash and kind-mismatch each reported together; unknown field warns; a golden file.
- **Edit API.** Import copies and registers; import in place; import an image over 4096 downscales and the manifest keeps the `Guid`; every `AssetEdit::undo`; remove then undo restores file and entry; `find_references` finds a `Guid` in a scene and a `.ui` file and in no other file.
- **Scene.** Round trips in a world with test components (derive-free ones plus `Parent`, `Name`, an `Option<Entity>`, a `Vec<Entity>`, a map keyed by `Entity`, a `Guid` field): write, read into a fresh world, write again, the two texts are identical, with the entity ids differing between loads; every envelope error (missing `id`, duplicate label, unknown label with a suggestion, parent cycle, duplicate component, unknown component type with suggestions); `ReferenceOutsideScene`; the unreflected-component warning, `strict` and `ignore_unreflected`, and the out-of-scope parent rule; **atomic failure**: a scene whose last component fails leaves the world with the same entity count and no `SceneMember`; forward reference; `parent` option attaches roots; sibling order preserved; skipped components (`derived`, `Children`, no `ReflectComponent`); path convenience on and off; `asset_refs` deduplicated; comments are regenerated and ignored on read; CRLF input; a `.scene` for every golden. Entities that are not members are never written. The real plugin components (`Transform2d`, `Sprite`) round-trip in a facade test (jam-mvp.md Stage 2), not here, since `exitium_scene` has no edge to them (C1).
- **Audio.** With `AudioOutput::Null`: play and the voice limit; stop and `is_playing`; bus volumes (dB conversion, `0` is silence); music crossfade and "same clip is a no-op"; **locked behaviour**: SFX dropped and counted, music remembered and started by `unlock`; `Unavailable` makes every call a no-op; `remove_clip` stops its voices; sample-rate conversion is kira's, so a test only checks the frame count and duration; an invalid clip is an error. Examples are not tests.
- **CLI.** `new`: file list and contents, name validation, overwrite refusal, the three engine-reference modes, and (toolchain present) `cargo check` and the wasm32 check of the output. `export`: the plan (command lists) with a fake `cargo`, `wasm-bindgen` and `wasm-opt` for both targets; a missing manifest file is reported in one list; the zip is deterministic and has `index.html` at the root; backslash paths never appear; the `wasm-bindgen` version check (equal, different, missing); `WASM_BINDGEN_VERSION` equals the workspace lock and the template; `--json` output is checked by a small test-only JSON validator; `serve` serves a file with the right MIME type and refuses non-loopback binds.
- **Examples (manual, recorded in the PR).** Assets example on Windows and in Chrome: loads a PNG, WAV, OGG and font, prints states and decoded sizes, a failing file shows `Failed`. Audio example on Windows and in Chrome: a tone, a music loop with a crossfade, volume keys, and in Chrome nothing sounds until the first click.
- **Platform.** `cargo build --workspace --target wasm32-unknown-unknown` passes for the three runtime crates; `--no-default-features` builds of assets and audio pass (Stage 1 shape).

## Dependencies

All versions were checked on crates.io on 2026-11-18. A scratch crate outside the repo (`kira` with `default-features = false, features = ["cpal"]`, `png`, `hound`, `lewton`, `ttf-parser` with `std`, the wasm-only crates below, and `zip` on native) passed `cargo check` for the host and for `wasm32-unknown-unknown`, using the exact APIs this spec names (`png::Decoder`, `hound::WavReader`, `lewton::inside_ogg::OggStreamReader`, `ttf_parser::Face::parse`, kira's `MockBackend`, sub-tracks and `StaticSoundData { frames: Arc<[Frame]>, .. }`).

| Crate | Version | Features | Used by | Licence | wasm32 | Why not std |
|---|---|---|---|---|---|---|
| `kira` | 0.12.6 | `default-features = false`, `["cpal"]` | `exitium_audio` | MIT OR Apache-2.0 | yes (cpal WebAudio, its own wasm-only `send_wrapper`) | Mixing, resampling, tracks, tweens and a lock-free command queue are a large library. No symphonia: the default features pull in `symphonia`, which is MPL-2.0. |
| `png` | 0.18.1 | default | `exitium_assets` | MIT OR Apache-2.0 | yes | PNG decode and encode (inflate, filters, Adam7). Chosen over `image` (0.25.10): `image` adds format abstraction, `moxcms` and more for formats the MVP does not read. Its `png` feature is this crate. |
| `hound` | 3.5.1 | default | `exitium_assets` | Apache-2.0 | yes | WAV parsing. Last release 2023. |
| `lewton` | 0.10.2 | default | `exitium_assets` | MIT OR Apache-2.0 | yes | Ogg Vorbis decode; brings `ogg` 0.8 (BSD-3-Clause), `byteorder`, `tinyvec`. Last release January 2021 with about 10 million downloads: maintenance risk, accepted for the MVP, and the decoder is behind one function so it can be replaced. |
| `ttf-parser` | 0.25.1 | `default-features = false`, `["std"]` | `exitium_assets` | MIT OR Apache-2.0 | yes | Font validation without the layout tables. `cosmic-text` (render's text stack) also uses it, so the lock may gain no package; intake checks. |
| `wasm-bindgen-futures` | 0.4.80 | default | `exitium_assets`, wasm32 | MIT OR Apache-2.0 | wasm only | `spawn_local` and `JsFuture` for `fetch`; std has no executor. Declared once in `[workspace.dependencies]` (render lists the same entry). |
| `js-sys` | 0.3.107 | default | `exitium_assets`, wasm32 | MIT OR Apache-2.0 | wasm only | `Uint8Array` and `Promise`. Already in the wasm tree. |
| `web-sys` | 0.3.107 | the workspace entry plus `Window`, `Response`, `Request`, `RequestInit`, `RequestMode`, `Headers` for assets (a superset, so the fetch code never needs a manifest edit); `Window`, `Document`, `Event`, `EventTarget`, `console` for the audio and assets examples | `exitium_assets`, `exitium_audio` (dev) | MIT OR Apache-2.0 | wasm only | `fetch` and the gesture listener in the example. Features are additive, so the workspace entry from app-platform.md is reused with `features = [..]` added per crate. |
| `web-time` | 1.1.0 | the workspace entry of app-platform.md | `exitium_assets`, wasm32 | MIT OR Apache-2.0 | yes | The decode budget measures time; `std::time::Instant::now()` panics on `wasm32-unknown-unknown`. |
| `wasm-bindgen` | 0.2.130 | the workspace entry | `exitium_assets`, `exitium_audio` (dev) | MIT OR Apache-2.0 | wasm only | Already declared by app-platform.md. |
| `zip` | 9.7.0 | `default-features = false`, `["deflate-flate2"]` | `exitium_cli` | MIT | native only (CLI) | Deterministic zip with forward-slash entries. Brings `flate2` with `miniz_oxide` (pure Rust), `crc32fast`, `indexmap` and others. The default features (aes, bzip2, zstd, xz, time) are not wanted. |

Dev-dependencies: `criterion` (workspace) for assets, audio and scene. `exitium_cli` also needs no test crate (a fake-tool shim is a few lines).

External tools (not Cargo dependencies): `wasm-bindgen-cli` 0.2.130 and optional `wasm-opt`, see "Export tools".

### Licence decision (owner, decided)

`kira` depends on **`triple_buffer` 9.0.1, licensed MPL-2.0** (not optional; it is in the tree for every kira feature set). `cargo deny` as configured in `deny.toml` would fail on it, and ADR-0003 says "copyleft stays banned". A check of the full scratch tree with `cargo metadata` against the `deny.toml` allow list found exactly this one non-permissive licence (`symphonia` and its codecs, also MPL-2.0, are avoided by the feature choice above). MPL-2.0 is file-level weak copyleft: linking it into an MIT program is permitted; the obligation is to keep the MPL-covered source files available and to publish changes to them. We do not modify the crate and it is available on crates.io.

**Decision (owner):** allow a narrow exception for `triple_buffer` only: MPL-2.0, unmodified, used through kira. It lands in the **intake 1 PR** as (a) a `[[licenses.exceptions]]` entry in `deny.toml` with `crate = "triple_buffer"` and `allow = ["MPL-2.0"]`, which allows no other crate and no other licence, and (b) a new ADR in `docs/decisions/` that records why and amends ADR-0003's "copyleft stays banned" for this one crate. The ADR states the conditions: the crate stays unmodified, it is reached only through kira, a second MPL-2.0 (or other copyleft) dependency needs its own decision, and shipped games carry a licence notice (open question 2). **Alternative considered:** drop kira and write a small mixer on `cpal` (Apache-2.0, about 400 lines); rejected because it costs code the engine would have to maintain, and `exitium_audio`'s API exposes no kira type, so it stays possible later.

## Wiring for the intake PRs

Everything the implementation issues need from `Cargo.toml`, `Cargo.lock`, `deny.toml` and `xtask/` (C5). The intake PRs add the union of every merged spec's list.

**Intake 1 (Stage 1).**

- Workspace edges: `exitium_assets -> exitium_core` and `exitium_audio -> exitium_core`. Nothing else; in particular none to `exitium_serial`, `exitium_reflect`, `exitium_ecs`, `exitium_app` or another runtime crate in Stage 1.
- `[workspace.dependencies]` (each with a one-line comment: licence, wasm, why not std): `kira`, `png`, `hound`, `lewton`, `ttf-parser`, `wasm-bindgen-futures`, `js-sys` (from the table). `web-sys` and `wasm-bindgen` entries already come from app-platform.md.
- `crates/exitium_assets/Cargo.toml`: `[dependencies] exitium_core, png, hound, lewton, ttf-parser`; `[target.'cfg(target_arch = "wasm32")'.dependencies] wasm-bindgen, wasm-bindgen-futures, js-sys, web-sys (features Window, Response, Request, RequestInit, RequestMode, Headers), web-time`; `[dev-dependencies] criterion, and on wasm32 web-sys (feature console)`; `[[bench]]` stanzas are added by the first issue that adds a benchmark. `web-time` is the workspace entry (1.1.0) the app-platform spec declares.
- `crates/exitium_audio/Cargo.toml`: `[dependencies] exitium_core, kira`; `[dev-dependencies] criterion`; `[target.'cfg(target_arch = "wasm32")'.dev-dependencies] wasm-bindgen, web-sys (features Window, Document, Event, EventTarget, console)`. The audio example does not use `exitium_platform` (C1); on Windows it needs no window.
- `deny.toml`: the `triple_buffer`-only `[[licenses.exceptions]]` entry ("Licence decision"). Intake runs `cargo deny check` and records the result.
- `xtask`: the wasm step also builds `--examples` for `exitium_assets` and `exitium_audio` (proposed, optional).
- `docs/decisions/`: the new ADR for the licence exception ("Licence decision"), numbered at filing.
- **Intake 1's `## Footprint` lists `Hotspots: Cargo.toml, Cargo.lock, deny.toml, docs/decisions/`.** jam-mvp.md's intake 1 block names the first three; this spec adds `docs/decisions/`, so the ADR number cannot collide with another PR. Both are held by intake 1 because it carries the exception and its ADR.

**Intake 2 (Stage 2 to 4).**

- Workspace edges, with the optional-dependency and feature shape the platform crate uses (app-platform.md): 
  - `exitium_assets`: optional `exitium_reflect`, `exitium_serial`, `exitium_ecs`, `exitium_app`; `[features] manifest = ["dep:exitium_reflect", "dep:exitium_serial"]`, `plugin = ["manifest", "dep:exitium_ecs", "dep:exitium_app"]`, `default = ["plugin"]`. Stage 1 style use is `default-features = false`.
  - `exitium_audio`: optional `exitium_reflect`, `exitium_ecs`, `exitium_app`; `plugin = [..]`, `default = ["plugin"]`. **Dev-dependency `exitium_serial`** (AUP1's text round trip of the reflected types; a dev-dependency counts for R3, as in ui-mvp.md).
  - `exitium_scene -> exitium_core, exitium_ecs, exitium_reflect, exitium_serial` (normal). **No `exitium_app`** (D7). Dev-dependency: `criterion` only (its test components use the derives re-exported by `exitium_ecs` and `exitium_reflect`).
  - `exitium_cli -> exitium_serial` (normal), and `zip`. No dev-dependency. No `exitium_assets`, `exitium_reflect` or `exitium_scene`: the CLI reads the manifest and the project file through the syntax tree.
  - Facade: `exitium -> exitium_assets, exitium_audio, exitium_scene`.
  - **`exitium_render`: dev-dependency `exitium_serial`** (RA1's text-binding test of `Sprite`, `Camera2d` and `Text`; render-v1.md's wiring says "no further dev-dependencies" because it predates RA1; a dev-dependency counts for R3, as for audio and in ui-mvp.md). `exitium_ui` already has `exitium_serial` as a dev-dependency (ui-mvp.md) for UA1. Both manifests and `Cargo.lock` change in intake 2, so RA1 and UA1 edit no manifest.
- `crates/exitium_reflect_derive/`: add `exitium_assets`, `exitium_audio` and `exitium_scene` to `ENGINE_CRATES` (each depends on `exitium_reflect` directly and derives `Reflect`; the `crate_paths.rs` drift test fails until they are classified) and `exitium_cli` to `NOT_DERIVE_USERS` (if the lane has not already). Intake 2 lists the derive crate under `Edits` for this.
- `xtask`: `--no-default-features` build checks for `exitium_assets` and `exitium_audio` (or a PR note if `FEATURE_CHECKS` cannot express it).
- No `NATIVE_ONLY` change (`exitium_cli` is already listed).
- **Not added, on purpose:** an edge among `exitium_assets`, `exitium_audio`, `exitium_scene`, `exitium_render`, `exitium_ui`, `exitium_physics`, `exitium_platform` (C1); `exitium_scene -> exitium_app`; `exitium_cli -> exitium_assets`; `clap`, `toml`, `serde_json`, `image`, `symphonia`, `rodio`, `getrandom`, `rand`.

## Open questions

2. **Exported-game licence notices.** MPL-2.0 and the MIT and Apache licences of every dependency need a notice file in a shipped game. This spec does not generate one (it would need `cargo metadata` JSON or `cargo about`). Recommendation: a follow-up issue before the first public export, owner's call on the wording.
3. **More formats.** JPEG and WebP (an agent's generated art often arrives as one of them) and MP3. Adding `image` features or symphonia is a dependency and licence decision. Until then import fails with "convert to PNG" and names the format found.
4. **Web audio unlock.** The design creates the device inside the frame after the first gesture, relying on sticky user activation. If Chrome leaves the context suspended, the fallback is to call `unlock` from a DOM listener registered by the audio example and by the facade (a `wasm-bindgen` closure on `pointerdown` and `keydown`), which makes the creation happen in the event handler itself. The audio example decides this with a real run.
5. **itch.io and WebGPU.** itch.io embeds a game in an iframe. Whether WebGPU is available there without extra permissions is not verified. The rehearsal (Stage 4) checks it; if it needs an `allow` attribute, the itch page setting is the owner's, and a fallback is the "open in a new tab" link.
6. **Root order in a saved scene.** Roots are written in entity index order, which is not creation order once indices are reused. An explicit `SceneOrder` component or a stored sibling index would make saves exactly stable. Recommendation: wait for a real diff-noise complaint.
7. **Incremental scene load.** A very large scene blocks a frame (150 ms at 10,000 entities). An `instantiate_step` that works through the entity table across frames is additive. Not in the MVP.
8. **Asset unloading and streaming music.** Unloading is manual and music is fully decoded (at most about 6 minutes). Reference counting, an LRU and kira's `StreamingSoundData` through a custom reader are the path; both need the data model to say who owns a clip. Not in the MVP.
9. **`AssetRef<T>`.** Typed references would help the inspector and catch a font in an image field. They need an `exitium_core` change (core is frozen) or an edge, so they wait for a core lane boundary. A `transparent` wrapper over `Guid` reads and writes the same text, so adding it later changes no file.
10. **Asset root layout for big games.** One flat manifest of 5,000 entries is fine for the MVP. A pack file (fewer web requests, one cache entry) and per-folder manifests are later work; the manifest is the only index, so a pack can sit behind `AssetSource`.
11. **Default font.** Still render-v1.md open question 7. The export copies assets only, so a game that needs a font imports one.
12. **`wasm-opt` flags** and itch.io's current file-count and size limits are verified by the implementer on a real export, not assumed here.

## Implementation issues

Each issue below is PR-sized (about 600 changed lines or fewer), edits exactly one crate (the facade issues edit none and hold the `crates/exitium/` hotspot), has a labelled Footprint, acceptance criteria and `Blocked by` lines, and goes into the Jam MVP milestone. The architect files them with `file-issue` after this spec merges, labels `area:assets`, `area:audio`, `area:scene`, `area:cli` and `area:facade`, and `area:render` for RA1 and `area:ui` for UA1. `<intake-1>` and `<intake-2>` are the intake issues' numbers, filled in at filing. Stage 1 issues are `Blocked by <intake-1>` and Stage 2 to 4 issues by `<intake-2>` (C6). One crate has at most one issue in flight, so each lane runs in the order shown.

Every issue states its Footprint in the full labelled form (`Edits`, `Governs`, `Hotspots`, `New crate edges`), and at filing it becomes the `## Footprint` block of the `factory-run` skill, line for line. Lane headings repeat the form for the lane. The facade issues carry `Edits: none` and `Hotspots: crates/exitium/` (their manifest edges come from intake 2). Each issue updates its crate's `CLAUDE.md` (A1, AU1, SC1 and C1 create them), adds the benchmarks it names, and states the high-risk areas it touches in the PR Risk section.

### Stage 1: assets lane (`exitium_assets`, Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none)

**A1. `feat(assets): asset paths, kinds, errors and the registry`** (about 450 lines). Blocked by `<intake-1>`.
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds `AssetPath` and its rules, `sanitize`, `AssetKind` and the extension table, `AssetError`, `AssetInfo`, `ImageSettings`, `AssetRegistry`, Guid minting; creates `CLAUDE.md`.
- Acceptance criteria:
  - [ ] Every path rule has an accept and a reject test, including the case-only clash and the reserved name.
  - [ ] `mint` golden vectors, UUID version and variant bits, collision bump; `rename` keeps the `Guid`.
  - [ ] `exitium_assets/registry/*` benchmarks within budget; the wasm32 build passes; no `unsafe`.

**A2. `feat(assets): PNG decode, edge bleed and PNG encode`** (about 500 lines). Blocked by `<intake-1>`, A1.
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds `image.rs`: `decode_png` into one `Vec<u8>`, all colour types and depths, limits, `edge_bleed`, `encode_png`.
- Acceptance criteria:
  - [ ] The "Decoders" test group for images, including truncation and random-bytes sweeps with no panic.
  - [ ] `decode_png/2048` and `edge_bleed/2048` meet the budget.
  - [ ] An image over `max_image_dim` is `TooLarge` with the downscale hint.

**A3. `feat(assets): WAV, Ogg Vorbis, font and text decoding`** (about 450 lines). Blocked by `<intake-1>`, A2.
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds `audio.rs` and `font.rs`: `decode_wav`, `decode_ogg`, `validate_font`, `decode_text`, limits, the small CC0 Ogg fixture and the OFL font fixture with licence files.
- Acceptance criteria:
  - [ ] Every depth and channel case, mono duplication, `Unsupported` and `TooLarge` cases, bad bytes for every decoder.
  - [ ] `decode_wav/60s` and `decode_ogg/60s` meet the budget.
  - [ ] Fixture licence files are present; the PR body lists them under Risk (licensing).

**A4. `feat(assets): sources, the AssetServer and native worker threads`** (about 600 lines). Blocked by `<intake-1>`, A3.
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds `source.rs` and `server.rs`: `AssetSource`, `FsSource`, `MemorySource`, `AssetConfig` with `discover`, `AssetServer`, the state machine, `take_loaded`, back-pressure, stats.
- Acceptance criteria:
  - [ ] The "Server" test group with `MemorySource`, including exactly-once delivery, `Drop` joining the workers and back-pressure.
  - [ ] `poll` idle and `poll` with one payload meet the budget; `FsSource` refuses a file over `max_file_bytes` before reading it.
  - [ ] The server is `Send + Sync` (a compile-time assertion that also builds for wasm32).

**A5. `feat(assets): web fetch source and budgeted decode`** (about 400 lines). Blocked by `<intake-1>`, A4.
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds `web.rs` (wasm32): `fetch`, `max_fetches`, the `Mutex` queue, `poll` under `decode_budget_ms`, `Http` and `Network` errors, URL building.
- Acceptance criteria:
  - [ ] `cargo build --target wasm32-unknown-unknown` passes and the module has pure-logic unit tests that run on the host (URL building, error mapping, the budget loop with an injected clock).
  - [ ] The server type is shared with A4 (one public API on both targets).

**A6. `feat(assets): native scan, copy-in, downscale, atomic write and trash`** (about 450 lines). Blocked by `<intake-1>`, A5.
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds `native.rs` (not wasm32) as described under "Native-only helpers".
- Acceptance criteria:
  - [ ] `scan` skips illegal names and `.trash`; `write_atomic` leaves no partial file when interrupted (a test kills the write at the rename step by a failing closure); `downscale_png` by an integer factor is exact on a checker image.
  - [ ] The module is gated out of the wasm32 build, which still passes.

**A7. `feat(assets): assets_demo example for Windows and Chrome`** (about 300 lines). Blocked by `<intake-1>`, A6.
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds `examples/assets_demo.rs`, `examples/web/index.html` and the fixtures: requests a PNG, a WAV, an OGG, a font and one missing file with `auto_register`, polls each frame, prints states, decoded sizes and the `Failed` error (console on Windows, `console.log` in Chrome). The header comment gives the Windows command and the Chrome recipe with `wasm-bindgen-cli` at the locked version (`cargo install wasm-bindgen-cli --version 0.2.130 --locked`).
- Acceptance criteria:
  - [ ] The PR body records a run on Windows and in Chrome, including the failed file.
  - [ ] `cargo build --example assets_demo` and the same with `--target wasm32-unknown-unknown` pass. The example uses no other runtime crate (C1).

### Stage 2: assets lane (feature `manifest` and `plugin`)

**AP1. `feat(assets): the manifest on reflection and the text format`** (about 450 lines). Blocked by `<intake-2>`, A7.
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds `manifest.rs`: `#[cfg_attr(feature = "manifest", derive(Reflect))]` on the registry types, `Manifest`, read and write on `exitium_serial`, validation, sorted output, `exitium_assets/manifest/*` benchmarks.
- Acceptance criteria:
  - [ ] The "Manifest" test group and a golden file; an empty and a missing manifest.
  - [ ] Read and write meet the 5,000-entry budget; the crate builds with `--no-default-features` and with `manifest` only.

**AP2. `feat(assets): AssetPlugin, status and events`** (about 500 lines). Blocked by `<intake-2>`, AP1, app-platform.md A1 (`exitium_app` Plugin, schedules and sets).
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds `plugin/`: `AssetPlugin` (inserts `AssetServer`, loads the manifest in `Startup`: synchronously on native, by fetch on the web with `ManifestState`), set `ASSETS` in `First` after `INPUT`, the `poll_assets` system, reflected `AssetStatus`, events `AssetLoaded` and `AssetFailed`, the untaken-payload warning.
- Acceptance criteria:
  - [ ] A headless-app test loads a manifest and an asset from `MemorySource`, sees the events and the status counts, and a missing manifest on native gives an empty registry.
  - [ ] Idle `poll_assets` costs at most 300 ns; the reflected types register and round-trip through the text format.

**AP3. `feat(assets): edit API (import, register, rename, remove, settings, references)`** (about 550 lines). Blocked by `<intake-2>`, AP2.
- Footprint: Edits `crates/exitium_assets/`; Governs none; Hotspots none; New crate edges none.
- Adds `edit.rs` as in the MCP table: the functions, `AssetEdit` undo records, `find_references`, `save_manifest` (atomic).
- Acceptance criteria:
  - [ ] The "Edit API" test group, including every undo and the downscale at import.
  - [ ] `find_references` reads only `Scene` and `Text` assets and is bounded by `max_text_bytes`.

### Stage 1: audio lane (`exitium_audio`, Footprint: Edits `crates/exitium_audio/`; Governs none; Hotspots none; New crate edges none)

**AU1. `feat(audio): clips, buses and the AudioEngine over kira`** (about 550 lines). Blocked by `<intake-1>` (which carries the `triple_buffer` exception and its ADR).
- Footprint: Edits `crates/exitium_audio/`; Governs none; Hotspots none; New crate edges none.
- Adds `clip.rs`, `engine.rs`: `ClipData`, `insert_clip`, buses, `play`, `stop`, voice limit, state and `Unavailable`, `AudioOutput::Null` and `advance_null`; creates `CLAUDE.md`.
- Acceptance criteria:
  - [ ] The play, stop, bus-volume and `Unavailable` tests run with `AudioOutput::Null` and no sound card; `advance_null` shows non-zero peak for a played clip and zero after it stops.
  - [ ] **The voice limit is global**: with `max_voices = 4`, a fifth `play` fails whichever buses the first four used, and `AudioStats.voices` equals the number of live voices (a test mixes the three buses).
  - [ ] No kira type is public (a doc test of the public API names); the wasm32 build passes.

**AU2. `feat(audio): music slot, crossfade, the web unlock and stats`** (about 450 lines). Blocked by `<intake-1>`, AU1.
- Footprint: Edits `crates/exitium_audio/`; Governs none; Hotspots none; New crate edges none.
- Adds `play_music`, `stop_music`, `unlock`, pending music while `Locked`, `update`, `AudioStats`.
- Acceptance criteria:
  - [ ] The crossfade, same-clip, locked and unlock tests (`AudioConfig { output: Null, start_locked: Some(true) }` simulates the web's `Locked` start on the host).
  - [ ] `play` meets 3 µs.

**AU3. `feat(audio): benchmarks and audio_demo example for Windows and Chrome`** (about 400 lines). Blocked by `<intake-1>`, AU2.
- Footprint: Edits `crates/exitium_audio/`; Governs none; Hotspots none; New crate edges none.
- Adds `benches/audio.rs` (`mix/32voices`, `insert_clip/1m_frames`, `play`), `examples/audio_demo.rs` and `examples/web/index.html`: a generated tone, a music loop with crossfade, volume keys; on the web a `pointerdown` and `keydown` listener (web-sys, no `exitium_platform`) calls `unlock`.
- Acceptance criteria:
  - [ ] The PR body records a run on Windows and in Chrome, stating that nothing sounds before the first click or key and sound starts after it.
  - [ ] The benchmarks meet their budgets; both example builds pass.

### Stage 2: audio lane

**AUP1. `feat(audio): AudioPlugin, ClipStore, settings and events`** (about 500 lines). Blocked by `<intake-2>`, AU3, app-platform.md A1.
- Footprint: Edits `crates/exitium_audio/`; Governs none; Hotspots none; New crate edges none.
- Adds `plugin/` as in "Plugin": `Audio`, `ClipStore`, `AudioSettings`, `AudioStats`, the four events, the three systems, `play_guid`.
- Acceptance criteria:
  - [ ] A headless-app test (`AudioOutput::Null`) plays through each event, changes a volume through `AudioSettings`, and reads `AudioStats`; the reflected types round-trip through the text format (`exitium_serial` is a dev-dependency of this crate, added by intake 2).
  - [ ] `--no-default-features` builds the Stage 1 shape.

### Stage 2: scene lane (`exitium_scene`, Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none)

The scene crate does not depend on `exitium_app`, so SC1 may start right after intake 2, beside the `exitium_app` spine (R3 passes: no edge between them).

**SC1. `feat(scene): Name, SceneMember, Scenes and type registration`** (about 350 lines). Blocked by `<intake-2>`.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `components.rs`, `register_types`, `mark_member`, label derivation (`label.rs`); creates `CLAUDE.md`.
- Acceptance criteria:
  - [ ] Label derivation table tests (collapse, empty, collisions, unnamed `@e<n>`).
  - [ ] The reflected types register and round-trip in a world built from `exitium_ecs` and `exitium_reflect` only.

**SC2. `feat(scene): write_scene`** (about 550 lines). Blocked by `<intake-2>`, SC1.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `write.rs`, `refs.rs` (write side of `SceneRefs`), `error.rs`: member collection, pre-order, silent and reported skip rules, `SavedScene` with `SceneWarning`, `strict` and `ignore_unreflected`, component order, labels, `ReferenceOutsideScene`, out-of-scope parents, asset path comments, re-indent.
- Acceptance criteria:
  - [ ] Golden files for a small scene, a hierarchy, a map keyed by `Entity` and an asset comment; the goldens equal the examples in this spec for the same components; non-members never written; derived components absent.
  - [ ] A component without `ReflectComponent` gives one `UnsavedComponent` warning per type with the entity count, an error under `strict`, and nothing when listed in `ignore_unreflected`; a root whose parent is not a member is written `parent: None` and warns in `SaveScope::Scene` only.
  - [ ] Output is identical across two writes and across entity-id churn; `exitium_scene/write/10000` meets budget.

**SC3. `feat(scene): ParsedScene (syntax, envelope and label checks)`** (about 450 lines). Blocked by `<intake-2>`, SC2.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `parse.rs`: `ParsedScene::parse`, envelope shape, declared and used labels, parent cycles, error construction with spans.
- Acceptance criteria:
  - [ ] Every envelope error of the Test plan with its span and, for `UnknownLabel`, up to 3 suggestions.
  - [ ] A file in every accepted shape (defaults omitted, CRLF, comments) parses; `exitium_scene/parse/10000` syntax-only meets budget.

**SC4. `feat(scene): instantiate into a world (atomic, hierarchy, asset refs)`** (about 550 lines). Blocked by `<intake-2>`, SC3, and the Stage 1 hierarchy helpers (#49), `World::run_system_once` and `Commands` (#45, the scheduler issues) and `ReflectComponent` (#50).
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `instantiate.rs` and the read side of `SceneRefs`: the load algorithm, `SceneInstance`, `InstantiateOptions`, `despawn_scene`, the path convenience.
- Acceptance criteria:
  - [ ] The "atomic failure", forward reference, parent, sibling-order, duplicate component, unknown type and path-convenience tests. Hierarchy is applied through `World::run_system_once` before return: after `instantiate` and after `despawn_scene` returns, `Parent` and `Children` are consistent.
  - [ ] A dead `opts.parent` returns `InvalidParent` before anything is spawned: the entity count and `Scenes` are unchanged.
  - [ ] **Rollback with `opts.parent = Some(p)`**: a forced failure after the roots were attached (a test hook makes the apply report a skip) leaves `p`'s `Children` equal to what it was before the call, with no dead entity in it, and the world has the original entity count.
  - [ ] Write, read into a fresh world, write gives identical text.
  - [ ] `instantiate/10000` meets budget.

**SC5. `feat(scene): validate_scene, large-scene tests and benchmarks`** (about 450 lines). Blocked by `<intake-2>`, SC4.
- Footprint: Edits `crates/exitium_scene/`; Governs none; Hotspots none; New crate edges none.
- Adds `validate_scene`, `ValidationReport`, the 10,000-entity round trip, the benchmarks.
- Acceptance criteria:
  - [ ] `validate_scene` leaves no trace in the live world and reports the same errors as `instantiate`.
  - [ ] A 50,000-entity round trip (write, read, instantiate, write) stays under 2 s in CI (the 1M-entity case is cut from the MVP); `parse`, `instantiate`, `write` and `validate` benchmarks meet the budgets.

### Stage 2: facade glue (Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none)

These four run alone, one after another, and after the plugin lanes they glue (R2, R3). Their order relative to the other specs' facade issues (app-platform.md F1 to F3, render-v1.md G1 and I18, ui-mvp.md, physics) is the orchestrator's; the `Blocked by` lines below are only what each glue needs.

**FA1. `feat(exitium): asset feeding glue for render and UI`** (about 350 lines; this is render-v1.md's G1b, filed once). Blocked by `<intake-2>`, AP2, render-v1.md I16 (`TextureStore`, `FontStore`) and I18, and ui-mvp.md U11 (`UiPlugin`, the `UiTree` resource and `paint_revision`).
- Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none.
- Acceptance criteria:
  - [ ] A headless test loads a generated PNG and a font through `MemorySource` and sees `TextureStore::size(guid)` and the font after `ASSETS`; `ImageSettings` reach `TextureDesc`; a `Sprite.image` set after load is requested once; a failed asset logs once.
  - [ ] A `NotFound` asset is retried at 1, 2, 4, 8 and 16 s (a test clock) and then stays `Failed` with one log line; a `Decode` failure is never retried; a `forget` followed by a sprite change requests it again.
  - [ ] A UI image and a UI font `Guid` in the `UiTree` are requested once, and a frame with an unchanged `paint_revision` does not walk the draw list (a counter test).
  - [ ] `exitium/glue/request_scan/20000` (20,000 unchanged sprites, then 100 changed) stays within the budget; the system uses `Changed<..>` filters, not a full query.

**FA2. `feat(exitium): asset feeding glue for audio and the web unlock`** (about 300 lines). Blocked by `<intake-2>`, AUP1, FA1, app-platform.md PP1 (`InputState`).
- Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none.
- Acceptance criteria:
  - [ ] A headless test builds `AudioPlugin` with `AudioOutput::Null` (no kira type in the test), plays a loaded clip through `PlayAudio`, and asserts the peak from `advance_null` is above zero; with `start_locked: Some(true)` an injected key press calls `unlock` once (`Locked` becomes `Ready`) and not again.

**FA3. `feat(exitium): scene loading glue (LoadScene, SceneLoaded)`** (about 450 lines). Blocked by `<intake-2>`, SC5, FA1.
- Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none.
- Acceptance criteria:
  - [ ] `LoadScene` additive and replace; asset references of the scene are requested; `SceneLoaded` and `SceneLoadFailed` events; the real `Transform2d`, `Sprite` and `Camera2d` round-trip through a scene in a headless app (the test C1 puts in the facade).

**FA4. `feat(exitium): re-export the assets, audio and scene APIs`** (about 200 lines; high-risk review: facade API). Blocked by `<intake-2>`, FA3.
- Footprint: Edits none; Governs none; Hotspots `crates/exitium/`; New crate edges none.
- Adds three modules and **this complete list** (a compile test names every item; each is one `pub use`):
  - `exitium::assets`: `AssetPath`, `AssetKind`, `AssetError`, `AssetInfo`, `ImageFilter`, `ImageSettings`, `AssetRegistry`, `AssetConfig`, `AssetLimits`, `AssetServer`, `AssetState`, `AssetStats`, `Payload`, `DecodedImage`, `DecodedAudio`, `Loaded`, `Manifest`, `ManifestState`, `AssetPlugin`, `AssetStatus`, `AssetFailure`, `AssetLoaded`, `AssetFailed`, `AssetGluePlugin`, the set `sets::ASSETS`, and the decoders as `decode::{decode_png, encode_png, decode_wav, decode_ogg, validate_font, decode_text}`.
  - `exitium::audio`: `AudioEngine`, `AudioConfig`, `AudioOutput`, `AudioState`, `AudioError`, `AudioStats`, `Bus`, `ClipData`, `ClipId`, `Voice`, `PlayOptions`, `MusicOptions`, `AudioPlugin`, `Audio`, `ClipStore`, `AudioSettings`, `PlayAudio`, `StopAllAudio`, `PlayMusic`, `StopMusic`, `AudioGluePlugin`.
  - `exitium::scene`: `Name`, `SceneId`, `SceneMember`, `SceneInfo`, `Scenes`, `ParsedScene`, `InstantiateOptions`, `SceneInstance`, `SaveScope`, `SaveOptions`, `SavedScene`, `SceneWarning`, `SceneErrors`, `SceneError`, `SceneErrorKind`, `ValidationReport`, `write_scene`, `validate_scene`, `despawn_scene`, `mark_member`, `register_types`, and the facade-defined `ScenePlugin`, `LoadScene`, `LoadMode`, `SceneLoaded`, `SceneLoadFailed` and the constant `INTERNAL_COMPONENTS`.
  - **Kept private** (not re-exported; the editor and tools depend on the crates directly): `exitium_assets::{source (AssetSource, FsSource, MemorySource), native, edit, web}`, the scene crate's `label` and `SceneRefs`, the parser internals, and every kira type and backend of `exitium_audio` (there is none in its public API).
  - Adds `AssetPlugin`, `AssetGluePlugin`, `AudioPlugin`, `AudioGluePlugin` and `ScenePlugin` to `DefaultPlugins`.
- Acceptance criteria:
  - [ ] Every name above is reachable as `exitium::<module>::<Name>` and every kept-private name is not; no kira type is reachable (the compile test also builds an `AudioPlugin` with `AudioOutput::Null` using only `exitium::` paths); `cargo doc` has no broken re-export; high-risk review (facade API).

### Stage 2: reflect defaults and the asset-picker attribute in render and UI

Two small issues in crates this spec does not govern, so that render I14 and ui-mvp.md U12 keep their sizes. Each edits one crate. They run in their crate's lane, after the issue that creates the types, and nothing in this spec's other issues needs them to merge first (the template and scenes in this spec write every field).

**RA1. `feat(render): reflect defaults and the asset-picker attribute on Sprite, Camera2d and Text`** (about 150 lines). Blocked by `<intake-2>` and render-v1.md I14 (which creates the types).
- Footprint: Edits `crates/exitium_render/`; Governs none; Hotspots none; New crate edges none.
- Adds `#[reflect(default = ..)]` to every field of `Sprite`, `Camera2d` and `Text` with the defaults listed under "Downstream adjustments", and `custom(widget = "asset", kind = "image")` on `Sprite.image`, `custom(widget = "asset", kind = "font")` on `Text.font`.
- Acceptance criteria:
  - [ ] `Sprite(image: Some("<uuid>"))` binds through `exitium_serial` with one `DefaultedField` warning per filled field and equals `Sprite { image: Some(..), ..default values }`; the same for `Camera2d()` and `Text(text: "x")`.
  - [ ] The attribute is readable through `TypeInfo` (`custom("widget")` and `custom("kind")`); the schema hash of the three types does not change (attributes are not hashed).

**UA1. `feat(ui): the asset-picker attribute on image and font Guid fields`** (about 80 lines). Blocked by `<intake-2>` and ui-mvp.md U12.
- Footprint: Edits `crates/exitium_ui/`; Governs none; Hotspots none; New crate edges none.
- Adds `custom(widget = "asset", kind = "image")` to `ImageData.image` and `kind = "font"` to the `font` fields of the reflected UI data.
- Acceptance criteria:
  - [ ] Each field's `TypeInfo` carries the attribute; the `UiNodeDesc` text round trip is unchanged.

### Stage 4: CLI lane (`exitium_cli`, Footprint: Edits `crates/exitium_cli/`; Governs none; Hotspots none; New crate edges none)

**C1. `feat(cli): argument parsing, the project file and exitium new`** (about 550 lines). Blocked by `<intake-2>`, SC5, and the facade chain the template's `main.rs` compiles against: FA4 (the assets, audio and scene re-exports and their `DefaultPlugins` entries), app-platform.md F3 (`App`, `DefaultPlugins`, the `Startup` schedule re-exports), render-v1.md I18 and G1a to G1d (`RenderPlugin` and the window and UI glue that `DefaultPlugins` includes), the facade sample-game issue (jam-mvp.md Stage 2, item 5), and the Stage 3 facade hook issue of #110 (the `editor` feature of the game binary, C2). Stage 4 starts after the Stage 3 exit, so all of these have merged; they are listed so the template is never generated against a facade that lacks them.
- Footprint: Edits `crates/exitium_cli/`; Governs none; Hotspots none; New crate edges none.
- Adds `args.rs`, `project.rs`, `new.rs` and the embedded `template/`; creates `CLAUDE.md`.
- Acceptance criteria:
  - [ ] The project file round trips the six fields above, including `export_features`; the "CLI" `new` tests; the generated project passes `cargo check` and the wasm32 check when the toolchain is present; the project file is read through the syntax tree.

**C2. `feat(cli): export staging, manifest checks and the deterministic zip`** (about 500 lines). Blocked by `<intake-2>`, C1.
- Footprint: Edits `crates/exitium_cli/`; Governs none; Hotspots none; New crate edges none.
- Adds `export/{mod,stage,zip}.rs`: manifest-driven staging, missing and unlisted file reports, the deterministic zip, `--json` output.
- Acceptance criteria:
  - [ ] Byte-identical zips on two runs; sorted entries; no backslash; the 1,000-file packaging benchmark meets budget.

**C3. `feat(cli): exitium export --target windows`** (about 350 lines). Blocked by `<intake-2>`, C2.
- Footprint: Edits `crates/exitium_cli/`; Governs none; Hotspots none; New crate edges none.
- Acceptance criteria:
  - [ ] The command plan test with a fake `cargo` (never `editor`, `--no-default-features`); the PR body records a real export of the `exitium new` template and a run of the exe from the zip.

**C4. `feat(cli): exitium export --target web`** (about 550 lines). Blocked by `<intake-2>`, C3.
- Footprint: Edits `crates/exitium_cli/`; Governs none; Hotspots none; New crate edges none.
- Adds `export/web.rs`, `export/tools.rs`, the `index.html` template, preflight and `--check`, the `WASM_BINDGEN_VERSION` constant and its test.
- Acceptance criteria:
  - [ ] The preflight tests (missing target, tool, wrong version, optional `wasm-opt`) each print the fix command; the command plan test for both with and without `wasm-opt`.
  - [ ] A real export of the template, served and run in Chrome, is recorded in the PR; `index.html` is at the zip root; the `wasm-bindgen` version check reads `Cargo.lock`.

**C5. `feat(cli): exitium serve (local static server)`** (about 250 lines, optional for the MVP exit). Blocked by `<intake-2>`, C4.
- Footprint: Edits `crates/exitium_cli/`; Governs none; Hotspots none; New crate edges none.
- Acceptance criteria:
  - [ ] `.wasm` and `.js` MIME types, `404`, a path-traversal attempt is refused, loopback only.

### Downstream adjustments

Applied by the architect when this spec merges (the bullets are the contract):

- **#61 (world snapshot):** its text half is SC2 to SC4. Close it as superseded for the MVP; its binary half stays with #24.
- **reflect-model.md open question 13 and render-v1.md open question 1:** resolved by D1 (`Guid` fields, no `AssetRef<T>` in the MVP). **render-v1.md open question 5:** resolved by edge bleed in the decoder. **render-v1.md D3's "the importer downscales":** import downscales (default 4096); the decoder refuses over 8192 with the same hint.
- **app-platform.md open question 3 (derived components):** resolved. The scene skips `custom(derived = true)`, `SceneMember`, `Parent` and `Children` as described under "Skipped on save".
- **ui-mvp.md open question 8 (UI persistence):** a UI tree is a `.ui` `Text` asset holding a `UiNodeDesc` in the text form; the UI plugin parses it. Scenes carry no UI nodes.
- **jam-mvp.md:** its known-wiring line "scene -> core, ecs, reflect, serial, app" is refined to drop `app` (D7), so the scene lane may start right after intake 2. Its Stage 2 count (scene about 2 PRs, plugins about 8) is replaced by this spec's list: assets 3, audio 1, scene 5, facade 4 in Stage 2, plus RA1 in render and UA1 in ui; the scene lane (5) is the longest and runs beside the spine, so the Stage 2 critical path is unchanged. The roadmap's "known" Stage 1 counts for assets and audio become 7 and 3.
- **#110 (command bus, editor, MCP):** implements the tools and panels in "MCP / editor surface", including the generic asset-picker inspector widget keyed on `custom("widget") == "asset"` and `custom("kind")`, must call `mark_member` from its spawn command, must let a command declare "no undo", and starts the `exitium` binary for the three ops tools. Its export tool is blocked by C3 and C4.
- **#41 (archetypes and `World`):** `Bundle` is implemented for tuples of arity 0 to 8, and `()` is the empty bundle, so `world.spawn(())` creates an entity in the empty archetype (scene `instantiate` relies on it). One impl and one test.
- **reflect derive:** `SceneMember` and `GlobalTransform2d` rely on a type-level `custom(derived = true)` attribute, which app-platform.md open question 3 already asks the derive to allow.
- **render-v1.md I14 is not changed** (it is already about 600 lines). The `#[reflect(default)]` and `custom(widget)` work on `Sprite`, `Camera2d` and `Text` is the separate issue RA1 below, which edits `exitium_render` after I14 has created the types. G1b is FA1 of this spec: one issue, with `Blocked by` extended by ui-mvp.md U11. RA1 sets the defaults the spec's field comments already state (anchor (0.5, 0.5), tint white, `mask` 1, `visible` true, `layers` 1, `zoom` 1.0, `viewport` (0,0)-(1,1), `scaling` `PixelsPerUnit(1.0)`, `active` true, `align` Left, `wrap` None) and the zero, false or `None` value otherwise, so a hand-written scene may omit them, and adds `custom(widget = "asset", kind = "image")` to `Sprite.image` and `kind = "font"` to `Text.font`.
- **ui-mvp.md U12 is not changed.** The same attribute on the `Guid` fields of image and font data (`ImageData.image`, the `font` fields) is the separate issue UA1 below.
- **#49 (hierarchy):** optionally marks `Children` with `custom(derived = true)`; until then the scene's name list covers it.
- **intake 1 and intake 2:** add the lists in "Wiring for the intake PRs", including the `ENGINE_CRATES` edit and the licence decision.
