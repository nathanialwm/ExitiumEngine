# Spec: Render v1 (wgpu sprites, camera, text, screenshots; WebGPU on web)

- Issue: #107
- Crates: `exitium_render` (Stage 1 depends on `exitium_core` only; the Stage 2 `plugin` module adds `exitium_ecs`, `exitium_app`, `exitium_reflect`)
- Status: draft
- Amendments: #122 (wasm-safe `RenderWindow`, I12 blockers); **SP5 (#141)**: the render v2 hooks and the surface overlay (AM1 of `editor-mcp-mvp.md` "Downstream adjustments"). The SP5 text is in "Frame plan and the surface overlay (AM1)" and "Render v2 hooks", and in the edits it makes to the sections they touch.

## Goal

`exitium_render` is the 2D renderer of the MVP. It turns a list of sprites, text and debug shapes plus one or more cameras into pixels on a window surface (Windows, and the browser through **WebGPU**) or into an offscreen texture that can be read back as an image for MCP. It must be fast on the 4-core, 8 GB min-spec (INTERVIEW #13), hold 20,000 alpha-blended sprites at 60 fps, and keep the Stage 4 paths open: pixel art with integer scaling, depth sorting for top-down and isometric games, normal-mapped 2D lights and user materials (all in the full MVP, specified later by `render-2d-breadth.md`, SP12). This spec ships none of them and settles the **hooks** they plug into, so Stage 4 adds no rework ("Render v2 hooks"). It also lets the editor paint over the window surface ("Frame plan and the surface overlay").

The crate ships in two stages (contract of #107):

| Stage | What | Depends on |
|---|---|---|
| **1. Standalone** | `Renderer`, `DrawList`, `Camera`, textures, text, debug draw, capture, and a windowed Windows and Chrome example. No ECS: a plain Rust program (or test) fills a `DrawList` and calls `render`. | `exitium_core`, `wgpu`, `glyphon`, `bytemuck` |
| **2. Plugin** | A `plugin` module: the `Sprite`, `Camera2d` and `Text` components, extraction from the ECS into a `DrawList`, settings, stats and a screenshot service. | Stage 1, `exitium_ecs`, `exitium_app`, `exitium_reflect` |

The design is a **plan/execute split**. Everything that decides *what* to draw (cull, sort, batch, instance data, camera math, text layout keys) is plain CPU code that needs no GPU, so it is unit-tested and benchmarked on CI without an adapter. Only the last step (upload, encode, submit) touches `wgpu`.

Versions were checked on 2026-11-18 (crates.io; browser support from the caniuse WebGPU table): `wgpu` 30.0.2, `glyphon` 0.12.0 (built on `cosmic-text` 0.19, needs `wgpu ^30`), `raw-window-handle` 0.6.2, `winit` 0.30.13 stable (0.31 is still beta). `wgpu`+`glyphon` was compiled for `wasm32-unknown-unknown` and for `x86_64-pc-windows-msvc` with the exact feature list under "Dependencies and wiring" and no extra rustflags; the result is recorded there.

## Non-goals

- **WebGL2 and any native GL fallback.** Owner decision (2026-11-18): web is WebGPU only, Windows uses DX12 or Vulkan. The `gles` and `webgl` wgpu features stay off. The WebGL2 text in INTERVIEW.md (decision 3, line 31; risk R5, line 136) and PLAN.md (the web build pipeline line, line 220) is stale; those root files are hotspots outside this spec's footprint, so a follow-up issue updates them (D6). A browser without WebGPU gets a clear error (`RenderError::WebGpuUnavailable`), not a degraded path. caniuse lists WebGPU as fully supported in Chrome and Edge 113+ and partial in Firefox and Safari; the MVP tests Chrome only.
- 3D, general post-processing, MSAA, HDR output, a depth buffer, and hot reload of the **built-in** shaders (they are embedded `&'static str`). Normal-mapped 2D lighting and user-written materials (whose WGSL source is hot-reloadable) are **in the full MVP, in Stage 4** (`mvp-roadmap.md`, SP12). v1 ships neither and settles only their hooks (H2, H3, H5).
- Tilemaps are the `exitium_tilemap` crate (SP11, Stage 4): they reach the renderer as ordinary `SpriteDraw` items through facade glue, so render has no tilemap type and no edge to that crate (C1).
- Particles, sprite animation, 9-slice (the UI draws nine ordinary sprites), atlas packing, texture compression, SDF text, rotated text, rich-text spans (open question 4). All can be added on top of the layouts below.
- The pixel-art pipeline (virtual resolution with the integer-scaled upsample, palette swap, sub-pixel camera, pixel-perfect rotation): Stage 4 (SP12). v1 ships the pixel-perfect camera snap, nearest filtering and render-to-texture, plus the hooks H3 to H5 that the rest plugs into.
- Decoding image files (PNG) and encoding the screenshot PNG. `exitium_render` takes decoded RGBA8 and returns raw RGBA8. It has no image-codec dependency.
- Asset loading, GUIDs, hot reload (assets spec #111), the window and input (app/platform spec #106), the UI tree (#109), physics (#108), the command bus and MCP wiring (#110). This spec names the interfaces it needs from them and nothing more. It files no issue in `exitium_mcp` or `exitium_editor`: it lists the reflected types and plain Rust services that #110 must expose, and #110 builds the tools generically.
- Edges to other runtime crates. `exitium_render` has none to platform, physics, audio, assets, ui or scene (cross-spec ruling). Code that connects them to the renderer lives in the `exitium` facade.
- Automatic recovery from a lost GPU device. v1 reports `RenderError::DeviceLost`; the app decides (log and exit is acceptable for the MVP).
- Multiple windows. One surface per `Renderer`.

## Design

### Module layout

```
crates/exitium_render/
  Cargo.toml
  CLAUDE.md          crate guide (I1): invariants, module map, how to run the GPU tests
  src/
    lib.rs           module docs, re-exports; the intake PR declares every module below
    error.rs         RenderError
    config.rs        RendererConfig, PresentMode, Space, Flip, limits (MAX_INSTANCES, MAX_TEXT_BATCHES)
    gpu.rs           Gpu (instance, adapter, device, queue), surface setup, resize, block_on
    camera.rs        Camera, Scaling, CameraView: pure math, no wgpu
    texture.rs       TextureDesc, texture table, mip generation, samplers, bind groups
    batch.rs         SpriteDraw, DrawList, sort key, cull, instance build, BatchPlan: pure CPU, no wgpu
    sprite.wgsl      sprite vertex + fragment shader
    mipmap.wgsl      mip downsample shader
    text.rs          TextDraw, TextStyle, FontId, shaped-buffer cache, glyphon wrapper
    debug.rs         DebugDraw, tessellation to line segments, line.wgsl
    capture.rs       CaptureId, Capture, row-padding removal, BGRA fix-up
    renderer.rs      Renderer: frame orchestration (surface, passes, submit, present)
    frame.rs         Pass, CameraPass, RenderTarget, FrameStatus, plan_frame and FramePlan::resolve (which passes run and whether the surface is acquired: pure CPU), SurfaceOverlay, OverlayCx
    material.rs      v2 (H5), light.rs v2 (H2, H3), present.rs v2 (H4): empty in v1, content decided by SP12. Intake-1 creates them with a `//!` doc so `lib.rs` is edited once; SP12 may add more files and intake-1 declares the union
    stats.rs         RenderStats
    lib.rs also has `pub use wgpu;`, so `RendererConfig` and the public wgpu types need no second dependency in a game
    plugin/          Stage 2: mod.rs, components.rs, window.rs (RenderWindow, sets), extract.rs, stores.rs, screenshot.rs, settings.rs
  benches/batch.rs   (I4), benches/text.rs (I9), benches/extract.rs (I15), benches/frame.rs (I20); empty files created by intake-1
  examples/render_window.rs   (I12) the Stage 1 windowed and web example, no ECS
  tests/common/mod.rs   (I1) GPU test helper: software adapter, error handler, blocking readback with BGRA swap
  tests/             headless GPU tests, golden images (tests/golden/*.rgba8)
```

`unsafe` is not used. `bytemuck`'s derive casts instance structs to bytes. `wgpu` and `glyphon` contain their own `unsafe`; we neither add nor wrap any.

### Coordinate spaces

| Space | Origin, axes, unit | Used for |
|---|---|---|
| **World** | y up, +x right, one unit is whatever the game says (texels for pixel art, meters for physics games). Rotation is counter-clockwise radians. | Sprites, text and debug shapes with `Space::World`; the camera maps it to the screen. |
| **Screen** | Top-left origin, y down, **logical pixels** (physical pixels divided by `scale_factor`). Independent of the camera position, zoom and rotation. | UI, HUD, debug overlays (`Space::Screen`). |
| **Physical** | Top-left origin, y down, device pixels of the render target. | Camera viewports, scissor rectangles, capture. |
| **Clip** | wgpu convention (y up, depth 0 to 1). There is no depth buffer: order comes from the CPU sort. | Shaders only. |

Colors are `exitium_core::Color`: **linear** RGBA `f32`. Targets are always sRGB-format views, so shaders work in linear space and the hardware encodes on write. This matches the core `Color` docs (linear storage, sRGB at the edges).

### Device, surface and web

```rust
pub struct RendererConfig {
    pub present_mode: PresentMode,        // Vsync (default), LowLatency, Immediate
    pub power: PowerPreference,           // HighPerformance (default), LowPower
    pub max_frame_latency: u32,           // default 2
    pub clear_color: Color,               // default opaque black; per-camera `clear` overrides it
    pub headless_format: wgpu::TextureFormat, // Rgba8UnormSrgb (default) or Bgra8UnormSrgb; only used when there is no surface (tests exercise the BGRA path)
    pub force_software: bool,             // request the fallback (software) adapter; tests and CI set it, default false
}
pub enum PresentMode { Vsync, LowLatency, Immediate }   // AutoVsync, Mailbox-or-Fifo, AutoNoVsync

impl Renderer {
    /// Windowed. `window` is anything wgpu accepts: an `Arc<winit::window::Window>` works on
    /// Windows and on the web. Async because the browser cannot block (see below).
    pub async fn new(
        config: RendererConfig,
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        size: UVec2,                      // physical pixels
        scale_factor: f32,
    ) -> Result<Renderer, RenderError>;
    /// No surface: render-to-texture only. Used by tests, CI, headless apps and MCP screenshots.
    pub async fn headless(config: RendererConfig) -> Result<Renderer, RenderError>;

    #[cfg(not(target_arch = "wasm32"))] pub fn new_blocking(..) -> Result<Renderer, RenderError>;
    #[cfg(not(target_arch = "wasm32"))] pub fn headless_blocking(..) -> Result<Renderer, RenderError>;

    pub fn resize(&mut self, size: UVec2, scale_factor: f32);
    pub fn set_present_mode(&mut self, mode: PresentMode);
    pub fn device(&self) -> &wgpu::Device;     // for the editor (egui-wgpu shares the device)
    pub fn queue(&self) -> &wgpu::Queue;
}
```

**Creation order** (same on both targets): `Instance` from `InstanceDescriptor::new_without_display_handle()` with `.with_env()` on native (so `WGPU_BACKEND` works in CI), backends `Backends::PRIMARY`; `create_surface(window)`; `request_adapter` with `compatible_surface`, `power_preference` and `force_fallback_adapter: config.force_software`; `request_device` with **no optional features** and `Limits::default()` raised to the adapter's `max_texture_dimension_2d`. Using no optional feature keeps the renderer inside the portable WebGPU subset, so a native run cannot silently depend on something the browser lacks.

**Surfaces.** In wgpu 30 `SurfaceTarget: From<T>` for any `T` with a window and a display handle, so `Arc<winit::window::Window>` is passed as is. `exitium_render` does not depend on `raw-window-handle` or `winit` directly: it comes in through `wgpu` (`raw-window-handle` 0.6.2) and the platform crate (#106, `winit` 0.30.13). `egui-wgpu` 0.37, which the editor needs (#110), requires `wgpu ^30` and `winit ^0.30.13`, so the three versions agree.

**Web (WebGPU only).** wgpu's `webgpu` feature compiles the browser backend; with `webgl` off it is the only backend on `wasm32`. `request_adapter` and `request_device` return futures that only complete when the browser's event loop runs, so `Renderer::new` is `async` and **never blocks**. On the web the caller spawns the future with `wasm_bindgen_futures::spawn_local` and stores the finished `Renderer` in a shared slot that the app polls (the Stage 2 plugin does this in `init_renderer`; the Stage 1 sample does it by hand). `wasm-bindgen-futures` is therefore a `wasm32`-only dependency of `exitium_render` (already in wgpu's web tree, so it adds no new lock entry); the web entry point itself (`#[wasm_bindgen(start)]`, the canvas, packaging) is the sample's and the export spec's (#111), not this crate's. Native gets `*_blocking` wrappers built on a 25-line `block_on` in `gpu.rs` (`std::task::Wake` plus `thread::park`): no `pollster` dependency. If `navigator.gpu` is missing or no adapter is found on the web, creation returns `RenderError::WebGpuUnavailable { reason }` with the text "WebGPU is not available in this browser (Chrome or Edge 113+ is required)". On native the equivalent is `RenderError::NoAdapter`. wgpu's web handles wrap JavaScript objects and are not `Send` by default; the crate enables wgpu's `fragile-send-sync-non-atomic-wasm` feature on `wasm32` only, which makes them `Send + Sync` when the build has no wasm atomics (wgpu gates it on `not(target_feature = "atomics")`). The MVP's wasm has one thread (`exitium_tasks` runs inline there). If a threaded web build (atomics) is added later, the feature becomes inert, the handles are `!Send` again, and the compile-time `Send + Sync` assertion on `Renderer` (below) fails to compile: the build breaks instead of racing (open question 8).

**Surface format.** Prefer a format `f` with `f.is_srgb()` from the capabilities. If there is none (the WebGPU canvas offers only `Bgra8Unorm` or `Rgba8Unorm`), configure the first format and add its sRGB sibling (`TextureFormat::add_srgb_suffix`) to `view_formats`, and render through a view with the sRGB format. wgpu 30 requires `SurfaceConfiguration::color_space`; v1 uses `SurfaceColorSpace::Auto`. The pure format-selection function returns the triple `(configured, F, O)`: the format the surface is configured with, the sRGB **view** format `F` the cameras use, and its non-sRGB sibling `O` for the surface overlay ("Frame plan and the surface overlay"). `view_formats` always contains both `F` and `O` unless one of them is the configured format itself, so the overlay's view needs no later reconfiguration. The chosen view format is the renderer's **target format** `F`: `Bgra8UnormSrgb` on a typical Windows or WebGPU surface, `Rgba8UnormSrgb` when headless (`RendererConfig::headless_format`, so tests can exercise the BGRA path) and on surfaces that offer it. **One format story for the whole crate:** every render target (the surface view and every render texture) has format `F`; the sprite, line and clear pipelines and the glyphon atlas are built once for `F`; only the mip-generation pipelines are built for the sampled-texture formats, `Rgba8UnormSrgb` at startup and `Rgba8Unorm` lazily on the first linear texture with mips (next section). A render texture drawn as a sprite is just a sampled `F` texture, so no pipeline mismatch is possible. Capture swaps B and R on the CPU when `F` is BGRA.

**Present modes and resize.** `Vsync` maps to `AutoVsync`, `LowLatency` to `Mailbox` when supported else `AutoVsync`, `Immediate` to `AutoNoVsync`. On the web every mode behaves as `Vsync` (the browser paces frames). `resize` only records the new size; the next `render` reconfigures the surface once, so several resize events in one frame cost one `configure`. A zero width or height (minimized) makes `render` return `FrameStatus::Skipped(Minimized)` without touching the surface. A skip concerns the **surface only**: texture-target passes still run and are submitted, so a screenshot or a scene-view target completes while the window is minimized or occluded (rule 5 of "Frame plan"). Wgpu 30's `Surface::get_current_texture` returns a `CurrentSurfaceTexture` enum, handled as: `Success` render; `Suboptimal` render, then reconfigure; `Outdated` or `Lost` reconfigure and skip this frame; `Timeout` or `Occluded` skip; `Validation` skip and count an error. Presenting is `Queue::present(frame)` (wgpu 30 removed `SurfaceTexture::present`).

**Errors.** `RenderError` is a `#[non_exhaustive]` enum with `NoAdapter`, `WebGpuUnavailable`, `DeviceRequest(String)`, `Surface(String)`, `TextureTooLarge { size, max }`, `BadImage { expected_len, got_len }`, `NoFont`, `BadFont(String)` (the bytes contain no face), `DeviceLost(String)`, `Capture(String)`, `CaptureBusy`, `NoCamera`, `StaleHandle` (a handle or id whose slot was destroyed or never existed, returned by the replace functions). wgpu errors are converted to strings so no wgpu error type is part of the API. The device's uncaptured-error handler and device-lost callback record into `RenderStats` and `tracing::warn!`; tests install a handler that fails the test.

### Textures

```rust
pub struct Texture;                                // marker for Handle<Texture>
pub struct ImageData { pub size: UVec2, pub rgba8: Vec<u8> }   // straight alpha, sRGB-encoded, row-major, top-left origin
#[non_exhaustive]                                  // with `Default` (the defaults named by the variants' docs); Stage 4 may add fields
pub struct TextureDesc { pub filter: Filter, pub address: Address, pub mips: Mips, pub color_space: ColorSpace, pub label: Option<String> }
pub enum ColorSpace { Srgb, Linear }               // Srgb (default): color art, decoded to linear when sampled. Linear: data such as normal maps and palette lookups, sampled as raw values
#[non_exhaustive]                                  // with `Default` and `new(filter)`; lights may need another format (H3)
pub struct RenderTextureDesc { pub filter: Filter, pub label: Option<String> }   // default Linear; Nearest for a virtual-resolution target (H4)
pub enum Filter { Linear, Nearest }                // Nearest for pixel art
pub enum Address { Clamp, Repeat }
pub enum Mips { None, Generate }                   // Generate for portraits and anything shown smaller than stored

impl Renderer {
    pub fn create_texture(&mut self, image: &ImageData, desc: &TextureDesc) -> Result<Handle<Texture>, RenderError>;
    pub fn create_render_texture(&mut self, size: UVec2) -> Result<Handle<Texture>, RenderError>;   // = create_render_texture_with(size, &RenderTextureDesc::default())
    pub fn create_render_texture_with(&mut self, size: UVec2, desc: &RenderTextureDesc) -> Result<Handle<Texture>, RenderError>;
    /// Hot reload: the same handle now shows the new image (see "Replacing a texture" below).
    pub fn replace_texture(&mut self, texture: Handle<Texture>, image: &ImageData, desc: &TextureDesc) -> Result<(), RenderError>;
    pub fn texture_revision(&self, texture: Handle<Texture>) -> Option<u32>;   // 0 when created, +1 per replace: egui-wgpu users re-register `texture_view` when it changes
    pub fn destroy_texture(&mut self, texture: Handle<Texture>);   // a stale handle is a no-op
    pub fn texture_size(&self, texture: Handle<Texture>) -> Option<UVec2>;
    pub fn texture_view(&self, texture: Handle<Texture>) -> Option<&wgpu::TextureView>;  // egui-wgpu registers it
    pub fn white_texture(&self) -> Handle<Texture>;   // built-in 1x1 white: solid-color sprites use it
}
```

- **Table.** `Slab<GpuTexture>` from `exitium_core` keyed by `Handle<Texture>`. `GpuTexture { texture, view, bind_group, size, bytes, revision }`. The handle is generational, so a destroyed texture drawn later is skipped and counted (`RenderStats::missing_textures`), never a panic or a wrong image.
- **Format.** Sampled textures (from `create_texture`) are `Rgba8UnormSrgb` for `ColorSpace::Srgb` and `Rgba8Unorm` for `ColorSpace::Linear`, `COPY_DST | TEXTURE_BINDING`, plus `RENDER_ATTACHMENT` when mips are generated. The color space is settled in v1, not in Stage 4, because a normal map is data: sampled through an sRGB format it would be gamma-decoded and every normal would tilt (H2). **Render textures use the renderer target format `F`** (see "Surface format"), with `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC`. Larger than `max_texture_dimension_2d` returns `TextureTooLarge`: the importer (#111) downscales, the renderer does not silently resample.
- **Mips** are generated on the GPU by a chain of draws (`mipmap.wgsl`: each level samples the previous one with a linear filter through the sRGB view, so averaging happens in linear space, and writes through the sRGB view; a `Linear` texture uses the same chain with the `Rgba8Unorm` pipeline and averages raw values, which a normal-map shader repairs by renormalizing). Compute is not used, so it works wherever render passes do. Upload is one `write_texture`.
- **Binding strategy.** One bind group per texture (view plus a shared sampler chosen from the four `Filter` and `Address` combinations), bound at **group 1** (and rebuilt in place by `replace_texture`, so the handle keeps working). Every sampled texture shares one bind-group layout, so a normal map or a material's extra texture (H2, H5) is the same kind of object. The four bind-group indices are planned once, in "Render v2 hooks"; v1 pipelines use groups 0 and 1 only. A draw call is a run of consecutive sorted sprites with the same texture. Sprite sheets (many sprites, one texture, per-sprite sub-rect) are therefore the batching tool; the asset pipeline owns atlas packing. Texture binding arrays are not in the WebGPU core spec, so they are not used. If a game needs more than about 200 texture switches per frame, a 2D-array path can replace the bind group without touching `SpriteDraw` (open question 10).
- **Alpha.** Textures keep straight alpha and the pipeline blends `SrcAlpha, OneMinusSrcAlpha` for color and `One, OneMinusSrcAlpha` for alpha. Transparent-edge pixels in source art should carry the neighbouring color, which the importer can do (open question 5).
- **Replacing a texture (asset hot reload).** `replace_texture` builds the new `GpuTexture` first and then swaps it into the **same slab slot**, so the `Handle<Texture>` (index and generation) stays valid and every sprite that holds it draws the new image from the next `render` call; nothing is rebuilt for the callers. It is all or nothing: if the new image fails (`TextureTooLarge`, `BadImage`) the old texture stays and the error is returned. A stale handle or a render texture returns `RenderError::StaleHandle` (render textures are not assets). The size may change: sprites carry `rect` in texels and the batcher reads `texture_size` when it builds `uv_rect` each frame, so a changed size needs no other update, while a sprite sheet whose layout changed is the game's to fix. `revision` is bumped by every replace. The old texture is dropped at once; wgpu keeps it alive until the frames already submitted have finished, so a replace never tears an in-flight frame, and `texture_bytes` can exceed its steady value by one old texture for up to `max_frame_latency` frames. Replacing a texture leaves its `ColorSpace` free to change (a normal map re-imported as sRGB is an error of the importer, not of render).
- **Cost of creating or uploading a texture** (what the assets spec cites). **1 Mpx in this spec is 2^20 pixels (4 MiB of RGBA8)**, so a 2048 x 2048 image is exactly 4 Mpx. `create_texture` and `replace_texture` run on the calling thread and are bounded by **0.1 ms fixed (texture, view, bind group) + 1.5 ms per Mpx** (one `write_texture`), **+ 0.5 ms** when `Mips::Generate` (about eleven small passes are recorded; the mip draws run on the GPU in the frame's submit and are not on the calling thread). A 1024 x 1024 texture is at most 1.6 ms (2.1 ms with mips), a 2048 x 2048 one at most 6.1 ms (6.6 ms), a 4096 x 4096 one at most 24.1 ms (more than a frame, which is why importers stop at 2048 in practice). Basis: the copy into wgpu's staging belt is a memory copy of 4 MiB per Mpx, which a 4-core min-spec machine does at 4 GB/s or more (about 1 ms per Mpx), and the ceiling keeps 50% headroom for wgpu's validation and allocation. The GPU finishes the upload during the next submit and is not on this budget. These are provisional ceilings like every other absolute figure here, measured by the `texture/create` test (I22, below) on the software adapter in the GPU job, report-only because adapter timings are not stable enough to gate. In the browser each `write_texture` crosses into JavaScript, so the same ceilings are checked by hand in Chrome from I12 on and reported.
- **Memory.** Textures are not evicted in v1. `RenderStats::texture_bytes` reports `width * height * 4 * (4/3 with mips)` summed. A 2048 x 2048 portrait with mips is about 22 MiB.

### Sprites, ordering and batching

```rust
pub enum Space { World, Screen }
pub struct Flip { pub x: bool, pub y: bool }

#[derive(Clone, Debug)]
#[non_exhaustive]                          // Stage 4 adds `normal`, `material` and `user` without breaking a caller (H2, H5)
pub struct SpriteDraw {
    pub texture: Option<Handle<Texture>>,  // None = solid color (built-in white texture)
    pub transform: Affine2,                // placement of the pivot in the item space (translation, rotation, scale); `size` is applied inside it
    pub size: Vec2,                        // in the item's space; for pixel art 1 texel = 1 unit
    pub anchor: Vec2,                      // pivot inside the sprite, (0,0) bottom-left .. (1,1) top-right; default (0.5, 0.5)
    pub rect: Option<Rect>,                // sub-rect in texels (sprite sheets); None = whole texture
    pub tint: Color,                       // multiplies the texel; channels clamped to 0..=1 (see the instance layout)
    pub flip: Flip,
    pub layer: i16,                        // coarse order; higher is drawn later (on top)
    pub depth: f32,                        // fine order inside a layer; finite, higher is on top
    pub space: Space,
    pub mask: u32,                         // matched against Camera::layers; default 1
    pub clip: Option<Rect>,                // scissor in the item's space (UI scroll views)
}
impl SpriteDraw {
    /// A white sprite at `transform` with `anchor (0.5, 0.5)`, layer 0, depth 0, `mask` 1, `Space::World`, no flip, no clip.
    pub fn new(texture: Option<Handle<Texture>>, transform: Affine2, size: Vec2) -> SpriteDraw;
}
impl Default for SpriteDraw { /* texture None, identity transform, size ONE; the rest as in `new` */ }

#[derive(Default)]
pub struct DrawList { /* sprites: Vec<SpriteDraw>, texts: Vec<TextItem>, text_arena: String */ }
impl DrawList {
    pub fn sprite(&mut self, s: SpriteDraw);
    pub fn text(&mut self, t: TextDraw<'_>);       // copies the string into the arena: no allocation per text
    pub fn clear(&mut self);                       // keeps capacity; steady state allocates nothing
    pub fn len(&self) -> usize;
}
```

`DrawList` owns all its data (`'static`, `Send`), so a later render thread can take it by value (see Threading).

**`#[non_exhaustive]` is part of the contract.** An external crate cannot build a `non_exhaustive` struct with a struct expression, so facade glue (G1d turns the UI's draw data into `SpriteDraw`) writes `let mut s = SpriteDraw::new(texture, transform, size); s.layer = ..; s.depth = ..;`. That is the price of letting Stage 4 add a field in `exitium_render` alone, without a fix-up PR in the facade (one crate per issue). The same holds for `Camera`, `CameraView`, `CameraPass`, `OverlayCx` and `RenderStats` (all with `Default` or `new`) and for the enum `Pass`. `RendererConfig`, `TextureDesc`, `RenderTextureDesc` and `TextStyle` stay exhaustive: Stage 4 needs no new field in them, and a later one arrives as a new constructor or a new struct.

**Instance layout (48 bytes, `step_mode: Instance`, no per-vertex buffer).** The unit quad is generated in the vertex shader from `vertex_index` (triangle strip, 4 vertices). The quad is in sprite space with +y up, so texture row 0 (the top of the image) lands on the quad edge at y = 1.

| Offset | Format | Field |
|---|---|---|
| 0 | `Float32x4` | `affine.matrix2` columns `(a, b, c, d)`: maps the unit quad `[0,1]^2` to **physical pixels of the target**. `size`, `anchor`, rotation, scale and the camera are folded in on the CPU. |
| 16 | `Float32x2` | `affine.translation` |
| 24 | `Float32x4` | `uv_rect` `(u0, v0, u1, v1)`, normalized. Flip swaps `u0/u1` or `v0/v1`. `Float32` rather than 16-bit unorm so a sub-rect edge is exact on 8192-pixel sheets and under minification. |
| 40 | `Unorm8x4` | `tint`, **sRGB-encoded** RGBA bytes, decoded to linear in the vertex shader (once per instance). 8-bit sRGB keeps precision in dark tints; HDR tint above 1.0 is not supported. **The CPU encode is not `Color::to_srgba_u8`** (three `powf` calls, 60 to 100 ns, more than the whole per-sprite budget): `batch.rs` encodes with a 4096-entry `u8` table indexed by `(c.clamp(0, 1) * 4095.0) as usize` (built once in a `OnceLock`, within one sRGB level of the exact value), with a fast path for `Color::WHITE` and for "same tint as the previous item" (runs of equal tints are the common case). Cost: about 3 ns per sprite. Debug-line colors use the same table. |
| 44 | `Uint32` | `user`, a per-instance word for materials (a palette row, a frame index, a flag). **v1 writes 0 and no v1 shader reads it.** It is in the layout now so Stage 4 changes neither `batch.rs`, the vertex layout nor `sprite.wgsl` (H5). |

The vertex attributes are `@location(0)` affine columns, `(1)` translation, `(2)` `uv_rect`, `(3)` tint, `(4)` `user`. **Flip is recoverable in a shader without a flag**: it is `u1 < u0` (x) and `v1 < v0` (y) in `uv_rect`, and rotation, scale and mirror are in the affine's linear part, so a normal-mapped sprite can orient its normals (H2) from the 48 bytes.

100,000 sprites are 4.8 MB per frame, written with one `write_buffer` into a per-frame-in-flight buffer that doubles when full. **All passes of one `render` call share that one buffer; each pass owns a contiguous range of it** (its offset is a multiple of 4), and the pass binds its range with `set_vertex_buffer(0, buffer.slice(range))`. Nothing a later pass writes can overwrite an earlier pass's data, because no pass writes into another's range and the whole buffer is uploaded once before the single submit. Debug lines use a second buffer with the same per-pass ranges. The hard cap is `MAX_INSTANCES = 1 << 20` per frame summed over passes; extra sprites are dropped and counted (`RenderStats::dropped`).

**Per pass pipeline** (`BatchPlan::build`, pure CPU, no wgpu):

1. **Filter.** Keep an item when `item.mask & camera.layers != 0` and its bounding box intersects the pass's viewport rectangle in physical pixels. The box of the transformed unit quad is `center +/- 0.5 * (|a| + |c|, |b| + |d|)` (about ten flops) once the affine has been composed with the camera (or the screen scale).
2. **Key.** One `u64` per kept item: `layer` biased to `u16` (bits 63..48) | `depth` as an order-preserving `u32` (bits 47..16) | the 16-bit **group hint** (bits 15..0). Sprites use `0..=0xFFFE`; **`0xFFFF` is reserved for text**, so at an equal layer and depth text always sorts after sprites (the hint of a sprite is reduced modulo `0xFFFF`, never 0xFFFF). The depth is first passed through `view.depth_mode` (H1; `DepthMode::Z` returns it unchanged), and `-0.0` is normalized to `+0.0` (`depth + 0.0`) so equal depths tie. The depth transform is `b ^ (((b as i32) >> 31) as u32 | 0x8000_0000)` on `f32::to_bits`. A non-finite depth is treated as 0 and counted. The sort sorts `(key, index)` pairs with `sort_unstable`, so the submission index breaks ties and the result is deterministic. The group hint is a cheap mix of the item's `BatchKey` (v1: the texture slot index; Stage 4 adds the normal map and the material): it only brings equal batches next to each other inside one (layer, depth) and never overrides depth. The key's width and layout are settled in H1.
3. **Instances.** Walk the sorted pairs, write `SpriteInstance`s, and start a new **run** when the `BatchKey` (v1: `BatchKey { texture }`, an exact comparison, not the hint) or the `clip` changes or the kind switches between sprite and text. A run is `Run { kind, batch, clip, range }`. A hint collision therefore costs at most an extra run split, never a wrong image or a wrong order.
4. **Text.** Consecutive text items form one text batch (below).

Because `depth` is an arbitrary `f32` chosen by the caller, isometric depth sorting with height levels needs no new sprite field: the iso code computes `depth` from tile coordinates and height, either in extraction or through the pass's `DepthMode` (H1). A y-sorted top-down game uses `depth = z - y * k` (open question 6). A sidescroller uses `layer` for parallax planes.

**Render pass.** One `RenderPass` per `Pass::Camera`, in the sorted pass order of "Frame plan". Instances are in target pixels, so the pass sets the **viewport explicitly to the whole target** (`set_viewport(0, 0, target_w, target_h, 0, 1)`) and the **camera's rectangle with the scissor**: `set_scissor_rect(viewport_px)` at the start of the pass, and for each run `viewport_px` intersected with the run's `clip` (an empty intersection skips the run; glyphon's `TextBounds` get the same intersection). So nothing a camera draws can leave its viewport. The pass binds its instance range once, a camera uniform bind group (group 0, dynamic offset into one uniform buffer with a 256-byte slot per pass; the slot is keyed by the pass's `slot`, its position in the sorted list before `draw_cameras_to_surface` drops anything, which counts every pass kind; see "Frame plan") and, per run, `set_bind_group(1, texture)`, the scissor, `draw(0..4, range)`. Debug lines are drawn last in each pass, always on top.

**Clear semantics.** A render target must start defined, so the first pass on each target (lowest `order`) uses `LoadOp::Clear` with its own `clear` color, or `RendererConfig::clear_color` when that is `None`; this clears the **whole target**. Later passes on the same target use `LoadOp::Load`. (When no pass targets the surface but an overlay is set, the frame plan adds one clear pass, rule 4 of "Frame plan".) A later pass with `clear: Some(c)` fills **only its viewport**: its first draw is a scissored solid-color quad (`white_texture` tinted `c`, through the normal sprite pipeline, so a `c` with alpha below 1 blends over what is there). A minimap camera with a smaller `viewport` and a clear color therefore replaces only its own rectangle.

**Where the camera is applied.** On the CPU. The affine is composed with the camera once in step 1 (kept in a scratch array for steps 2 and 3). The instance affine maps the unit quad straight to **physical pixels of the target**: for `Space::World` the CPU multiplies the item's affine by `CameraView::view_from_world`, for `Space::Screen` by a uniform scale of `scale_factor`. So the shader has one code path for both spaces and no view matrix, the cull test is a plain rectangle test in pixels, and pixel-perfect snapping (rounding the quad's corner to a whole pixel) is a CPU `round`. The per-pass uniform holds only `target_size: vec2<f32>` (padded to 16 bytes) for the pixel-to-clip conversion, in one uniform buffer with a 256-byte slot per pass.

### Camera

```rust
/// Plain data. In Stage 2, `Camera2d` holds everything except `center` and `rotation`,
/// which come from the entity's `GlobalTransform2d`.
#[derive(Clone, Debug)]
#[non_exhaustive]                     // with `Default` (the defaults named below); Stage 4 adds `virtual_resolution` and a depth mode (H1, H4)
pub struct Camera {
    pub center: Vec2,                 // world point at the middle of the viewport
    pub rotation: f32,                // radians, counter-clockwise
    pub scaling: Scaling,
    pub zoom: f32,                    // multiplier on top of `scaling`, > 0, default 1
    pub viewport: Rect,               // normalized 0..1 inside the target; default (0,0)-(1,1)
    pub pixel_perfect: bool,          // default false
    pub clear: Option<Color>,         // Some: fill this viewport (first pass: whole target); None: keep the target (see Clear semantics)
    pub layers: u32,                  // visibility mask, default 1
    pub order: i32,                   // ascending: lower renders first
}
pub enum Scaling {
    PixelsPerUnit(f32),               // logical px per world unit at zoom 1 (default 1.0)
    FixedHeight(f32),                 // the viewport shows this many world units vertically
    FixedWidth(f32),
    Fit(Vec2),                        // this world area is fully visible, extra shown on one axis
}
#[non_exhaustive]
pub struct CameraView {               // the resolved camera for one pass
    pub view_from_world: Affine2,     // world -> physical px of the TARGET (top-left, y down)
    pub screen_from_target: Affine2,  // target physical px -> logical window px. `resolve` sets scale(1 / scale_factor); a camera drawn into an offscreen target that is later presented in the window sets the presentation transform (H4)
    pub depth_mode: DepthMode,        // how the sort key derives depth from an item (H1); `Z` in v1
    pub viewport_px: Rect,
    pub target_size: UVec2,
    pub scale_factor: f32,
    pub pixel_snap: bool,
    pub clear: Option<Color>, pub layers: u32, pub order: i32,
}
#[non_exhaustive]
pub enum DepthMode { Z }              // v1: the item's `depth` unchanged. Stage 4 adds y-sort and isometric height-level variants (SP12)
impl Camera {
    pub fn resolve(&self, target_size: UVec2, scale_factor: f32) -> CameraView;
}
impl CameraView {
    pub fn world_to_screen(&self, p: Vec2) -> Vec2;    // logical window px: MCP clicks, UI anchored to the world. = screen_from_target * view_from_world * p
    pub fn screen_to_world(&self, p: Vec2) -> Vec2;    // the inverse; with a letterboxed or integer-scaled presentation (H4) it still maps window clicks to the world
    pub fn visible_rect(&self) -> Rect;                // axis-aligned bound of the visible world area
}
```

**Resolution of the scale.** `ppu` (physical pixels per world unit) is `PixelsPerUnit(n)`: `n * scale_factor`; `FixedHeight(h)`: `viewport_px.height / h`; `FixedWidth(w)`: `viewport_px.width / w`; `Fit(s)`: the smaller of the two. Then `ppu *= zoom`.

**Pixel-perfect mode.** When `pixel_perfect` is true: `ppu` is rounded to the nearest integer and is at least 1 (integer scaling: one texel is a whole number of screen pixels); `center` is snapped to a multiple of `1 / ppu` (whole physical pixels, no sub-pixel shimmer while scrolling); and each sprite's quad corner is rounded to a whole physical pixel on the CPU when the instance is built, which removes the one-pixel jitter between neighbouring sprites. It composes with `Filter::Nearest` textures. Not in v1 (Stage 4, SP12): virtual resolution, the integer-scaled upsample blit from a low-resolution render texture, sub-pixel camera and pixel-perfect rotation. Their hooks (a render texture with a chosen filter, `CameraView::screen_from_target`, and the `Pass::Fullscreen` slot after the last camera) are settled in H4 of "Render v2 hooks", so they are additive (INTERVIEW #19).

**Several cameras.** `render` takes `&[Pass]` (cameras are `Pass::Camera`) and sorts them by `order`. A second camera with `clear: None` draws on top of the first (HUD, minimap with a smaller `viewport`). Each pass filters the same `DrawList` by its `layers` mask and view, so the list is built once per frame.

### Text

Crate: **`glyphon` 0.12** (rendering) over **`cosmic-text` 0.19** (shaping, bidi, line breaking, font fallback, glyph rasterization with `swash`). Alternatives rejected: `fontdue` and `ab_glyph` (no shaping, no bidi, no fallback, so no complex scripts and no emoji), writing our own atlas on `cosmic-text` (about 600 lines of atlas and pipeline code that `glyphon` already has and tests; it remains the fallback if `glyphon` lags a `wgpu` major release; the crate boundary `text.rs` hides the choice). The `glyphon`/`cosmic-text` types never appear in the public API.

```rust
pub struct FontId(u32);
impl Renderer {
    pub fn load_font(&mut self, ttf_or_otf: Vec<u8>) -> Result<FontId, RenderError>;
    /// Hot reload: the same `FontId` now means the new font. See "Replacing a font" below. A stale id is `RenderError::StaleHandle`; on a parse error the old font stays.
    pub fn replace_font(&mut self, font: FontId, ttf_or_otf: Vec<u8>) -> Result<(), RenderError>;
    pub fn font_revision(&self, font: FontId) -> Option<u32>;   // 0 when loaded, +1 per replace
}

pub enum TextAlign { Left, Center, Right, Justify }
pub enum TextWrap { None, Word, WordOrGlyph }   // None: one line per '\n', never wraps

pub struct TextStyle {
    pub font: FontId,
    pub size: f32,                  // em size in the item's space (world units or logical px)
    pub line_height: f32,           // multiplier of `size`, default 1.2
    pub color: Color,               // linear, tint of the glyphs
    pub align: TextAlign,           // default Left
    pub wrap: TextWrap,             // default None
    pub max_width: Option<f32>,     // layout width in the item's space; None = unbounded. With wrap None it is the ALIGNMENT width: no wrapping, Center and Right align inside it
}
pub struct TextDraw<'a> {
    pub text: &'a str, pub style: TextStyle,
    pub position: Vec2,             // item space: where `anchor` of the laid-out block sits
    pub anchor: Vec2,               // (0,0) top-left of the block .. (1,1) bottom-right; default (0,0)
    pub scale: f32,                 // uniform, default 1; text cannot be rotated in v1
    pub layer: i16, pub depth: f32, pub space: Space, pub mask: u32, pub clip: Option<Rect>,
}
impl Renderer { pub fn measure_text(&mut self, text: &str, style: &TextStyle) -> Vec2; } // laid-out size, item units
```

- **Replacing a font (asset hot reload).** What glyphon 0.12 and cosmic-text 0.19 allow decides the design. `TextAtlas::trim` only clears the atlas's in-use set and frees nothing; glyphs are evicted least-recently-used only when the atlas is full, and growing the atlas re-uploads every cached glyph. cosmic-text's `FontSystem` keeps every face it ever loaded in its `font_cache`, so removing a face from the font database does not free it. glyphon exposes no count of glyph misses. A replace therefore **rebuilds the text state** instead of invalidating it piece by piece, and `exitium_render` does this itself inside `replace_font`: no caller, store or UI flushes anything. The renderer keeps each font's bytes in a table `FontSlot { bytes: Arc<[u8]>, family, revision }` keyed by `FontId`; `replace_font` then does, in order:
  1. Parse the new bytes into a scratch `fontdb::Database`. fontdb returns no face for bytes that are not a font (it does not error), so **zero faces returns `RenderError::BadFont` and changes nothing**.
  2. Store the new bytes in the slot (dropping the old `Arc`) and bump `revision`.
  3. Build a **new `FontSystem`** (`new_with_locale_and_db`, empty database), add the bytes of every live slot, and drop the old one. This is the only way to free the replaced faces, so the leak is bounded: after a replace the process holds each live font once, not once per reload, however many reloads happen.
  4. Clear the shaped-buffer cache (its `Buffer`s refer to face ids of the old `FontSystem`) and reset the GPU half of text (`TextGpu::reset`, written by I10): a new `TextAtlas` and a new pool of `TextRenderer`s (the shared `Cache` and the per-pass `Viewport`s stay). The new atlas holds none of the old glyphs and starts at glyphon's initial size.

  **Cost.** Every loaded font is re-parsed (the sum of the per-font ceilings in the next bullet), and the visible text is re-shaped and re-rasterized once while the atlas regrows: one frame with a hitch of about `visible_labels x 20 us` (1,000 labels: about 20 ms), then warm again. That is acceptable for a hot reload, which is a development action, and it is bounded. `load_font` (a first load) does none of this: it only adds faces to the database. `font_revision` is public and the stores emit `RenderAssetReplaced`, so UI glue can drop its text-measure cache.

  **Counting and testing.** `RenderStats::shape_misses` is the number of shaped-buffer cache misses, counted by our own cache (not read from glyphon). Two `#[cfg(test)]` accessors make the claims testable: `Renderer::font_face_count()` (faces in the live `FontSystem`) and `Renderer::atlas_generation()` (incremented when the atlas is rebuilt).
- **Cost of loading a font.** `load_font` and `replace_font` parse the file and index its faces; they do not shape or rasterize anything. Provisional ceiling **`2 ms + 3 ms per MB beyond the first 0.2 MB`**: a 200 KB Latin font is at most 2 ms, a 5 MB CJK font at most 16.4 ms (`2 + 3 x 4.8`). Measured by `text/load_font` (below). A **replace** pays the sum of this for every loaded font plus the re-shape above. The first use of a new font costs shaping and rasterization of what is drawn, covered by `text/shape_cold`.
- **Fonts.** Only fonts passed to `load_font` exist. System fonts are **not** loaded: the web has none, scanning them costs hundreds of ms at startup, and results must match across machines. `FontSystem` is built with `FontSystem::new_with_locale_and_db("en-US", empty fontdb)` and fonts are added to that database. Drawing text before any font is loaded returns nothing and counts `RenderStats::text_no_font`. The engine's default font and its license are open question 7.
- **Shaped-buffer cache.** The expensive step is shaping, so it is cached: `HashMap<u64, Cached>` keyed by a hash of `(text, font, size, line_height, align, wrap, max_width)` (std `DefaultHasher`, 64 bits: a collision among 10^4 live entries is below 10^-11). `Cached` holds the `glyphon::Buffer` and `last_used_frame`. An unchanged label costs one hash and one lookup per frame. Entries unused for 120 frames are dropped by a sweep every 64 frames. Layout is done at the unscaled size, so zooming reshapes nothing; only glyph rasterization depends on the zoom.
- **World text** is drawn with `TextArea.scale = ppu * scale`, quantized to steps of 1/16 so a continuous zoom does not fill the glyph atlas with near-identical sizes. **Screen text** uses `scale_factor * scale`. Position, anchor and layout width are converted to physical pixels per pass.
- **Atlas and batches.** One `glyphon::TextAtlas` (format = the renderer target format, `ColorMode::Accurate` because targets are sRGB views, color glyphs such as emoji supported) shared by every text batch of every pass, and `atlas.trim()` after each frame. **Text state is per pass.** glyphon's `TextRenderer::prepare` writes its vertex buffer with `queue.write_buffer`, and its `Viewport` is a uniform written the same way; all passes of a `render` call share one submit, so every write lands before any pass runs, and a second pass that prepared the same renderer or viewport would overwrite the first pass's data. Therefore the renderer owns a pool of `TextRenderer`s **per (pass index, text batch index)** and one `glyphon::Viewport` **per pass index** (updated with that pass's target size), grown on demand and reused across frames. A text batch is a run of consecutive text items in one pass's sorted order. `MAX_TEXT_BATCHES = 64` is **per pass**; beyond it, later text in that pass joins the last batch (its order against sprites is then approximate) and `RenderStats::text_batches_merged` counts it. Real UIs sort to far fewer batches. Shaped buffers are cached once and shared: only `prepare` (positions, scale, bounds) is repeated per pass, so a screenshot or editor pass costs one more prepare of the visible text, not a re-shape.
- **Clipping.** `clip` maps to glyphon's `TextBounds`; the same rectangle becomes the scissor rect for sprite runs.
- **Wasm.** `glyphon` and `cosmic-text` compiled for `wasm32-unknown-unknown` in the version check. `cosmic-text`'s default features (`std`, `swash`, `fontconfig`) cannot be turned off through `glyphon`; `fontconfig` is Linux only and unused because no system fonts are scanned.
- **Measuring.** `measure_text` shapes through the same cache and returns the laid-out size. The UI crate cannot call it (no `ui -> render` edge in the crate graph); how a measure function reaches it is open question 4.

### Debug lines and shapes

`DebugDraw` is a resource-like buffer for physics debug, bounds and editor gizmos. It is separate from `DrawList` so it can be turned off globally and so it never takes part in sorting: it is drawn last in every pass, on top.

```rust
pub struct DebugDraw { /* Vec<LineInstance> */ }
impl DebugDraw {
    pub fn line(&mut self, a: Vec2, b: Vec2, color: Color, width_px: f32, space: Space);
    pub fn polyline(&mut self, points: &[Vec2], closed: bool, color: Color, width_px: f32, space: Space);
    pub fn rect(&mut self, rect: Rect, color: Color, width_px: f32, space: Space);   // outline
    pub fn circle(&mut self, center: Vec2, radius: f32, color: Color, width_px: f32, space: Space);
    pub fn cross(&mut self, p: Vec2, half_size: f32, color: Color, width_px: f32, space: Space);
    pub fn clear(&mut self);
}
```

Lines have a **constant pixel width** (default 1 px, set per call) whatever the zoom. A line is one instance of 24 bytes: `a: Float32x2`, `b: Float32x2`, `color: Unorm8x4` (sRGB-encoded), `width: Float32`. The endpoints are already in physical pixels (the camera is applied on the CPU when the pass is built, as for sprites) and the vertex shader expands the segment to a quad perpendicular to it. `circle` and `polyline` are tessellated into segments on the CPU: a circle uses `clamp(ceil(radius_px / 4), 8, 64)` segments. Filled rectangles are not debug draws: use a `SpriteDraw` with `texture: None`. The cap is `MAX_DEBUG_LINES = 1 << 18` per frame summed over passes (physics emits up to 100,000 by default); extra lines are dropped and counted in `RenderStats::dropped`. The cap is shared by **every pass of the frame, the editor's `SceneView` pass included**, so the lines of a frame with a scene view and a game camera are counted twice against it. A screenshot pass draws debug lines only when the request sets **`include_debug: true`**; without it the capture shows none.

### Render to texture and capture (MCP screenshots)

```rust
pub enum RenderTarget { Surface, Texture(Handle<Texture>) }

#[non_exhaustive]
pub struct CameraPass { pub view: CameraView, pub target: RenderTarget, pub order: i32 }   // `new(view, target)`, order 0
impl CameraPass { pub fn new(view: CameraView, target: RenderTarget) -> Self; }

/// One unit of GPU work in a frame. v1 has one variant; Stage 4 adds `Lights` and `Fullscreen` (see "Render v2 hooks").
#[non_exhaustive]
pub enum Pass { Camera(CameraPass) }
impl Pass { pub fn order(&self) -> i32; }

pub enum FrameStatus { Presented, Rendered, Skipped(SkipReason) }   // Rendered: no surface wanted (headless, or texture passes only); Skipped: surface wanted but unavailable, texture passes still ran
pub enum SkipReason { Minimized, Occluded, Timeout, Reconfigured }

impl Renderer {
    pub fn render(&mut self, list: &DrawList, debug: &DebugDraw, passes: &[Pass]) -> Result<FrameStatus, RenderError>;

    pub fn request_capture(&mut self, texture: Handle<Texture>) -> Result<CaptureId, RenderError>;
    pub fn poll_captures(&mut self) -> Vec<(CaptureId, Result<Capture, RenderError>)>;   // non-blocking
}
pub struct Capture { pub size: UVec2, pub rgba8: Vec<u8> }   // sRGB-encoded, tightly packed, top-left origin
```

`render` sorts the passes by `(order, slice position)` (stable), records all of them (surface and texture targets) in **one** command encoder, submits once and presents once. A pass with `Texture` target needs no surface, which is the headless path. The sequence of decisions (is the surface acquired, is a clear pass needed, is the overlay called) is two pure functions, `plan_frame` and `FramePlan::resolve` (next section), so it is unit-tested without a GPU. The MCP screenshot is: create (or reuse) a render texture of the requested size, `render` the current `DrawList` into it through a `CameraPass` (the same list, any camera, any size, independent of the window), `request_capture`, and later collect the result from `poll_captures`.

**Readback.** `request_capture` records `copy_texture_to_buffer` at the end of the frame that renders the texture (rows padded to `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`, 256), then `map_async` after submit. `poll_captures` is called once per frame: on native it calls `Device::poll` without waiting and drains finished buffers; on the web the browser event loop completes the mapping between frames and `poll` just drains. Results are delivered one or two frames after the request; callers never wait inside a frame, which is the only model that works on the web (it cannot block on a map). Wgpu 30's `get_mapped_range` returns a `Result`, so a failed map becomes `RenderError::Capture`. The crate removes the row padding and, when the target format is BGRA, swaps channels, so `Capture` is always RGBA8. **Limits.** At most `MAX_PENDING_CAPTURES = 2` captures may be in flight (requested and not yet collected by `poll_captures`); a third `request_capture` returns `RenderError::CaptureBusy` and records nothing, so readback memory is bounded (a 2048 x 2048 RGBA8 capture is 16 MiB, a 4096 x 4096 one 64 MiB). The mapped buffer is released as soon as `poll_captures` has copied it out. Scene screenshots should use an opaque clear color. The window surface itself (what the human sees including editor panels) is not captured in v1 (open question 9).

### Frame plan and the surface overlay (AM1)

The standalone editor draws its egui UI into the window's surface, on top of the game, in the same frame. It needs three things from render: a hook that runs after the last camera pass and before submit, a way to switch the game's own surface passes off (the editor shows the game inside a panel, from a render texture), and the surface format. They are small, they are on `Renderer`, and they change nothing when unused.

```rust
/// Records extra GPU work onto the surface after every pass. Implemented by the editor (egui-wgpu).
pub trait SurfaceOverlay: Send + Sync + 'static {
    fn record(&mut self, cx: OverlayCx<'_>);
}

#[non_exhaustive]
pub struct OverlayCx<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,   // the frame's one encoder; do not finish or submit it
    pub target: &'a wgpu::TextureView,           // the acquired surface texture, viewed in `format` (the overlay format, not the sRGB format the cameras use)
    pub format: wgpu::TextureFormat,             // `Renderer::overlay_format()`
    pub size_px: UVec2,                          // physical pixels
    pub scale_factor: f32,
}

impl Renderer {
    pub fn set_overlay(&mut self, overlay: Option<Box<dyn SurfaceOverlay>>);   // None removes it
    pub fn set_draw_cameras_to_surface(&mut self, on: bool);                   // default true
    pub fn target_format(&self) -> wgpu::TextureFormat;                        // F: the view format every camera pass renders in (sRGB); `RendererConfig::headless_format` when there is no surface
    pub fn overlay_format(&self) -> wgpu::TextureFormat;                       // O: F without its sRGB suffix (non-sRGB view of the same surface); what the overlay must be built for
}
```

`RenderSettings.draw_cameras_to_surface` (Stage 2) is the resource form of the same flag: `render_frame` copies it into the renderer every frame, so an MCP tool or the editor flips it with a normal command. `set_overlay` has no resource form because a boxed trait object is not data: the editor owns the overlay and calls `set_overlay` once at startup. `OverlayCx` is `#[non_exhaustive]` and is only ever built by render (its constructor is crate-private and it has no `Default`), so a field can be added later without breaking the editor.

**The frame plan.** What `render` does with the surface is decided in **two** pure steps in `frame.rs`, because whether the surface can be acquired is only known after `Surface::get_current_texture` has run, and whether to call it is itself one of the decisions:

1. `plan_frame(&PlanInputs) -> FramePlan` runs **before** acquiring. `PlanInputs` is `{ has_surface, overlay_set, draw_cameras_to_surface, passes: &[(order, RenderTarget)] }`; `FramePlan` is `{ passes: Vec<PlannedPass { source, slot }>, want_surface }`. It knows nothing about the swapchain.
2. `FramePlan::resolve(self, acquire: Acquire) -> FrameSteps` runs **after**, with `Acquire` = `NotWanted | Acquired | Unavailable(SkipReason)`. The `CurrentSurfaceTexture` mapping in "Present modes and resize" is a third pure function that produces `Acquire` (`Success` and `Suboptimal` give `Acquired`; `Outdated` and `Lost` give `Unavailable(Reconfigured)`; `Timeout` and `Occluded` give the matching reason; a zero size gives `Unavailable(Minimized)` without calling the surface). `FrameSteps` is `{ run: Vec<PlannedPass>, surface_clear, call_overlay, status }`.

`render` is therefore: sort the passes, `plan_frame`, call `get_current_texture` **only if** `want_surface`, `resolve`, then record `run`, the clear pass if `surface_clear`, and the overlay if `call_overlay`. The rules, which are the whole contract (each is tested in the step that owns it):

1. *(plan)* Passes run in `(order, slice position)` order (stable sort). A planned pass's **`slot`** is its position in this sorted list **before** rule 2 removes anything. Per-pass resources (uniform slot, text-renderer pool, viewport) are keyed by `slot`, so setting `draw_cameras_to_surface` to false removes the surface passes without re-keying the texture passes, and toggling it back reuses every cached resource. Instance ranges are laid out per surviving pass in the frame, not per slot.
2. *(plan)* With `draw_cameras_to_surface == false`, passes whose target is `Surface` are dropped. Texture passes are unaffected, so screenshots and the scene view keep working.
3. *(plan)* `want_surface` is true if and only if the renderer has a surface **and** (an overlay is set **or** at least one surface pass survives rule 2). In particular, with an overlay set, `render` acquires the surface even when every pass targets a texture.
4. *(resolve)* If the result is `Acquired` and no surface pass is in `run`, `surface_clear` is true: `render` records one empty pass that clears the surface (clear colour from `RendererConfig`) before the first overlay call, so the overlay can always load (`LoadOp::Load`) and never sees an uninitialised surface. When a surface pass is in `run`, the first of them clears the surface as in v1.
5. *(resolve)* If the result is `Unavailable(reason)`, the surface passes are removed from `run`, `surface_clear` and `call_overlay` are false, and the status is `Skipped(reason)`. **Texture passes still run** and their captures are still produced, which is how an MCP screenshot works while the editor window is minimised.
6. *(resolve)* `call_overlay` is true if and only if the result is `Acquired` and an overlay is set. The overlay is called **once**, after the last pass (by order) has been recorded and before the encoder is finished and submitted. A renderer without a surface (headless) never calls it.
7. *(resolve)* Status: `Presented` when the surface was acquired and presented; `Skipped(reason)` when it was wanted but is unavailable; `Rendered` when it was not wanted (headless, or only texture passes and no overlay).
8. **Relation to v1.** For every input that v1 defines (no overlay, `draw_cameras_to_surface == true`, the surface acquired or not wanted), `FrameSteps` is what v1 does: the same passes in the same order, the same clearing and the same status. Rule 5 is new behaviour for an input v1 left open (texture passes while the surface is skipped; v1 said only that the frame is skipped).

Tests (I20): `plan_frame` over `has_surface x overlay_set x draw_cameras_to_surface x {no, texture-only, surface-only, mixed} passes`, and `resolve` over each `Acquire` value for each plan, asserting rules 1 to 7, the slot-stability property of rule 1 (the slots of texture passes are equal with the flag on and off) and the v1-equivalence of rule 8. The overlay itself cannot run headless (no surface), so the end-to-end check is one Windows test that creates a hidden window, sets an overlay that clears the target to a known colour, renders a texture-only frame and reads the surface back with a copy recorded by the test overlay.

**Blocker note for the editor spec.** `editor-mcp-mvp.md` AM1 says the overlay is "Blocked by render I16 and I17". That is stale: the overlay, `draw_cameras_to_surface` and `target_format`/`overlay_format` come from **I20** (this spec), which is the render blocker for the editor's overlay issue (ED8). I16 and I17 are plugin issues and are not needed for it. SP8 aligns the editor spec's text; the dependency is stated here so it does not depend on that edit.

The overlay runs on the main thread inside `render`, so it counts toward the frame's CPU time and a slow overlay is visible in the profiler's `render_frame` span. A panic in `record` propagates like any other panic in `render`. **Overlay format (decided here, not deferred to ED8).** egui-wgpu 0.37 warns and takes a slower shader path when its target format is sRGB, so the overlay does not receive the sRGB view the cameras use. Render creates a second view of the same surface texture in the **overlay format** `O = F.remove_srgb_suffix()` (for example `Bgra8Unorm`) and passes it as `OverlayCx.target` with `OverlayCx.format = O`. This needs `view_formats` to contain **both** `F` and `O` when the surface is configured, which I1's format selection does (see "Surface format"), so no later issue has to change the configuration. When the configured surface format is already non-sRGB (the WebGPU canvas case), `O` is that format and `F` is its sRGB view. The overlay writes gamma-space colours unconverted into `O`, which is what egui expects. The editor builds its egui-wgpu renderer with `Renderer::overlay_format()`, never with `target_format()`. A headless renderer has no surface and so no overlay call; `overlay_format()` still returns the sibling of `headless_format` so the editor can be constructed in a test.

### Render v2 hooks (Stage 4)

The full MVP restores 2D lighting, pixel-art presentation, user materials and isometric depth (roadmap SP12) and tilemap rendering (SP11). They are built **inside `exitium_render`**, in Stage 4, and each needs a seam in the v1 design. The seams are fixed here and in the sections above so that v1 code never has to be reshaped, and Stage 4 adds fields, enum variants and files, never a signature change. Five hooks, each with the v1 provision and the Stage 4 addition:

| Hook | v1 provision (this spec) | Stage 4 addition (inside `exitium_render` only) |
|---|---|---|
| **H1 depth sort key** | The key is a `u64`: layer 16 bits, depth 32 bits, group hint 16 bits (step 2). Depth is the order-preserving `u32` image of an `f32` (sign flip, `-0.0` normalised by adding `0.0`). Sprite hints are `0..=0xFFFE`; `0xFFFF` is reserved for text. `CameraView.depth_mode: DepthMode` chooses what depth means. The width is the decision: 32 bits of depth keep y-sort and isometric `x + y` ordering exact to well below a texel over a 100,000-unit world, and 16 bits of layer and of hint are enough (65,536 layers; the hint is a heuristic). | New `DepthMode` variants (`YSort`, `Iso`) and their per-sprite depth formula, owned by SP12. No new key bits. |
| **H2 normal-map slot** | `ColorSpace::Linear` textures (normal maps are data, not colour). Bind group 2 is reserved for the normal map and is not created in v1. `BatchKey` is an exact comparison, so adding a field splits runs correctly. | `SpriteDraw.normal: Option<Handle<Texture>>` (same `rect` as the colour texture, which must have the same size), a group-2 binding, and a lit pipeline variant chosen per run. |
| **H3 ordered extra passes (lights)** | `Pass` is an enum and every pass has an `order` in one shared space; per-pass resources (uniform slot, instance range) are keyed by the pass's `slot` (its position in the sorted list before `draw_cameras_to_surface` drops surface passes, so toggling it never re-keys a texture pass), not by its kind. `plan_passes` is the one place passes are built from cameras (shared by `extract_cameras` and `Screenshots`). | `Pass::Lights(LightPass { lights, ambient, target, order })` renders light shapes additively into a light texture; a `Pass::Fullscreen` pass composites it over the scene. Lit sprite pipelines read lights through bind group 0. The algorithm (forward, tiled or accumulation) belongs to the lighting issue; the hook guarantees only the order space, the per-pass resources and that a later pass may sample an earlier pass's texture. |
| **H4 offscreen virtual-resolution target** | `create_render_texture_with(size, &RenderTextureDesc { filter, label })` (use `Nearest` for pixel art); `RenderTarget::Texture` passes are ordinary; `CameraView.screen_from_target: Affine2` maps window pixels to target pixels, so `screen_to_world` stays correct under letterbox and integer scaling (the editor and UI click mapping need it from the start). | `Pass::Fullscreen(FullscreenPass { source, target, scaling, order })` in `present.rs`: a blit from the virtual-resolution texture to the surface with `Scaling::{Fit, IntegerScale, Stretch}` and letterbox bars; it also sets `screen_from_target`. |
| **H5 user material slot** | The instance has a reserved `user: u32` word (offset 44) that the fragment shader can read; bind group 3 is reserved for materials; pipelines are cached by a key struct that can gain a `material` field. | `SpriteDraw.material: Option<Handle<Material>>`; `Renderer::create_material(&MaterialDesc { wgsl, uniform_bytes, textures })` (`#[non_exhaustive]`; `textures: Vec<Handle<Texture>>`, at most 4, bound in group 3 after the uniform: a palette-swap lookup table is one) validating the WGSL with naga (already a wgpu dependency) and returning `RenderError::Shader(String)` on failure; a pipeline per `(material, format, blend, normal/lit variant)`; the run key includes the material. Only WGSL, which WebGPU takes natively, so there is no shader-translation dependency. |

**Bind groups (WebGPU's default limit is 4).**  0 = pass uniform (and the light data in Stage 4), 1 = colour texture, 2 = normal map, 3 = material. v1 creates only 0 and 1. **Replacing a texture rebuilds what refers to it:** `replace_texture` creates a new view, so it rebuilds the group-1 bind group in place, and the Stage 4 groups 2 and 3, which also hold texture views (a normal map, a material's textures), are cached by `(handle, texture_revision)` and rebuilt when the revision of a texture they reference changes. A hot-reloaded normal map or palette table therefore shows on the next frame with no action by the caller. The vertex input is 5 locations (0 to 4) in v1; Stage 4 may add at most one more, so the instance stays under 64 bytes.

**Compatibility rule.** Every public plain-data struct that Stage 4 might extend is `#[non_exhaustive]` with `Default` or `new`, so no other crate can write a struct literal that a new field would break: Stage 1 `RendererConfig`, `TextureDesc`, `RenderTextureDesc`, `SpriteDraw`, `Camera`, `CameraView`, `CameraPass`, `TextStyle`, `TextDraw` (`TextDraw::new(text, style)`), `RenderStats`; Stage 2 `Sprite`, `Camera2d`, `Text`, `SceneView`, `RenderSettings`, `RenderPlugin`, `ScreenshotRequest`; and `OverlayCx`, which only render builds (crate-private constructor, no `Default`). Outside the crate such a value is built with `Default` (or `new`) and field assignment. `ImageData`, `Capture`, `Flip` and `ViewOverride` have a fixed shape and stay literal. Every public enum that gains variants is `#[non_exhaustive]` (`Pass`, `DepthMode`, `RenderError`, `RenderAssetKind`). The facade glue that builds `SpriteDraw`, `CameraPass` and the Stage 2 types (tilemap, UI, assets: all in `exitium`) therefore compiles unchanged when Stage 4 lands, and each Stage 4 issue touches one crate. The text format fills missing fields from defaults, so the additive component fields Stage 4 adds (`Sprite.normal`, `Sprite.material`, a `Light2d` component) load old scenes. An external-crate doc-test builds each listed type with `Default` and assignment, which keeps the attribute honest.

**Wiring and dependencies of this amendment.** None. No crates.io dependency is added or changed (naga arrives with wgpu; egui-wgpu belongs to the editor spec and is declared there), and no crate edge is added: `exitium_editor -> exitium_render` already exists from intake-2 (C3), and tilemaps reach render as ordinary `SpriteDraw` values through facade glue with no edge to `exitium_tilemap`. Intake-1 declares the Stage 4 module files (`material.rs`, `light.rs`, `present.rs`) once, empty, so Stage 4 issues need no manifest or `lib.rs` edit outside their crate.

### Threading

| Thread | Role |
|---|---|
| Main (the app thread) | Owns `Renderer`. Builds `DrawList`, calls `render`, `resize`, `poll_captures`. All of it is `&mut self`, single-threaded in v1. |
| wgpu internals | wgpu may use its own threads or the driver's; we do not manage them. |
| Web | Single thread. `Renderer::new` is driven by the browser's event loop; there is no `block_on`. |

- `Renderer` is `Send + Sync` on native and, on the web, through the `fragile-send-sync-non-atomic-wasm` feature, so it can be an ECS resource. This is checked, not assumed: a scratch crate asserted `Send + Sync` for `wgpu::{Device, Queue, Surface, Texture, RenderPipeline}` and `glyphon::{FontSystem, SwashCache, Buffer, TextAtlas, TextRenderer, Viewport, Cache}` and compiled for native Windows and for `wasm32-unknown-unknown`. The crate pins it with a compile-time assertion in `renderer.rs` (`const _: fn() = || { fn assert<T: Send + Sync>() {} assert::<Renderer>(); };`), added by I10 (when the glyphon types enter `Renderer`), so the native build and the CI wasm build both compile it. If a later glyphon release makes one of its types `!Sync`, the assertion fails the build and `text.rs` wraps that type in an uncontended `Mutex` instead of weakening the bound.
- `DrawList`, `DebugDraw` and `Pass` own their data, so **pipelined rendering is possible later without an API change**: the main thread fills list N+1 while a render thread runs `render(list N)`. It is not part of the MVP (it would need `exitium_tasks` integration). `BatchPlan::build` is a pure function of `&DrawList` and a `CameraView`, so extraction and batching can also be parallelized by the ECS executor later.
- The crate reads no clock (`Instant::now` panics on wasm); `RenderStats` holds counters only. Timing comes from the profiler's system spans (Stage 2) and from criterion.
- Locks: none of our own. Shared mutable state is none.

### Stage 2: the plugin

Stage 2 is a `plugin` module in the same crate. The exact `App`, `Plugin`, schedule-builder and ordering signatures belong to the app spec (**#106, an open dependency**). In **contract terms** the render plugin registers the following into the named schedules and reads `Transform2d` and `GlobalTransform2d`:

| Where | What |
|---|---|
| Components | `Sprite`, `Camera2d`, `Text` (all `Reflect`, below). Registered with the reflection registry. |
| Resources | `RendererSlot(Option<Renderer>)` (not reflected; empty until `init_renderer` fills it, and every render system reads the renderer through it, so a headless app with no renderer runs a frame), `RenderWindow` (not reflected, the window handoff below), `DrawList`, `DebugDraw`, `TextureStore`, `FontStore`, `Screenshots`, `CameraViews`, `SceneViewTarget` (not reflected); `RenderSettings`, `RenderStats`, `SceneView` (reflected). `RenderPlugin::build` inserts every one of them (the slot empty, the others default) at once, as app-platform.md requires, so facade glue can write them as early as `PreStartup`. |
| **System set** | Render exports one: `exitium_render::plugin::sets::RENDER_WINDOW`, a `const SystemSet` in `First` (below). It also uses the app's `EXTRACT` and `RENDER` sets (`exitium_app::sets`). |
| **PreStartup** | Nothing from render. The facade glue copies the platform window into `RenderWindow` here. |
| **Startup** | `init_renderer`: builds the `Renderer` into `RendererSlot`, reading **only `RenderWindow`**: `new_blocking` on native, a future spawned with `wasm_bindgen_futures::spawn_local` on the web, `Renderer::headless` when `RenderPlugin::headless` is set. With no window target yet it does nothing and logs once. |
| **First**, set `RENDER_WINDOW` (after the app's `INPUT`) | `poll_renderer_init` (web: moves the finished `Renderer` into `RendererSlot`; native: retries the init when a window target appears late), `apply_resize` (reads **only `RenderWindow`**: physical size and scale factor to `Renderer::resize`), `apply_render_settings`, `poll_captures`. |
| **FixedUpdate**, **Update** | Nothing. Rendering is variable-rate. |
| **Extract**, set `EXTRACT` | `extract_cameras` (cameras and, when enabled, the scene view to `CameraPass`es, and the `CameraViews` resource), `extract_draws` (sprites and text to `DrawList`), then any facade glue that appends to `DrawList` or `DebugDraw` (assets, physics debug, UI). |
| **Extract**, set `RENDER` (after `EXTRACT`, set by the app) | `render_frame`: renders, presents, fulfills screenshots, clears the lists. |

The app runs `PostUpdate` (transform propagation, physics interpolation) before `Extract` in every frame, so extraction reads settled `GlobalTransform2d` values.

**`RenderWindow`, the window handoff.** Render names no platform type and the platform names no render type (cross-spec ruling (a)). `RenderWindow` is the render-owned meeting point, and the **facade glue** is the only code that writes it. It matches what app-platform.md exposes: `PlatformWindow` is `Clone + Send + Sync + 'static` and implements `HasWindowHandle` and `HasDisplayHandle`, so `RenderWindow` can hold it; the platform plugin inserts `PrimaryWindow(PlatformWindow)` and `WindowInfo { physical_size, scale_factor, focused }` before `PreStartup`.

```rust
/// Raw window and display handles, the physical size and the scale factor. Render reads only this.
#[derive(Resource)]
pub struct RenderWindow { /* source: Option<Arc<dyn WindowSource>>, size: UVec2, scale_factor: f32 */ }
/// Private. Blanket-implemented for every `T: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static`.
trait WindowSource: wgpu::rwh::HasWindowHandle + wgpu::rwh::HasDisplayHandle + Send + Sync + 'static {}
impl Default for RenderWindow { /* no source, size zero, scale_factor 1.0, never 0.0: Startup can read it before the first set_size */ }
impl RenderWindow {
    /// Once, from the glue: `window.set_target(primary_window.0.clone())`. Anything with both raw
    /// handle traits that is `Send + Sync` (a `PlatformWindow`, an `Arc<winit::window::Window>`) works.
    /// It is stored as a handle holder, never as a `wgpu::SurfaceTarget` (see `Send + Sync` below).
    pub fn set_target(&mut self, window: impl wgpu::rwh::HasWindowHandle + wgpu::rwh::HasDisplayHandle + Send + Sync + 'static);
    /// Every frame, from the glue, copied from the platform's `WindowInfo`. PHYSICAL pixels and the scale factor.
    pub fn set_size(&mut self, physical_size: UVec2, scale_factor: f32);
    pub fn size(&self) -> UVec2;  pub fn scale_factor(&self) -> f32;  pub fn has_target(&self) -> bool;
    // crate-private `take_target()`: builds `wgpu::SurfaceTarget::from(Arc<dyn WindowSource>)`. Only `init_renderer`
    // calls it, exactly once, when it builds the Renderer.
}
```

- **Glue (facade, G1a).** `PreStartup`: `set_target` from `Option<Res<PrimaryWindow>>`. `First`, `after_set(INPUT)` and `before_set(RENDER_WINDOW)`: `set_size` from `Option<Res<WindowInfo>>`. Both read `Option<Res<..>>` and do nothing when it is `None`, so a headless app (no window, no renderer) runs a frame without panicking. Ordering the glue before `RENDER_WINDOW` is what makes a resize reach the renderer in the same frame.
- **Units.** `RenderWindow` is physical, like `WindowInfo` and like `Renderer::resize`. Logical pixels (physical divided by the scale factor) are what `Space::Screen` and `CameraView::screen_to_world` use, and what `InputState::cursor_logical` holds, so the cursor-to-world glue passes `cursor_logical` and never the physical position (a physical value would be wrong by the scale factor on a 1.5x display).
- **Zero size.** A zero width or height is legal at startup (a web canvas not yet laid out): `Renderer::new` accepts it, and `render` returns `Skipped(Minimized)` until a non-zero size arrives.
- **`Send + Sync`.** `RenderWindow` must be a `Resource` (`Send + Sync + 'static`) on wasm32 too, so it does **not** store a `wgpu::SurfaceTarget`. In wgpu 30.0.2 that enum has `Canvas(HtmlCanvasElement)` and `OffscreenCanvas(..)` variants under `cfg(web)`, which the `webgpu` feature turns on; they hold a `JsValue` (`PhantomData<*mut u8>`), so the enum is `!Send + !Sync` there. wgpu asserts `Send + Sync` only for `Surface` and `SurfaceConfiguration`, and `fragile-send-sync-non-atomic-wasm` only relaxes the boxed window handle bounds, so it does not rescue the enum. `RenderWindow` stores an `Arc<dyn WindowSource>` instead. `raw-window-handle` 0.6 implements both handle traits for `Arc<H: ?Sized>`, and `wgpu::SurfaceTarget: From<T>` for any `T` with both handles and `Send + Sync`, so `take_target` builds the `SurfaceTarget` from the `Arc` inside `init_renderer`, on the app thread (the only thread on the web). I14 pins `assert::<RenderWindow>()` (`Send + Sync`) with a compile-time assertion next to the one on `Renderer`, built for native and for wasm32. app-platform.md pins `PlatformWindow: Send + Sync` on wasm32 too (its PL3), and names the ECS non-send path as the fallback if winit's web window is not.
- **Web handle.** wgpu 30.0.2's WebGPU backend accepts three raw window handles: `Web` (looks the canvas up with `[data-raw-handle="<id>"]` and panics unless one matches), `WebCanvas` (a pointer to the canvas's `JsValue`, cloned while the surface is created) and `WebOffscreenCanvas`. winit 0.30.13's web window returns `WebCanvas` (`raw_window_handle_rwh_06` in its `window.rs`), so the canvas needs no id or data attribute. The pointer is valid only while the winit window lives; the `SurfaceTarget` built from the `Arc` holds it until the surface exists, and wgpu clones the `JsValue` when it creates the surface. `WebCanvasWindowHandle` is `!Send + !Sync`, which does not matter: the handle is produced and consumed inside `init_renderer` on the one web thread, and only the winit window (`Send + Sync`, asserted by app-platform.md PL3) is stored. A custom holder that returned a `Web` handle would have to set `data-raw-handle` on its canvas.
- **`CameraViews`** (not reflected): rewritten by `extract_cameras` each frame with the resolved `CameraView` of every active camera, sorted by `order`, and kept after `render_frame` (it is not a per-frame list that gets cleared). `CameraViews::main()` is the main camera of the "default camera" rule below. The cursor-to-world glue (app-platform F1) reads it in `First` and so uses the camera as of the previous frame, which is what is on screen.


**`RenderPlugin`.** The one type a game adds to its app:

```rust
pub struct RenderPlugin {
    pub config: RendererConfig,   // present mode, power preference, default clear color
    pub headless: bool,           // true: Renderer::headless, no window, screenshots still work
}
impl Default for RenderPlugin { /* config default, headless false */ }
impl Plugin for RenderPlugin { /* signature from #106; registers every row of the table above */ }
```

I14 creates `RenderPlugin` and registers the types and resources; I15 adds the extraction systems to its `build`; I16 adds the init, resize and settings systems; I21 adds the texture and font stores; I17 adds the screenshot service. Each issue edits `plugin/mod.rs` in turn (one issue at a time in this crate).

```rust
#[derive(Component, Reflect, Clone)]
pub struct Sprite {
    pub image: Option<Guid>,        // stable asset id; None = solid color. See open question 1.
    pub rect: Option<Rect>,         // sub-rect in texels
    pub size: Option<Vec2>,         // None = rect (or texture) size in world units, 1 texel = 1 unit
    pub anchor: Vec2,               // default (0.5, 0.5)
    pub tint: Color,                // default white
    pub flip_x: bool, pub flip_y: bool,
    pub layer: i16, pub z: f32,     // sort key (depth = z in v1)
    pub mask: u32,                  // default 1
    pub visible: bool,              // default true
}
#[derive(Component, Reflect, Clone)]
pub struct Camera2d {
    pub scaling: Scaling, pub zoom: f32, pub viewport: Rect, pub pixel_perfect: bool,
    pub clear: Option<Color>, pub layers: u32, pub order: i32, pub active: bool,
}
#[derive(Component, Reflect, Clone)]
pub struct Text {
    pub text: String, pub font: Option<Guid>, pub size: f32, pub line_height: f32,
    pub color: Color, pub align: TextAlign, pub wrap: TextWrap, pub max_width: Option<f32>,
    pub anchor: Vec2, pub layer: i16, pub z: f32, pub mask: u32, pub visible: bool,
}
/// The editor's scene view (pan, zoom) as plain reflected data, so a human and an agent
/// see and set the same thing. Not an entity: it has no transform in the world.
#[derive(Resource, Reflect, Clone)]
pub struct SceneView {
    pub enabled: bool,              // default false; the editor turns it on
    pub center: Vec2, pub rotation: f32, pub zoom: f32,
    pub scaling: Scaling, pub pixel_perfect: bool,
}
```

- **Why separate component types.** Stage 1 types (`SpriteDraw`, `Camera`, `TextDraw`) hold resolved data (a `Handle<Texture>`, a world `Affine2`) and need no reflection, so Stage 1 depends on `exitium_core` only. The components hold authoring data (a `Guid`, no transform) that reflection can persist (`Handle<T>` is not reflectable, reflect-model spec). Extraction is the explicit conversion. `Camera2d` has no `center` or `rotation`: they come from the entity's `GlobalTransform2d`, so a camera can follow a parent. The enums used as component fields (`Scaling`, `TextAlign`, `TextWrap`) are Stage 1 types; the Stage 2 issue I14 adds `#[derive(Reflect)]` to them, which is possible because the crate depends on `exitium_reflect` from Stage 2 on.
- **Needs from `GlobalTransform2d`** (D1): a 2D affine (translation, rotation, scale) in world space, already propagated through the hierarchy when the extraction schedule runs. The render crate does not use a z coordinate from the transform: order is `layer` and `z` on the component.
- **`extract_cameras`** (through `plan_passes`, below) emits one `Pass::Camera` per active `Camera2d` (target: surface, view from the entity's `GlobalTransform2d`). When `SceneView.enabled`, it also emits one pass built from `SceneView` into the render texture held by `SceneViewTarget` (created and resized by the editor through `Renderer::create_render_texture`); with no `SceneViewTarget` texture, the scene view is ignored.
- **`extract_draws`** iterates `(&Sprite, &GlobalTransform2d)` and `(&Text, &GlobalTransform2d)`, resolves `Guid` to `Handle<Texture>` / `FontId` through `TextureStore` and `FontStore` (hash lookups, cached per frame for repeats), skips `visible == false`, and pushes `SpriteDraw`/`TextDraw`. Missing textures draw nothing and count `missing_textures`. It rebuilds the list every frame; a retained instance cache is a later optimization.
- **`TextureStore` and `FontStore`** hold `Guid -> Handle<Texture>` and `Guid -> FontId`, plus `insert(guid, ImageData, TextureDesc)` and `insert_font(guid, bytes)`, and `TextureStore::size(guid) -> Option<UVec2>` (texture pixels, unscaled) for the UI's image measure. **Replace semantics (asset hot reload).** `insert` and `insert_font` called with a `Guid` that is **already present replace** the stored texture or font: the store keeps the same `Handle<Texture>` or `FontId`, calls `Renderer::replace_texture` or `replace_font` on it, and every sprite or text that resolved to it draws the new asset from the next frame, with no re-extraction by the caller. A new `Guid` creates. A replace that fails (`BadImage`, `TextureTooLarge`, an unparsable font) leaves the old asset in place, returns the error, and emits nothing. A successful replace pushes a `RenderAssetReplaced { guid, kind: RenderAssetKind }` event (`RenderAssetKind::{Texture, Font}`, `#[non_exhaustive]`) (a plain event type delivered through the app's event mechanism, #106), which UI glue (G1d) uses to re-measure and re-layout, because the size of an image or the metrics of a font can change. **Upload queue.** `TextureStore::insert` does not touch the GPU: it records the decoded image in a pending queue (and the new size, so `size(guid)` answers at once), and the store's `flush` system, in set `RENDER_WINDOW` after `init_renderer`, uploads from it in insertion order. The cap is checked **before** each upload: an image is uploaded if the Mpx already uploaded this frame plus its own stay within `RenderSettings.max_upload_mpx_per_frame` (default 6.0 Mpx, 1 Mpx = 2^20 pixels; `0.0` means unlimited), **or if it is the first upload of the frame**. The guarantee is exactly this and nothing stronger: *a frame spends at most `max(cap, the one texture that is first)` of upload work*, which is at most about 9 ms at the default and 6.1 ms when a single 2048 x 2048 portrait is bigger than what is left. A texture larger than the cap is therefore never stuck and is uploaded alone. **Drain and ready.** Nothing waits on the cap unless asked: `TextureStore::pending() -> usize` and `is_ready(guid) -> bool` read the queue; `TextureStore::flush_all(&mut Renderer)` uploads everything now and ignores the cap (a loading screen calls it); `RenderStats::pending_uploads` is the same count, readable by an agent; an agent can set `RenderSettings.max_upload_mpx_per_frame = 0` (a plain set-field command) to drain the queue on the next frame; and **`ScreenshotRequest.wait_for_uploads` (default true)** makes `render_frame` run `flush_all` in the frame that starts the capture, so an agent that imports an asset and screenshots straight after sees it. These are the MCP-reachable ready and drain paths. Until its upload runs, a **new** texture draws nothing (counted in `missing_textures`), and a **replaced** texture keeps showing the old image, so a hot reload never flashes. A texture queued twice before its upload keeps only the newest image. The queue also lets assets load before `init_renderer` has built the renderer. Fonts are not queued: `insert_font` and its replace run at once on the renderer when it exists (parsing costs at most a few ms, "Cost of loading a font"; a replace also rebuilds the text state, "Replacing a font"), or when the renderer is built if it did not exist yet. **Text state on a font replace** is rebuilt by the renderer inside `replace_font` ("Replacing a font"), not by the store, the UI or the caller; the store only forwards the call and then emits the event. Render has no dependency on `exitium_assets`. **Cross-spec ruling (a):** runtime crates have no edges between them, and the glue that connects them lives in the `exitium` facade. That covers asset loading into these stores, physics debug drawing (a facade system reads the physics world and writes `DebugDraw`) and UI drawing (a facade system turns the UI tree's draw data into `DrawList` entries, and hands the UI a text-measure closure that calls `Renderer::measure_text`). Those systems are the separate issue G1 (four slices, below), filed after the other specs merge (D2 to D4). Their data contracts are fixed by the merged specs and mapped in "Handoffs from the other runtime specs".
- **`Screenshots`** (the plain Rust service #110 builds the screenshot tool on): `request(ScreenshotRequest { camera: Option<Entity>, view: Option<ViewOverride>, size: UVec2, include_debug: bool, wait_for_uploads: bool /* default true */ }) -> Result<ScreenshotId, RenderError>` and `take(id) -> Option<Result<Capture, RenderError>>`, with `ViewOverride { center: Vec2, rotation: f32, zoom: f32 }`. `render_frame` fulfills pending requests in the frame after the request, through a pooled render texture per distinct size. It works in headless apps.
  - **Default camera.** `camera: None` means the **main camera**: the active `Camera2d` with the lowest `order` (ties: lowest entity index). If there is none, the `SceneView` is used when `enabled`. If there is neither, `request` returns `RenderError::NoCamera`.
  - **View override.** `view: Some(v)` replaces the center, rotation and zoom of the chosen camera (keeping its `scaling`, `pixel_perfect` and `layers`), or, when no camera exists, applies to `Scaling::PixelsPerUnit(1.0)` with all layers. An agent can therefore frame any part of the world without editing a camera.
  - **Limits.** `size` is at most 2048 x 2048 per side (larger returns `RenderError::Capture("size")`; the lower tool maximum keeps the base64 PNG within a stdio message, and #110 must halve the size or fail when the encoded payload would exceed 8 MiB). At most 2 requests may be pending (the renderer's `MAX_PENDING_CAPTURES`); a third returns `RenderError::CaptureBusy`.
- **`RenderSettings`** (reflected resource): `vsync: bool`, `clear_color: Color`, `debug_draw: bool` (read by glue such as the physics debug system), `show_bounds: bool`, `draw_cameras_to_surface: bool` (default true; false skips every pass that targets the surface and still serves screenshots and texture passes, see "Frame plan and the surface overlay"), `max_upload_mpx_per_frame: f32` (default **6.0** Mpx of 2^20 pixels; `0.0` means unlimited; the store's per-frame upload cap, see "Upload queue"). Changing it takes effect next frame. Stage 4 adds fields with defaults, which old saved settings load unchanged.
- **`plan_passes`** (in `plugin/passes.rs`) is the one function that turns resolved cameras into `Vec<Pass>` (view from `Camera2d` and `GlobalTransform2d`, target, `order`). `extract_cameras` calls it for the world cameras and `Screenshots` calls it for a capture, so a Stage 4 pass kind is added in one place and both paths get it. Per-pass GPU resources are keyed by the sorted position of the pass, never by the camera entity.
- **Interpolation.** Render draws `GlobalTransform2d` as it stands after propagation and adds no interpolation of its own. Physics bodies are interpolated by physics itself (it writes the interpolated pose into `Transform2d` in `PostUpdate`, before propagation, per physics-2d.md), so they move smoothly on a 144 Hz display. Moving objects that physics does not drive step at the fixed rate when game code moves them in `FixedUpdate` (open question 3: app-platform.md defers a general mechanism).
- **`Text.font`.** `None` means the `FontStore` default font (open question 7).

**Open dependencies on other specs.** Render fixes its side of each interface here and leaves the other side to the named spec; none of them changes the Stage 1 API.

| Id | Spec | What render needs or assumes |
|---|---|---|
| D1 | #106 (app), merged as app-platform.md | Resolved: `GlobalTransform2d(Affine2)`, propagated in `PostUpdate` set `PROPAGATE`, which runs before `Extract`; sets `EXTRACT` then `RENDER` in `Extract`; resources inserted in `Plugin::build`. Render uses exactly those names. |
| D2 | #106 (platform), merged as app-platform.md | Resolved: `PrimaryWindow(PlatformWindow)` (`Send + Sync`, raw handle traits) and `WindowInfo { physical_size, scale_factor, focused }`. Render owns `RenderWindow` and exports `RENDER_WINDOW`; the facade glue G1a (app-platform's F1 window half) copies the platform values into it. |
| D3 | #111 (assets) | Decoded RGBA8 and font bytes reach `TextureStore::insert` and `FontStore::insert_font` through facade glue (G1); a re-insert of a `Guid` is a hot-reload replace ("Stage 2"), whose costs are in "Performance" (asset-cost budgets); the persisted image reference type (open question 1). |
| D4 | #108 (physics), #109 (UI), both merged | Their output reaches `DebugDraw` and `DrawList` through facade glue (G1c, G1d). Render's guarantees to the UI are in "Handoffs from the other runtime specs". |
| D5 | #110 (editor, MCP) | Exposes the reflected types and services in the next section through its generic tools and commands. Render files no issue there. |
| D6 | follow-up (owner or coordinator) | Updates the stale WebGL2 text in INTERVIEW.md (line 31, line 136) and PLAN.md (line 220) to WebGPU only. Root-file hotspot outside this spec's footprint. |

### Handoffs from the other runtime specs

The merged specs fix how their data reaches render. Render does not change for any of them; this is what each side relies on.

**UI (ui-mvp.md, "Render handoff" and its open question 1).** The UI facade glue (G1d) turns `UiDrawList` into `SpriteDraw` and `TextDraw` items in `Space::Screen`, with `layer = layer_base + 2 * item.layer` for graphics and `+ 1` for text, `depth` = the item's index, `clip: None`. Render guarantees, and I4, I9 and I10 test:

1. **Equal-layer order.** Items of the same `layer` draw in ascending `depth`. Equal `layer` and `depth` break by texture (grouping), then sprites before text, then submission order. The UI's depths are distinct list indices, so the order is the list order.
2. **One text batch per layer.** Consecutive text items in a pass's sorted order form one text batch, and a sprite sorts between them only if its `(layer, depth)` falls between theirs. UI text sits on its own odd layer (`layer_base + 2L + 1`), so a root layer needs one text batch, and 60 root layers stay under `MAX_TEXT_BATCHES = 64` (per pass). If that ever fails, ui-mvp.md's fallback (aligning with `anchor`, merging planes) applies.
3. **Alignment width.** `TextStyle::max_width` with `wrap: None` is the alignment width: no wrapping, and `Center` and `Right` align inside it (the UI passes the node rectangle's width). The shaped-buffer cache key includes `max_width`. If `cosmic-text` does not align inside the width when it does not wrap, `text.rs` offsets each line by its measured width; I9 has a test for it.
4. **Measure and draw share the cache.** The UI measures at the width offered during layout and draws at the final width, so the first draw after a change can shape a second time; a repeat is a hit. `Renderer::measure_text` and the draw use the same cache.
5. **Scale.** `TextDraw.scale = ui_scale / dpi_scale` with `Space::Screen` text rasterized at `scale_factor * scale` = `ui_scale`, **not** quantized (only world text is, to 1/16). The UI divides physical coordinates by `dpi_scale` and render multiplies by `scale_factor` again in f32, so snapped edges return within 1e-3 px up to 8,192 px.
6. **Shaping cost.** The UI's row for one changed 8-character label (<= 20 us) is covered by `text/shape_cold` (16 characters, <= 20 us). A digit fast path for rapidly changing numerals is not in v1; it is a render-side feature to add if the first measurement misses (ui-mvp.md open question 2).
7. **Images.** `TextureStore::size(guid)` gives the UI an image's pixel size; an image whose texture is not loaded yet draws nothing.

**Physics (physics-2d.md, "Debug draw").** Physics writes `PhysicsDebugDraw.lines` in world units each `PostUpdate` when enabled. The facade glue (G1c) emits each `DebugLine` as `DebugDraw::line(a, b, color, 1.0, Space::World)` in the `Extract` schedule, set `EXTRACT`, and sets physics' `DebugDrawOptions::bounds` from `CameraViews::main()` (`visible_rect()`), so only visible shapes are generated. Physics and render each have a type named `DebugDraw`; they are different types and the facade exposes them as `exitium::physics::DebugDraw` and `exitium::render::DebugDraw`. Physics interpolates its own bodies by writing `Transform2d` before propagation, so render needs no interpolation for them.

**App and platform (app-platform.md).** Render uses the app's names: schedules `PreStartup`, `Startup`, `First`, `Extract`, sets `INPUT`, `EXTRACT`, `RENDER`, resources inserted in `Plugin::build`. The cursor is converted by facade glue (F1) with `InputState::cursor_logical` into `CameraView::screen_to_world`, which takes **logical** pixels (the same unit as `Space::Screen`); it uses `CameraViews::main()` from the previous frame's extraction. The window is `RenderWindow` (above). World axes agree (y up, counter-clockwise radians).

## MCP / editor surface

Agent parity (design rule 1): render adds no editor capability that an agent cannot reach. Per the cross-spec ruling this spec files **no issue in `exitium_mcp` or `exitium_editor`**. It lists what must be exposed, and #110 builds the tools generically over reflection, the command bus and the services below.

| Capability | What render provides for #110 to expose |
|---|---|
| Create, delete, move, edit `Sprite`, `Camera2d`, `Text` (any field, including `layer`, `z`, `tint`, `zoom`, `pixel_perfect`) | The three components are `Component + Reflect`; the generic entity, component and set-field commands (with undo) need nothing render-specific. |
| **The editor scene view** (what the human pans and zooms) | The reflected resource `SceneView { enabled, center, rotation, zoom, scaling, pixel_perfect }`. The generic get and set of a reflected resource lets an agent read and set exactly the view the human sees; the editor panel reads and writes the same resource. |
| Asset upload progress (ready and drain) | `RenderStats.pending_uploads` (read); `RenderSettings.max_upload_mpx_per_frame` (set `0.0` to drain everything next frame); screenshots wait for uploads by default (`wait_for_uploads`). `TextureStore::{pending, is_ready, flush_all}` are the Rust side. |
| Render settings (vsync, clear color, debug overlays, `draw_cameras_to_surface`, upload budget) | The reflected resource `RenderSettings`; set-field on a resource. `draw_cameras_to_surface = false` is how an agent (or the editor) stops the game drawing to the window while screenshots keep working. |
| Frame statistics | The reflected resource `RenderStats` (`sprites_submitted`, `sprites_drawn`, `draw_calls`, `passes`, `text_areas`, `text_batches_merged`, `texture_count`, `texture_bytes`, `instance_bytes`, `missing_textures`, `dropped`, `errors`, plus the v2 fields, each owned by the issue that fills it: `texture_replaces` (I22), `shape_misses` (I9), `overlay_calls` (I20), `pending_uploads` (I21)): read-only. |
| Screenshot | The plain Rust service `Screenshots::{request, take}` with `ScreenshotRequest { camera?, view?, size, include_debug, wait_for_uploads }` (defaults, limits and errors in "Stage 2"). #110 wraps it as a query tool (suggested name `render_screenshot`, args `{ camera?: entity, view?: { center, rotation, zoom }, width?: 1..=2048 (default 1280), height?: 1..=2048 (default 720), include_debug?: bool (default false), wait_for_uploads?: bool (default true) }`, result `{ width, height, png_base64 }`), waits for `take` over the next frames, and **encodes the PNG itself** (render returns RGBA8 only). The payload limit of 8 MiB is #110's to enforce. |
| Surface overlay (the editor's egui layer) | Editor infrastructure, not a capability an agent needs: `Renderer::set_overlay` is called by the editor once. Everything the overlay shows is reachable through MCP because the overlay draws state that the reflected resources and commands already expose (agent parity holds at the data level). The one render-owned switch it needs, `draw_cameras_to_surface`, is in `RenderSettings` above. |
| Hot-reloaded assets | `TextureStore::insert` and `FontStore::insert_font` replace in place ("Stage 2"); the asset spec's reload path (SP4) calls them. Agents reload by changing the asset file; there is no render-specific tool. |
| Click in a screenshot to a world point | The pure functions `CameraView::{screen_to_world, world_to_screen}` and `Camera::resolve`; #110 resolves the camera the same way `Screenshots` does (main camera, then `SceneView`). |

**Editor.** The editor viewport renders into a render texture (`RenderTarget::Texture`, held by `SceneViewTarget`) from the `SceneView` pass, and shows it in an egui panel by registering `Renderer::texture_view` with `egui-wgpu` on the same `device()` and `queue()` (the editor is a feature of the game binary, so there is one device). The egui layer itself is drawn into the window surface by a `SurfaceOverlay` the editor installs with `Renderer::set_overlay` ("Frame plan and the surface overlay"), built with `Renderer::target_format()`; the editor sets `RenderSettings.draw_cameras_to_surface = false` so the game appears only inside its panel. A texture that is replaced (hot reload) keeps its handle, but its `texture_view` is a new object: egui-wgpu users compare `texture_revision` each frame and re-register the view when it changed. Gizmos and selection outlines use `DebugDraw` with `Space::World` or `Space::Screen`. Public `wgpu` types (`device()`, `queue()`, `texture_view()`, `SurfaceTarget`) are an intentional part of the API and tie `exitium_render` to one `wgpu` major version shared with `egui-wgpu`.

**Inspector fields.** `Sprite`: `image` (asset picker for a `Guid`), `rect`, `size`, `anchor` (0..1), `tint` (color picker via `custom(widget = "color")`), `flip_x/y`, `layer`, `z`, `mask`, `visible`. `Camera2d`: `scaling` (enum), `zoom` (`range(min = 0.01, max = 100.0)`), `viewport`, `pixel_perfect`, `clear`, `layers`, `order`, `active`. `Text`: `text` (multiline), `font`, `size` (`range(min = 1.0, max = 512.0)`), `align`, `wrap`, `max_width`, `color`. `SceneView`: `enabled`, `center`, `rotation`, `zoom` (`range(min = 0.01, max = 100.0)`), `scaling`, `pixel_perfect`. All have `tooltip` text.

## Dependencies and wiring

Everything the issues below need, in one place, so the intake PRs add the union with the other Jam MVP specs' lists and **no implementation issue touches `Cargo.lock`, a `Cargo.toml` or `lib.rs`**. Versions checked on 2026-11-18.

### crates.io dependencies

| Crate | Where | Version (published) | License | wasm32 | Why not std |
|---|---|---|---|---|---|
| `wgpu` | `exitium_render`, normal | **30.0.2** (2026-11-12), MSRV 1.87. `default-features = false`, features **`std`, `parking_lot`, `wgsl`, `dx12`, `vulkan`, `metal`, `webgpu`**. Not enabled: `gles`, `webgl`, `angle`, `spirv`, `glsl`, `serde`, `trace`, `web` (pulled in by `webgpu`). | MIT OR Apache-2.0 | Yes. The `webgpu` feature is the browser backend; `dx12`, `vulkan` and `metal` are `cfg`-gated inside wgpu and compile to nothing on `wasm32`. | The GPU API. `metal` serves only the macOS CI job (PLAN) and is a no-op on Windows and on the web. |
| `wgpu` (wasm entry) | `exitium_render`, `[target.'cfg(target_arch = "wasm32")'.dependencies]` | same version, `default-features = false`, feature **`fragile-send-sync-non-atomic-wasm`** | same | wasm only | Makes `Renderer` `Send + Sync` on a single-threaded web build (see "Device, surface and web"). |
| `wasm-bindgen-futures` | `exitium_render`, `wasm32` target dependency | **0.4.80** (2026-10-30) | MIT OR Apache-2.0 | wasm only | `spawn_local` drives the async `Renderer::new` on the web; std has no executor and the browser cannot block. Already in wgpu's `webgpu` tree, so it adds no new lock entry. |
| `glyphon` | `exitium_render`, normal | **0.12.0** (2026-07-09), needs `wgpu ^30`; pulls `cosmic-text` 0.19 (MIT OR Apache-2.0), `etagere` 0.3, `lru` 0.16, `rustc-hash` 2 | MIT OR Apache-2.0 OR Zlib | Yes | Text shaping, bidi, line breaking, fallback and glyph atlas. std has none of it. |
| `bytemuck` | `exitium_render`, normal, feature `derive` | **1.25.2** | MIT OR Apache-2.0 OR Zlib | Yes | `unsafe` is denied workspace-wide, so instance structs are cast to bytes with `Pod` derives instead of hand-written casts. wgpu already depends on it. |
| `tracing` | `exitium_render`, normal | workspace version (0.1.44) | MIT | Yes | Warnings and errors from the renderer. Already a workspace dependency, default features off. |
| `criterion` | `exitium_render`, dev | workspace version | MIT OR Apache-2.0 | n/a | Benchmarks, per `docs/benchmarks.md`. |
| `winit` | `exitium_render`, dev (the Stage 1 example, I12) | **0.30.13**, the workspace entry of app-platform.md (`default-features = false`, `["rwh_06", "x11"]`), referenced with `workspace = true` | Apache-2.0 (allowed by `deny.toml`) | yes (`spawn_app`, canvas) | The example needs a window and event loop of its own: ruling C1 forbids using `exitium_platform`. Same version `wgpu` 30 and `egui-wgpu` 0.37 need; 0.31 is beta. A dev-dependency does not make `exitium_render` depend on winit for games. |
| `web-time` | `exitium_render`, dev (I12) | **1.1.0**, the workspace entry of app-platform.md | MIT OR Apache-2.0 | yes (re-exports `std::time` on native) | The example reports frame times; `std::time::Instant::now()` panics on `wasm32-unknown-unknown`. Already in winit's wasm tree. |
| `wasm-bindgen` | `exitium_render`, dev, `wasm32` target (I12) | **0.2.130** (2026-10-30), the workspace entry of app-platform.md | MIT OR Apache-2.0 | wasm only | The web build of the example (the `wasm-bindgen` CLI recipe, which must equal the locked version). Already in winit's and wgpu's wasm trees. |
| `web-sys` | `exitium_render`, dev, `wasm32` target (I12) | **0.3.107**, features `Window`, `Document`, `Element`, `HtmlCanvasElement`, `console`, the workspace entry of app-platform.md | MIT OR Apache-2.0 | wasm only | The example's console panic hook and canvas lookup. Already in winit's and wgpu's wasm trees, so the lock gains nothing. |

Not added, on purpose: `pollster` (a 25-line std `block_on`), `raw-window-handle` (0.6.2 comes through `wgpu`; `winit` re-exports the same version), `image` and `png` (assets #111 and #110), `lyon` (circles and polylines tessellate in about 80 lines), `egui-wgpu` (editor #110; 0.37.0 needs `wgpu ^30` and `winit ^0.30.13`, which matches).

**Verified on 2026-11-18** in a scratch crate outside the repo: `cargo check` of `wgpu` (the exact feature list above plus the wasm-only entry) with `glyphon`, `bytemuck` (`derive`) and `wasm-bindgen-futures` passes for `wasm32-unknown-unknown` and for `x86_64-pc-windows-msvc`, with no extra rustflags. The same crate asserted `Send + Sync` for the wgpu and glyphon types listed in "Threading" on both targets.

**Licenses.** The wasm tree is 98 packages and the Windows tree 108; every license is permissive (MIT, Apache-2.0, Zlib, BSD-2-Clause, ISC, Unicode-3.0 and OR-combinations). Expressions to note for `cargo deny`: `self_cell` is `Apache-2.0 OR GPL-2.0-only` (satisfied by Apache-2.0), `termcolor` and `winapi-util` are `Unlicense OR MIT`. `multiple-versions` warns for `skrifa`, `read-fonts` and `font-types` (two versions each, from `cosmic-text` and `swash`); `deny.toml` has that as `warn`. The intake PR still runs `cargo deny check` and reports it.

### Crate edges and manifest wiring

| Edge or entry | Added by | Reason |
|---|---|---|
| `exitium_render -> exitium_core` (normal) | intake-1 | Stage 1 uses `Handle`, `Slab`, `Rect`, `Color`, the math types. Today `crates/exitium_render/Cargo.toml` has no dependencies. |
| `exitium_render`: the dependency rows above (dev-dependencies `criterion`, `winit`, `web-time`, and on `wasm32` `wasm-bindgen`, `web-sys`, all `workspace = true`), `[[bench]]` entries `batch`, `text`, `extract`, `frame` with `harness = false` | intake-1 | One manifest edit up front. The workspace entries for `winit`, `web-time`, `wasm-bindgen` and `web-sys` are declared once, in the union with app-platform.md's list (same versions and features). |
| Empty `benches/batch.rs`, `benches/text.rs`, `benches/extract.rs`, `benches/frame.rs` (a `//!` doc and an empty `fn main() {}`) | intake-1 | Cargo fails on an explicit `[[bench]]` target with no file. I4, I9, I15 and I20 fill them. |
| `lib.rs` declaring every module in the layout (empty files with a `//!` doc, `plugin` included but empty, and the v2 files `frame.rs`, `material.rs`, `light.rs`, `present.rs`) and `pub use wgpu;` | intake-1 | No later issue edits `lib.rs`. The re-export lets a game name `wgpu::PowerPreference` and `wgpu::TextureFormat` (in `RendererConfig`) through `exitium::render::wgpu` with no `wgpu` dependency of its own. |
| `exitium_render -> exitium_ecs`, `exitium_app`, `exitium_reflect` (normal) | intake-2 | Stage 2 plugin: components, systems and sets, `#[derive(Reflect)]`. No further dev-dependencies: the headless app tests use these normal edges. |
| `exitium -> exitium_render` (normal) | intake-2 | The facade re-exports render (I18) and runs the sample (I19). |
| `exitium -> exitium_assets`, `exitium_physics`, `exitium_ui`, `exitium_platform` (normal) | intake-2 | Facade glue G1 (G1a needs the platform edge). Declared here so the union intake adds them once; G1 itself edits no manifest. |
| CI: a GPU job on the Linux runner (lavapipe, `EXITIUM_REQUIRE_GPU=1`) and a Windows job on the software adapter | owner (a `.github/workflows/` change) | The headless GPU tests. |
| `xtask` | unchanged | `exitium_render` builds for wasm32, so it is not `NATIVE_ONLY`. |
| **Render v2 hooks and overlay (SP5, #141)** | no change | No crates.io dependency and no crate edge is added by this amendment. Intake-1's manifest also has the `frame` `[[bench]]` entry (with its empty file), and its `lib.rs` also declares the module files `frame.rs`, `material.rs`, `light.rs` and `present.rs` (empty, `//!` doc only), so I20 and the Stage 4 issues edit no `lib.rs`. `exitium_editor -> exitium_render` (for `SurfaceOverlay`) already comes from intake-2 (C3) of the editor spec. |

**The two intake PRs** (the architect files them after the Jam MVP specs merge; each is the union of every merged spec's list):

```
## Footprint   (intake-1)
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: Cargo.toml, Cargo.lock, deny.toml (only if cargo deny needs an entry)
- New crate edges: exitium_render -> exitium_core

## Footprint   (intake-2)
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: Cargo.toml, Cargo.lock, crates/exitium/
- New crate edges: exitium_render -> exitium_ecs, exitium_render -> exitium_app, exitium_render -> exitium_reflect, exitium -> exitium_render, exitium -> exitium_assets, exitium -> exitium_physics, exitium -> exitium_ui, exitium -> exitium_platform
```

## Performance

Budgets are for **release profile** (`lto = "thin"`), medians from criterion on the reference dev PC, GPU-free (the plan/execute split makes them CI-gatable, 5% regression rule). **Absolute figures are provisional ceilings**: the first bench PR (I4, I9, I15) re-baselines them with the `perf` agent. The binding product target is the min-spec line below.

**Product target (min-spec: 4 cores, 8 GB, integrated GPU, 1080p).**

- **20,000 sprites per frame at 60 fps**, alpha-blended, from one or a few sprite sheets, average 32 x 32 px on screen: render-side CPU (extract + build + encode + submit) <= 3.0 ms per frame. Headroom: 100,000 sprites at 30 fps.
- Overdraw: the GPU fill budget is 4x 1080p (about 8.3 Mpx per frame, 500 Mpx/s at 60 fps).
- Draw calls: one per run of consecutive same-texture sprites; <= 64 in a typical frame, plus one per text batch and one for debug lines. `write_buffer` calls per frame: one for sprites, one for debug lines, one for camera uniforms.
- Startup: from `Renderer::new` to first presented frame <= 500 ms on native and <= 1.5 s on the web (excluding wasm download), including the three pipelines (sprite, which also draws clear quads; line; mip) <= 50 ms.
- Memory: renderer CPU-side fixed overhead <= 32 MiB excluding textures and font data; instance buffer `48 B * capacity` (20,000 sprites: 0.96 MB per frame in flight); glyph atlas <= 32 MiB typical, growing with distinct glyph sizes.

**Benchmarks** (`crates/exitium_render/benches/`, IDs `exitium_render/<area>/<case>`, sizes 1k, 100k, 1M where it makes sense, per `docs/benchmarks.md`):

| Benchmark | Budget | Kind |
|---|---|---|
| `batch/build` (filter + key + sort + instances) random layer, depth **and tint**, 8 textures, all visible: 1k / 100k / 1M | <= 60 us / <= 9 ms / <= 130 ms (about 90 ns per sprite, dominated by the sort) | ceiling |
| `batch/key_pack` 100k: the `u64` key (layer, order-preserving float-to-`u32` depth, group hint) for every sprite, `DepthMode::Z` | <= 0.3 ms (3 ns per sprite) | ceiling |
| `frame/plan` (`plan_frame` then `resolve` with 8 passes, overlay set; `benches/frame.rs`) | <= 1 us | ceiling |
| `text/load_font` a 200 KB Latin font (parse and index, no shaping) | <= 2 ms (3 ms per MB beyond that) | ceiling |
| `batch/build_sorted` (input already in order: the common frame-to-frame case) 100k | <= 4 ms (40 ns per sprite) | ceiling |
| `batch/cull` 100k, 10% visible | <= 1.5 ms (15 ns per item) | ceiling |
| `batch/tint_encode` 100k sprites with varied tints (LUT path), and the same with all-white tints (fast path) | <= 0.5 ms (5 ns per sprite) / <= 0.2 ms | ceiling |
| `batch/build` with `layers` mask rejecting all 100k | <= 0.4 ms | ceiling |
| `camera/resolve` | <= 50 ns | ceiling |
| `debug/tessellate_circle` (radius 200 px) | <= 1 us | ceiling |
| `text/prepare_cached` 1,000 unchanged labels of 12 glyphs, one pass (hash, lookup, glyphon prepare) | <= 3 ms | ceiling |
| `text/prepare_cached` the same with 2 passes of different sizes | <= 5.5 ms (shaping is shared, `prepare` repeats per pass) | ceiling |
| `text/shape_cold` one 16-character label | <= 20 us | ceiling |
| `extract/sprites` (Stage 2) 100k entities with `Sprite` + `GlobalTransform2d` | <= 4 ms (40 ns per sprite) | ceiling |

Steady-state **allocations in our code: zero** per frame after the first (`DrawList`, `BatchPlan` and the instance staging `Vec`s keep their capacity; verified by asserting capacities do not grow across 100 identical frames in a test). `glyphon` and `wgpu` may allocate internally.

Complexity: `build` is O(n) filter + O(k log k) sort over the k visible items; text is O(glyphs) after a cache hit; textures O(1) lookup; capture O(pixels).

**Asset-cost budgets (provisional, cited by the assets spec).** With 1 Mpx = 2^20 pixels: creating or replacing a texture costs on the calling thread **0.1 ms + 1.5 ms per Mpx** (+0.5 ms with `Mips::Generate`), at most 1.6 ms for 1024 x 1024 and 6.1 ms for 2048 x 2048. `TextureStore` uploads at most `max_upload_mpx_per_frame` (default 6.0 Mpx, about 9 ms) per frame, except that the first upload of a frame always runs, so a frame's upload work is at most `max(cap, one texture)`; a hot reload of one 2048 x 2048 image costs one frame 6.1 ms. Loading or replacing a font costs `2 ms + 3 ms per MB beyond 0.2 MB` (a 5 MB font: 16.4 ms), and a **replace** also re-parses every other loaded font and re-shapes the visible text once ("Replacing a font"). `texture/create` is a GPU-job test (`tests/texture_cost.rs`, not a criterion target, so no manifest entry) that creates 256 x 256, 1024 x 1024 and 2048 x 2048 textures on the software adapter, **prints** microseconds per Mpx and asserts only a gross bound (20x the ceiling), because adapter timings are not stable enough to gate. The web numbers are checked by hand in Chrome from I12 and reported. These figures are the basis for the assets spec: the "5 ms upload" it assumed is replaced by this formula.

GPU-side timings are not gated in CI (no GPU on runners). The Stage 1 example (I12) reports frame times for manual min-spec checks. The wasm size contributed by `wgpu`+`glyphon` is measured once in the Stage 1 smoke build and reported, not gated.

## Test plan

- **Pure CPU unit tests** (no adapter, run everywhere, including the wasm build check): sort-key order (negative, zero, large depths; layer bias; NaN depth counted), run splitting on texture and clip change, mask filtering, cull boxes under rotation and non-uniform scale, flip and sub-rect UVs, `Camera::resolve` for every `Scaling` (including zero-size and extreme zoom), `world_to_screen` / `screen_to_world` round trips (property test), pixel-perfect rounding and snapping, debug tessellation segment counts, capture row-padding removal and BGRA swap, readback of a 1 x 1 and a 255 x 3 image (odd row sizes).
- **Headless GPU tests** (`tests/*.rs`): the helper `tests/common/mod.rs` (I1) has its own blocking readback with the BGRA swap, so pixels can be checked before the production `capture.rs` (I8) exists, and I8 is tested against it. It requests the software adapter (WARP on Windows, lavapipe on Linux), builds the instance with `InstanceFlags::STRICT_WEBGPU_COMPLIANCE` (new in wgpu 30; it limits adapters and features to what WebGPU v1 allows, so native tests run under the browser's rules), and installs an uncaptured-error handler that **fails the test**. With no adapter a test **skips**, unless `EXITIUM_REQUIRE_GPU=1` (set on CI), where it fails. Cases: clear color; one sprite; tint; flip x and y; sub-rect from a sheet; layer and depth order with overlap and alpha; `texture: None` solid color; nearest vs linear sampling; mip generation (a 256 x 256 checker minified shows the average, not a sample); camera zoom, rotation, viewport; pixel-perfect on and off; screen space with `scale_factor` 1.5; clip rectangle; two cameras with `clear: None`; **a minimap camera with a smaller `viewport` and its own `clear` that leaves the rest of the target untouched and draws nothing outside its rectangle**; **a BGRA target (`headless_format: Bgra8UnormSrgb`) whose capture equals the RGBA golden**; **two passes of different target sizes in one `render` call, both drawing text and sprites, each matching its own golden (per-pass instance ranges, text renderers and viewports)**; text with an embedded test font (any OFL font under 100 KB; its license file sits beside it in `tests/fonts/`); text align, wrap and `measure_text`; debug line, circle, rect; resize and zero-size resize; render-to-texture plus capture; a third capture while two are pending returns `CaptureBusy`; stale-handle draw is skipped; texture too large; more than `MAX_INSTANCES`.
- **Golden images**: `tests/golden/*.rgba8` (a `u32` width, `u32` height, then bytes; 64 x 64, about 16 KB each; written and read with std only, no PNG dependency; regenerate with `EXITIUM_UPDATE_GOLDENS=1`). Compare per channel with tolerance 2 (3 for text) and at most 0.5% pixels over it, because software rasterizers differ by about one level. The Linux lavapipe run is authoritative.
- **Tint table:** for all 4096 table entries and a sweep of 100k values, the LUT result is within one level of `Color::to_srgba_u8`; white and "same as previous" fast paths return the same bytes.
- **Frame plan and overlay (AM1).** `plan_frame` and `FramePlan::resolve` truth tables (I20): every combination of `has_surface`, `overlay_set`, `draw_cameras_to_surface` and {no passes, texture-only, surface-only, both} for the plan, and each `Acquire` value (`NotWanted`, `Acquired`, each `Unavailable(SkipReason)`) for the resolution, asserting rules 1 to 8 of "Frame plan" (the surface is acquired for a texture-only frame when an overlay is set; the clear pass is planned only when no surface pass survives; the overlay is called once, last, and never when headless or skipped; texture passes still run when the surface is skipped; status values); for the inputs v1 defines (no overlay, `draw_cameras_to_surface == true`, surface acquired or not wanted) the result equals the v1 behaviour; texture-pass slots are equal with the flag on and off; a pure test maps every `CurrentSurfaceTexture` variant to its `Acquire`. One Windows test with a hidden window and a test overlay that fills the target with a known colour, then a texture-only frame, checking the surface contents and that a screenshot of the same frame is still correct; a second frame with `draw_cameras_to_surface = false` and a surface pass checks that the pass drew nothing. Headless GPU tests check that a headless renderer never calls an overlay.
- **v2 seams.** `size_of::<Instance>() == 48` and the offset of every field (including `user` at 44); sort-key tests for `-0.0` versus `0.0`, negative depths, equal depth with different layers, the group hint never overriding layer or depth, and a sprite whose `BatchKey` mixes to `0xFFFF` being mapped below the text value (sprites before text at equal layer and depth); `ColorSpace::Linear` maps to `Rgba8Unorm` and its mip pipeline is built lazily; `CameraView::screen_from_target` round trip under a letterboxed `Scaling`; `Pass` and `CameraPass` built with `new`/`Default` compile when a field is added (the external-crate doc-test of the compatibility rule keeps `#[non_exhaustive]` honest).
- **Hot reload.** `replace_texture` (I22): the handle is unchanged, `texture_revision` increases, a sprite drawn before and after shows the new pixels (golden), a size change updates `uv_rect`, a failed replace keeps the old texture, a stale handle and a render texture return `StaleHandle`, `texture_replaces` counts, and `texture_bytes` returns to its steady value. `replace_font` (I9 and I10): the `FontId` is unchanged; text drawn before and after differs (golden); `font_revision` increases; garbage bytes return `BadFont` and `font_face_count()`, `font_revision` and the measured size of a label are unchanged (a face-count test, because fontdb reports no error); after a replace `font_face_count()` equals the faces of the live fonts only, also after 100 replaces; `atlas_generation()` increases by exactly one per replace; `shape_misses` equals the number of distinct visible labels on the first frame after a replace and 0 on the second. Stores (I21): `insert` with an existing `Guid` replaces and emits `RenderAssetReplaced`; the upload queue never exceeds `max_upload_mpx_per_frame` except for the first upload of a frame; `0.0` drains everything; `flush_all` and `ScreenshotRequest.wait_for_uploads` leave `pending_uploads == 0` before the capture; a new texture draws nothing until uploaded (`missing_textures`), a replaced one keeps the old image until its upload, and an image queued twice uploads once.
- **Compile-time:** `assert::<Renderer>()` for `Send + Sync` in `renderer.rs`, compiled by the native build and the CI wasm build.
- **Stage 2** (headless app, no window): spawn entities with `Sprite`, `Camera2d`, `Text`, run one frame, assert the golden image; hierarchy (a child sprite follows its parent); hidden entities draw nothing; reflection round trip of the three components (text format) and the schema hash of each is pinned; `Screenshots` request and `take` work headless, with `camera: None` choosing the lowest-`order` active camera, falling back to an enabled `SceneView`, and returning `NoCamera` otherwise; `view` overrides center, rotation and zoom; sizes above 2048 are rejected; `RenderSettings` change applies next frame.
- **wasm**: `cargo build --workspace --target wasm32-unknown-unknown` covers `exitium_render` (checked in CI by `cargo xtask ci wasm`). There is no automated browser test in the MVP. **From I12 (the Stage 1 example) on**, every render PR that changes the GPU path states in its description that the example ran in Chrome (clear, sprites, text, two cameras and a capture, no uncaptured wgpu error); I1 to I11 rely on the headless tests, and I12 is the first issue that runs the surface, resize and present path on a real window and in a browser. This applies until the export spec (#111) provides a browser harness.
- **Benchmarks** under the 5% gate (table above).

## Open questions

1. **Sprite image reference.** `Sprite.image` is a `Guid` today because `Handle<T>` is not reflectable and `AssetRef<T>` is decided in the assets spec (#111, reflect-model open question 13). If #111 chooses `AssetRef<Image>`, the field type changes inside the plugin only.
2. **Who owns the glue.** Settled by cross-spec ruling (a) and app-platform.md: the `exitium` facade holds the systems that feed render from the platform window, assets, physics debug and the UI (issue G1, slices G1a to G1d). Nothing is left open here: the UI hook is `UiMeasure` (ui-mvp.md).
3. **Interpolation of non-physics movers.** Physics interpolates its own bodies. Anything else that game code moves in `FixedUpdate` steps at 60 Hz on faster displays; app-platform.md defers a general previous-tick transform (its open question 6). Additive later.
4. **Text measurement and rich text for the UI (#109).** Taffy layout needs a measure function, and there is no `ui -> render` edge. Resolved by ui-mvp.md: the UI takes a `UiMeasure` trait object, and the facade glue (G1d) implements it over `Renderer::measure_text` and `TextureStore::size`. Rich text (several colors in one block) is not in v1; the UI can lay out several `TextDraw`s, or `TextDraw` gains `spans` additively.
5. **Premultiplied alpha.** v1 blends straight alpha and relies on importer-side edge bleeding, so a mip of a portrait with a transparent border can show a dark fringe. A premultiplied pipeline needs an importer step; decide with #111 after the first portrait test.
6. **Y-sort and isometric depth policy.** `depth` is a caller-supplied `f32` and `CameraView.depth_mode` (hook H1) is the seam, so nothing is blocked. The `YSort` and `Iso` formulas, a `y_sort` flag on `Sprite` and height levels are decided by SP12 (#148) inside the hook.
7. **Default font.** No font ships in `exitium_render`. The editor and debug overlays need one. A small OFL font embedded in the `exitium` facade (license file in the repo, ADR-0003) is the proposal; decide with #110.
8. **Web threads.** `fragile-send-sync-non-atomic-wasm` only takes effect without wasm atomics. If the web build ever gets threads (cross-origin isolation, nightly `build-std`), the feature is inert, the wgpu handles are `!Send`, and the `Send + Sync` assertion on `Renderer` fails to compile: the build breaks instead of racing. The fix then is to make `Renderer` a main-thread-only resource on the web.
9. **Capturing the window surface.** v1 captures only render textures, which is what agents need. Capturing exactly what the human sees (egui panels included) needs `COPY_SRC` on the surface configuration; additive if wanted.
10. **More than about 200 texture switches per frame.** If real games hit it, add a 2D-array texture path behind the same `SpriteDraw`. Not needed for sprite sheets or Glutton's portraits.
11. **Visibility inheritance.** `visible` is per entity in v1; a hidden parent does not hide its children. Add a `Visibility` hierarchy pass if needed.
12. **GPUs without DX12 or Vulkan.** With `gles` off, a very old Windows GPU has no backend and gets `NoAdapter`. Acceptable for the MVP (owner decision); revisit with the first user reports.
13. **Stage 4 details.** The lighting algorithm (H3), the `DepthMode::Iso` formula (H1), the `MaterialDesc` uniform layout and the pixel-art present rules (H4, `IntegerScale` with a non-integer window) are decided by SP12 (#148) within the seams fixed here. The seams promise only what "Render v2 hooks" lists.
14. **Texture cost figures.** The per-Mpx ceiling is a calculation with a 50% margin, not a measurement on min-spec hardware. I22 reports the software-adapter numbers and I12 the Chrome ones; if either exceeds the ceiling, the figures here and the default upload budget change in one follow-up and the assets spec is told.

## Implementation issues

The architect files these with the `file-issue` skill in the Jam MVP milestone after this spec merges, each with the literal `## Footprint` block shown. Every issue carries acceptance criteria and the `area:render` label, edits exactly one crate (the facade issues edit none and only hold the `crates/exitium/` hotspot) and is sized to about 600 changed lines or fewer. **One crate has at most one issue in flight** (factory rule), so the render issues run strictly in the order below. The order puts what Glutton and the MVP need first (draw sprites, then text, and see the result through capture and a real window) and the optional pieces last. `Blocked by` names slices here and is rewritten to `#N` when filed; blockers in other specs use their placeholder names (`PP2`, `U11`, `F1`) until their issue numbers exist. **Three slices sit out of numeric order and run where they are listed: I22 (`replace_texture`) right after I2, I20 (frame plan and surface overlay, AM1) after I13 and before I14, and I21 (the plugin stores) right after I16.** **Every Stage 1 slice (I1 to I13, I20 and I22) is blocked by intake-1; every Stage 2 slice (I14 to I19, I21 and G1) by intake-2**; the intake PRs are defined under "Dependencies and wiring".

**I1. `feat(render): device, surface, resize and present modes`** (about 500 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `error.rs`, `config.rs`, `gpu.rs`, the `stats.rs` skeleton, the std `block_on`, `crates/exitium_render/CLAUDE.md`, and the GPU test helper `tests/common/mod.rs`.
- Acceptance: `Renderer::headless_blocking` works on the software adapter. Format selection is a pure function returning `(configured, F, O)` (the sRGB view format for cameras and its non-sRGB sibling for the overlay) with `view_formats` containing both `F` and `O`, and unit tests for sRGB, non-sRGB plus `view_formats`, BGRA (`O = Bgra8Unorm`), the WebGPU-canvas case (only `Bgra8Unorm` offered) and empty capabilities; `Renderer::target_format()` returns `F` (`headless_format` when headless) and `overlay_format()` returns `O`; `RendererConfig` is `#[non_exhaustive]` with `Default`. The `CurrentSurfaceTexture` mapping is a pure function tested for every variant. `resize` coalescing and zero-size handling (including a zero size at creation) are unit tested. `WebGpuUnavailable` and `NoAdapter` are returned in the matching cases. The helper provides the software adapter, `STRICT_WEBGPU_COMPLIANCE`, a fail-on-uncaptured-error handler, `EXITIUM_REQUIRE_GPU`, and **its own blocking readback of any render texture with the BGRA swap** (std-only, test code), so I2 to I7 can verify pixels before the production `capture.rs` of I8 exists. `crates/exitium_render/CLAUDE.md` is written. The wasm build passes.
- Blocked by: intake-1.

**I2. `feat(render): textures, samplers, mip generation and render textures`** (about 600 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `texture.rs`, `mipmap.wgsl`.
- Acceptance: create, destroy, stale-handle and size-limit behavior is tested. `ColorSpace::{Srgb, Linear}` (`Rgba8UnormSrgb` and `Rgba8Unorm`) and `RenderTextureDesc` with `create_render_texture_with`. `TextureDesc` and `RenderTextureDesc` are `#[non_exhaustive]` with `Default`; `texture_revision` is 0 at creation. Sampled textures are `Rgba8UnormSrgb` or `Rgba8Unorm` by `ColorSpace`; render textures use the target format `F` (also with a BGRA `F`); the mip pipeline is built for `Rgba8UnormSrgb` at startup and for `Rgba8Unorm` lazily. Mips of a checker texture are verified with the I1 helper's readback. `white_texture`, `create_render_texture` and `texture_bytes` accounting work.
- Blocked by: intake-1, I1.

**I22. `feat(render): replace_texture and the texture cost test`** (about 300 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `texture.rs` (`replace_texture`), `stats.rs` (`texture_replaces`), `tests/texture_cost.rs`.
- Acceptance: `replace_texture` swaps into the same slab slot, so the handle stays valid and sprites show the new image next frame (golden), rebuilds the group-1 bind group, bumps `texture_revision`, handles a size change (the batcher's `uv_rect` follows), is all or nothing (a failed replace keeps the old texture and returns the error), returns `RenderError::StaleHandle` for a stale handle or a render texture, and keeps `texture_bytes` correct; `RenderStats::texture_replaces` counts successful replaces. `tests/texture_cost.rs` (the `texture/create` test, a GPU-job test and not a criterion target) creates 256 x 256, 1024 x 1024 and 2048 x 2048 textures with and without mips on the software adapter, **prints** microseconds per Mpx (1 Mpx = 2^20 pixels) and asserts only a gross bound (20x the ceiling in "Textures"); the PR body reports the numbers for the cost budget (open question 14).
- Blocked by: intake-1, I2.

**I3. `feat(render): camera math with scaling modes and pixel-perfect snap`** (about 400 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `camera.rs`.
- Acceptance: `Camera` and `CameraView` are `#[non_exhaustive]` with `new` and `Default`; `CameraView` carries `screen_from_target` (identity when the target is the window) and `depth_mode: DepthMode` (`DepthMode::Z` only); `Camera`, `Scaling`, `CameraView`, `resolve`, `world_to_screen`, `screen_to_world` (logical pixels in and out, documented as such) and `visible_rect` for every scaling mode. Pixel-perfect rounding and center snap. A property test for round trips at scale factors 1.0 and 1.5. The `camera/resolve` benchmark within its ceiling.
- Blocked by: intake-1.

**I4. `feat(render): sprite sort, cull, tint table and instance batching (CPU)`** (about 600 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `batch.rs`, `benches/batch.rs` (the file intake-1 created empty).
- Acceptance: `SpriteDraw` (`#[non_exhaustive]`, with `new` and `Default`), `DrawList` (sprites only at this point), the 48-byte instance layout with the reserved `user` word at offset 44 (size and offset tests), key packing (layer 16 | depth 32 | group hint 16, `-0.0` normalised, `DepthMode::Z`) with the exact `BatchKey` deciding run splits, `BatchPlan::build` with the camera composed in step 1, runs split on texture, clip and kind. The ordering guarantees 1 and 2 of "Handoffs from the other runtime specs" (equal-layer depth order, tie-breaks) have unit tests. The 4096-entry tint table and its fast paths, with the accuracy test (within one level of `Color::to_srgba_u8`). The CPU unit tests from the test plan. A test that capacities stay constant over 100 identical frames. Every `batch/*` benchmark (varied tints and `batch/key_pack` included) within its ceiling.
- Blocked by: intake-1, I2, I3.

**I5. `feat(render): sprite pipeline and single-pass Renderer::render`** (about 550 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `sprite.wgsl` (instance input locations 0 to 4), `frame.rs` (`Pass`, `CameraPass`, `RenderTarget`, `FrameStatus`, `SkipReason`, where the layout puts them), `renderer.rs`, the golden-image helpers (std-only read and write of `.rgba8`, comparison with tolerance).
- Acceptance: one pass on the surface or on a render texture; explicit `set_viewport` to the whole target and `set_scissor_rect` to the camera rectangle and per-run `clip`; the first pass clears the whole target; instance and uniform upload with `MAX_INSTANCES` drop counted; `render` takes `&[Pass]` (`Pass::Camera`, `#[non_exhaustive]`, with `CameraPass::new` and `order`) and bind groups 0 and 1 only (the slot plan in "Render v2 hooks"); `FrameStatus` values for the cases that exist before I20. Golden tests for clear, one sprite and tint, read back with the I1 helper.
- Blocked by: intake-1, I1, I2, I4.

**I6. `feat(render): several camera passes, per-pass instance ranges and viewports`** (about 450 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `renderer.rs` (pass loop, per-pass ranges of the one instance buffer, uniform slots), the clear-quad for later passes.
- Acceptance: passes sorted by `(order, slice position)`, stable; per-pass uniform slots and instance ranges are keyed by the pass's `slot`, its sorted position before any pass is dropped (not the camera or pass kind); each pass has its own contiguous range of the shared instance buffer; `clear: Some` on a later pass fills only its viewport; a minimap camera with a smaller `viewport` draws and clears nothing outside its rectangle (golden); two passes with different target sizes in one `render` call each match their golden; a BGRA target gives the same pixels as an RGBA one when read back with the I1 helper (the helper does the swap).
- Blocked by: intake-1, I5.

**I7. `test(render): sprite golden suite`** (about 500 lines, mostly goldens)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `tests/` and `tests/golden/` only.
- Acceptance: goldens for flip x and y, sub-rect from a sheet, layer and depth order with alpha, `texture: None`, nearest vs linear, mip minification, camera zoom and rotation, pixel-perfect on and off, screen space with `scale_factor` 1.5, clip rectangle, stale-handle draw skipped, `MAX_INSTANCES`. No production code changes except fixes the goldens reveal (listed in the PR).
- Blocked by: intake-1, I6.

**I8. `feat(render): capture readback for render textures`** (about 350 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `capture.rs`.
- Acceptance: `request_capture` and `poll_captures`; `MAX_PENDING_CAPTURES = 2` with `CaptureBusy`; row-padding removal; BGRA fix-up; error mapping; tests for odd sizes and a test that the production readback equals the I1 helper's readback on the I5 to I7 scenes; non-blocking on native; builds for wasm.
- Blocked by: intake-1, I6.

**I9. `feat(render): fonts, text layout, shaped-buffer cache and measuring (no drawing)`** (about 550 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `text.rs` (layout half), `benches/text.rs` (`text/shape_cold`), `DrawList::text` with the string arena in `batch.rs`.
- Acceptance: `load_font` with the empty-database `FontSystem`; **`replace_font` and `font_revision`** per "Replacing a font" steps 1 to 4 (the atlas half of step 4 is I10's): the `FontId` stays; a stale id is `StaleHandle`; bytes with no face return `BadFont`, shown by a **face-count test** (`font_face_count()`, `font_revision` and the measured size of a label are unchanged); a successful replace leaves `font_face_count()` equal to the faces of the live fonts only (also after 100 replaces), empties the shaped-buffer cache, and gives `shape_misses` equal to the number of distinct labels drawn on the next frame and 0 on the one after; `RenderStats::shape_misses`; `TextStyle` and `TextDraw` are `#[non_exhaustive]` (`TextDraw::new(text, style)`); the `text/load_font` benchmark within its ceiling; `TextStyle`, `TextDraw`; the cache with its 120-frame sweep and a key that includes `max_width`; `measure_text`; `text_no_font`; tests with the embedded OFL test font for align, wrap and measure results (sizes, not pixels), including **alignment inside `max_width` with `wrap: None`** (handoff guarantee 3); `text/shape_cold` within its ceiling.
- Blocked by: intake-1, I4.

**I10. `feat(render): draw text in passes with per-pass renderers and viewports`** (about 550 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `text.rs` (drawing half), `renderer.rs` (text runs), the `Send + Sync` assertion.
- Acceptance: **the atlas half of `replace_font`** (`TextGpu::reset`): a replace rebuilds the `TextAtlas` and the `TextRenderer` pool, because `TextAtlas::trim` only clears the in-use set and frees nothing; `atlas_generation()` increases by exactly one per replace, text drawn after a replace matches the new font (golden), and 100 replaces in a loop keep `font_face_count()` and the atlas size bounded; nothing outside render flushes a cache. One `TextAtlas`; a `TextRenderer` pool per (pass, batch) and one `Viewport` per pass; `MAX_TEXT_BATCHES = 64` per pass with `text_batches_merged`; clip and viewport intersected into `TextBounds`; world scale quantized to 1/16, screen scale not; **a test with two passes of different sizes, both drawing text, in one `render` call**; a test that a root-layer-like run of 200 text items on one layer produces one text batch (handoff guarantee 2); goldens for align, wrap and color; `assert::<Renderer>()` for `Send + Sync` compiles on native and in the wasm build; the `text/prepare_cached` benchmarks (one and two passes) within their ceilings.
- Blocked by: intake-1, I6, I9.

**I11. `feat(render): debug lines, circles, rects and polylines`** (about 450 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `debug.rs`, `line.wgsl`.
- Acceptance: the `DebugDraw` API; constant pixel width; the segment-count rule; per-pass instance ranges; `MAX_DEBUG_LINES` drop counted; drawn last; world and screen space; goldens; the `debug/tessellate_circle` benchmark within its ceiling.
- Blocked by: intake-1, I6.

**I12. `feat(render): windowed and web example (Stage 1, no ECS)`** (about 550 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `examples/render_window.rs` and `examples/web/index.html`. The example opens **its own window** with the intake-1 dev-dependencies (`winit` 0.30.13, `web-time` 1.1.0, and on wasm32 `wasm-bindgen` 0.2.130 and `web-sys` 0.3.107); it does not use `exitium_platform` (ruling C1). It creates the `Renderer` from an `Arc<winit::window::Window>` (native: `new_blocking`; web: `spawn_local` into a shared slot), forwards resize and scale-factor changes, and draws from a procedurally generated sprite sheet (no image decoder), the embedded OFL test font, two cameras (a main camera and a minimap with its own `viewport` and `clear`), a key that toggles 20,000 sprites, a key that requests a capture and logs its checksum, and a once-per-second frame-time line from `web-time`.
- Acceptance: runs on Windows and in Chrome (the PR states both, with no uncaptured wgpu error and a non-blank canvas); `cargo build --example render_window --target wasm32-unknown-unknown` passes and the PR gives the `wasm-bindgen` CLI recipe at the locked version; frame times at 20,000 sprites on the dev PC are reported. **From this issue on, every render PR that changes the GPU path repeats the Chrome check** (see the test plan).
- Blocked by: intake-1, I7, I8, I10, I11.

**I13. `feat(render): diagnostics: device-lost, uncaptured errors and RenderStats`** (about 300 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `stats.rs`, `gpu.rs`.
- Acceptance: every `RenderStats` field owned by I1 to I12 is filled by its code path (the fields that land later are owned and tested by their own issues: `texture_replaces` I22, `overlay_calls` I20, `pending_uploads` I21; `shape_misses` is I9's); device-lost surfaces as `RenderError::DeviceLost`; uncaptured errors are counted and logged; a test forces one.
- Blocked by: intake-1, I12.

**I20. `feat(render): frame plan, surface overlay and draw_cameras_to_surface`** (about 450 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `frame.rs` (`PlanInputs`, `FramePlan`, `plan_frame`, `FramePlan::resolve`, `Acquire`, `FrameSteps`, the surface-clear pass), `renderer.rs` (use the plan in `render`; `set_overlay`, `set_draw_cameras_to_surface`, `target_format`; `SurfaceOverlay`, `OverlayCx`), `stats.rs` (`overlay_calls`), `benches/frame.rs` (the file intake-1 created empty).
- Acceptance: `plan_frame` and `FramePlan::resolve` implement rules 1 to 8 of "Frame plan and the surface overlay" (the plan before acquiring, the resolution after `get_current_texture`), both tested exhaustively on the CPU, including the stable `slot` of texture passes when `draw_cameras_to_surface` toggles; `overlay_format()` returns the non-sRGB sibling and `OverlayCx.target` is a view in that format (a Windows test checks the overlay's colour is written unconverted); the `frame/plan` benchmark is within its ceiling. With an overlay set, `render` acquires the surface even when every pass targets a texture, records the surface-clear pass when no surface pass survives, and calls the overlay once, after the last camera pass and before submit, with the surface view, `target_format()`, the physical size and the scale factor. With no overlay set and `draw_cameras_to_surface == true`, the existing I5 to I10 goldens are unchanged (rule 7). With `draw_cameras_to_surface == false`, surface passes are skipped and a screenshot through a texture pass still works. A skipped surface still runs texture passes and delivers captures. A headless renderer never calls the overlay. One Windows hidden-window test of the overlay (the test plan). `exitium_render` still builds for wasm. High-risk review: public API consumed by the editor.
- Blocked by: intake-1, I13.

**I14. `feat(render): plugin components, RenderPlugin, window resource, settings and scene view`** (about 600 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `plugin/mod.rs` (`RenderPlugin` and registration), `plugin/components.rs`, `plugin/settings.rs` (`RenderSettings` with `draw_cameras_to_surface` and `max_upload_mpx_per_frame`, `SceneView`), **`plugin/window.rs` (`RenderWindow`, the `RENDER_WINDOW` set)**, **`stats.rs` (`#[derive(Reflect)]` on `RenderStats`, so the file is named here)**, and `#[derive(Reflect)]` on `Scaling`, `TextAlign` and `TextWrap` in `camera.rs` and `text.rs`.
- Acceptance: `Sprite`, `Camera2d`, `Text` derive `Component` and `Reflect` with the attributes in the inspector list; `Sprite`, `Camera2d`, `Text`, `SceneView`, `RenderSettings` and `RenderPlugin` are `#[non_exhaustive]` with `Default` (the external-crate doc-test builds each with `Default` and assignment); `RenderSettings`, `RenderStats` and `SceneView` are reflected resources; `RenderWindow` has `set_target`, `set_size`, `size`, `scale_factor`, `has_target` and a unit test with a stub window target; `assert::<RenderWindow>()` for `Send + Sync` compiles on native and in the wasm build (the type holds an `Arc<dyn WindowSource>`, not a `SurfaceTarget`); `RenderWindow::default().scale_factor() == 1.0`; `RenderPlugin` (config, headless flag, default) builds against the app-platform.md `Plugin` trait and inserts every resource in `build`, with `RendererSlot` empty; reflection round trip and pinned schema hashes; no GPU needed. If the PR passes about 600 lines, `RenderWindow` and its set move to their own issue before this one.
- Blocked by: intake-2, I20, the app spine issues of app-platform.md (A1 to A4: `Plugin`, `App`, sets, `GlobalTransform2d`).

**I15. `feat(render): plugin extraction and render_frame systems`** (about 550 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `plugin/extract.rs`, `plugin/passes.rs` (`plan_passes`), `benches/extract.rs`, additions to `RenderPlugin::build`.
- Acceptance: `extract_cameras` (cameras and the enabled scene view, and the `CameraViews` resource that survives `render_frame`) and `extract_draws` in set `EXTRACT`, `render_frame` in set `RENDER`, of the `Extract` schedule, which copies `RenderSettings.draw_cameras_to_surface` into the renderer each frame (a test: false skips the surface pass and a screenshot still works), with passes built only by `plan_passes`, reading the renderer through `RendererSlot` and doing nothing when it is empty (a test runs a frame with an empty slot); `Guid` resolved through the stores (empty until I21); a headless app golden test (sprite, text, camera, hierarchy, hidden entity); the `extract/sprites` benchmark within its ceiling.
- Blocked by: intake-2, I14.

**I16. `feat(render): plugin renderer init, resize and settings`** (about 350 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `plugin/init.rs` (`init_renderer`, `poll_renderer_init`, `apply_resize`, `apply_render_settings`), additions to `RenderPlugin::build`.
- Acceptance: `init_renderer` in `Startup` reads **only `RenderWindow`** (native blocking, web `spawn_local` into a shared slot, headless, and a late target); `poll_renderer_init`, `apply_resize` (reads only `RenderWindow`) and `apply_render_settings` in `First` set `RENDER_WINDOW`, ordered after `INPUT`; a headless-app test where a test system plays the glue (writes a stub `RenderWindow` before `RENDER_WINDOW`) and a resize reaches `Renderer::resize` in the same frame; the `assert::<Renderer>()` still compiles in the wasm build.
- Blocked by: intake-2, I15.

**I21. `feat(render): plugin texture and font stores, replace semantics and upload queue`** (about 450 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `plugin/stores.rs` (`TextureStore` with `size`, the upload queue, `pending`, `is_ready` and `flush_all`; `FontStore`; the `RenderAssetReplaced` event and `RenderAssetKind`), `stats.rs` (`pending_uploads`), the `flush` system in set `RENDER_WINDOW`.
- Acceptance: **replace semantics.** `TextureStore::insert` and `FontStore::insert_font` called with a `Guid` that is already present replace the stored asset in place: the handle or `FontId` is unchanged, existing sprites and text show the new asset next frame, a failed replace keeps the old asset and emits nothing, and a success emits `RenderAssetReplaced { guid, kind }`. **Upload queue:** `flush` honours `max_upload_mpx_per_frame` (1 Mpx = 2^20 pixels, default 6.0, `0.0` unlimited) and the first upload of a frame always runs, so a texture larger than the cap is uploaded alone and a frame never spends more than `max(cap, one texture)`; a new texture draws nothing until uploaded; a replaced one keeps the old image until its upload; an image queued twice uploads once; `size(guid)` answers at once; `pending()`, `is_ready` and `flush_all` behave as in "Upload queue", and `RenderStats::pending_uploads` is filled. A font replace rebuilds the text state through the renderer (no store or caller flush). Fonts inserted before `init_renderer` are loaded when the renderer is built.
- Blocked by: intake-2, I16, I22.

**I17. `feat(render): plugin screenshot service`** (about 400 lines)
```
## Footprint
- Edits: crates/exitium_render/
- Governs: none
- Hotspots: none
- New crate edges: none
```
- Scope: `plugin/screenshot.rs`.
- Acceptance: `Screenshots::request` and `take` with the default-camera rule, `view` override, `NoCamera`, the 2048 size cap and `CaptureBusy`; pooled render textures; a headless test of the whole path, a test that a screenshot of a frame still arrives when `draw_cameras_to_surface` is false, `wait_for_uploads` (a screenshot requested right after a store insert shows the new texture, with the cap bypassed), and `ScreenshotRequest` is `#[non_exhaustive]` with `Default`.
- Blocked by: intake-2, I21.

**I18. `feat(exitium): re-export the render API through the facade`** (about 150 lines)
```
## Footprint
- Edits: none
- Governs: none
- Hotspots: crates/exitium/
- New crate edges: none
```
- Scope: `crates/exitium/src/` only; the edge `exitium -> exitium_render` is added by intake-2, so no manifest changes. **This is the only issue that adds render re-exports**, and it adds the complete list under a module, `exitium::render`, so the render `DebugDraw` and `Texture` names cannot collide with physics or assets: `wgpu` (the crate, re-exported by render), Stage 1 `Renderer`, `RendererConfig`, `PresentMode`, `RenderError`, `FrameStatus`, `SkipReason`, `Texture`, `ImageData`, `TextureDesc`, `ColorSpace`, `RenderTextureDesc`, `Filter`, `Address`, `Mips`, `SpriteDraw`, `Flip`, `Space`, `DrawList`, `Camera`, `CameraView`, `DepthMode`, `CameraPass`, `Pass`, `RenderTarget`, `SurfaceOverlay`, `OverlayCx`, `Scaling`, `FontId`, `TextStyle`, `TextDraw`, `TextAlign`, `TextWrap`, `DebugDraw`, `Capture`, `CaptureId`; Stage 2 `RenderPlugin`, `Sprite`, `Camera2d`, `Text`, `SceneView`, `SceneViewTarget`, `RenderWindow`, `CameraViews`, `RenderSettings`, `RenderStats`, `TextureStore`, `FontStore`, `RenderAssetReplaced`, `RenderAssetKind`, `Screenshots`, `ScreenshotRequest`, `ScreenshotId`, `ViewOverride`, and the set `RENDER_WINDOW` (by re-exporting the module `exitium_render::plugin::sets`, so the path is `exitium::render::plugin::sets::RENDER_WINDOW`). It also adds `RenderPlugin` to the `DefaultPlugins` group after `PlatformPlugin`, unless app-platform.md's F3 already did. A later change to render's public API gets its re-export in a facade follow-up.
- Acceptance: every name above is reachable as `exitium::render::<Name>` (a compile test lists them, and builds a `RenderPlugin { config: RendererConfig { power: exitium::render::wgpu::PowerPreference::LowPower, .. }, .. }` without a direct `wgpu` dependency); `cargo doc` has no broken re-export; high-risk review (facade API).
- Blocked by: intake-2, I17.

**I19. `feat(exitium): windowed sample through the plugins`** (about 250 lines)
```
## Footprint
- Edits: none
- Governs: none
- Hotspots: crates/exitium/
- New crate edges: none
```
- Scope: `crates/exitium/examples/` (a sample that uses `DefaultPlugins`: `PlatformPlugin`, `RenderPlugin`, the G1a window glue); it adds no manifest entry (the sample's web entry is the platform's, per app-platform.md).
- Acceptance: draws sprites, text, two cameras and a screenshot request through the ECS plugins on Windows and in Chrome, with the window glue from G1a; resizing the window reaches the renderer within one frame.
- Blocked by: intake-2, I18, G1a, the platform plugin issue PP2 of app-platform.md.

**G1. `feat(exitium): facade glue that feeds the renderer`** (four slices, each about 150 to 450 lines; filed after #108, #109 and #111 merge)

All slices have the footprint below (`New crate edges: none`: `exitium -> exitium_render`, `exitium_assets`, `exitium_physics`, `exitium_ui` and `exitium_platform` come from intake-2), run one at a time (shared hotspot), and are `Blocked by: intake-2, I18` plus what each lists. Where another spec already has an issue for the same systems (app-platform.md F1, physics-2d.md G1, ui-mvp.md F1), the architect files it once and cross-links it.
```
## Footprint
- Edits: none
- Governs: none
- Hotspots: crates/exitium/
- New crate edges: none
```
- **G1a, window glue** (about 150 lines; the window half of app-platform.md F1). `PreStartup`: `RenderWindow::set_target(primary_window.0.clone())` from `Option<Res<PrimaryWindow>>`. `First`, `after_set(INPUT)` and `before_set(RENDER_WINDOW)`: `RenderWindow::set_size(info.physical_size, info.scale_factor)` from `Option<Res<WindowInfo>>`. Acceptance: a window opens and the renderer takes the surface; a resize reaches the renderer in the same frame; a headless app (no `PrimaryWindow`, no renderer) runs a frame without panicking. Blocked by: intake-2, I16, the platform plugin issue PP2.
- **G1b, assets** (about 300 lines). Decoded images and fonts from `exitium_assets` into `TextureStore` and `FontStore`. Acceptance: a headless golden through the glue. Blocked by: intake-2, I21, I18, the assets Stage 2 issue of #111.
- **G1c, physics debug** (about 150 lines). **This is the same issue as physics-2d.md's G1** ("physics re-export and debug draw into the renderer"): it is filed once, and both specs name that one issue. `PhysicsDebugDraw.lines` into `DebugDraw::line(.., Space::World)` in set `EXTRACT`, and `DebugDrawOptions::bounds` from `CameraViews::main()`. Acceptance: a headless golden of collider outlines. Blocked by: intake-2, I15, I18, the physics plugin issue (`PhysicsDebugDraw`).
- **G1d, UI** (about 450 lines; ui-mvp.md F1). `UiMeasure` over `Renderer::measure_text` and `TextureStore::size`, then `UiDrawList` into `SpriteDraw` and `TextDraw` per the ui-mvp.md handoff (layers, depth, `dpi_scale` division, `ui_scale / dpi_scale` text scale). Acceptance: the round-trip test for `dpi_scale` in {1, 1.25, 1.5, 2} and a headless golden of a panel with text. Blocked by: intake-2, I15, I21, I18, the UI issues U8 and U11.

The reflected types and services listed under "MCP / editor surface" are exposed by the #110 issues; this spec files nothing in `exitium_mcp` or `exitium_editor`.
