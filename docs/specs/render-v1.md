# Spec: Render v1 (wgpu sprites, camera, text, screenshots; WebGPU on web)

- Issue: #107
- Crates: `exitium_render` (Stage 1 depends on `exitium_core` only; the Stage 2 `plugin` module adds `exitium_ecs`, `exitium_app`, `exitium_reflect`)
- Status: draft

## Goal

`exitium_render` is the 2D renderer of the Jam MVP. It turns a list of sprites, text and debug shapes plus one or more cameras into pixels on a window surface (Windows, and the browser through **WebGPU**) or into an offscreen texture that can be read back as an image for MCP. It must be fast on the 4-core, 8 GB min-spec (INTERVIEW #13), hold 20,000 alpha-blended sprites at 60 fps, and keep the later paths open: pixel art with integer scaling, isometric depth sorting, and a render thread.

The crate ships in two stages (Jam MVP contract, #107):

| Stage | What | Depends on |
|---|---|---|
| **1. Standalone** | `Renderer`, `DrawList`, `Camera`, textures, text, debug draw, capture. No ECS: a plain Rust program (or test) fills a `DrawList` and calls `render`. | `exitium_core`, `wgpu`, `glyphon`, `bytemuck` |
| **2. Plugin** | A `plugin` module: the `Sprite`, `Camera2d` and `Text` components, extraction from the ECS into a `DrawList`, settings, stats and a screenshot service. | Stage 1, `exitium_ecs`, `exitium_app`, `exitium_reflect` |

The design is a **plan/execute split**. Everything that decides *what* to draw (cull, sort, batch, instance data, camera math, text layout keys) is plain CPU code that needs no GPU, so it is unit-tested and benchmarked on CI without an adapter. Only the last step (upload, encode, submit) touches `wgpu`.

Versions were checked on 2026-11-18 (crates.io; browser support from the caniuse WebGPU table): `wgpu` 30.0.2, `glyphon` 0.12.0 (built on `cosmic-text` 0.19, needs `wgpu ^30`), `raw-window-handle` 0.6.2, `winit` 0.30.13 stable (0.31 is still beta). `wgpu`+`glyphon` was compiled for `wasm32-unknown-unknown` with the `webgpu` feature and no extra rustflags; the result is recorded under "Dependencies".

## Non-goals

- **WebGL2 and any native GL fallback.** Owner decision (2026-11-18): web is WebGPU only, Windows uses DX12 or Vulkan. The `gles` and `webgl` wgpu features stay off. PLAN.md decision 3 and risk R5 still say "WebGL2 fallback"; the roadmap issue (#112) updates them, not this spec. A browser without WebGPU gets a clear error (`RenderError::WebGpuUnavailable`), not a degraded path. caniuse lists WebGPU as fully supported in Chrome and Edge 113+ and partial in Firefox and Safari; the MVP tests Chrome only.
- 3D, lighting (the later normal-mapped 2D lighting is a separate pass), post-processing, MSAA, HDR output, depth buffer, user-written shaders and materials, shader hot reload (shaders are embedded `&'static str`).
- Tilemaps, particles, sprite animation, 9-slice (the UI draws nine ordinary sprites), atlas packing, texture compression, SDF text, rotated text, rich-text spans (open question 4). All can be added on top of the layouts below.
- The pixel-art pipeline (virtual resolution, palette swap, pixel-perfect rotation). v1 keeps the door open (pixel-perfect camera snap, nearest filtering, render-to-texture) and ships none of it.
- Decoding image files (PNG) and encoding the screenshot PNG. `exitium_render` takes decoded RGBA8 and returns raw RGBA8. It has no image-codec dependency.
- Asset loading, GUIDs, hot reload (assets spec #111), the window and input (app/platform spec #106), the UI tree (#109), physics (#108), the command bus and MCP wiring (#110). This spec names the interfaces it needs from them and nothing more.
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
    stats.rs         RenderStats
    plugin/          Stage 2: mod.rs, components.rs, extract.rs, stores.rs, screenshot.rs, settings.rs
  benches/batch.rs   (I4), benches/text.rs (I8), benches/extract.rs (I11)
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

**Web (WebGPU only).** wgpu's `webgpu` feature compiles the browser backend; with `webgl` off it is the only backend on `wasm32`. `request_adapter` and `request_device` return futures that only complete when the browser's event loop runs, so `Renderer::new` is `async` and **never blocks**. On the web the platform spawns the future (`wasm_bindgen_futures::spawn_local`) and hands the finished `Renderer` back to the app (Stage 2 does this in the plugin; Stage 1 samples do it by hand). Native gets `*_blocking` wrappers built on a 25-line `block_on` in `gpu.rs` (`std::task::Wake` plus `thread::park`): no `pollster` dependency. If `navigator.gpu` is missing or no adapter is found on the web, creation returns `RenderError::WebGpuUnavailable { reason }` with the text "WebGPU is not available in this browser (Chrome or Edge 113+ is required)". On native the equivalent is `RenderError::NoAdapter`. wgpu handles are not `Send` on the web by default; the crate enables wgpu's `fragile-send-sync-non-atomic-wasm` feature on `wasm32` only. That is sound only because the MVP's wasm has no threads (`exitium_tasks` runs inline there); if web threads are added later the feature must come off (open question 8).

**Surface format.** Prefer a format `f` with `f.is_srgb()` from the capabilities. If there is none (the WebGPU canvas offers only `Bgra8Unorm` or `Rgba8Unorm`), configure the first format and add its sRGB sibling (`TextureFormat::add_srgb_suffix`) to `view_formats`, and render through a view with the sRGB format. wgpu 30 requires `SurfaceConfiguration::color_space`; v1 uses `SurfaceColorSpace::Auto`. The chosen view format is the renderer's **target format**. Offscreen textures use the same format when a surface exists and `Rgba8UnormSrgb` when headless, so there is one set of pipelines and one text atlas.

**Present modes and resize.** `Vsync` maps to `AutoVsync`, `LowLatency` to `Mailbox` when supported else `AutoVsync`, `Immediate` to `AutoNoVsync`. On the web every mode behaves as `Vsync` (the browser paces frames). `resize` only records the new size; the next `render` reconfigures the surface once, so several resize events in one frame cost one `configure`. A zero width or height (minimized) makes `render` return `FrameStatus::Skipped(Minimized)` without touching the surface. Wgpu 30's `Surface::get_current_texture` returns a `CurrentSurfaceTexture` enum, handled as: `Success` render; `Suboptimal` render, then reconfigure; `Outdated` or `Lost` reconfigure and skip this frame; `Timeout` or `Occluded` skip; `Validation` skip and count an error. Presenting is `Queue::present(frame)` (wgpu 30 removed `SurfaceTexture::present`).

**Errors.** `RenderError` is a `#[non_exhaustive]` enum with `NoAdapter`, `WebGpuUnavailable`, `DeviceRequest(String)`, `Surface(String)`, `TextureTooLarge { size, max }`, `BadImage { expected_len, got_len }`, `NoFont`, `DeviceLost(String)`, `Capture(String)`. wgpu errors are converted to strings so no wgpu error type is part of the API. The device's uncaptured-error handler and device-lost callback record into `RenderStats` and `tracing::warn!`; tests install a handler that fails the test.

### Textures

```rust
pub struct Texture;                                // marker for Handle<Texture>
pub struct ImageData { pub size: UVec2, pub rgba8: Vec<u8> }   // straight alpha, sRGB-encoded, row-major, top-left origin
pub struct TextureDesc { pub filter: Filter, pub address: Address, pub mips: Mips, pub label: Option<String> }
pub enum Filter { Linear, Nearest }                // Nearest for pixel art
pub enum Address { Clamp, Repeat }
pub enum Mips { None, Generate }                   // Generate for portraits and anything shown smaller than stored

impl Renderer {
    pub fn create_texture(&mut self, image: &ImageData, desc: &TextureDesc) -> Result<Handle<Texture>, RenderError>;
    pub fn create_render_texture(&mut self, size: UVec2) -> Result<Handle<Texture>, RenderError>;
    pub fn destroy_texture(&mut self, texture: Handle<Texture>);   // a stale handle is a no-op
    pub fn texture_size(&self, texture: Handle<Texture>) -> Option<UVec2>;
    pub fn texture_view(&self, texture: Handle<Texture>) -> Option<&wgpu::TextureView>;  // egui-wgpu registers it
    pub fn white_texture(&self) -> Handle<Texture>;   // built-in 1x1 white: solid-color sprites use it
}
```

- **Table.** `Slab<GpuTexture>` from `exitium_core` keyed by `Handle<Texture>`. `GpuTexture { texture, view, bind_group, size, bytes }`. The handle is generational, so a destroyed texture drawn later is skipped and counted (`RenderStats::missing_textures`), never a panic or a wrong image.
- **Format** `Rgba8UnormSrgb`, `COPY_DST | TEXTURE_BINDING`, plus `RENDER_ATTACHMENT` when mips are generated. Render textures add `COPY_SRC` and `RENDER_ATTACHMENT`. Larger than `max_texture_dimension_2d` returns `TextureTooLarge`: the importer (#111) downscales, the renderer does not silently resample.
- **Mips** are generated on the GPU by a chain of draws (`mipmap.wgsl`: each level samples the previous one with a linear filter through the sRGB view, so averaging happens in linear space, and writes through the sRGB view). Compute is not used, so it works wherever render passes do. Upload is one `write_texture`.
- **Binding strategy.** One bind group per texture (view plus a shared sampler chosen from the four `Filter` and `Address` combinations). A draw call is a run of consecutive sorted sprites with the same texture. Sprite sheets (many sprites, one texture, per-sprite sub-rect) are therefore the batching tool; the asset pipeline owns atlas packing. Texture binding arrays are not in the WebGPU core spec, so they are not used. If a game needs more than about 200 texture switches per frame, a 2D-array path can replace the bind group without touching `SpriteDraw` (open question 10).
- **Alpha.** Textures keep straight alpha and the pipeline blends `SrcAlpha, OneMinusSrcAlpha` for color and `One, OneMinusSrcAlpha` for alpha. Transparent-edge pixels in source art should carry the neighbouring color, which the importer can do (open question 5).
- **Memory.** Textures are not evicted in v1. `RenderStats::texture_bytes` reports `width * height * 4 * (4/3 with mips)` summed. A 2048 x 2048 portrait with mips is about 22 MiB.

### Sprites, ordering and batching

```rust
pub enum Space { World, Screen }
pub struct Flip { pub x: bool, pub y: bool }

#[derive(Clone, Debug)]
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

**Instance layout (44 bytes, `step_mode: Instance`, no per-vertex buffer).** The unit quad is generated in the vertex shader from `vertex_index` (triangle strip, 4 vertices). The quad is in sprite space with +y up, so texture row 0 (the top of the image) lands on the quad edge at y = 1.

| Offset | Format | Field |
|---|---|---|
| 0 | `Float32x4` | `affine.matrix2` columns `(a, b, c, d)`: maps the unit quad `[0,1]^2` to **physical pixels of the target**. `size`, `anchor`, rotation, scale and the camera are folded in on the CPU. |
| 16 | `Float32x2` | `affine.translation` |
| 24 | `Float32x4` | `uv_rect` `(u0, v0, u1, v1)`, normalized. Flip swaps `u0/u1` or `v0/v1`. `Float32` rather than 16-bit unorm so a sub-rect edge is exact on 8192-pixel sheets and under minification. |
| 40 | `Unorm8x4` | `tint`, **sRGB-encoded** RGBA bytes, decoded to linear in the vertex shader (once per instance). 8-bit sRGB keeps precision in dark tints; HDR tint above 1.0 is not supported. |

100,000 sprites are 4.4 MB per frame, written with one `write_buffer` into a per-frame-in-flight buffer that doubles when full. The hard cap is `MAX_INSTANCES = 1 << 20` per frame; extra sprites are dropped and counted (`RenderStats::dropped`).

**Per pass pipeline** (`BatchPlan::build`, pure CPU, no wgpu):

1. **Filter.** Keep an item when `item.mask & camera.layers != 0` and its bounding box intersects the pass's viewport rectangle in physical pixels. The box of the transformed unit quad is `center +/- 0.5 * (|a| + |c|, |b| + |d|)` (about ten flops) once the affine has been composed with the camera (or the screen scale).
2. **Key.** One `u64` per kept item: `layer` biased to `u16` (bits 63..48) | `depth` as an order-preserving `u32` (bits 47..16) | low 16 bits of the texture slot (bits 15..0; text uses `0xFFFF`). The depth transform is `b ^ (((b as i32) >> 31) as u32 | 0x8000_0000)` on `f32::to_bits`. A non-finite depth is treated as 0 and counted. The sort sorts `(key, index)` pairs with `sort_unstable`, so the submission index breaks ties and the result is deterministic. The texture bits only group equal textures inside one (layer, depth); they never override depth.
3. **Instances.** Walk the sorted pairs, write `SpriteInstance`s, and start a new **run** when the texture or the `clip` changes or the kind switches between sprite and text. A run is `Run { kind, texture, clip, range }`.
4. **Text.** Consecutive text items form one text batch (below).

Because `depth` is an arbitrary `f32` chosen by the caller, isometric depth sorting with height levels needs nothing here: the iso code computes `depth` from tile coordinates and height. A y-sorted top-down game uses `depth = z - y * k` (open question 6). A sidescroller uses `layer` for parallax planes.

**Render pass.** One `RenderPass` per `CameraPass`: `LoadOp::Clear(color)` or `Load` when `Camera::clear` is `None`, the instance buffer bound once, a camera uniform bind group (dynamic offset into one uniform buffer with a 256-byte slot per pass) and, per run, `set_bind_group(1, texture)`, `set_scissor_rect` when `clip` is set, `draw(0..4, range)`. Debug lines are drawn last in each pass, always on top.

**Where the camera is applied.** On the CPU. The affine is composed with the camera once in step 1 (kept in a scratch array for steps 2 and 3). The instance affine maps the unit quad straight to **physical pixels of the target**: for `Space::World` the CPU multiplies the item's affine by `CameraView::view_from_world`, for `Space::Screen` by a uniform scale of `scale_factor`. So the shader has one code path for both spaces and no view matrix, the cull test is a plain rectangle test in pixels, and pixel-perfect snapping (rounding the quad's corner to a whole pixel) is a CPU `round`. The per-pass uniform holds only `target_size: vec2<f32>` (padded to 16 bytes) for the pixel-to-clip conversion, in one uniform buffer with a 256-byte slot per pass.

### Camera

```rust
/// Plain data. In Stage 2, `Camera2d` holds everything except `center` and `rotation`,
/// which come from the entity's `GlobalTransform2d`.
#[derive(Clone, Debug)]
pub struct Camera {
    pub center: Vec2,                 // world point at the middle of the viewport
    pub rotation: f32,                // radians, counter-clockwise
    pub scaling: Scaling,
    pub zoom: f32,                    // multiplier on top of `scaling`, > 0, default 1
    pub viewport: Rect,               // normalized 0..1 inside the target; default (0,0)-(1,1)
    pub pixel_perfect: bool,          // default false
    pub clear: Option<Color>,         // None keeps what is in the target (stacked cameras)
    pub layers: u32,                  // visibility mask, default 1
    pub order: i32,                   // ascending: lower renders first
}
pub enum Scaling {
    PixelsPerUnit(f32),               // logical px per world unit at zoom 1 (default 1.0)
    FixedHeight(f32),                 // the viewport shows this many world units vertically
    FixedWidth(f32),
    Fit(Vec2),                        // this world area is fully visible, extra shown on one axis
}
pub struct CameraView {               // the resolved camera for one pass
    pub view_from_world: Affine2,     // world -> physical px (top-left, y down)
    pub viewport_px: Rect,
    pub target_size: UVec2,
    pub scale_factor: f32,
    pub pixel_snap: bool,
    pub clear: Option<Color>, pub layers: u32, pub order: i32,
}
impl Camera {
    pub fn resolve(&self, target_size: UVec2, scale_factor: f32) -> CameraView;
}
impl CameraView {
    pub fn world_to_screen(&self, p: Vec2) -> Vec2;    // logical px: MCP clicks, UI anchored to the world
    pub fn screen_to_world(&self, p: Vec2) -> Vec2;
    pub fn visible_rect(&self) -> Rect;                // axis-aligned bound of the visible world area
}
```

**Resolution of the scale.** `ppu` (physical pixels per world unit) is `PixelsPerUnit(n)`: `n * scale_factor`; `FixedHeight(h)`: `viewport_px.height / h`; `FixedWidth(w)`: `viewport_px.width / w`; `Fit(s)`: the smaller of the two. Then `ppu *= zoom`.

**Pixel-perfect mode.** When `pixel_perfect` is true: `ppu` is rounded to the nearest integer and is at least 1 (integer scaling: one texel is a whole number of screen pixels); `center` is snapped to a multiple of `1 / ppu` (whole physical pixels, no sub-pixel shimmer while scrolling); and each sprite's quad corner is rounded to a whole physical pixel on the CPU when the instance is built, which removes the one-pixel jitter between neighbouring sprites. It composes with `Filter::Nearest` textures. Not in v1: virtual resolution, the integer-scaled upsample blit from a low-resolution render texture, sub-pixel camera and pixel-perfect rotation. Their ingredients (render textures, a fullscreen pass slot after the last camera, `Camera::viewport`) already exist, so they are additive (INTERVIEW #19).

**Several cameras.** `render` takes `&[CameraPass]` and sorts them by `order`. A second camera with `clear: None` draws on top of the first (HUD, minimap with a smaller `viewport`). Each pass filters the same `DrawList` by its `layers` mask and view, so the list is built once per frame.

### Text

Crate: **`glyphon` 0.12** (rendering) over **`cosmic-text` 0.19** (shaping, bidi, line breaking, font fallback, glyph rasterization with `swash`). Alternatives rejected: `fontdue` and `ab_glyph` (no shaping, no bidi, no fallback, so no complex scripts and no emoji), writing our own atlas on `cosmic-text` (about 600 lines of atlas and pipeline code that `glyphon` already has and tests; it remains the fallback if `glyphon` lags a `wgpu` major release; the crate boundary `text.rs` hides the choice). The `glyphon`/`cosmic-text` types never appear in the public API.

```rust
pub struct FontId(u32);
impl Renderer { pub fn load_font(&mut self, ttf_or_otf: Vec<u8>) -> Result<FontId, RenderError>; }

pub enum TextAlign { Left, Center, Right, Justify }
pub enum TextWrap { None, Word, WordOrGlyph }   // None: one line per '\n', never wraps

pub struct TextStyle {
    pub font: FontId,
    pub size: f32,                  // em size in the item's space (world units or logical px)
    pub line_height: f32,           // multiplier of `size`, default 1.2
    pub color: Color,               // linear, tint of the glyphs
    pub align: TextAlign,           // default Left
    pub wrap: TextWrap,             // default None
    pub max_width: Option<f32>,     // wrap width in the item's space; None = unbounded
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

- **Fonts.** Only fonts passed to `load_font` exist. System fonts are **not** loaded: the web has none, scanning them costs hundreds of ms at startup, and results must match across machines. `FontSystem` is built with `FontSystem::new_with_locale_and_db("en-US", empty fontdb)` and fonts are added to that database. Drawing text before any font is loaded returns nothing and counts `RenderStats::text_no_font`. The engine's default font and its license are open question 7.
- **Shaped-buffer cache.** The expensive step is shaping, so it is cached: `HashMap<u64, Cached>` keyed by a hash of `(text, font, size, line_height, align, wrap, max_width)` (std `DefaultHasher`, 64 bits: a collision among 10^4 live entries is below 10^-11). `Cached` holds the `glyphon::Buffer` and `last_used_frame`. An unchanged label costs one hash and one lookup per frame. Entries unused for 120 frames are dropped by a sweep every 64 frames. Layout is done at the unscaled size, so zooming reshapes nothing; only glyph rasterization depends on the zoom.
- **World text** is drawn with `TextArea.scale = ppu * scale`, quantized to steps of 1/16 so a continuous zoom does not fill the glyph atlas with near-identical sizes. **Screen text** uses `scale_factor * scale`. Position, anchor and layout width are converted to physical pixels per pass.
- **Atlas and batches.** One `glyphon::TextAtlas` (format = the renderer target format, `ColorMode::Accurate` because targets are sRGB views, color glyphs such as emoji supported) shared by every text batch, and `atlas.trim()` after each frame. Each `glyphon::TextRenderer` can be prepared and rendered once per frame, so text that sorts between sprites needs one renderer per text batch. A `Vec<TextRenderer>` grows on demand and is reused across frames, with a cap of `MAX_TEXT_BATCHES = 64`; beyond it later text joins the last batch (its order against sprites is then approximate) and `RenderStats::text_batches_merged` counts it. Real UIs sort to far fewer batches than that.
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

Lines have a **constant pixel width** (default 1 px, set per call) whatever the zoom. A line is one instance of 24 bytes: `a: Float32x2`, `b: Float32x2`, `color: Unorm8x4` (sRGB-encoded), `width: Float32`. The endpoints are already in physical pixels (the camera is applied on the CPU when the pass is built, as for sprites) and the vertex shader expands the segment to a quad perpendicular to it. `circle` and `polyline` are tessellated into segments on the CPU: a circle uses `clamp(ceil(radius_px / 4), 8, 64)` segments. Filled rectangles are not debug draws: use a `SpriteDraw` with `texture: None`.

### Render to texture and capture (MCP screenshots)

```rust
pub enum RenderTarget { Surface, Texture(Handle<Texture>) }
pub struct CameraPass { pub view: CameraView, pub target: RenderTarget }
pub enum FrameStatus { Presented, Rendered, Skipped(SkipReason) }   // Rendered: only texture targets, no surface
pub enum SkipReason { Minimized, Occluded, Timeout, Reconfigured }

impl Renderer {
    pub fn render(&mut self, list: &DrawList, debug: &DebugDraw, passes: &[CameraPass]) -> Result<FrameStatus, RenderError>;

    pub fn request_capture(&mut self, texture: Handle<Texture>) -> Result<CaptureId, RenderError>;
    pub fn poll_captures(&mut self) -> Vec<(CaptureId, Result<Capture, RenderError>)>;   // non-blocking
}
pub struct Capture { pub size: UVec2, pub rgba8: Vec<u8> }   // sRGB-encoded, tightly packed, top-left origin
```

`render` records all passes (surface and texture targets) in **one** command encoder, submits once and presents once. A pass with `Texture` target needs no surface, which is the headless path. The MCP screenshot is: create (or reuse) a render texture of the requested size, `render` the current `DrawList` into it through a `CameraPass` (the same list, any camera, any size, independent of the window), `request_capture`, and later collect the result from `poll_captures`.

**Readback.** `request_capture` records `copy_texture_to_buffer` at the end of the frame that renders the texture (rows padded to `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`, 256), then `map_async` after submit. `poll_captures` is called once per frame: on native it calls `Device::poll` without waiting and drains finished buffers; on the web the browser event loop completes the mapping between frames and `poll` just drains. Results are delivered one or two frames after the request; callers never wait inside a frame, which is the only model that works on the web (it cannot block on a map). Wgpu 30's `get_mapped_range` returns a `Result`, so a failed map becomes `RenderError::Capture`. The crate removes the row padding and, when the target format is BGRA, swaps channels, so `Capture` is always RGBA8. Scene screenshots should use an opaque clear color. The window surface itself (what the human sees including editor panels) is not captured in v1 (open question 9).

### Threading

| Thread | Role |
|---|---|
| Main (the app thread) | Owns `Renderer`. Builds `DrawList`, calls `render`, `resize`, `poll_captures`. All of it is `&mut self`, single-threaded in v1. |
| wgpu internals | wgpu may use its own threads or the driver's; we do not manage them. |
| Web | Single thread. `Renderer::new` is driven by the browser's event loop; there is no `block_on`. |

- `Renderer` is `Send` on native (wgpu 30 types are `Send + Sync`) and on the web through the `fragile-send-sync-non-atomic-wasm` feature, so it can be an ECS resource.
- `DrawList`, `DebugDraw` and `CameraPass` own their data, so **pipelined rendering is possible later without an API change**: the main thread fills list N+1 while a render thread runs `render(list N)`. It is not part of the MVP (no `exitium_tasks` integration, contract cut list). `BatchPlan::build` is a pure function of `&DrawList` and a `CameraView`, so extraction and batching can also be parallelized by the ECS executor later.
- The crate reads no clock (`Instant::now` panics on wasm); `RenderStats` holds counters only. Timing comes from the profiler's system spans (Stage 2) and from criterion.
- Locks: none of our own. Shared mutable state is none.

### Stage 2: the plugin

Stage 2 is a `plugin` module in the same crate. The exact `App`, `Plugin`, schedule-builder and ordering signatures belong to the app spec (**#106, an open dependency**). In **contract terms** the render plugin registers the following into the named schedules and reads `Transform2d` and `GlobalTransform2d`:

| Where | What |
|---|---|
| Components | `Sprite`, `Camera2d`, `Text` (all `Reflect`, below). Registered with the reflection registry. |
| Resources | `Renderer` (not reflected), `DrawList`, `DebugDraw`, `TextureStore`, `FontStore`, `Screenshots`, `RenderSettings` (reflected), `RenderStats` (reflected). |
| **Startup** | `init_renderer`: gets the window from the platform (D2 below) and builds the `Renderer`: `new_blocking` on native, a spawned future on the web. Headless apps use `Renderer::headless`. |
| **FixedUpdate** | Nothing. Rendering is variable-rate. |
| **Update** | `poll_renderer_init` (web: moves the finished `Renderer` into its resource), `apply_resize` (window size and scale factor to `Renderer::resize`), `apply_render_settings`, `poll_captures`. |
| **Render extraction** (runs after transform propagation, once per frame) | `extract_cameras` (cameras to `CameraPass`es), `extract_draws` (sprites and text to `DrawList`), then any glue system that appends to `DrawList` or `DebugDraw`, then `render_frame` (ordered after all of them: renders, presents, clears the lists). |

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
```

- **Why separate component types.** Stage 1 types (`SpriteDraw`, `Camera`, `TextDraw`) hold resolved data (a `Handle<Texture>`, a world `Affine2`) and need no reflection, so Stage 1 depends on `exitium_core` only. The components hold authoring data (a `Guid`, no transform) that reflection can persist (`Handle<T>` is not reflectable, reflect-model spec). Extraction is the explicit conversion. `Camera2d` has no `center` or `rotation`: they come from the entity's `GlobalTransform2d`, so a camera can follow a parent. The enums used as component fields (`Scaling`, `TextAlign`, `TextWrap`) are Stage 1 types; the Stage 2 issue I10 adds `#[derive(Reflect)]` to them, which is possible because the crate depends on `exitium_reflect` from Stage 2 on.
- **Needs from `GlobalTransform2d`** (D1): a 2D affine (translation, rotation, scale) in world space, already propagated through the hierarchy when the extraction schedule runs. The render crate does not use a z coordinate from the transform: order is `layer` and `z` on the component.
- **`extract_draws`** iterates `(&Sprite, &GlobalTransform2d)` and `(&Text, &GlobalTransform2d)`, resolves `Guid` to `Handle<Texture>` / `FontId` through `TextureStore` and `FontStore` (hash lookups, cached per frame for repeats), skips `visible == false`, and pushes `SpriteDraw`/`TextDraw`. Missing textures draw nothing and count `missing_textures`. It rebuilds the list every frame; a retained instance cache is a later optimization.
- **`TextureStore` and `FontStore`** hold `Guid -> Handle<Texture>` and `Guid -> FontId`, plus an `insert(guid, ImageData, TextureDesc)` and `insert_font(guid, bytes)` that call `create_texture` and `load_font` on the renderer. **Who calls them is not render's decision.** Render has no dependency on `exitium_assets`; the assumption is that **glue systems live in the `exitium` facade crate**, which depends on both. The same rule covers physics debug drawing (a facade system reads physics and writes `DebugDraw`) and the UI (a facade system converts the UI tree into `DrawList` entries). If #112 chooses another rule, only the glue moves; render's resources stay the same (D3, D4).
- **`Screenshots`** (the MCP service): `request(ScreenshotRequest { camera: Option<Entity>, size: UVec2, include_debug: bool }) -> ScreenshotId` and `take(id) -> Option<Result<Capture, RenderError>>`. `render_frame` fulfills pending requests in the frame after the request; it uses a pooled render texture per distinct size. Size is clamped to 4096 x 4096. It works in headless apps.
- **`RenderSettings`** (reflected resource): `vsync: bool`, `clear_color: Color`, `debug_draw: bool` (read by glue such as the physics debug system), `show_bounds: bool`. Changing it takes effect next frame.
- **Interpolation.** v1 draws `GlobalTransform2d` as of the last propagation. With the fixed step at 60 Hz and a 144 Hz display, moving objects will step. Fixing it needs a previous-tick transform from #106 (open question 3).
- **`Text.font`.** `None` means the `FontStore` default font (open question 7).

**Open dependencies on other specs.** Render fixes its side of each interface here and leaves the other side to the named spec; none of them changes the Stage 1 API.

| Id | Spec | What render needs or assumes |
|---|---|---|
| D1 | #106 (app) | `GlobalTransform2d` exposes a 2D affine and is propagated before the render extraction schedule runs; the schedule builder can order `render_frame` after `extract_*` and glue systems; the exact `App` and `Plugin` signatures. |
| D2 | #106 (platform) | A primary-window resource that gives an `Into<wgpu::SurfaceTarget<'static>>` value, the physical size, the scale factor and resize events; on the web, a way to drive the async `Renderer::new` future to completion. |
| D3 | #111 (assets) | Decoded RGBA8 and font bytes reach `TextureStore::insert` and `FontStore::insert_font` through glue; the persisted image reference type (open question 1). |
| D4 | #108 (physics), #109 (UI) | Their output reaches `DebugDraw` and `DrawList` through glue systems (facade assumption above); a text measure function for taffy (open question 4). |
| D5 | #110 (editor, MCP) | Implements the tools in the next section on the command bus and the MCP server. |
| D6 | #112 (roadmap) | Updates PLAN.md decision 3 and risk R5 from "WebGPU with WebGL2 fallback" to WebGPU only. |

## MCP / editor surface

Agent parity (design rule 1): render adds no editor feature that is not reachable by MCP. Because everything is reflected, almost all of it is generic.

| Capability | Route | Owner |
|---|---|---|
| Create, delete, move, edit `Sprite`, `Camera2d`, `Text` (any field, including `layer`, `z`, `tint`, `zoom`, `pixel_perfect`) | The generic entity and component tools and commands (spawn, set field, undo), schemas from `#[derive(Reflect)]` | #110, reflection |
| Render settings (vsync, clear color, debug overlays) | Generic set-field on the reflected `RenderSettings` resource | #110 |
| Read frame statistics | Generic read of the reflected `RenderStats` resource (`sprites_submitted`, `sprites_drawn`, `draw_calls`, `passes`, `text_areas`, `texture_count`, `texture_bytes`, `instance_bytes`, `missing_textures`, `dropped`, `errors`) | #110 |
| **Screenshot** | New tool `render_screenshot` with args `{ camera?: entity, width?: 1..=4096 (default 1280), height?: 1..=4096 (default 720), include_debug?: bool }`, result `{ width, height, png_base64 }`. The tool is a query (no undo). It calls `Screenshots::request`, waits for `take` over the next frames, and **encodes the PNG itself** (render only returns RGBA8). | #110 implements, render defines the contract |
| Convert a screen click to a world point (agent clicks in a screenshot) | Tool `render_screen_to_world { camera?, x, y }` and `render_world_to_screen` using `CameraView::{screen_to_world, world_to_screen}` | #110 |

**Editor.** The editor viewport renders the game into a render texture (`RenderTarget::Texture`) and shows it in an egui panel by registering `Renderer::texture_view` with `egui-wgpu` on the same `device()` and `queue()` (the editor is a feature of the game binary, so there is one device). The editor's scene camera is a plain `Camera` (pan, zoom) passed as an extra `CameraPass`; it is not an ECS entity. Gizmos and selection outlines use `DebugDraw` with `Space::World` or `Space::Screen`. Public `wgpu` types (`device()`, `queue()`, `texture_view()`, `SurfaceTarget`) are an intentional part of the API and tie `exitium_render` to one `wgpu` major version shared with `egui-wgpu`.

**Inspector fields.** `Sprite`: `image` (asset picker for a `Guid`), `rect`, `size`, `anchor` (0..1), `tint` (color picker via `custom(widget = "color")`), `flip_x/y`, `layer`, `z`, `mask`, `visible`. `Camera2d`: `scaling` (enum), `zoom` (`range(min = 0.01, max = 100.0)`), `viewport`, `pixel_perfect`, `clear`, `layers`, `order`, `active`. `Text`: `text` (multiline), `font`, `size` (`range(min = 1.0, max = 512.0)`), `align`, `wrap`, `max_width`, `color`. All have `tooltip` text.

## Dependencies

Stage 1 intake PR adds these to `[workspace.dependencies]` and `crates/exitium_render/Cargo.toml` at once, so no implementation issue touches `Cargo.lock`. Versions checked on 2026-11-18.

| Crate | Version (published) | License | wasm32 | Why not std |
|---|---|---|---|---|
| `wgpu` | **30.0.2** (2026-11-12). `default-features = false`, features `std`, `parking_lot`, `wgsl`, `dx12`, `vulkan`, `metal`, `webgpu`. On `wasm32` also `fragile-send-sync-non-atomic-wasm` (a `[target.'cfg(target_arch = "wasm32")'.dependencies]` entry). Not enabled: `gles`, `webgl`, `angle`, `spirv`, `glsl`, `serde`, `trace`. MSRV 1.87. | MIT OR Apache-2.0 | Yes: the `webgpu` feature is the browser backend. `cargo check --target wasm32-unknown-unknown` passed with no extra rustflags (2026-11-18). | The GPU API. `metal` is only for the macOS CI job (PLAN) and is a no-op elsewhere. |
| `glyphon` | **0.12.0** (2026-07-09), `wgpu ^30`; pulls `cosmic-text` 0.19 (MIT OR Apache-2.0), `etagere` 0.3, `lru` 0.16, `rustc-hash` 2 | MIT OR Apache-2.0 OR Zlib | Yes: compiled together with wgpu `webgpu` for `wasm32-unknown-unknown` (2026-11-18). | Text shaping, bidi, line breaking, fallback and glyph atlas. std has none of it. |
| `bytemuck` | **1.25.2**, feature `derive` | MIT OR Apache-2.0 OR Zlib | Yes | `unsafe` is denied workspace-wide, so instance structs are cast to bytes with `Pod` derives instead of hand-written casts. `wgpu` already depends on it. |
| `tracing` | workspace version (0.1.44), already declared | MIT | Yes | Warnings and errors from the renderer. Already a workspace dependency. |
| `criterion` | workspace version, dev only | MIT OR Apache-2.0 | n/a (dev) | Benchmarks, per `docs/benchmarks.md`. |

Not added, on purpose: `pollster` (a 25-line std `block_on` instead), `raw-window-handle` and `winit` (platform #106: `winit` 0.30.13; `raw-window-handle` 0.6.2 arrives through `wgpu`), `image` and `png` (assets #111 and MCP #110), `lyon` (circles and polylines tessellate in about 80 lines), `egui-wgpu` (editor #110; 0.37.0 needs `wgpu ^30` and `winit ^0.30.13`, which matches).

**License check.** The wasm build tree of `wgpu` + `glyphon` is 98 packages. Every license is permissive (MIT, Apache-2.0, Zlib, BSD-2-Clause, Unicode-3.0 and OR-combinations of them). Two expressions are worth a note for `cargo deny`: `self_cell` is `Apache-2.0 OR GPL-2.0-only` (satisfied by Apache-2.0) and `termcolor` is `Unlicense OR MIT`. `multiple-versions` will warn for `skrifa`, `read-fonts` and `font-types` (two versions each, from `cosmic-text` and `swash`); `deny.toml` has that as `warn`. The native Windows tree (DX12/Vulkan crates) was not enumerated here; the intake PR runs `cargo deny check` and reports it.

**Intake PR checklist (architect files it after this spec merges).** Root `Cargo.toml` and `Cargo.lock` (dependencies above); `crates/exitium_render/Cargo.toml` (deps, `[dev-dependencies] criterion`, `[[bench]]` entries for `batch`, `text`, `extract`); `lib.rs` declaring every module in the layout as an empty file with a `//!` doc, so no later issue edits `lib.rs`, `Cargo.toml` or the lockfile; a CI GPU job on the Linux runner (lavapipe, `EXITIUM_REQUIRE_GPU=1`) and a Windows job on the software adapter (a `.github/workflows/` change, which the owner pushes); `xtask` unchanged (`exitium_render` builds for wasm32, so it is not `NATIVE_ONLY`). Stage 2 intake adds the edges `exitium_render -> exitium_ecs`, `-> exitium_app`, `-> exitium_reflect` and the facade re-exports.

## Performance

Budgets are for **release profile** (`lto = "thin"`), medians from criterion on the reference dev PC, GPU-free (the plan/execute split makes them CI-gatable, 5% regression rule). **Absolute figures are provisional ceilings**: the first bench PR (I4, I7, I11) re-baselines them with the `perf` agent. The binding product target is the min-spec line below.

**Product target (min-spec: 4 cores, 8 GB, integrated GPU, 1080p).**

- **20,000 sprites per frame at 60 fps**, alpha-blended, from one or a few sprite sheets, average 32 x 32 px on screen: render-side CPU (extract + build + encode + submit) <= 3.0 ms per frame. Headroom: 100,000 sprites at 30 fps.
- Overdraw: the GPU fill budget is 4x 1080p (about 8.3 Mpx per frame, 500 Mpx/s at 60 fps).
- Draw calls: one per run of consecutive same-texture sprites; <= 64 in a typical frame, plus one per text batch and one for debug lines. `write_buffer` calls per frame: one for sprites, one for debug lines, one for camera uniforms.
- Startup: from `Renderer::new` to first presented frame <= 500 ms on native and <= 1.5 s on the web (excluding wasm download), including three pipelines (sprite, line, mip) <= 50 ms.
- Memory: renderer CPU-side fixed overhead <= 32 MiB excluding textures and font data; instance buffer `44 B * capacity` (20,000 sprites: 0.9 MB per frame in flight); glyph atlas <= 32 MiB typical, growing with distinct glyph sizes.

**Benchmarks** (`crates/exitium_render/benches/`, IDs `exitium_render/<area>/<case>`, sizes 1k, 100k, 1M where it makes sense, per `docs/benchmarks.md`):

| Benchmark | Budget | Kind |
|---|---|---|
| `batch/build` (filter + key + sort + instances) random layer and depth, 8 textures, all visible: 1k / 100k / 1M | <= 60 us / <= 9 ms / <= 130 ms (about 90 ns per sprite, dominated by the sort) | ceiling |
| `batch/build_sorted` (input already in order: the common frame-to-frame case) 100k | <= 4 ms (40 ns per sprite) | ceiling |
| `batch/cull` 100k, 10% visible | <= 1.5 ms (15 ns per item) | ceiling |
| `batch/build` with `layers` mask rejecting all 100k | <= 0.4 ms | ceiling |
| `camera/resolve` | <= 50 ns | ceiling |
| `debug/tessellate_circle` (radius 200 px) | <= 1 us | ceiling |
| `text/prepare_cached` 1,000 unchanged labels of 12 glyphs (hash, lookup, glyphon prepare) | <= 3 ms | ceiling |
| `text/shape_cold` one 16-character label | <= 20 us | ceiling |
| `extract/sprites` (Stage 2) 100k entities with `Sprite` + `GlobalTransform2d` | <= 4 ms (40 ns per sprite) | ceiling |

Steady-state **allocations in our code: zero** per frame after the first (`DrawList`, `BatchPlan` and the instance staging `Vec`s keep their capacity; verified by asserting capacities do not grow across 100 identical frames in a test). `glyphon` and `wgpu` may allocate internally.

Complexity: `build` is O(n) filter + O(k log k) sort over the k visible items; text is O(glyphs) after a cache hit; textures O(1) lookup; capture O(pixels).

GPU-side timings are not gated in CI (no GPU on runners). The sample app reports frame times for manual min-spec checks (I13). The wasm size contributed by `wgpu`+`glyphon` is measured once in the Stage 1 smoke build and reported, not gated.

## Test plan

- **Pure CPU unit tests** (no adapter, run everywhere, including the wasm build check): sort-key order (negative, zero, large depths; layer bias; NaN depth counted), run splitting on texture and clip change, mask filtering, cull boxes under rotation and non-uniform scale, flip and sub-rect UVs, `Camera::resolve` for every `Scaling` (including zero-size and extreme zoom), `world_to_screen` / `screen_to_world` round trips (property test), pixel-perfect rounding and snapping, debug tessellation segment counts, capture row-padding removal and BGRA swap, readback of a 1 x 1 and a 255 x 3 image (odd row sizes).
- **Headless GPU tests** (`tests/*.rs`): a helper requests the software adapter (WARP on Windows, lavapipe on Linux), builds the instance with `InstanceFlags::STRICT_WEBGPU_COMPLIANCE` (new in wgpu 30; it limits adapters and features to what WebGPU v1 allows, so native tests run under the browser's rules), and installs an uncaptured-error handler that **fails the test**. With no adapter a test **skips**, unless `EXITIUM_REQUIRE_GPU=1` (set on CI), where it fails. Cases: clear color; one sprite; tint; flip x and y; sub-rect from a sheet; layer and depth order with overlap and alpha; `texture: None` solid color; nearest vs linear sampling; mip generation (a 256 x 256 checker minified shows the average, not a sample); camera zoom, rotation, viewport; pixel-perfect on and off; screen space with `scale_factor` 1.5; clip rectangle; two cameras with `clear: None`; text with an embedded test font (any OFL font under 100 KB; its license file sits beside it in `tests/fonts/`); text align, wrap and `measure_text`; debug line, circle, rect; resize and zero-size resize; render-to-texture plus capture; stale-handle draw is skipped; texture too large; more than `MAX_INSTANCES`.
- **Golden images**: `tests/golden/*.rgba8` (a `u32` width, `u32` height, then bytes; 64 x 64, about 16 KB each; written and read with std only, no PNG dependency; regenerate with `EXITIUM_UPDATE_GOLDENS=1`). Compare per channel with tolerance 2 (3 for text) and at most 0.5% pixels over it, because software rasterizers differ by about one level. The Linux lavapipe run is authoritative.
- **Stage 2** (headless app, no window): spawn entities with `Sprite`, `Camera2d`, `Text`, run one frame, assert the golden image; hierarchy (a child sprite follows its parent); hidden entities draw nothing; reflection round trip of the three components (text format) and the schema hash of each is pinned; `Screenshots` request and `take` work headless; `RenderSettings` change applies next frame.
- **wasm**: `cargo build --workspace --target wasm32-unknown-unknown` covers `exitium_render` (checked in CI by `cargo xtask ci wasm`). There is no automated browser test in the MVP. Each Stage 1 and Stage 2 render PR that changes the GPU path states in its description that the sample ran in Chrome (clear, sprites, text and a capture), until the export spec (#111) provides a browser harness.
- **Benchmarks** under the 5% gate (table above).

## Open questions

1. **Sprite image reference.** `Sprite.image` is a `Guid` today because `Handle<T>` is not reflectable and `AssetRef<T>` is decided in the assets spec (#111, reflect-model open question 13). If #111 chooses `AssetRef<Image>`, the field type changes inside the plugin only.
2. **Who owns the glue.** This spec assumes the `exitium` facade holds the systems that feed render from assets, physics debug and the UI, because backend crates may not depend on each other (contract). #112 should confirm, or define a neutral place for the shared types (for example a draw-list type in `exitium_core`).
3. **Interpolation between fixed ticks.** Needs a previous-tick `GlobalTransform2d` (or an interpolated copy) from #106. Without it, moving objects step at 60 Hz on faster displays.
4. **Text measurement and rich text for the UI (#109).** Taffy layout needs a measure function; the crate graph forbids `ui -> render`. Options: a trait in `exitium_core` implemented by a render-plugin resource, or measurement through the facade. Rich text (several colors in one block) is not in v1; the UI can lay out several `TextDraw`s, or `TextDraw` gains `spans` additively.
5. **Premultiplied alpha.** v1 blends straight alpha and relies on importer-side edge bleeding, so a mip of a portrait with a transparent border can show a dark fringe. A premultiplied pipeline needs an importer step; decide with #111 after the first portrait test.
6. **Y-sort and isometric depth policy.** `depth` is a caller-supplied `f32`, so nothing is blocked. A `y_sort` flag on `Sprite` (depth from the world y) and height levels belong to the later isometric work.
7. **Default font.** No font ships in `exitium_render`. The editor and debug overlays need one. A small OFL font embedded in the `exitium` facade (license file in the repo, ADR-0003) is the proposal; decide with #110.
8. **Web threads.** `fragile-send-sync-non-atomic-wasm` is sound only without wasm atomics. If the web build ever gets threads (cross-origin isolation, nightly `build-std`), remove the feature and make `Renderer` a main-thread-only resource on the web.
9. **Capturing the window surface.** v1 captures only render textures, which is what agents need. Capturing exactly what the human sees (egui panels included) needs `COPY_SRC` on the surface configuration; additive if wanted.
10. **More than about 200 texture switches per frame.** If real games hit it, add a 2D-array texture path behind the same `SpriteDraw`. Not needed for sprite sheets or Glutton's portraits.
11. **Visibility inheritance.** `visible` is per entity in v1; a hidden parent does not hide its children. Add a `Visibility` hierarchy pass if needed.
12. **GPUs without DX12 or Vulkan.** With `gles` off, a very old Windows GPU has no backend and gets `NoAdapter`. Acceptable for the MVP (owner decision); revisit with the first user reports.

## Implementation issues

The architect files these with the `file-issue` skill in the Jam MVP milestone after this spec merges. Every issue edits exactly one crate and is PR-sized (about 600 changed lines or fewer). **One crate has at most one issue in flight** (factory rule), so the render issues run strictly in the order below. The order puts what Glutton and the MVP need first (draw sprites and text, then see the result through MCP) and the optional pieces last. `Blocked by` names slices here and is rewritten to `#N` when filed; blockers in other specs are written "after #106 stage 2" until their issue numbers exist. Each issue gets the `area:render` label.

**Intake (filed first, not a slice).** The Stage 1 intake PR from "Dependencies" above. Footprint: Edits `crates/exitium_render/`; Governs none; Hotspots `Cargo.toml`, `Cargo.lock`, `deny.toml` (if needed); New crate edges none. The CI job for the GPU tests is a `.github/workflows/` change that the owner pushes. Every slice below is blocked by the intake.

**I1. `feat(render): device, surface, resize and present modes`** (about 500 lines)
- Footprint: Edits `crates/exitium_render/`; Governs none; Hotspots none; New crate edges none.
- Scope: `error.rs`, `config.rs`, `gpu.rs`, the `stats.rs` skeleton, the std `block_on`, `crates/exitium_render/CLAUDE.md`, the GPU test helper.
- Acceptance: `Renderer::headless_blocking` works on the software adapter. Surface-format choice is a pure function with unit tests for sRGB, non-sRGB plus `view_formats`, and empty capabilities. The `CurrentSurfaceTexture` mapping is a pure function tested for every variant. `resize` coalescing and zero-size handling are unit tested. `WebGpuUnavailable` and `NoAdapter` are returned in the matching cases. The helper (software adapter, `STRICT_WEBGPU_COMPLIANCE`, fail on uncaptured error, `EXITIUM_REQUIRE_GPU`) exists. The crate `CLAUDE.md` is written. The wasm build passes.
- Blocked by: intake.

**I2. `feat(render): textures, samplers, mip generation and render textures`** (about 550 lines)
- Footprint: same as I1.
- Scope: `texture.rs`, `mipmap.wgsl`.
- Acceptance: create, destroy, stale-handle and size-limit behavior is tested. Mips of a checker texture are verified by readback. `white_texture`, `create_render_texture` and `texture_bytes` accounting work.
- Blocked by: I1.

**I3. `feat(render): camera math with scaling modes and pixel-perfect snap`** (about 400 lines)
- Footprint: same as I1.
- Scope: `camera.rs`.
- Acceptance: `Camera`, `Scaling`, `CameraView`, `resolve`, `world_to_screen`, `screen_to_world` and `visible_rect` for every scaling mode. Pixel-perfect rounding and center snap. A property test for round trips. The `camera/resolve` benchmark.
- Blocked by: I1.

**I4. `feat(render): sprite sort, cull and instance batching (CPU)`** (about 600 lines)
- Footprint: same as I1.
- Scope: `batch.rs`, `benches/batch.rs`.
- Acceptance: `SpriteDraw`, `DrawList`, key packing and `BatchPlan::build`; runs split on texture, clip and kind; the CPU unit tests from the test plan; a test that capacities stay constant over 100 identical frames; every `batch/*` benchmark within its ceiling.
- Blocked by: I1, I3.

**I5. `feat(render): sprite pipeline and Renderer::render`** (about 600 lines)
- Footprint: same as I1.
- Scope: `sprite.wgsl`, `renderer.rs`, the golden-image test helper.
- Acceptance: one pass per camera; the `Presented`, `Rendered` and `Skipped` statuses; instance and uniform upload; scissor; CPU pixel snap; `MAX_INSTANCES` drop counted. Golden tests for clear, sprite, tint, flip, sub-rect, order, alpha, camera zoom and rotation, pixel-perfect, screen space and two cameras.
- Blocked by: I2, I4.

**I6. `feat(render): capture readback for render textures`** (about 350 lines)
- Footprint: same as I1.
- Scope: `capture.rs`.
- Acceptance: `request_capture` and `poll_captures`; row-padding removal; BGRA fix-up; error mapping; tests for odd sizes; non-blocking on native; builds for wasm.
- Blocked by: I5.

**I7. `feat(render): text with glyphon, shaped-buffer cache and text batches`** (about 600 lines)
- Footprint: same as I1.
- Scope: `text.rs`, `benches/text.rs`.
- Acceptance: `load_font`, `TextDraw`, the cache with its sweep, text batches interleaved with sprites by sort order, `MAX_TEXT_BATCHES`, clip, world and screen scale, `measure_text`, `text_no_font`. Goldens for align, wrap and color with the test font. The `text/*` benchmarks within their ceilings.
- Blocked by: I5.

**I8. `feat(render): debug lines, circles, rects and polylines`** (about 450 lines)
- Footprint: same as I1.
- Scope: `debug.rs`, `line.wgsl`.
- Acceptance: the `DebugDraw` API; constant pixel width; the segment-count rule; drawn last; world and screen space; goldens; the `debug/tessellate_circle` benchmark.
- Blocked by: I5.

**I9. `feat(render): diagnostics: device-lost, uncaptured errors and RenderStats`** (about 300 lines)
- Footprint: same as I1.
- Scope: `stats.rs`, `gpu.rs`.
- Acceptance: every `RenderStats` field in the MCP table is filled by its code path; device-lost surfaces as `RenderError::DeviceLost`; uncaptured errors are counted and logged; a test forces one.
- Blocked by: I7, I8.

**I10. `feat(render): plugin components, settings and registration`** (about 450 lines)
- Footprint: same as I1, plus New crate edges `exitium_render -> exitium_ecs`, `exitium_app`, `exitium_reflect` (added by the Stage 2 intake, so the issue itself adds none).
- Scope: `plugin/mod.rs`, `components.rs`, `settings.rs`, and `#[derive(Reflect)]` on `Scaling`, `TextAlign` and `TextWrap` in `camera.rs` and `text.rs`.
- Acceptance: `Sprite`, `Camera2d` and `Text` derive `Component` and `Reflect` with the attributes in the inspector list; `RenderSettings` and `RenderStats` are reflected resources; reflection round trip and pinned schema hashes; no GPU needed.
- Blocked by: I9, Stage 2 intake, after #106 stage 2 (App, Plugin, schedules).

**I11. `feat(render): plugin extraction and render_frame systems`** (about 550 lines)
- Footprint: same as I10.
- Scope: `plugin/extract.rs`, `benches/extract.rs`.
- Acceptance: `extract_cameras`, `extract_draws` and `render_frame` registered in the render extraction schedule with the ordering in "Stage 2"; `Guid` resolved through the stores; a headless app golden test (sprite, text, camera, hierarchy, hidden entity); the `extract/sprites` benchmark within its ceiling.
- Blocked by: I10, `GlobalTransform2d` from #106.

**I12. `feat(render): plugin stores, init and screenshot service`** (about 550 lines)
- Footprint: same as I10.
- Scope: `plugin/stores.rs`, `screenshot.rs`, the init systems.
- Acceptance: `TextureStore` and `FontStore`; `init_renderer` (native blocking, web spawned future, headless); `poll_renderer_init`; `apply_resize`; `Screenshots::request` and `take` with the size clamp and pooled textures; a headless test of the whole screenshot path.
- Blocked by: I11.

Two issues land outside the render crate and are listed here so the architect files them with their own footprints:

**I13. `feat(exitium): windowed and web render sample`**
- Footprint: Edits `crates/exitium/examples/`; Governs none; Hotspots `crates/exitium/`; New crate edges `exitium -> exitium_render`.
- Acceptance: draws sprites, text, a camera and a capture on Windows and in Chrome; reports frame times at 20,000 sprites for the min-spec check.
- Blocked by: I12 and the platform window issue from #106.

**I14. `feat(exitium): re-export render types through the facade`**
- Footprint: Edits `crates/exitium/` (hotspot, high-risk review: facade API); Governs none; Hotspots `crates/exitium/`; New crate edges none.
- Acceptance: `Sprite`, `Camera2d`, `Text`, `RenderSettings` and `RenderPlugin` are reachable through `exitium`; the glue systems from open question 2 live here if #112 confirms the facade rule.
- Blocked by: I12.

The MCP tools `render_screenshot`, `render_screen_to_world` and `render_world_to_screen` are implemented by the #110 issues, against the contract in "MCP / editor surface".
