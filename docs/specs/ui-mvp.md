# Spec: Game UI MVP (retained tree on taffy)

- Issue: #109
- Crates: exitium_ui (Stage 1 depends on exitium_core and taffy only; the Stage 2 `plugin` module adds exitium_ecs, exitium_app, exitium_reflect, exitium_command and exitium_render)
- Status: draft

## Goal

Specify `exitium_ui` for the Jam MVP: a retained widget tree with flex layout on [taffy](https://crates.io/crates/taffy), five widgets (panel, label, image, button, progress bar), screen-space anchoring and scaling, pointer and keyboard-focus input that the UI consumes before gameplay, and basic theming. An agent builds and edits the tree through commands (MCP), a human refines it in the editor, and the same tree runs on Windows and in the browser (wasm32).

This spec also settles PLAN Phase 3 item 1, "ECS entities or a dedicated arena": **a dedicated arena** (see "Node storage decision"). PLAN says "decided by ADR"; the owner-approved issue records it here, with alternatives, to keep `docs/decisions/` (a hotspot) out of this PR.

Design in one paragraph: `UiTree` owns a generational arena of nodes. Each node has a `WidgetKind`, an authored `UiStyle` and a parent and children list. Layout data (the taffy style, taffy's cache and the computed rectangle) sits in a parallel array so paint and hit-test walks never load it. Layout runs through taffy's low-level traits directly on the arena, one independent layout per root, with anchoring and scaling applied after layout. Edits clear exactly the caches they invalidate, and edits that cannot change geometry (counters with a fixed size, progress values, hover state) skip layout entirely. Painting turns the laid-out tree into a flat `UiDrawList` of quads, images and text runs in physical pixels. Input is a small plain-data frame in, `UiEvent`s and a `UiCapture` flag pair out. **Stage 1 is all of this with no ECS, no render and no platform dependency**; **Stage 2** wraps it in a plugin that registers resources and systems into the app schedules.

## Non-goals

Deferred from the MVP, each addable later without breaking the layout below (design rule 5):

- Widgets beyond the five: rich text, toggle, slider, scroll view, list and virtualized list, grid, tooltip, modal, tabs, text input. `WidgetKind` is `#[non_exhaustive]`.
- Data binding, UI markup files with hot reload, and the `Num` display widget (the path is under "Path to the deferred items"). A game updates a label from Rust with `set_text` and formats numbers with `exitium_core::NumFormat`.
- 9-slice images, rounded corners, borders, transitions and tweens, clipping and scrolling (no scissor items in the draw data yet).
- The immediate-mode layer for debug overlays.
- Taffy's grid and block layout. Only flexbox is compiled in.
- World-space UI (health bars over entities). Those are sprites and text drawn by the renderer, not a retained tree.
- Gamepad and spatial navigation, touch gestures, IME and text editing, right-to-left text, localization, accessibility.
- Luau bindings (cut from the MVP) and networking.
- Parallel layout. The MVP executor is single-threaded and so is the layout pass; independent roots leave room for it (see Threading).

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
  draw.rs         UiDrawList, UiDrawItem, paint walk                         safe
  input.rs        UiInputFrame, hit list, hover/press/click/focus, UiCapture safe
  plugin/         Stage 2: mod.rs, systems.rs, measure.rs, commands.rs       safe
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
| Inspector, schema, serialization, MCP | free from `Reflect` + `ReflectComponent` | free from `Reflect` on the data types plus `ui_*` tools | free |
| Visible to ECS queries, scenes, `ecs_*` tools | yes | no (the cost) | yes, duplicated |
| Duplicated code | none | parent/child maintenance, about 150 lines | most |
| Fit to scale | built for 1M rows | UI trees are hundreds to tens of thousands of nodes | as B |

Reasons for B:

1. **The crate graph decides Stage 1.** The contract has `exitium_ui` build in parallel with the ECS and depend on `exitium_core` only. Layout, input and draw output can then be written and tested headlessly before the ECS exists.
2. **UI is a tree walked as a tree.** Taffy asks for children, style and cache of one node many times per pass. An arena answers with an index; ECS storage answers with several lookups and keeps nodes of different widget kinds in different archetypes. The ECS advantage, bulk iteration over millions of homogeneous rows, does not apply to a few thousand heterogeneous nodes.
3. **Hot and cold data split cleanly** in an arena (see data layout), which keeps the paint walk fast. Per-component storage would split it by component instead of by access pattern.
4. **Nothing in the design rules is lost.** Reflection still derives the schema (Stage 2 puts `#[derive(Reflect)]` on the data types), all mutation still goes through typed commands with undo, and every capability has an MCP tool. The `ui_*` tools replace `ecs_*` for UI nodes.
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
    rect: Rect,                     // absolute, logical px, set by the post-layout pass
    flags: NodeFlags,               // hovered, pressed, focused
}

struct LayoutData {                 // about 720 B per node with taffy's flexbox-only build
    style: taffy::Style,            // derived from UiStyle + theme + widget kind (248 B)
    cache: taffy::Cache,            // 392 B
    unrounded: taffy::Layout,       // parent-relative, 76 B
}
```

- **Arena.** `NodeArena` is a `Vec` of slots with a `NonZeroU32` generation and a LIFO free list, the same rules as `exitium_core::Slab` (stale ids fail every lookup in O(1), a slot whose generation would overflow is retired). It is a separate small type, not `Slab`, because it adds `restore` (below). If another crate needs `restore`, fold it into `Slab` later; that is not a UI decision.
- **Why parallel arrays.** Paint and hit-test read `kind`, `style`, `rect` and `flags`. Layout reads `LayoutData`. Splitting them keeps the paint walk's working set about 1 MB per 5,000 nodes instead of 5 MB. `layout[i]` is valid for every live slot `i` and is resized with the arena.
- **Memory budget:** at most 1.25 KiB per node including `LayoutData` and excluding strings; 10,000 nodes fit in 12.5 MB. Measured sizes on a 64-bit build with taffy's `std` + `flexbox` features: `Style` 248 B, `Cache` 392 B, `Layout` 76 B.
- **Names.** A node may have a unique name (`"score_label"`). `find(name)` is how Rust game code and agents address nodes without holding ids. A duplicate name is `UiError::DuplicateName`.
- **Roots.** A root is a node with `parent == None` and `root: Some(RootConfig)`; `roots` is the draw order (lower `layer` first, so higher layers draw on top and are hit first).

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
pub struct LabelData    { pub text: String, pub font_size: Option<f32>, pub color: Option<Color>, pub align: TextAlign, pub wrap: bool }
pub struct ImageData    { pub image: Guid, pub tint: Color, pub fit: ImageFit /* Stretch | Contain */, pub filter: ImageFilter /* Linear | Nearest */ }
pub struct ButtonData   { pub enabled: bool }
pub struct ProgressData { pub value: f32 /* clamped 0..=1 on set */, pub direction: FillDirection /* LeftToRight | BottomToTop */, pub fill: Option<Color>, pub track: Option<Color> }
```

`None` colors and sizes mean "take it from the theme". Images name an asset by `Guid` (stable, serializable, already in core); the renderer or asset layer resolves it. `ImageFilter::Nearest` is what pixel-art games set. A button is a flex container: its text is an ordinary child label, so a button can hold an icon and a label.

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
    pub offset: Vec2,                    // logical px, +x right, +y down
    pub size: RootSize,                  // Fill (viewport) | Fixed(Vec2) | Fit (content)
    pub layer: i16,
}
pub struct UiViewport {
    pub physical_size: UVec2,            // render target size in pixels
    pub dpi_scale: f32,                  // window scale factor
    pub scale_mode: ScaleMode,           // Constant { multiplier } | ReferenceHeight { height, snap_integer }
}
```

- `ui_scale`: `Constant` is `dpi_scale * multiplier`. `ReferenceHeight` is `physical_size.y / height`, rounded to the nearest integer (at least 1) when `snap_integer` is set, which is what keeps pixel art crisp. The logical viewport is `physical_size / ui_scale`. All layout runs in logical pixels, so a 1080p design works unchanged at any resolution.
- Origin of a root: `anchor_fraction * logical_viewport - pivot * root_size + offset`. A `TopRight` root with offset `(-16, 16)` sits 16 px in from the top-right corner at every size.
- Taffy's own pixel rounding is **off**. Rectangles are snapped to whole **physical** pixels when the draw list is built, so edges and text origins are crisp at fractional scales.

Draw data (the only thing the renderer sees; plain data, no ECS and no GPU types):

```rust
pub struct UiDrawList { pub revision: u64, pub items: Vec<UiDrawItem>, pub text: String }

#[non_exhaustive]
pub enum UiDrawItem {                    // back-to-front; later items cover earlier ones; no z field
    Quad  { rect: Rect, color: Color },
    Image { rect: Rect, uv: Rect, image: Guid, tint: Color, filter: ImageFilter },
    Text  { origin: Vec2, run: TextRun }, // run.text is a byte range into UiDrawList::text
}
pub struct TextRun { pub text: Range<u32>, pub font: Option<Guid>, pub size: f32, pub color: Color, pub max_width: Option<f32>, pub align: TextAlign }
```

All coordinates are physical pixels, origin top-left, y down. `size` is physical px. One shared `text` buffer means a rebuild allocates nothing once the buffers are warm. `revision` increases only when the content changes, so a renderer can keep its uploaded batch when it is unchanged. The enum is `#[non_exhaustive]` so clip push/pop can be added when scrolling lands.

Theme:

```rust
pub struct UiTheme {
    pub font: Option<Guid>, pub font_size: f32,                    // 16.0 logical px
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

**Dependency:** `taffy 0.15.1` (see "Dependencies"). It is a pure layout library: `#![deny(unsafe_code)]`, no platform calls, builds for `wasm32-unknown-unknown`, and with `default-features = false, features = ["std", "flexbox"]` it has **no transitive dependencies** (checked with `cargo tree` and `cargo check --target wasm32-unknown-unknown`).

**Integration choice: taffy's low-level traits over our arena, not `TaffyTree`.** `TaffyTree` owns a second copy of the tree (its own storage, ids and children), which we would have to keep in sync with the arena on every add, remove and reparent. The low-level API (`TraversePartialTree`, `LayoutPartialTree`, `LayoutFlexboxContainer`, `CacheTree`, `compute_root_layout`, `compute_flexbox_layout`, `compute_leaf_layout`) is the documented route for embedding taffy in an existing widget tree, and taffy's `custom_tree_vec` example is the template. The implementer verifies the exact trait set against 0.15.1; rounding (`RoundTree`) is not used. `layout/adapter.rs` is a view struct that borrows the arena, `layout` array and a `&mut dyn UiMeasure` for the duration of one pass. Taffy `NodeId`s are the slot indices, which is safe because no structural change can happen during a pass.

Dispatch in `compute_child_layout`: `Panel` and `Button` go to `compute_flexbox_layout`; `Label`, `Image` and `ProgressBar` are leaves and go to `compute_leaf_layout` with a measure closure. A node with `visible == false` goes to `compute_hidden_layout`. Every call goes through taffy's cache first (`compute_cached_layout`).

Measuring leaves, with no render dependency in Stage 1:

```rust
pub trait UiMeasure {
    /// Logical size of the text at `scale`; `max_width` is the available width when it wraps.
    fn measure_text(&mut self, req: &TextMeasure<'_>) -> Vec2;
    /// Intrinsic logical size of an image, or None when unknown (then Auto size is 0).
    fn image_size(&mut self, image: Guid) -> Option<Vec2>;
}
pub struct TextMeasure<'a> { pub text: &'a str, pub font: Option<Guid>, pub size: f32, pub max_width: Option<f32>, pub scale: f32 }
```

`scale` is the current `ui_scale`: glyph advances depend on the physical size, so text is measured at `size * scale` and the result is divided back to logical px. Stage 1 tests use a fixed-advance monospace measurer. The Stage 2 implementation wraps the render crate's text shaping and **must memoize** by `(text, font, size, max_width, scale)`; layout calls it at most twice per dirty label per pass, and zero times for a clean one.

Progress bars measure as their style size (default `Px(100) x Px(8)` when both are `Auto`).

**The per-frame pass** (`UiTree::layout(&mut self, measure: &mut dyn UiMeasure) -> LayoutStats`):

1. If the viewport's `ui_scale`, the logical size or the theme changed since the last pass, clear every cache (a size-only change clears only roots with `RootSize::Fill` or percentage sizes).
2. For each visible root in layer order whose cache is empty: compute its available space (`Fill`: the logical viewport, definite; `Fixed`: its size; `Fit`: max-content) and call `compute_root_layout`. Roots with a populated cache are skipped entirely.
3. If any root was laid out: one depth-first walk of those roots computes absolute `rect`s (parent origin plus relative location, plus the root's anchor origin), appends interactive nodes to a fresh `hit_list` in paint order, and bumps `layout_revision` and `paint_revision`.

`LayoutStats { roots_laid_out, nodes_laid_out, measure_calls }` counts real (non-cached) work. It is what the invalidation tests assert, and `ui_get_stats` exposes it to agents.

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
| `ui_scale` changes (window resize in `ReferenceHeight` mode) | all caches | yes |
| viewport size changes only | `Fill` roots | yes |

Consequences for "heavy live numbers": a counter label with a fixed size updates at the cost of one string compare and a copy into its existing capacity, with no layout. A counter that must resize re-lays-out only its ancestor chain (siblings hit their caches). The draw list is rebuilt whenever `paint_revision` moved since the last build, as one linear walk; per-subtree draw caching is a later optimization, enabled by the same flags (open question 6).

### Painting

`UiTree::build_draw_list(&mut self, out: &mut UiDrawList)` clears and refills `out` when `paint_revision` is newer than `out.revision`, and returns immediately otherwise. The walk is depth-first from each root in layer order, children in order, skipping invisible subtrees. Per widget:

- **Panel:** a quad for its background, if the resolved color is not fully transparent.
- **Button:** an optional focus-ring quad (the rectangle outset by 2 logical px, theme `focus_ring`) when focused, then a quad colored by state with precedence disabled, pressed, hovered, normal. Children (the label) follow. Disabled buttons pass `text_disabled` to descendants' default label color.
- **Label:** one `Text` item. The origin is inside the node rectangle according to `align`; `wrap` sets `max_width` to the rectangle's width.
- **Image:** one `Image` item. `Stretch` uses the full rectangle and `uv = (0,0)-(1,1)`; `Contain` shrinks the rectangle to the image's aspect ratio inside the node, centered.
- **ProgressBar:** a track quad, then a fill quad whose extent is `value` of the track along `direction`.

Rectangles convert to physical pixels as `round(logical * ui_scale)` per edge, so adjacent widgets stay edge-to-edge.

### Input

Stage 1 input is data in, events out. The tree never reads a window, so it needs no platform dependency and tests are table-driven.

```rust
pub struct UiInputFrame {
    pub pointer: Option<Vec2>,           // logical UI px (window px / ui_scale); None: not over the window
    pub primary_down: bool,              // level, not edge; the tree derives press and release
    pub tab: bool, pub activate: bool,   // edge-triggered: Tab, Enter or Space went down this frame
    pub cancel: bool, pub shift: bool,   // cancel (Escape) is an edge; shift is a level that reverses tab
}
#[non_exhaustive] pub enum UiEvent { Click(UiNodeId), HoverEnter(UiNodeId), HoverExit(UiNodeId), FocusGained(UiNodeId), FocusLost(UiNodeId) }
pub struct UiCapture { pub pointer: bool, pub keyboard: bool }   // true: gameplay must not react

impl UiTree {
    pub fn process_input(&mut self, frame: &UiInputFrame) -> UiCapture;
    pub fn events(&self) -> &[UiEvent];  // everything from the last process_input
}
```

`process_input` clears `events` first and returns the new capture, so every system that runs after it in the same frame reads the same slice (there is no drain). Rules, evaluated in this order each frame:

1. **Hit test.** Walk `hit_list` back to front (topmost first) and take the first entry whose absolute rectangle contains the pointer. The list holds only nodes whose resolved `PointerMode` is `Block`, plus buttons, so the cost scales with the number of interactive widgets, not nodes. The hit is the *topmost blocking node*, which may be a panel covering a button, in which case the button is not hovered. Invisible subtrees and a `None` pointer hit nothing.
2. **Hover.** The hovered node is the hit if it is an enabled button, else none. Changes emit `HoverExit` then `HoverEnter` and mark paint dirty.
3. **Press** (`primary_down` rising): origin is `Ui` if there was a hit, else `Game`. If the hit is an enabled button it becomes `pressed` and **focused**; any other press clears focus.
4. **Release** (`primary_down` falling): if the pressed button is still the hovered one, emit `Click(button)`. The pressed state always clears. Releasing elsewhere is not a click.
5. **Keyboard.** `tab` moves focus to the next enabled, visible button in paint order (previous with `shift`), wrapping, and with no focus picks the first. `activate` with a focus emits `Click(focused)`. `cancel` with a focus clears it. Focus changes emit `FocusLost`/`FocusGained`.
6. **Capture.** `pointer` is true when the press origin is `Ui` (from the press until the frame after release ends), or when no button is down and the pointer is over any hit-list node. A press that **started on gameplay** keeps `pointer` false until release even if it drags over UI. `keyboard` is true when a node had or has focus this frame (so a cancel that clears focus is consumed).
7. **Removal.** Removing a node clears hover, pressed and focus that name it, without events.

Disabled buttons are not hit-testable as hover targets, not focusable and ignore `activate`; they still block (a `Block` node under the pointer captures it).

**How UI consumes input before gameplay (Stage 2):** the plugin's input system is the first system in `Update`. It converts the platform input state to a `UiInputFrame`, calls `process_input`, and stores the result in a `UiCapture` resource. Gameplay input code checks `UiCapture` before reacting (a one-line guard; whether the engine's input helpers apply it automatically is open question 4). `FixedUpdate` systems read the capture value from the latest `Update`, which is exact for level-based decisions and at most one frame stale for edges. What the platform input resource looks like, and whether the UI should also clear consumed events from it, are open dependencies (open questions 3 and 4).

### Stable ids and undo

Commands must be undoable (rule 3), and a command recorded after another may refer to a node the earlier command created. So ids must survive undo and redo:

- `detach(node) -> DetachedSubtree` removes a subtree and returns every node's full data, **its original id** and its position in the parent. Slots are freed and their generations bumped as usual.
- `restore(DetachedSubtree)` re-occupies exactly those slots with exactly those generations. It succeeds only if each slot is still free and at the generation immediately after the recorded one. Under the command bus's last-in, first-out undo that always holds (anything created later has been undone first). It otherwise returns `UiError::RestoreConflict`.
- `build(parent, &UiNodeDesc)` returns the new root id; its undo is `detach`. Redo of a build re-runs `build` with the ids recorded by the first run via `build_with_ids`, so later redo steps still find their nodes.
- `reparent(node, new_parent, index)` returns the old `(parent, index)`; reparenting under one's own descendant is `UiError::Cycle`.

### Threading

`UiTree` is plain owned data (`Send + Sync`, no interior mutability) and lives in a resource. Every UI system takes the resource by `&mut` (or `&` for read-only), so the scheduler serializes them. Layout, input and painting are single-threaded, as the MVP executor is. Roots are independent layout subtrees with no shared cache, which allows a later parallel layout (splitting the arena by root) without a layout change; that is not designed here. The crate uses no threads, no clocks and no file I/O, so it builds and runs unchanged on wasm32.

### Stage 1 and Stage 2

- **Stage 1 (`exitium_core` and `taffy` only).** Everything above except the `plugin` module: arena, tree, styles, theme, desc, layout, painting, input, plus unit tests and benches. Nothing in Stage 1 names the ECS, the app, the platform or the renderer. The coupling points are three plain-data seams: `UiMeasure` (in), `UiInputFrame` (in), `UiDrawList` (out).
- **Stage 2 (`plugin` module).** A `UiPlugin` that registers, using the app spec's plugin mechanism (#106; exact `App`/`Plugin` signatures are an open dependency):
  - **Resources:** `UiTree`, `UiViewport` (synced from the window), `UiCapture`, `UiDrawList`.
  - **Systems into `Startup`:** insert the resources with defaults. Game code builds its UI in its own `Startup` systems from Rust (builder or `UiNodeDesc`).
  - **Systems into `Update`:** `ui_input` first (before any game system); game systems then read `UiTree::events()` and `UiCapture` and edit the tree; `ui_layout` last (sync viewport, run the pass). The ordering mechanism (labels, sets, before/after) comes from #106 and is an open dependency.
  - **System into render extraction:** `ui_extract` builds the draw list when dirty and hands it to the renderer as the plain draw data above. The render spec (#107) decides how the renderer takes that data; this spec requires the quads, images and text runs to arrive in list order, in physical pixels, drawn after the game world and before any editor overlay.
  - **Nothing into `FixedUpdate`.** Gameplay that runs at the fixed rate may call tree setters; the dirty flags make that cheap and the next `ui_layout` picks it up.
  - **Commands** for the command bus (`exitium_command`), `Reflect` derives on the data types, and the measure adapter over the render crate's text shaping.
- **Crate edges added in Stage 2:** `exitium_ui -> exitium_ecs, exitium_app, exitium_reflect, exitium_command`, and **proposed** `exitium_ui -> exitium_render` for the text measure and draw submission. The render-side direction is #107's to confirm (open question 1). There must be exactly one edge between the two crates and no cycle; UI to render is preferred because the render crate then stays UI-agnostic and the UI crate already owns the bridge to the ECS.

### Path to the deferred items

- **Data binding.** A `UiBinding { node, source, field }` record set stored in a resource, updated by a system that reads game state through ECS `Changed<T>` (the ECS spec guarantees editor and agent edits stamp the tick, so bindings see them) and calls the arena setters. The setters' equal-value no-op and the fixed-size no-layout rule are what make thousands of live counters cheap; they are built and benchmarked now for that reason. No tree changes are needed.
- **Markup.** `UiNodeDesc` is the in-memory form of any markup file. A file format (text, later binary) is a parser to and from `UiNodeDesc`; Stage 2 already serializes it through the reflection text format, which is the MVP's way to save and load a UI tree. Hot reload is `detach` plus `build` of the new desc under the same names.
- **`Num` display.** A `NumLabel` variant of `WidgetKind` holding a `Num` and a `NumFormat`, formatting into the label's text buffer. Until then, format with `NumFormat` and call `set_text`.
- **More widgets, scrolling, 9-slice, tweens.** New `WidgetKind` variants and `UiDrawItem` variants (both non-exhaustive), clip items in the draw list, and a per-node animated property layer that writes through the same setters.
- **Immediate-mode layer.** A second front end that builds a throwaway tree or draw list per frame through the same painter.

## MCP / editor surface

Agent parity (rule 1): every UI capability below is an MCP tool and an editor action. Mutation goes through the command bus (rule 3); the schemas are generated from the reflected data types (rule 2). Tool names are `ui_<verb>`. The tools are defined in `exitium_mcp`, which is a separate stdio process: each one forwards a request to the running game or editor over localhost, and the game applies the command through the bus. `exitium_mcp` must not depend on `exitium_editor`; tool schemas come from the reflect schema dump (the MCP spec's mechanism), so it needs no link-time dependency on `exitium_ui` either.

Commands (typed, serializable, each with an undo derived from the arena operations above). Node ids travel as the text form `"{index}v{generation}"`, like `Entity`; a node may be addressed by its unique name instead.

| Command | MCP tool | Editor action | Undo |
|---|---|---|---|
| `UiBuildTree { parent: Option<Node>, tree: UiNodeDesc }` (a whole subtree or a new root in one step) | `ui_build_tree` | add from palette, paste | `detach` of the built subtree |
| `UiAddNode { parent, index, kind, name? }` | `ui_add_node` | add child | `detach` |
| `UiRemoveNode { node }` | `ui_remove_node` | delete | `restore` the `DetachedSubtree` |
| `UiReparent { node, new_parent, index }` | `ui_reparent` | drag in the tree view | reparent back |
| `UiSetField { node, path, value }` (any reflected field: style, widget data, root config, name) | `ui_set_field` | inspector edit | previous value via `clone_boxed` |
| `UiSetTheme { path, value }` | `ui_set_theme` | theme panel | previous value |
| `UiClick { node }` (debug action: emits `Click` as if pressed; not undoable) | `ui_click` | "test click" button | none |

Read-only tools: `ui_get_tree { root?, depth? }` (the `UiNodeDesc` JSON plus each node's id, computed rectangle and interaction flags), `ui_get_node`, `ui_find { name }`, `ui_get_theme`, `ui_hit_test { x, y }` (the node under a logical-pixel point), `ui_get_stats` (`LayoutStats`, node count, revisions). Computed rectangles are as of the last layout pass; a response carries `layout_revision` and `stale: true` when edits since then have not been laid out, and `fresh: true` on a request runs a pass first. Verifying how the UI looks is the screenshot tool owned by the render/editor specs, not this spec.

`UiSetField` is one generic command instead of one per field, built on the reflection path machinery (`apply`, `reflect_path_mut`), so a new widget or style field is editable by agents and the inspector the day it is added. Structural edits stay explicit commands because they need ids and undo data.

Editor: a **UI panel** in `exitium_editor` with the node tree (drag to reparent, delete, add from a palette of the five widgets), an inspector for the selected node generated from `UiNodeDesc` reflection (ranges, tooltips and `hidden` honored), a theme editor, and a selection outline in the play-in-editor viewport drawn from the node's `rect` by the egui overlay. The panel only issues the commands above and reads `ui_get_tree`-equivalent data; it has no private mutation path. Mouse input over the game viewport is translated to logical UI pixels and fed to `process_input` as in a built game.

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
| Whole-frame UI CPU, 10,000 nodes, 1,000 counters changing each frame | <= 2 ms (12% of a 60 Hz frame) |
| Memory per node (hot plus layout, excluding strings) | <= 1.25 KiB |
| `UiDrawItem` size | <= 96 B |

Evidence for the layout rows: a throwaway probe of taffy 0.15.1 (flexbox only, rounding off, native release build on a developer machine, not min-spec) measured about 4.4 ms for the cold 10,000-node pass and about 35 us for one dirty leaf, so the budgets keep a margin for min-spec and for the arena adapter's extra indirection. The first bench PR replaces these with real numbers.

Complexity: layout O(dirty ancestor chain + dirty subtrees); paint O(visible nodes) per rebuild and zero when clean; hit test O(interactive entries); `set_*` O(1) amortized.

Where the budgets are tight, the first answer is the data layout (hot/cold split, flat hit list, shared text buffer) and the second is a smaller rebuild (subtree draw caching), never a bypass of the command bus or the dirty flags.

## Test plan

- **Arena:** stale ids fail every lookup, LIFO reuse, generation retirement, `restore` succeeds under LIFO order and returns `RestoreConflict` otherwise. A seeded random sequence of add, remove, reparent, undo and redo checks parent/child consistency and id stability after every step.
- **Layout goldens:** known trees (row, column, gap, padding, grow and shrink, `Percent`, min/max, nested) compared to exact rectangles using a fixed-advance measurer. Anchoring and scaling: every anchor at three viewport sizes, a non-default pivot, `snap_integer`, and physical-pixel snapping of adjacent edges.
- **Invalidation:** each row of the invalidation table asserts `LayoutStats` (a fixed-size counter edit does zero layout; a resizing counter lays out only its ancestor chain; a clean pass does none). A test for the empty-cache invariant covers adding a child to a clean parent.
- **Painting:** golden draw lists for a panel with children, each button state and the focus ring, `Contain` images, progress fill in both directions, layer order across roots, an invisible subtree, and `revision` stability when nothing changed.
- **Input:** table-driven sequences of `UiInputFrame`s: press and release on a button (click), press then release elsewhere (no click), press on gameplay then drag over UI (not captured), a transparent root passing clicks, a blocking panel over a button, disabled buttons, tab and shift-tab wrap, activate and cancel, and removing the hovered, pressed and focused node mid-sequence.
- **Description round trip:** `describe(build(desc)) == desc`, including names and roots; duplicate names are rejected atomically (no partial subtree).
- **Benchmarks** for every row of the performance table; a `size_of` test pins `UiDrawItem` and the per-node memory budget.
- **wasm:** `cargo build --workspace --target wasm32-unknown-unknown` covers the crate (it is not `NATIVE_ONLY`).
- **Stage 2:** headless app tests that insert the plugin, send input and assert events and `UiCapture` before a game system runs; command tests for apply and undo of every command; a reflection round trip of `UiNodeDesc`; an `exitium_mcp` parity test mapping every command to its tool.

## Open questions

1. **Draw submission edge (needs #107).** Which crate owns the draw primitive types, and does the renderer take a `UiDrawList` or primitives defined in `exitium_render`? Preferred: UI depends on render and converts in `ui_extract` (one function), so the render crate stays UI-agnostic. The only hard requirement here is the ordered, physical-pixel, back-to-front content.
2. **Image identity.** `Guid` is used because it is stable and already in core; the asset layer (not specced here) or the renderer resolves it to a texture, and decides what happens for an unloaded image (draw nothing, or a placeholder).
3. **Platform input shape.** The platform spec defines the per-frame pointer and key state the `ui_input` system converts into a `UiInputFrame`, including how wheel and touch arrive.
4. **Consumption.** Capture flags are the contract. Whether the shared input resource should also mask consumed buttons and keys (so gameplay needs no guard) is a platform or app decision.
5. **System ordering (needs #106).** The Stage 2 plugin needs "first in `Update`", "last in `Update`" and "in render extraction" expressible by the app spec's mechanism; if sets or labels are not in the MVP app, the plugin registers in fixed order and the spec's contract is that order.
6. **Draw caching.** Is a full rebuild per paint change good enough at Glutton's heaviest screen? The first bench PR answers it; subtree caching keyed on per-node revisions is the next step if not.
7. **Command bus details.** How non-undoable actions (`ui_click`) and bulk commands (`UiBuildTree`) are represented belongs to the command-bus spec; this spec only fixes what each command must carry.
8. **Persistence.** How a saved UI tree is stored as an asset and loaded by the scene or asset crate is not decided; the MVP minimum is the reflection text form of a `UiNodeDesc`.
9. **Clipping.** Needed once scroll views exist; requires scissor or clip items in the draw data and a matching render capability.

## Implementation issues

Filed by the architect after this spec merges, into the Jam MVP milestone with the `file-issue` skill. Each edits one crate, fits one PR (about 600 changed lines or fewer), has acceptance criteria and `Blocked by` lines. Because they all edit `crates/exitium_ui/` (and the factory allows one in-flight issue per crate), Stage 1 is a chain; the order below is also the critical path for the Glutton UI. `Cargo.toml`, `Cargo.lock` and the facade are the intake PRs' job.

Footprint shorthand used below: `E` is `Edits: crates/exitium_ui/`, with `Governs: none`, `Hotspots: none` and `New crate edges: none`. The architect writes the full `## Footprint` block into each issue. Every issue gets the label `area:ui` (U11 `area:mcp`, U12 `area:editor`), the matching role label (`role:impl-ui`; U11 and U12 `role:impl-editor-tools`) and the Jam MVP milestone.

Stage 1 (no ECS). Each issue's acceptance criteria are the matching Test plan bullets, the performance rows it owns, and `cargo xtask ci all` including the wasm build.

| # | Issue | Footprint | Blocked by |
|---|---|---|---|
| U1 | `feat(ui): node arena, tree structure and stable ids`: `NodeArena` with `restore`, `UiTree` add, remove, reparent, names, `detach` and `restore`, cycle check, arena fuzz test. | E | Stage 1 intake |
| U2 | `feat(ui): widget data, UiStyle and UiNodeDesc`: `WidgetKind` and widget data, `UiStyle` and friends, `UiTheme`, `UiNodeDesc` with `build`, `build_with_ids` and `describe`, the setters with equal-value no-op (`set_text`, `set_progress`, `set_enabled`, `set_style`). | E | U1 |
| U3 | `feat(ui): taffy layout core`: `UiStyle` to `taffy::Style` conversion, the arena adapter traits, `UiMeasure` and a monospace test measurer, per-root layout, absolute rectangles, `LayoutStats`, the invalidation rules and cache invariant, layout golden tests. | E | U2 |
| U4 | `feat(ui): viewport, scaling and anchoring`: `UiViewport`, `RootConfig`, `ui_scale` modes, anchor and pivot math, scale-change invalidation, plus the layout benchmarks (10,000-node cold, one leaf, clean) and the `set_*` benchmarks. | E | U3 |
| U5 | `feat(ui): draw list and theme resolution`: `UiDrawList`, the paint walk for the five widgets and button states, physical-pixel snapping, `revision`, draw benchmarks and the `size_of` pin. | E | U4 |
| U6 | `feat(ui): input, hit test, focus and capture`: hit list built in the layout post-pass, `UiInputFrame`, rules 1 to 7, `UiEvent`, `UiCapture`, input benchmarks. | E | U5 |

Stage 2 (ECS, app, render, command bus). The Cargo edges are added by the Stage 2 intake, so none of these touches `Cargo.toml` or `Cargo.lock`.

| # | Issue | Footprint | Blocked by |
|---|---|---|---|
| U7 | `feat(ui): UiPlugin with resources and systems`: the `plugin` module: resources, `ui_input` and `ui_layout` systems in the named schedules, headless app tests. | E | U6, #106 and its app/plugin implementation issues, Stage 2 intake |
| U8 | `feat(ui): render handoff and text measurement`: `ui_extract` into render extraction, `UiMeasure` over the render crate's text shaping with memoization. | E | U7, #107 and its render implementation issues |
| U9 | `feat(ui): reflection derives and UiNodeDesc text round trip`: `Reflect` on the data types, hand-written `UiNodeId` reflection and text form, round-trip tests. | E | U7, the reflect implementation issues |
| U10 | `feat(ui): UI commands with undo`: the commands in the MCP table implemented against the command-bus trait, undo and redo tests. | E | U9, the command-bus crate issues |
| U11 | `feat(mcp): ui_* tools`: the tools in the MCP table forwarding to the commands, schemas from the reflect dump, parity test entries. | `Edits: crates/exitium_mcp/`, `Governs: none`, `Hotspots: none`, `New crate edges: none` | U10, the MCP server spec's implementation issues |
| U12 | `feat(editor): UI panel`: node tree, palette, reflected inspector, theme editor, selection outline. | `Edits: crates/exitium_editor/`, `Governs: none`, `Hotspots: none`, `New crate edges: none` | U10, the editor shell spec's implementation issues |

The real `Blocked by #N` numbers for the intake, command-bus, MCP and editor items are filled in when the architect files these, because those specs and issues are written in parallel with this one. U1 to U6 need no other spec and can start the moment Stage 1 intake merges, which is why the Glutton UI path does not wait on the app, render or command-bus specs.

Intake PRs (not issues of this spec; listed for the architect, they hold the `Cargo.toml`, `Cargo.lock` and facade hotspots):

- **Stage 1 intake:** add `taffy` to `[workspace.dependencies]` and `exitium_core` plus `taffy` to `exitium_ui`'s dependencies, and `criterion` as a dev-dependency with a `[[bench]]` entry. Crate edges: `exitium_ui -> exitium_core`.
- **Stage 2 intake:** add the edges `exitium_ui -> exitium_ecs, exitium_app, exitium_reflect, exitium_command` and the proposed `exitium_ui -> exitium_render` (pending #107, open question 1), plus the `exitium::ui` facade re-export (hotspot `crates/exitium/`). `exitium_ui` stays out of `NATIVE_ONLY`.

### Dependencies (crates.io, for the intake PRs)

Added to `[workspace.dependencies]` by the Stage 1 intake PR, so implementation issues never touch `Cargo.lock`. High-risk area (new dependency): the reviewer checks this table.

| Crate | Version | Features | License | wasm | Why not std |
|---|---|---|---|---|---|
| `taffy` | `0.15.1` | `default-features = false`, `["std", "flexbox"]` | MIT (already allowed in `deny.toml`) | yes: `cargo check --target wasm32-unknown-unknown` passes; no platform calls | A CSS-conformant flexbox solver with caching is thousands of lines of subtle algorithm; std has no layout. 0.15.1 was published 2026-10-25, MSRV 1.71 (our toolchain is newer), `#![deny(unsafe_code)]`. |

- With these features taffy has **no transitive dependencies** (`grid` would pull `smallvec`, `taffy_tree` would pull `slotmap`; neither is enabled). `serde`, `parse`, `calc`, `block_layout`, `float_layout`, `content_size` and `detailed_layout_info` are also off.
- Taffy is `0.x`: a minor-version bump may break the low-level traits. The caret requirement `0.15.1` allows only patch updates; upgrading is a deliberate PR confined to `layout/`.
- Nothing else is needed: colors, rectangles and vectors come from `exitium_core`, ids from `Guid`, text shaping and font data from the render crate (Stage 2) behind `UiMeasure`.

Stage 2 adds only workspace crates (no crates.io dependencies): see "Stage 1 and Stage 2", "Crate edges added in Stage 2".
