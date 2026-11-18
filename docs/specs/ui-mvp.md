# Spec: Game UI MVP (retained tree on taffy)

- Issue: #109
- Crates: exitium_ui. Stage 1 depends on exitium_core and taffy only. The Stage 2 `plugin` module adds exitium_ecs, exitium_app, exitium_reflect and exitium_command, plus exitium_serial as a dev-dependency. There is **no edge** to exitium_render, exitium_platform or exitium_assets (runtime crates have no edges between them); the facade holds all cross-crate glue.
- Status: draft (revised after review round 1)

## Goal

Specify `exitium_ui` for the Jam MVP: a retained widget tree with flex layout on [taffy](https://crates.io/crates/taffy), five widgets (panel, label, image, button, progress bar), screen-space anchoring and scaling, pointer and keyboard-focus input that the UI consumes before gameplay, and basic theming. An agent builds and edits the tree through commands, a human refines it in the editor, and the same tree runs on Windows and in the browser (wasm32).

This spec also settles PLAN Phase 3 item 1, "ECS entities or a dedicated arena": **a dedicated arena** (see "Node storage decision"). PLAN says "decided by ADR"; the owner-approved issue records it here, with alternatives, to keep `docs/decisions/` (a hotspot) out of this PR.

Design in one paragraph: `UiTree` owns a generational arena of nodes. Each node has a `WidgetKind`, an authored `UiStyle` and a parent and children list. Layout data (the taffy style, taffy's cache and the computed rectangle) sits in a parallel array so paint and hit-test walks never load it. Layout runs through taffy's low-level traits directly on the arena, one independent layout per root, with anchoring applied after layout. Edits clear exactly the caches they invalidate, and edits that cannot change geometry (counters with a fixed size, progress values, hover state) skip layout entirely. Painting turns the laid-out tree into a flat `UiDrawList` of quads, images and text runs in physical pixels. Input is a small plain-data frame in, `UiEvent`s and a `UiCapture` out. **Stage 1 is all of this with no ECS, no render and no platform dependency.** **Stage 2** wraps it in a plugin that registers resources and systems into the app schedules; the facade feeds the plain draw data to the renderer and supplies the text measurer.

## Non-goals

Deferred from the MVP, each addable later without breaking the layout below (design rule 5):

- Widgets beyond the five: rich text, toggle, slider, scroll view, list and virtualized list, grid, tooltip, modal, tabs, text input. `WidgetKind` is `#[non_exhaustive]`.
- Data binding, UI markup files with hot reload, and the `Num` display widget (see "Path to the deferred items"). A game updates a label from Rust with `set_text` and formats numbers with `exitium_core::NumFormat`.
- 9-slice images (the render spec draws them as nine ordinary sprites, so the UI can add them later), rounded corners, borders, transitions and tweens, clipping and scrolling (no clip items in the draw data yet).
- The immediate-mode layer for debug overlays.
- Taffy's grid and block layout. Only flexbox is compiled in.
- World-space UI (health bars over entities). Those are sprites and text drawn by the renderer, not a retained tree.
- Gamepad and spatial navigation, touch gestures, IME and text editing, right-to-left text, localization, accessibility.
- Luau bindings (cut from the MVP) and networking.
- Parallel layout. The MVP executor is single-threaded and so is the layout pass; independent roots leave room for it (see Threading).
- Per-node image filtering. Nearest or linear sampling is a property of the texture (`TextureDesc.filter` in the render spec), chosen at import, so pixel-art images are imported with `Nearest`.

## Design

### Module layout

```
exitium_ui/src/
  lib.rs
  arena.rs        NodeArena: slots, generations, free list, restore          safe
  tree.rs         UiTree: structure ops, names, revisions, events            safe
  node.rs         UiNode, WidgetKind, per-widget data                        safe
  desc.rs         UiNodeDesc: build a subtree from data, describe it back    safe
  style.rs        UiStyle, Dim, Edges, flex enums, PointerMode               safe
  theme.rs        UiTheme                                                    safe
  layout/
    mod.rs        the per-frame pass, UiViewport, invalidation, LayoutStats  safe
    convert.rs    UiStyle -> taffy::Style (the only place taffy types appear) safe
    adapter.rs    taffy tree traits implemented over the arena               safe
  draw.rs         UiDrawList, UiDrawItem, paint walk, render-facing mapping  safe
  input.rs        UiInputFrame, hit list, hover/press/click/focus, UiCapture safe
  plugin/         Stage 2: mod.rs, systems.rs, commands.rs                   safe
```

The crate has no `unsafe`. Taffy types never appear in the public API, so a taffy 0.x upgrade is internal to `layout/`.

### Node storage decision (PLAN Phase 3 item 1)

**Decision: a dedicated arena inside `UiTree`, which the Stage 2 plugin stores as a resource. UI nodes are not ECS entities.**

| | A. ECS entities | B. Dedicated arena (chosen) | C. Arena as truth plus entity mirrors |
|---|---|---|---|
| Stage 1 without ECS (contract) | impossible: the layout core would need `exitium_ecs` | yes: depends on `exitium_core` only | impossible |
| Layout access pattern | each taffy callback (children, style, cache) is an entity lookup: location table, archetype, column | one slot index per callback, parallel arrays | as B plus mirror sync |
| Reparent and kind change | an archetype move copies the whole row, including taffy's 392 B cache | pointer-size edits in two vectors | as B plus mirror sync |
| Dirty tracking up the tree | `get_mut` per ancestor plus tick stamps | direct, early-out walk (see Invalidation) | two systems of truth |
| Stable ids under undo | despawn then restore gives a new `Entity` | arena can restore the exact id (see "Stable ids and undo") | both |
| Inspector, schema, serialization, agent tools | free from `Reflect` + `ReflectComponent` | free from `Reflect` on the data types plus registered UI commands | free |
| Visible to ECS queries, scenes, `ecs_*` tools | yes | no (the cost) | yes, duplicated |
| Duplicated code | none | parent/child maintenance, about 150 lines | most |
| Fit to scale | built for 1M rows | UI trees are hundreds to tens of thousands of nodes | as B |

Reasons for B:

1. **The crate graph decides Stage 1.** The contract has `exitium_ui` build in parallel with the ECS and depend on `exitium_core` only. Layout, input and draw output can then be written and tested headlessly before the ECS exists.
2. **UI is a tree walked as a tree.** Taffy asks for children, style and cache of one node many times per pass. An arena answers with an index; ECS storage answers with several lookups and keeps nodes of different widget kinds in different archetypes. The ECS advantage, bulk iteration over millions of homogeneous rows, does not apply to a few thousand heterogeneous nodes.
3. **Hot and cold data split cleanly** in an arena (see data layout), which keeps the paint walk fast. Per-component storage would split it by component instead of by access pattern.
4. **Nothing in the design rules is lost.** Reflection still derives the schema (Stage 2 puts `#[derive(Reflect)]` on the data types), all mutation still goes through typed commands with undo, and every capability is exposed to the generic agent tools (see "Command and query surface").
5. **ECS change detection is still used where it pays**: in data binding, which reads game state (`Changed<T>`) and pushes values into the arena. The tree needs only its own dirty flags.

Accepted costs: UI nodes do not appear in the ECS entity list, and scenes do not carry them as entities (a UI tree saves as a `UiNodeDesc`, see "Path to the deferred items"). A game entity that needs a UI node stores its `UiNodeId` in a component.

Revisit if per-widget ECS behavior (a system per widget, widgets as scene prefabs) becomes the main way games author UI. The `UiNodeDesc` boundary makes that migration a spawn-from-desc function, not a rewrite.

### Data layout

```rust
pub type UiNodeId = ...;            // generational index, same shape as exitium_core::RawHandle (index + NonZeroU32 generation)

pub struct UiTree {
    arena: NodeArena,               // slot -> hot node data
    layout: Vec<LayoutData>,        // parallel array, indexed by slot index, see below
    roots: Vec<UiNodeId>,           // sorted by RootConfig::layer, then creation order
    names: HashMap<Box<str>, UiNodeId>,
    theme: UiTheme, viewport: UiViewport,
    input: InputState,              // hover, pressed, focus, prev button, press origin
    hit_list: Vec<HitEntry>,        // interactive nodes in paint order, rebuilt after layout
    events: Vec<UiEvent>,           // valid until the next process_input
    capture: UiCapture,
    layout_revision: u64, paint_revision: u64,
}

struct NodeHot {                    // target <= 192 B excluding heap data
    parent: Option<UiNodeId>,
    children: Vec<UiNodeId>,        // paint order: later children draw on top
    name: Option<Box<str>>,
    kind: WidgetKind,
    style: UiStyle,                 // authored, compared on set to skip no-op edits
    root: Option<RootConfig>,       // Some only for roots
    rect: Rect,                     // absolute, UI-logical px, set by the post-layout pass
    flags: NodeFlags,               // hovered, pressed, focused
}

struct LayoutData {                 // about 720 B per node with taffy's flexbox-only build
    style: taffy::Style,            // derived from UiStyle + theme + widget kind (248 B)
    cache: taffy::Cache,            // 392 B
    unrounded: taffy::Layout,       // parent-relative, 76 B
}
```

- **Arena.** `NodeArena` is a `Vec` of slots with a `NonZeroU32` generation and a LIFO free list (`Vec<u32>`), the same rules as `exitium_core::Slab` (stale ids fail every lookup in O(1), a slot whose generation would overflow is retired). It is a separate small type, not `Slab`, because it adds `restore` (see "Stable ids and undo"). If another crate needs `restore`, fold it into `Slab` later; that is not a UI decision.
- **Why parallel arrays.** Paint and hit-test read `kind`, `style`, `rect` and `flags`. Layout reads `LayoutData`. Splitting them keeps the paint walk's working set about 1 MB per 5,000 nodes instead of 5 MB. `layout[i]` is valid for every live slot `i` and is resized with the arena.
- **Memory budget:** at most 1.25 KiB per node including `LayoutData` and excluding strings; 10,000 nodes fit in 12.5 MB. Measured sizes on a 64-bit build with taffy's `std` + `flexbox` features: `Style` 248 B, `Cache` 392 B, `Layout` 76 B.
- **Names.** A node may have a unique name (`"score_label"`). `find(name)` is how Rust game code and agents address nodes without holding ids. A duplicate name is `UiError::DuplicateName`.
- **Roots.** A root is a node with `parent == None` and `root: Some(RootConfig)`; `roots` is the draw order (lower `layer` first, so higher layers draw on top and are hit first). `RootConfig::layer` is limited to `-1000..=1000` (`UiError::LayerRange`), which the render mapping needs.

Widgets:

```rust
#[non_exhaustive]
pub enum WidgetKind {
    Panel,                                     // flex container with an optional background
    Label(LabelData),                          // text leaf
    Image(ImageData),                          // image leaf
    Button(ButtonData),                        // flex container with interaction states
    ProgressBar(ProgressData),                 // track plus fill leaf
}
pub struct LabelData    { pub text: String, pub font_size: Option<f32>, pub color: Option<Color>, pub align: TextAlign /* Start | Center | End */, pub wrap: bool }
pub struct ImageData    { pub image: Guid, pub tint: Color, pub fit: ImageFit /* Stretch | Contain */ }
pub struct ButtonData   { pub enabled: bool }
pub struct ProgressData { pub value: f32 /* clamped 0..=1 on set */, pub direction: FillDirection /* LeftToRight | BottomToTop */, pub fill: Option<Color>, pub track: Option<Color> }
```

`None` colors and sizes mean "take it from the theme". Images name an asset by `Guid` (stable, serializable, already in core); the facade resolves it to a texture. A button is a flex container: its text is an ordinary child label, so a button can hold an icon and a label.

Style (flex only; no taffy types):

```rust
pub struct UiStyle {
    pub direction: Direction,            // Row | Column (default Column)
    pub justify: Justify,                // Start | Center | End | SpaceBetween | SpaceAround
    pub align_items: Align,              // Start | Center | End | Stretch
    pub align_self: Option<Align>,
    pub gap: f32,
    pub padding: Option<Edges>,          // None: theme padding for Panel and Button, 0 for others
    pub margin: Edges,
    pub width: Dim, pub height: Dim,     // Dim = Auto | Px(f32) | Percent(f32)
    pub min_width: Dim, pub min_height: Dim, pub max_width: Dim, pub max_height: Dim,
    pub flex_grow: f32, pub flex_shrink: f32,
    pub visible: bool,                   // false: display none, no layout, no draw, no hit
    pub background: Option<Color>,       // None: theme for Panel and Button, transparent otherwise
    pub pointer: PointerMode,            // Auto | Block | Pass
}
pub struct Edges { pub left: f32, pub top: f32, pub right: f32, pub bottom: f32 }
```

`PointerMode::Auto` resolves to `Block` for buttons and for panels with a visible background, and to `Pass` for everything else, so a transparent full-screen HUD container does not eat gameplay clicks while a visible panel does.

Roots, anchoring and scaling:

```rust
pub struct RootConfig {
    pub anchor: Anchor,                  // TopLeft, Top, TopRight, Left, Center, Right, BottomLeft, Bottom, BottomRight
    pub pivot: Option<Vec2>,             // fraction of the root's own size, default = the anchor's fraction
    pub offset: Vec2,                    // UI-logical px, +x right, +y down
    pub size: RootSize,                  // Fill (viewport) | Fixed(Vec2) | Fit (content)
    pub layer: i16,                      // -1000..=1000
}
pub struct UiViewport {
    pub physical_size: UVec2,            // render target size in pixels
    pub dpi_scale: f32,                  // window scale factor (the renderer's `scale_factor`)
    pub scale_mode: ScaleMode,           // Constant { multiplier } | ReferenceHeight { height, snap_integer }
}
```

Three coordinate spaces, kept apart on purpose:

| Space | Unit | Used for |
|---|---|---|
| **UI-logical** | `ui_scale` physical pixels | layout, `UiStyle` sizes, hit testing, `UiInputFrame.pointer` |
| **Physical** | device pixels | `UiDrawList` coordinates (snapped here) |
| **Screen** (render spec #107, `Space::Screen`) | physical / `dpi_scale` | what the renderer takes; produced by the facade from the draw list (see "Render handoff") |

- `ui_scale` (UI-logical to physical): `Constant` is `dpi_scale * multiplier`. `ReferenceHeight` is `physical_size.y / height`, rounded to the nearest integer (at least 1) when `snap_integer` is set, which is what keeps pixel art crisp. The UI-logical viewport is `physical_size / ui_scale`. A 1080p design works unchanged at any resolution. `ui_scale` equals `dpi_scale` only in `Constant` mode with multiplier 1, which is why Screen space and UI-logical space differ in general.
- Origin of a root: `anchor_fraction * logical_viewport - pivot * root_size + offset`. A `TopRight` root with offset `(-16, 16)` sits 16 px in from the top-right corner at every size.
- Taffy's own pixel rounding is **off**. Rectangles are snapped to whole **physical** pixels when the draw list is built, so edges and text origins are crisp at fractional scales.

Draw data (the only thing the renderer side sees; plain data, no ECS and no GPU types):

```rust
pub struct UiDrawList {
    pub revision: u64,
    pub ui_scale: f32, pub dpi_scale: f32,   // the two factors the glue needs (see "Render handoff")
    pub items: Vec<UiDrawItem>, pub text: String,
}

#[non_exhaustive]
pub enum UiDrawItem {                        // every item carries its root's layer
    Quad  { layer: i16, rect: Rect, color: Color },
    Image { layer: i16, rect: Rect, image: Guid, tint: Color },   // always the whole texture
    Text  { layer: i16, origin: Vec2, run: TextRun },             // run.text is a byte range into UiDrawList::text
}
pub struct TextRun {
    pub text: Range<u32>, pub font: Option<Guid>,
    pub size: f32, pub line_height: f32,     // em size in UI-logical px; line_height is a multiplier of size (theme, default 1.2)
    pub color: Color, pub align: TextAlign, pub wrap: bool,
    pub max_width: f32,                      // the node rectangle's width, UI-logical px (the renderer aligns and wraps inside it)
}
```

`rect` and `origin` are **physical** pixels, origin top-left, y down. Every `Text` has `origin = node rectangle's top-left` and `max_width = rectangle width`, so the renderer aligns inside the node and the UI never needs a text width at paint time; text sits at the top of its node (vertical placement comes from layout: a label sized to its content fills its node). `size` and `max_width` stay UI-logical because the renderer lays text out at the unscaled size and scales the result (render spec, "Shaped-buffer cache"). One shared `text` buffer means a rebuild allocates nothing once the buffers are warm. `revision` increases only when the content changes, so a consumer can keep an uploaded batch when it is unchanged. The enum is `#[non_exhaustive]` so clip push and pop can be added when scrolling lands.

Order in the list: ascending `layer`; per root, **all graphics (quads, images) first, then all text**. See "Painting" for why and for the authoring rule this implies.

Theme:

```rust
pub struct UiTheme {
    pub font: Option<Guid>, pub font_size: f32,                    // 16.0 UI-logical px
    pub line_height: f32,                                          // 1.2
    pub text: Color, pub text_disabled: Color,
    pub padding: f32,                                              // default for Panel and Button
    pub panel_background: Color,
    pub button: ButtonColors,                                      // normal, hover, pressed, disabled
    pub focus_ring: Color,
    pub progress_track: Color, pub progress_fill: Color,
}
```

Resolution order for any color or size is the node's `Option` override, then the theme. Changing the theme clears all layout caches (padding and font size feed layout) and bumps `paint_revision`. Per-node overrides are the MVP's "styling"; named style classes are deferred.

### Layout (taffy integration)

**Dependency:** `taffy 0.15.1` (see "Wiring"). It is a pure layout library: `#![deny(unsafe_code)]`, no platform calls, builds for `wasm32-unknown-unknown`, and with `default-features = false, features = ["std", "flexbox"]` it has **no transitive dependencies** (checked with `cargo tree` and `cargo check --target wasm32-unknown-unknown`).

**Integration choice: taffy's low-level traits over our arena, not `TaffyTree`.** `TaffyTree` owns a second copy of the tree (its own storage, ids and children), which we would have to keep in sync with the arena on every add, remove and reparent. The low-level API (`TraversePartialTree`, `LayoutPartialTree`, `LayoutFlexboxContainer`, `CacheTree`, `compute_root_layout`, `compute_flexbox_layout`, `compute_leaf_layout`) is the documented route for embedding taffy in an existing widget tree, and taffy's `custom_tree_vec` example is the template. The implementer verifies the exact trait set against 0.15.1; rounding (`RoundTree`) is not used. `layout/adapter.rs` is a view struct that borrows the arena, `layout` array and a `&mut dyn UiMeasure` for the duration of one pass. Taffy `NodeId`s are the slot indices, which is safe because no structural change can happen during a pass.

Dispatch in `compute_child_layout`: `Panel` and `Button` go to `compute_flexbox_layout`; `Label`, `Image` and `ProgressBar` are leaves and go to `compute_leaf_layout` with a measure closure. A node with `visible == false` goes to `compute_hidden_layout`. Every call goes through taffy's cache first (`compute_cached_layout`).

Measuring, with no render dependency in Stage 1:

```rust
pub trait UiMeasure {
    /// Laid-out size of the text in UI-logical px. `max_width` is the available width when `wrap`.
    fn measure_text(&mut self, req: &TextMeasure<'_>) -> Vec2;
    /// Intrinsic size of an image in UI-logical px, or None when unknown (then an Auto size is 0).
    fn image_size(&mut self, image: Guid) -> Option<Vec2>;
}
pub struct TextMeasure<'a> {
    pub text: &'a str, pub font: Option<Guid>, pub size: f32, pub line_height: f32,
    pub align: TextAlign, pub wrap: bool, pub max_width: Option<f32>,
}
```

The request carries exactly the style fields a `TextRun` carries (the draw's `max_width` is the final rectangle width, the measure's is the width offered during layout). A renderer whose shaped-text cache is keyed by all of those fields (the render spec's is) then shapes a label once and the later draw is a cache hit. Because the renderer lays text out at the unscaled size, measuring needs **no scale**, and a `ui_scale` change never re-measures. Stage 1 tests use a fixed-advance monospace measurer. The Stage 2 measurer is the facade's (see "Render handoff"). Layout calls `measure_text` at most twice per dirty label per pass and never for a clean one.

Progress bars measure as their style size (default `Px(100) x Px(8)` when both are `Auto`).

**The per-frame pass** (`UiTree::layout(&mut self, measure: &mut dyn UiMeasure) -> LayoutStats`):

1. If the theme changed, clear every cache. If the UI-logical viewport size changed, clear the caches of roots whose size depends on it (`RootSize::Fill`). A change of `ui_scale` alone changes no UI-logical size, so it clears nothing.
2. For each visible root in layer order whose cache is empty: compute its available space (`Fill`: the logical viewport, definite; `Fixed`: its size; `Fit`: max-content) and call `compute_root_layout`. Roots with a populated cache are skipped entirely.
3. If any root was laid out, or the viewport (and so an anchor origin) changed: one depth-first walk of the affected roots computes absolute `rect`s (parent origin plus relative location, plus the root's anchor origin), rebuilds `hit_list` in paint order, and bumps `layout_revision` and `paint_revision`.

`LayoutStats { roots_laid_out, nodes_laid_out, measure_calls }` counts real (non-cached) work. It is what the invalidation tests assert, and the stats query exposes it to agents.

### Invalidation strategy

Each node's taffy cache is the layout dirty flag. **Invariant: if a node's cache is empty, so are all its ancestors'** (taffy relies on the same one). `invalidate_from(node)` clears `node`'s cache unconditionally, then walks up and stops at the first ancestor whose cache is already empty. New nodes are created with an empty cache, so adding a child calls `invalidate_from(parent)`, never `invalidate_from(child)` (the early-out would stop immediately and leave the parent stale). Paint has one tree-level flag, `paint_revision`.

| Edit | Layout | Paint |
|---|---|---|
| add, remove, reparent, `visible` change | `invalidate_from` the affected parent(s) | yes |
| `UiStyle` set, and the converted taffy style differs | `invalidate_from(node)` | yes |
| `UiStyle` set, only `background` or `pointer` differ | none | yes |
| `set_text`, label has `Auto` width or height | `invalidate_from(node)` | yes |
| `set_text`, label has fixed width and height | none | yes |
| `set_text` / `set_progress` / `set_enabled` with an equal value | none | none (no-op) |
| progress value, button enabled, image tint, hover, press, focus | none | yes |
| image changed, size `Auto` | `invalidate_from(node)` | yes |
| theme padding or font size changes | all caches | yes |
| UI-logical viewport size changes | `Fill` roots | yes |
| `ui_scale` changes with the UI-logical size unchanged | none | yes (snapping) |

Consequences for "heavy live numbers": a counter label with a fixed size updates at the cost of one string compare and a copy into its existing capacity, with no layout. A counter that must resize re-lays-out only its ancestor chain (siblings hit their caches). The draw list is rebuilt whenever `paint_revision` moved since the last build, as one linear walk; per-subtree draw caching is a later optimization, enabled by the same flags (open question 6). The cost this table does **not** remove is text shaping for a changed string, which belongs to the renderer (see Performance).

### Painting

`UiTree::build_draw_list(&mut self, measure: &mut dyn UiMeasure, out: &mut UiDrawList)` clears and refills `out` when `paint_revision` is newer than `out.revision`, and returns immediately otherwise. The measurer is used only to get the intrinsic size of `Contain` images (`image_size`); text needs none, because the renderer aligns inside the node rectangle. The walk is depth-first from each root in layer order, children in order, skipping invisible subtrees. Per widget:

- **Panel:** a quad for its background, if the resolved color is not fully transparent.
- **Button:** an optional focus-ring quad (the rectangle outset by 2 UI-logical px, theme `focus_ring`) when focused, then a quad colored by state with precedence disabled, pressed, hovered, normal. Children (the label) follow. Disabled buttons pass `text_disabled` to descendants' default label color.
- **Label:** one `Text` item as described under "Draw data".
- **Image:** one `Image` item. `Stretch` uses the full rectangle; `Contain` shrinks the rectangle to the image's aspect ratio inside the node, centered, using `measure.image_size`; an unknown size draws as `Stretch`.
- **ProgressBar:** a track quad, then a fill quad whose extent is `value` of the track along `direction`.

Rectangles convert to physical pixels as `round(logical * ui_scale)` per edge, so adjacent widgets stay edge-to-edge.

**Two planes per root.** Graphics and text are emitted as two planes: while walking a root, quads and images go to the list in tree order and text items go to a scratch list that is appended after the root's graphics. The reason is renderer batching: the render spec orders by `(layer, depth)` and needs one text batch for every run of text that sorts between sprite batches (capped at `MAX_TEXT_BATCHES = 64`, beyond which order is approximate). A screen with 200 buttons painted strictly in tree order alternates quad, text, quad, text and would need 200 text batches. Two planes need one text batch per root layer. **Authoring rule that follows:** inside one root, text always draws above every graphic of that root, so a panel that must cover another widget's text has to be its own root on a higher layer (modals, popups and tooltips are separate roots anyway). Roots that share a `layer` value share the planes (all their text over all their graphics), so overlapping roots need distinct layers.

### Render handoff (facade glue contract)

The UI does not depend on the renderer, and the renderer does not depend on the UI. The `exitium` facade owns a glue system in the render extraction schedule (after transform propagation, before the render frame; it is the facade item listed under "Implementation issues"). The seams are `UiMeasure` (in) and `UiDrawList` (out); this is what the glue must do, in terms of the render spec (#107):

1. **Measurer.** Implement `UiMeasure` over `Renderer::measure_text` (`TextStyle` built from the `TextMeasure` fields; font `Guid` mapped to the renderer's `FontId` by the font store; `None` is the engine's default font) and over the texture store for `image_size` (texture pixels, unscaled). The renderer's shaped-buffer cache is the memo; the glue adds none. Run `ui.layout(&mut measurer)`, then `ui.build_draw_list(&mut measurer, &mut list)`.
2. **Units.** The render space is `Space::Screen`: logical pixels, physical divided by `scale_factor`. The list is physical, so every `Quad`/`Image` rect and `Text` origin is **divided by `list.dpi_scale`**. Text is `TextStyle { size: run.size, line_height: run.line_height, max_width: Some(run.max_width), align, wrap }` with `TextDraw.scale = list.ui_scale / list.dpi_scale`, because the renderer lays out at the unscaled `size` and then applies `scale` (so glyphs end up `size * ui_scale` physical pixels tall, and the wrap width stays in UI-logical units). `TextAlign` maps Start, Center, End to Left, Center, Right; `wrap` maps to `Word` or `None`.
3. **Snapping survives the round trip.** The UI snaps to whole physical pixels; the glue divides by `dpi_scale` and the renderer multiplies it back, so an edge returns within one f32 ulp (at most 1e-3 px for coordinates up to 8,192 px). Fragment centers sit 0.5 px from any edge, so coverage is unchanged. A facade test asserts the round trip for several `dpi_scale` values.
4. **Order.** Render `layer = layer_base + 2 * item.layer` for graphics and `+ 1` for text (`layer_base` defaults to 10,000 and `mask` is configurable, both in the plain-data `UiRenderConfig` that this crate defines); `depth` is the item's index in the list; `space = Screen`; `clip = None`. UI sits above the game world and below editor overlays by choice of `layer_base`. A solid quad is a sprite with `texture: None`; an image is a sprite with the texture resolved from the `Guid` (an image that is not loaded yet draws nothing).
5. **What the renderer must guarantee** (a requirement on #107, open question 1): items of equal `layer` draw in `depth` order, and all text of one `layer` goes in one text batch when no sprite of that layer sorts between its items. Expected count: one text batch per root layer, so even 60 root layers stay under `MAX_TEXT_BATCHES = 64`.
6. **Skip work when clean.** If `list.revision` is unchanged since the last frame, the glue reuses what it built last frame instead of reconverting.

### Input

Stage 1 input is data in, events out. The tree never reads a window, so it needs no platform dependency and tests are table-driven.

```rust
pub struct UiInputFrame {
    pub pointer: Option<Vec2>,           // UI-logical px (window px / ui_scale); None: not over the window
    pub primary_down: bool,              // level, not edge; the tree derives press and release
    pub tab: bool, pub activate: bool,   // edge-triggered: Tab, Enter or Space went down this frame
    pub cancel: bool, pub shift: bool,   // cancel (Escape) is an edge; shift is a level that reverses tab
}
#[non_exhaustive] pub enum UiEvent { Click(UiNodeId), HoverEnter(UiNodeId), HoverExit(UiNodeId), FocusGained(UiNodeId), FocusLost(UiNodeId) }
pub struct UiCapture {
    pub pointer: bool,     // gameplay must not react to the mouse this frame
    pub keyboard: bool,    // the UI consumed a navigation key this frame: gameplay must ignore Tab, Enter, Space and Escape
    pub text_input: bool,  // a text-input widget has focus: gameplay must ignore every key. Always false in the MVP (no such widget)
}

impl UiTree {
    pub fn process_input(&mut self, frame: &UiInputFrame) -> UiCapture;
    pub fn events(&self) -> &[UiEvent];  // everything from the last process_input
}
```

`process_input` clears `events` first and returns the new capture, so every system that runs after it in the same frame reads the same slice (there is no drain). Rules, evaluated in this order each frame:

1. **Hit test.** Walk `hit_list` back to front (topmost first) and take the first entry whose absolute rectangle contains the pointer. The list holds only nodes whose resolved `PointerMode` is `Block`, plus buttons, so the cost scales with the number of interactive widgets, not nodes. The hit is the *topmost blocking node*, which may be a panel covering a button, in which case the button is not hovered. Invisible subtrees and a `None` pointer hit nothing.
2. **Hover.** The hovered node is the hit if it is an enabled button, else none. Changes emit `HoverExit` then `HoverEnter` and mark paint dirty.
3. **Press** (`primary_down` rising): the press origin is `Ui` if there was a hit, else `Game`. If the hit is an enabled button it becomes `pressed`. **Any press clears keyboard focus** (`FocusLost` if there was one): a mouse press never gives a button keyboard focus.
4. **Release** (`primary_down` falling): if the pressed button is still the hovered one, emit `Click(button)`. The pressed state always clears. Releasing elsewhere is not a click.
5. **Keyboard.** Focus is reached **only by `tab`**, which moves it to the next enabled, visible button in paint order (previous with `shift`), wrapping, and with no focus picks the first. `activate` with a focus emits `Click(focused)`. `cancel` with a focus clears it. Focus changes emit `FocusLost`/`FocusGained`.
6. **Capture.**
   - `pointer` follows the press origin. Origin `Ui`: true on the press frame, on every frame the button is held and **on the release frame**, so gameplay never sees the mouse-up of a UI press. Origin `Game`: false for the same span, even if the pointer drags over UI. No press in progress (including the frame after a release): true iff the pointer is over a hit-list node.
   - `keyboard` is true on a frame where the UI **consumed a key edge**: `tab` when at least one enabled button exists, or `activate`/`cancel` while a button had focus. It is **not** a level: a focused button does not capture WASD, and after a mouse click on a button (which leaves no focus) Space and Enter go to gameplay. A player who has pressed Tab and then wants to play presses Escape or clicks.
   - `text_input` is false until a text-input widget exists.
7. **Removal.** Removing a node clears hover, pressed and focus that name it, without events.

Disabled buttons are not hit-testable as hover targets, not focusable and ignore `activate`; they still block (a `Block` node under the pointer captures it).

**How UI consumes input before gameplay (Stage 2).** The plugin's `ui_input` system converts the platform input state to a `UiInputFrame`, calls `process_input` and stores the result in a `UiCapture` resource. Gameplay input code checks `UiCapture` before reacting: `pointer` for the mouse, `keyboard` for Tab, Enter, Space and Escape, `text_input` for everything (a one-line guard; whether the engine's input helpers apply it automatically is open question 4).

**Frame order and `FixedUpdate`.** The contract lists the schedules as Startup, FixedUpdate (60 Hz by default), Update and render extraction; the exact in-frame order is #106's. This spec assumes `FixedUpdate` runs before `Update` in a frame, and states the consequence precisely:

- Capture is computed once per frame. All `FixedUpdate` steps in that frame (zero or several) see the same value.
- **Requirement on #106:** `ui_input` runs after the platform input system and **before `FixedUpdate`** in the frame (a frame-start slot, or the first system of the first schedule that runs). Then press and release edges seen by gameplay in either schedule use this frame's capture.
- **If #106 offers no such slot** and `ui_input` can only run first in `Update`, a `FixedUpdate` system that handles a press edge sees the capture of the **previous** frame. Hover capture still covers a pointer that was already over UI, but a press on the same frame the pointer first moves onto UI is seen as a gameplay press once. That case is accepted for the MVP and tested, not hidden.

What the platform input resource looks like, and whether the UI should also clear consumed events from it, are open dependencies (open questions 3 and 4).

### Stable ids and undo

Commands must be undoable (rule 3), and a command recorded after another may refer to a node the earlier command created. So ids must survive undo and redo:

- `detach(node) -> DetachedSubtree` removes a subtree in **pre-order** and returns every node's full data, **its original id**, the pre-order slot list and its position in the parent. Each slot is freed (generation bumped, pushed on the free list) in that order, except that a slot whose generation was `u32::MAX` is retired instead of pushed.
- `restore(DetachedSubtree)` re-occupies the **same slots, with the same generations, in the reverse of the detach order**. Because the free list is LIFO, each slot is the list head when its turn comes, so each step is an O(1) pop, with no search and no doubly linked list. A step succeeds only if the slot is the head of the free list and its current generation is the recorded generation plus one; it then sets the generation back to the recorded one. Under the command bus's last-in, first-out undo this always holds: everything done after the detach has been undone first, which returns the free list to its state right after the detach. If any step fails (an out-of-order restore, or a slot that was **retired** at `u32::MAX`, which takes 2^32 - 1 reuses of one slot and is unreachable in practice) the whole restore is rolled back and returns `UiError::RestoreConflict`, and the command bus reports the undo as failed.
- `build(parent, &UiNodeDesc)` returns the new subtree's root id. **Its undo is `detach`, and the redo is `restore` of the `DetachedSubtree` that undo produced**, so a later command that names a node of the built subtree finds it at the same id after undo and redo. There is no "build with given ids" call.
- `reparent(node, new_parent, index)` returns the old `(parent, index)`; reparenting under one's own descendant is `UiError::Cycle`.

### Threading

`UiTree` is plain owned data (`Send + Sync`, no interior mutability) and lives in a resource. Every UI system takes the resource by `&mut` (or `&` for read-only), so the scheduler serializes them. Layout, input and painting are single-threaded, as the MVP executor is. Roots are independent layout subtrees with no shared cache, which allows a later parallel layout (splitting the arena by root) without a layout change; that is not designed here. The crate uses no threads, no clocks and no file I/O, so it builds and runs unchanged on wasm32.

### Stage 1 and Stage 2

- **Stage 1 (`exitium_core` and `taffy` only).** Everything above except the `plugin` module and the facade glue: arena, tree, styles, theme, desc, layout, painting, the render-facing mapping helpers (`UiRenderConfig` and the conversions the glue calls), input, plus unit tests and benches. Nothing in Stage 1 names the ECS, the app, the platform or the renderer. The coupling points are three plain-data seams: `UiMeasure` (in), `UiInputFrame` (in), `UiDrawList` (out).
- **Stage 2 (`plugin` module).** A `UiPlugin` that registers, using the app spec's plugin mechanism (#106; exact `App`/`Plugin` signatures are an open dependency):
  - **Resources:** `UiTree`, `UiViewport` (synced from the window), `UiCapture`, `UiDrawList`, `UiRenderConfig`.
  - **Systems into `Startup`:** insert the resources with defaults. Game code builds its UI in its own `Startup` systems from Rust (builder or `UiNodeDesc`).
  - **System into the input position:** `ui_input` (see "Frame order and `FixedUpdate`"). Game systems then read `UiTree::events()` and `UiCapture` and edit the tree.
  - **Layout and draw output are driven by the facade glue**, in render extraction, because they need the renderer's text measurer (see "Render handoff"). `UiPlugin { driver: UiDriver }` has two drivers: `External` (default; the facade runs `layout` and `build_draw_list`) and `Headless`, where the plugin adds a system at the end of `Update` that runs both with a fixed-advance measurer. `Headless` is for tests, tools and games with no renderer.
  - **Nothing into `FixedUpdate`.** Gameplay that runs at the fixed rate may call tree setters; the dirty flags make that cheap and the next layout picks it up.
  - **Commands and queries** registered with the command bus (see "Command and query surface") and `Reflect` derives on the data types.
- **Crate edges added in Stage 2** are listed under "Wiring". `exitium_ui` does **not** depend on `exitium_render`; the facade depends on both.

### Path to the deferred items

- **Data binding.** A `UiBinding { node, source, field }` record set stored in a resource, updated by a system that reads game state through ECS `Changed<T>` (the ECS spec guarantees editor and agent edits stamp the tick, so bindings see them) and calls the arena setters. The setters' equal-value no-op and the fixed-size no-layout rule are what make thousands of live counters cheap; they are built and benchmarked now for that reason. No tree changes are needed.
- **Markup.** `UiNodeDesc` is the in-memory form of any markup file. A file format (text, later binary) is a parser to and from `UiNodeDesc`; Stage 2 already serializes it through the reflection text format, which is the MVP's way to save and load a UI tree. Hot reload is `detach` plus `build` of the new desc under the same names.
- **`Num` display.** A `NumLabel` variant of `WidgetKind` holding a `Num` and a `NumFormat`, formatting into the label's text buffer. Until then, format with `NumFormat` and call `set_text`.
- **More widgets, scrolling, 9-slice, tweens.** New `WidgetKind` variants and `UiDrawItem` variants (both non-exhaustive), clip items in the draw list (the render spec's `clip` field is the target), and a per-node animated property layer that writes through the same setters.
- **Immediate-mode layer.** A second front end that builds a throwaway tree or draw list per frame through the same painter.

## MCP / editor surface

Agent parity (rule 1): every UI capability is a command or a query registered with the command bus, with a reflected payload, so the generic agent and editor tooling that #110 builds from the registry exposes it with no UI-specific tool or panel. **This spec files no issue in `exitium_mcp` or `exitium_editor`;** it lists what `exitium_ui` must expose. Names below are command and query names; #110 derives the tool names (suggested pattern `ui_<verb>`). Mutation goes through the bus (rule 3) and schemas come from the reflected data types (rule 2).

### Command and query surface

Commands are typed and serializable, each with an undo built from the arena operations above. Node ids travel as the text form `"{index}v{generation}"`, like `Entity`; a node may be addressed by its unique name instead.

| Command | Meaning | Undo |
|---|---|---|
| `UiBuildTree { parent: Option<Node>, tree: UiNodeDesc }` | a whole subtree or a new root in one step | `detach` (redo: `restore`) |
| `UiAddNode { parent, index, kind, name? }` | one node | `detach` |
| `UiRemoveNode { node }` | remove a subtree | `restore` the `DetachedSubtree` |
| `UiReparent { node, new_parent, index }` | move | reparent back |
| `UiSetField { node, path, value }` | any reflected field: style, widget data, root config, name | previous value via `clone_boxed` |
| `UiSetTheme { path, value }` | theme fields | previous value |
| `UiClick { node }` | debug action: emits `Click` as if pressed | not undoable (how the bus represents that is #110's, open question 7) |

`UiSetField` is one generic command instead of one per field, built on the reflection path machinery (`apply`, `reflect_path_mut`), so a new widget or style field is editable by agents and the inspector the day it is added. Structural edits stay explicit commands because they need ids and undo data.

Read-only queries: `UiGetTree { root?, depth? }` (the `UiNodeDesc` plus each node's id, computed rectangle and interaction flags), `UiGetNode`, `UiFind { name }`, `UiGetTheme`, `UiHitTest { x, y }` (the node under a UI-logical point), `UiGetStats` (`LayoutStats`, node count, revisions). Computed rectangles are as of the last layout pass: a response carries `layout_revision` and `stale: true` when edits since then have not been laid out, and a request may ask for `fresh` to run a pass first. Verifying how the UI looks is the screenshot capability owned by the render and editor specs, not this one.

### What the editor needs from the UI

Listed so #110 can build it generically; nothing here is a `exitium_ui` deliverable beyond the registrations above:

- the node tree for a hierarchy view (`UiGetTree`), with reparent by drag (`UiReparent`) and delete (`UiRemoveNode`);
- an inspector for the selected node generated from `UiNodeDesc` reflection (ranges, tooltips and `hidden` honored), writing through `UiSetField`, and a theme editor over `UiSetTheme`;
- a selection outline drawn from a node's `rect` (`UiGetNode`) by the editor overlay, and node picking from `UiHitTest`;
- a palette of the five widgets (the `WidgetKind` variants, discoverable through reflection);
- viewport mouse input translated to UI-logical pixels and fed to `process_input` as in a built game.

Whether the editor reads the `UiTree` directly (which would need an `exitium_editor -> exitium_ui` edge, already in the roadmap's expected wiring) or only through the query channel is #110's choice; this spec needs neither.

## Performance

Budgets are p50, single thread, on the min-spec reference runner (4 cores, 8 GB). Benches live in `exitium_ui/benches/` (criterion, per #18) and are held by the 5% regression gate once baselined; budgets may be re-baselined by the first bench PR with a note to the owner. The 10,000-node tree is 100 row containers of 99 fixed-size leaves each, the shape of a Glutton resource screen.

| Operation | Budget |
|---|---|
| Full cold layout, 10,000 nodes (flex, fixed-size leaves) | <= 6 ms |
| Relayout after one leaf resizes, 10,000 nodes | <= 100 us |
| Relayout with nothing dirty | <= 1 us |
| `set_text`, equal text (no-op) | <= 30 ns |
| `set_text` on a fixed-size label, warm buffer, no allocation | <= 100 ns |
| 1,000 fixed-size counter updates in one frame (setters only, no draw) | <= 100 us |
| Draw list rebuild, 10,000 nodes, warm buffers | <= 1 ms |
| Draw list rebuild, 500 nodes | <= 60 us |
| Hit test, 200 interactive entries | <= 2 us |
| `process_input`, 10,000-node tree, 200 interactive entries | <= 5 us |
| Hot-path allocations in a frame with no structural change and warm buffers | 0 |
| UI-side CPU per frame, 10,000 nodes, 1,000 counters changing (setters, layout, draw rebuild; **excludes text shaping**) | <= 2 ms |
| **Text shaping for changed labels** (renderer-owned, joint row with #107's `text/*` benchmarks): one changed short label (about 8 characters), shaped-cache miss | <= 20 us |
| **UI frame with shaping**: 100 labels change to new strings in one frame, 10,000 nodes (UI-side cost plus shaping and draw conversion) | <= 4 ms |
| Text batches per frame with 200 buttons in one root | 1 (one per root layer) |
| Memory per node (hot plus layout, excluding strings) | <= 1.25 KiB |
| `UiDrawItem` size | <= 112 B |

The "excludes text shaping" row is deliberate and honest: a counter that shows a new string every frame costs a shaped-buffer cache miss in the renderer's cache (keyed by text), which dwarfs the UI's own cost. 1,000 *distinct new strings* per frame would be about 20 ms at the ceiling above, so it is **not** within the frame budget, and the Phase 3 exit ("heavy live numbers at full frame rate") is covered by the shaping rows, not by the setter rows. The mitigations that keep real screens inside them: format to the displayed precision so a 1,000 Hz counter changes its text only when a visible digit changes (`set_text` with equal text is free), and the stats query reports `text_changes` per frame so a benchmark or an agent can see the rate. If measurement shows the shaping ceiling is not reachable for numerals, the options (a glyph-quad path for digits, a pre-shaped digit atlas) belong to #107 and are open question 2.

Evidence for the layout rows: a throwaway probe of taffy 0.15.1 (flexbox only, rounding off, native release build on a developer machine, not min-spec) measured about 4.4 ms for the cold 10,000-node pass and about 35 us for one dirty leaf, so the budgets keep a margin for min-spec and for the arena adapter's extra indirection. The first bench PR replaces these with real numbers.

Complexity: layout O(dirty ancestor chain + dirty subtrees); paint O(visible nodes) per rebuild and zero when clean; hit test O(interactive entries); `set_*` O(1) amortized.

Where the budgets are tight, the first answer is the data layout (hot/cold split, flat hit list, shared text buffer, two paint planes) and the second is a smaller rebuild (subtree draw caching), never a bypass of the command bus or the dirty flags.

## Test plan

- **Arena:** stale ids fail every lookup, LIFO reuse, generation retirement. `restore` succeeds under LIFO undo, including a multi-node subtree and a subtree whose slots were freed in several orders, returns `RestoreConflict` for an out-of-order restore and for a retired slot, and leaves the arena unchanged on failure. A seeded random sequence of add, remove, reparent, undo and redo checks parent/child consistency and id stability after every step.
- **Layout goldens:** known trees (row, column, gap, padding, grow and shrink, `Percent`, min/max, nested) compared to exact rectangles using a fixed-advance measurer. Anchoring and scaling: every anchor at three viewport sizes, a non-default pivot, `snap_integer`, and physical-pixel snapping of adjacent edges.
- **Invalidation:** each row of the invalidation table asserts `LayoutStats` (a fixed-size counter edit does zero layout; a resizing counter lays out only its ancestor chain; a clean pass does none; a `ui_scale` change with an unchanged UI-logical size lays out nothing). A test for the empty-cache invariant covers adding a child to a clean parent.
- **Painting:** golden draw lists for a panel with children, each button state and the focus ring, `Contain` images (known and unknown size), progress fill in both directions, layer order across roots, two planes per root (all text after all graphics), an invisible subtree, and `revision` stability when nothing changed.
- **Render mapping:** pure tests of the `UiRenderConfig` helpers: layer and depth from the list, the `dpi_scale` division and `ui_scale / dpi_scale` text scale, and the round trip of snapped edges for `dpi_scale` in {1, 1.25, 1.5, 2} (within 1e-3 px).
- **Input:** table-driven sequences of `UiInputFrame`s: press and release on a button (click), press then release elsewhere (no click), press on gameplay then drag over UI (not captured, including the release frame), press on UI then release (captured on the release frame, not on the next frame unless hovering), a transparent root passing clicks, a blocking panel over a button, disabled buttons, tab and shift-tab wrap, activate and cancel, `keyboard` capture only on consumed key edges, **click a button with the mouse and then press Space: no `Click` and `keyboard` is false**, and removing the hovered, pressed and focused node mid-sequence.
- **Description round trip:** `describe(build(desc)) == desc`, including names and roots; duplicate names are rejected atomically (no partial subtree).
- **Benchmarks** for every UI-side row of the performance table; a `size_of` test pins `UiDrawItem` and the per-node memory budget.
- **wasm:** `cargo build --workspace --target wasm32-unknown-unknown` covers the crate (it is not `NATIVE_ONLY`).
- **Stage 2:** headless app tests that insert the plugin (`Headless` driver), send input and assert events and `UiCapture` before a game system runs, including the `FixedUpdate` ordering case above; command tests for apply, undo and redo of every command; a reflection round trip of `UiNodeDesc` through the text format. **Facade (hotspot item):** the measurer and draw conversion against a headless renderer, and the 200-button text-batch count.

## Open questions

1. **Renderer guarantees (needs #107).** The glue relies on: equal-`layer` items draw in `depth` order; all text of a layer shares a text batch when no sprite of that layer sorts between; `TextStyle.max_width` with `wrap: None` is still used as the alignment width, and an aligned label hits the same shaped-buffer cache entry in measure and draw (the cache key includes `max_width`, and the measure offers the available width while the draw passes the final one, so a second shaping can occur on the first draw after a change). If #107 cannot guarantee these, the glue falls back to aligning with `anchor` and may merge the two planes, at the cost of more text batches.
2. **Shaping cost of changing numerals (needs #107).** The shaping rows in Performance are proposals for #107's `text/*` benchmarks. If numerals cannot meet them, a digit fast path is a render-side feature (not a UI one).
3. **Platform input shape.** The platform spec defines the per-frame pointer and key state the `ui_input` system converts into a `UiInputFrame`, including how wheel and touch arrive.
4. **Consumption.** Capture flags are the contract. Whether the shared input resource should also mask consumed buttons and keys (so gameplay needs no guard) is a platform or app decision.
5. **System ordering (needs #106).** The Stage 2 plugin needs a position for `ui_input` after platform input and before `FixedUpdate` (see "Frame order and `FixedUpdate`"), and a facade glue position in render extraction after transform propagation. If #106 offers neither, the plugin registers first in `Update` and the previous-frame capture case is accepted as written.
6. **Draw caching.** Is a full rebuild per paint change good enough at Glutton's heaviest screen? The first bench PR answers it; subtree caching keyed on per-node revisions is the next step if not.
7. **Command bus details (needs #110).** How non-undoable actions (`UiClick`) and bulk commands (`UiBuildTree`) are represented belongs to the command-bus spec; this spec only fixes what each command must carry. The `exitium_ui -> exitium_command` edge needed to register them is requested in "Wiring"; if #110 prefers registration from the facade, U13 moves to the facade chain.
8. **Persistence.** How a saved UI tree is stored as an asset and loaded by the scene or asset crate is not decided; the MVP minimum is the reflection text form of a `UiNodeDesc`.
9. **Clipping.** Needed once scroll views exist; requires clip items in the draw data and the render `clip` field (which exists).
10. **Image identity.** `Guid` is used because it is stable and already in core; the facade resolves it through the asset and texture stores and decides what an unloaded image draws (nothing, in this spec).

## Implementation issues

Filed by the architect after this spec merges, into the Jam MVP milestone with the `file-issue` skill. Every issue edits at most one crate directory, fits one PR (about 600 changed lines or fewer; each split below was made to stay under that), has acceptance criteria, `Blocked by` lines and the full labelled footprint. Labels: `area:ui` and `role:impl-ui`, except F1 (`area:ui`, role per the facade owner). Because all `exitium_ui` issues edit `crates/exitium_ui/` (one in-flight issue per crate), they are a chain; the order is also the Glutton UI critical path. `Cargo.toml`, `Cargo.lock` and the facade are hotspots owned by the intake PRs and the facade item.

**Stage 1** (no ECS). Each is `Blocked by` intake 1 (the Stage 1 dependency and crate-graph intake PR) and the issue before it. Every criterion list also includes `cargo xtask ci all` passing, the wasm build, and the benchmarks named for it.

- **U1 `feat(ui): node arena, tree structure and stable ids`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `NodeArena` with LIFO free list, retirement and `restore`; `UiTree` add, remove (`detach`), reparent with cycle check, names and `find`, roots ordered by layer; `restore` rules exactly as in "Stable ids and undo"; the arena fuzz and restore tests from the Test plan. Blocked by: intake 1.
- **U2 `feat(ui): widget data, UiStyle and UiTheme types`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `WidgetKind` and the widget data structs, `UiStyle`, `Dim`, `Edges`, the flex and pointer enums, `RootConfig` and `Anchor`, `UiTheme` with the defaults above, all `Clone + PartialEq + Debug + Default` where meaningful; `size_of` pins for `NodeHot`; no behavior beyond constructors and validation (progress clamp, layer range). Blocked by: intake 1, U1.
- **U3 `feat(ui): UiNodeDesc, builder and setters`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiNodeDesc` with `build` and `describe` (round-trip test, atomic duplicate-name rejection); the setters `set_text`, `set_progress`, `set_enabled`, `set_style`, `set_theme` with the equal-value no-op, no allocation on a warm buffer, and the dirty marking (a stub `invalidate_from` and `paint_revision` that U5 fills in); the setter benchmarks. Blocked by: intake 1, U2.
- **U4 `feat(ui): taffy style conversion and arena adapter`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiStyle` to `taffy::Style` conversion (theme padding applied per widget kind), the taffy tree traits implemented over the arena and `layout` array, `UiMeasure`, `TextMeasure` and a fixed-advance test measurer, dispatch for the five widgets, layout of a single root; flex golden tests (row, column, gap, padding, grow and shrink, percent, min and max, nested, leaves measured). Blocked by: intake 1, U3.
- **U5 `feat(ui): layout pass, absolute rectangles and invalidation`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiTree::layout` over several roots in layer order, absolute `rect`s, `LayoutStats`, `invalidate_from` and the cache-empty invariant wired into every row of the invalidation table; invalidation tests asserting `LayoutStats`; the 10,000-node cold, one-leaf and clean layout benchmarks. Blocked by: intake 1, U4.
- **U6 `feat(ui): viewport, scaling and anchoring`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiViewport`, `ScaleMode`, `ui_scale` and UI-logical viewport math, root sizes (`Fill`, `Fixed`, `Fit`), anchor and pivot origin, the viewport-change invalidation rows; anchoring and scaling golden tests; the tests that a `ui_scale`-only change lays out nothing. Blocked by: intake 1, U5.
- **U7 `feat(ui): draw list and theme resolution`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiDrawList`, `UiDrawItem`, `TextRun`, `build_draw_list(measure, out)` for the five widgets and button states, theme and override resolution, physical-pixel snapping, the two planes per root, `revision`; the painting golden tests; the draw benchmarks and the `UiDrawItem` size pin. Blocked by: intake 1, U6.
- **U8 `feat(ui): render-facing mapping helpers`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiRenderConfig` (`layer_base`, `mask`), pure helpers for render layer and depth from an item, the physical-to-Screen conversion and the text scale (`ui_scale / dpi_scale`), written against #107's `Space::Screen`; the render-mapping tests including the snapped round trip. Blocked by: intake 1, U7, #107 merged.
- **U9 `feat(ui): hit test, hover, press and click`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: the hit list built in the layout post-pass, `UiInputFrame`, `UiEvent`, `UiCapture`, rules 1 to 4, pointer capture as in rule 6 (all three origin cases, including the release frame), removal clearing (rule 7) for hover and pressed; the matching input tests and the hit-test benchmark. Blocked by: intake 1, U8.
- **U10 `feat(ui): keyboard focus and keyboard capture`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: rule 5 and the `keyboard` and `text_input` capture rules; a mouse press clears focus and gives none; the "click then Space" test and the Tab, shift-Tab, activate, cancel and removal tests; the `process_input` benchmark. Blocked by: intake 1, U9.

**Stage 2** (ECS, app, reflect, command bus). Each is `Blocked by` intake 2, which adds every edge below, so none of them touches `Cargo.toml` or `Cargo.lock`.

- **U11 `feat(ui): UiPlugin, resources and ui_input`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: the `plugin` module with the resources, the `ui_input` system in the position #106 provides, the `Headless` driver, and headless app tests (events and capture before a game system, the `FixedUpdate` ordering case). Blocked by: intake 2, U10, the app and plugin issues from #106.
- **U12 `feat(ui): reflection derives and UiNodeDesc text round trip`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `Reflect` on every data type, hand-written `UiNodeId` reflection with the `"{index}v{generation}"` text form, a `UiNodeDesc` round trip through the text format in a test (`exitium_serial` as a dev-dependency). Blocked by: intake 2, U11, the reflect issues and the text-serialization issues (#23).
- **U13 `feat(ui): UI commands and queries with undo`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: the commands and queries in "Command and query surface" registered against the command-bus API (#110), apply, undo and redo tests for each command. Blocked by: intake 2, U12, the `exitium_command` issues from #110.
- **F1 `feat(exitium): UI to renderer glue`.** Footprint: Edits: none; Governs: none; Hotspots: `crates/exitium/`; New crate edges: none (the facade's edges to `exitium_ui` and `exitium_render` are in intake 2). Acceptance: the glue system in render extraction exactly as in "Render handoff" (the `UiMeasure` over `Renderer::measure_text` and the texture store, the unit and scale conversions, layer and depth mapping, skip when `revision` is unchanged), the facade tests for the round trip and the 200-button text-batch count, and a UI example in `crates/exitium/examples/`. High-risk review: facade API. Blocked by: intake 2, U8, U11, the render plugin issues from #107, and the facade chain (the facade runs alone).

The real `Blocked by #N` numbers for the intake and for the other specs' implementation issues are filled in when the architect files these, because those specs and issues are written in parallel with this one. U1 to U7, U9 and U10 need no other spec and can start the moment intake 1 merges, which is why the Glutton UI path does not wait on the app, render or command-bus specs.

### Wiring: dependencies and crate edges

This is the complete list `exitium_ui` and its issues need. The intake PRs add the union of the merged specs' lists, so no implementation issue edits `Cargo.toml` or `Cargo.lock`.

**Intake 1 (Stage 1):**

- Crate edge: `exitium_ui -> exitium_core`.
- crates.io dependency (below): `taffy`.
- Dev-dependency: `criterion` (already in `[workspace.dependencies]`, used by `exitium_core`'s benches), with a `[[bench]] name = "ui", harness = false` entry in `exitium_ui/Cargo.toml`.
- No other dependency, and no edge to `exitium_render`, `exitium_platform`, `exitium_assets`, `exitium_physics`, `exitium_audio` or `exitium_scene`.

**Intake 2 (Stage 2):**

- `exitium_ui -> exitium_ecs`, `exitium_ui -> exitium_app`: resources, systems, schedules (the roadmap's expected wiring).
- `exitium_ui -> exitium_reflect`: `#[derive(Reflect)]` (the derive is re-exported by `exitium_reflect`, so `exitium_reflect_derive` is not a separate edge).
- `exitium_ui -> exitium_command`: registering commands and queries (not in the roadmap's expected wiring; requested here, see open question 7). `exitium_command` depends on core, ecs and reflect, so there is no cycle.
- `exitium_ui -> exitium_serial` as a **dev-dependency only**: the `UiNodeDesc` text-format round trip test. It is not in the runtime graph. Per the roadmap's rule R3, dev-dependencies count for the conflict check, so U12 runs after the serial lane has merged (it has, by Stage 2).
- Facade: `exitium -> exitium_ui` (already implied by "facade depends on every runtime crate"). F1 uses `exitium -> exitium_render` as well; it needs no edge between `exitium_ui` and `exitium_render`.
- Not required by this spec: `exitium_editor -> exitium_ui` (see "What the editor needs from the UI"; it is in the roadmap's expected wiring if #110 wants it).
- `exitium_ui` stays out of `NATIVE_ONLY`.

**Crates.io dependencies** (high-risk area: a new dependency; the reviewer checks this table):

| Crate | Version | Features | License | wasm | Why not std |
|---|---|---|---|---|---|
| `taffy` | `0.15.1` | `default-features = false`, `["std", "flexbox"]` | MIT (already allowed in `deny.toml`) | yes: `cargo check --target wasm32-unknown-unknown` passes; no platform calls | A CSS-conformant flexbox solver with caching is thousands of lines of subtle algorithm; std has no layout. 0.15.1 was published 2026-10-25, MSRV 1.71 (our toolchain is newer), `#![deny(unsafe_code)]`. |

- With these features taffy has **no transitive dependencies** (`grid` would pull `smallvec`, `taffy_tree` would pull `slotmap`; neither is enabled). `serde`, `parse`, `calc`, `block_layout`, `float_layout`, `content_size` and `detailed_layout_info` are also off.
- Taffy is `0.x`: a minor-version bump may break the low-level traits. The caret requirement `0.15.1` allows only patch updates; upgrading is a deliberate PR confined to `layout/`.
- Nothing else is needed: colors, rectangles and vectors come from `exitium_core`, ids from `Guid`, and text shaping and font data stay in the renderer behind `UiMeasure`, supplied by the facade. Stage 2 adds only workspace crates, no further crates.io dependency.
