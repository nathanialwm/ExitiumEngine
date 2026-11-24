# Spec: Game UI (retained tree on taffy)

- Issue: #109. Amended by #142 (SP6: the full UI, and AM6 of `editor-mcp-mvp.md` for the command bus).
- Crates: exitium_ui. Stage 1 depends on exitium_core and taffy only. The `plugin` module (also a cargo feature of that name, per the roadmap's crate graph) adds exitium_ecs, exitium_app, exitium_reflect, exitium_command and exitium_serial. There is **no edge** to exitium_render, exitium_platform or exitium_assets (C1: no edges between leaf crates); the facade holds all cross-crate glue.
- Status: draft (revised after review round 1; amended for the full MVP by SP6)

## Goal

Specify `exitium_ui`: a retained widget tree with flex layout on [taffy](https://crates.io/crates/taffy), the full widget set of epic #5 (panel, label, rich text, number label, image with 9-slice, button, toggle, slider, progress bar, scroll view, virtualized list, grid, tabs, tooltip, modal), screen-space anchoring and scaling, pointer and keyboard-focus input that the UI consumes before gameplay, themes with named classes, transitions, **data binding that updates only on change**, **text markup files (`.ui`) with hot reload**, and an **immediate-mode layer** for debug overlays. An agent builds and edits the tree through commands, a human refines it in the editor, and the same tree runs on Windows and in the browser (wasm32).

The Jam-era text of this spec specified five widgets and left the rest as later work. The owner's full-MVP direction (`mvp-roadmap.md`, SP6) brings all of it back, and **the whole design is decided here, once**, because data binding changes the arena shape (see "How the arena is shaped for binding"): deciding it after the layout core is built would mean reshaping the arena under ten merged issues. The implementation order does not change. U1 to U10 build the layout core on the final arena shape; the rest of the widgets, binding, markup, themes and the immediate layer arrive as the Stage 5 UI lane (see "Implementation issues").

This spec also settles PLAN Phase 3 item 1, "ECS entities or a dedicated arena": **a dedicated arena** (see "Node storage decision"). PLAN says "decided by ADR"; the owner-approved issue records it here, with alternatives, to keep `docs/decisions/` (a hotspot) out of this PR.

Design in one paragraph: `UiTree` owns a generational arena of nodes in three parallel arrays: **hot** (what paint and hit-test read: widget data, authored style, rectangle, flags), **cold** (what only tools and bookkeeping read: names, class, tooltip, the node's first binding, the runtime state of big widgets) and **layout** (taffy's style, cache and result). Layout runs through taffy's low-level traits directly on the arena, one independent layout per root, with anchoring applied after layout. Edits clear exactly the caches they invalidate, and edits that cannot change geometry (counters with a fixed size, progress values, hover state, scroll offsets, tween steps) skip layout entirely. **Data binding** is a table of records outside the arena that pushes game state in through the same setters, driven by ECS change detection, so only values that changed cost anything. **Markup** files are the reflection text form of a `UiDocument`; a hot reload diffs the new document against the mounted tree and touches only what changed. Painting turns the laid-out tree into a flat `UiDrawList` of quads, images, text runs and clip pushes in physical pixels. Input is a small plain-data frame in, `UiEvent`s and a `UiCapture` out. An **immediate-mode layer** builds a throwaway tree per frame on the same code. **Stage 1 is the layout core with no ECS, no render and no platform dependency.** **Stage 2** wraps it in a plugin that registers resources and systems into the app schedules; the facade feeds the plain draw data to the renderer and supplies the text measurer. **Stage 3** registers the commands and queries. The **Stage 5 lane** adds the rest of the widgets, binding, markup, themes and the immediate layer.

## Scope

What the Jam-era text left out, and where this spec now specifies it:

| Item | Where |
|---|---|
| Widgets beyond panel, label, image, button and progress bar: rich text, number label, toggle, slider, scroll view, list and virtualized list, grid, tabs, tooltip, modal | "The widget set" |
| Data binding that updates only on change; the `Num` display widget | "Data binding"; "The widget set" (`NumLabel`) |
| Markup files and hot reload | "Markup and hot reload" |
| Named style classes, widget themes, 9-slice images, borders, opacity, transitions and tweens | "Themes, transitions and 9-slice" |
| Clipping and scrolling | "The widget set" (`ScrollView`), "Painting" (clip items) |
| Relayout when an image or font changes on disk | "Reacting to asset changes" |
| The immediate-mode layer for debug overlays | "The immediate-mode layer" |
| A binary form of markup | `UiDocument` is reflected, so the binary serial format (#24) writes it with no UI code (see "Markup and hot reload") |

## Non-goals

Not in this spec. Each can be added later without breaking the layout below (design rule 5), and `WidgetKind` and `UiDrawItem` are `#[non_exhaustive]` for that reason:

- **Text input** (a text field, IME, text editing, selection). It needs a text-event path that `app-platform.md` does not have: its input is physical keys only, and SP3 adds gamepad, not text. `UiCapture::text_input` exists for it and stays false (open question 11).
- Rounded corners and other per-pixel effects on quads. Quads are plain rectangles; rounded or ornate frames are 9-slice images, which also suit pixel art. A rounded-rectangle shader is a render-side feature (open question 13).
- Taffy's CSS grid and block layout. Only flexbox is compiled in; `Grid` has its own equal-column algorithm (see "Layout of the new widgets").
- Keyframe timelines and animated layout. Tweens animate paint properties only, so an animation never invalidates layout.
- Includes or templates in markup (a file embedding another). A game reuses a subtree by building a `UiNodeDesc` in code; an `Include(Guid)` variant can be added later.
- Variable-height rows in the virtualized list, a two-way (UI to game state) binding, touch gestures and kinetic scrolling, gamepad and spatial navigation, right-to-left text, localization, accessibility.
- World-space UI (health bars over entities). Those are sprites and text drawn by the renderer, not a retained tree.
- Luau bindings for the UI. The scripting spec (SP9) generates bindings from reflection and the command bus; this crate hand-writes none.
- Parallel layout. The executor is single-threaded and so is the layout pass; independent roots leave room for it (see Threading).
- Per-node image filtering. Nearest or linear sampling is a property of the texture (`TextureDesc.filter` in the render spec), chosen at import, so pixel-art images are imported with `Nearest`.

## Design

### Module layout

```
exitium_ui/src/
  lib.rs
  arena.rs        NodeArena: slots, generations, two free lists, restore      safe
  tree.rs         UiTree: structure ops, names, image index, revisions, events safe
  node.rs         NodeHot, NodeCold, WidgetState, WidgetKind, widget data      safe
  desc.rs         UiNodeDesc, UiDocument: build a subtree from data, describe  safe
  style.rs        UiStyle, Dim, Edges, Border, flex enums, PointerMode         safe
  theme.rs        UiTheme, UiClass, widget themes, resolution order            safe
  tween.rs        Transition, Easing, the active tween list                    safe
  bind.rs         BindingTable, BindSource, BindFormat, the apply step         safe
  markup.rs       mounts, parse errors, the reload reconcile                   safe
  layout/
    mod.rs        the per-frame pass, UiViewport, invalidation, LayoutStats    safe
    convert.rs    UiStyle -> taffy::Style (the only place taffy types appear)  safe
    adapter.rs    taffy tree traits implemented over the arena                 safe
    grid.rs       the equal-column algorithm of `Grid`                         safe
    rich.rs       RichText word measuring and line breaking                    safe
    virt.rs       VirtualList realization and recycling                        safe
  draw.rs         UiDrawList, UiDrawItem, paint walk, render-facing mapping    safe
  input.rs        UiInputFrame, hit list, hover/press/click/focus, UiCapture   safe
  immediate.rs    UiImmediate, ImUi: the immediate-mode layer                  safe
  plugin/         mod.rs, systems.rs, bind_system.rs, markup_system.rs, commands.rs   safe
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

Accepted costs: UI nodes do not appear in the ECS entity list, and scenes do not carry them as entities (a UI tree saves as a `.ui` `UiDocument`, see "Markup and hot reload"). A game entity that needs a UI node stores its `UiNodeId` in a component.

Revisit if per-widget ECS behavior (a system per widget, widgets as scene prefabs) becomes the main way games author UI. The `UiNodeDesc` boundary makes that migration a spawn-from-desc function, not a rewrite.

### Data layout

```rust
pub type UiNodeId = ...;            // generational index, same shape as exitium_core::RawHandle (index + NonZeroU32 generation)

pub struct UiTree {
    arena: NodeArena,               // slot -> hot node data (paint and hit-test read this)
    cold: Vec<NodeCold>,            // parallel array: names, class, tooltip, binding head, runtime widget state
    layout: Vec<LayoutData>,        // parallel array indexed by slot index, see below
    roots: Vec<UiNodeId>,           // sorted by RootConfig::layer, then creation order
    names: HashMap<Box<str>, UiNodeId>,
    image_nodes: HashMap<Guid, Vec<UiNodeId>>,   // image -> the nodes that show it, see "Reacting to asset changes"
    bindings: BindingTable,         // see "Data binding"
    mounts: Vec<UiMount>,           // markup files mounted into the tree, see "Markup and hot reload"
    tweens: Vec<ActiveTween>,       // in-flight transitions, capped, see "Themes, transitions and 9-slice"
    theme: UiTheme, viewport: UiViewport,
    input: InputState,              // hover, pressed, focus, drag target, prev button, press origin
    hit_list: Vec<HitEntry>,        // interactive nodes in paint order, rebuilt after layout
    events: Vec<UiEvent>,           // valid until the next process_input
    capture: UiCapture,
    layout_revision: u64, paint_revision: u64,
}

struct NodeHot {                    // pinned <= 256 B excluding heap data (was 192 B before the full widget set)
    parent: Option<UiNodeId>,
    children: Vec<UiNodeId>,        // paint order: later children draw on top
    kind: WidgetKind,               // authored widget data; the big variants are boxed so the enum stays <= 64 B
    style: UiStyle,                 // authored, compared on set to skip no-op edits
    class: u16,                     // index into theme.classes, 0 = none; resolved from NodeCold::class_name
    root: Option<Box<RootConfig>>,  // Some only for roots
    rect: Rect,                     // absolute, UI-logical px, set by the post-layout pass
    flags: NodeFlags,               // hovered, pressed, focused, bound, tweening, runtime
}

struct NodeCold {                   // pinned <= 64 B; never loaded by the paint walk, the hit test or layout
    name: Option<Box<str>>,
    class_name: Option<Box<str>>,   // the authored name; re-resolved when the theme changes
    tooltip: Option<Box<str>>,      // rich-text markup, shown by the built-in tooltip root
    bind_head: u32,                 // first of this node's records in BindingTable, NONE when unbound
    state: Option<Box<WidgetState>>,// runtime state of the big widgets, see "The widget set"
}

struct LayoutData {                 // about 720 B per node with taffy's flexbox-only build
    style: taffy::Style,            // derived from UiStyle + theme + widget kind (248 B)
    cache: taffy::Cache,            // 392 B
    unrounded: taffy::Layout,       // parent-relative, 76 B
}
```

- **Arena.** `NodeArena` is a `Vec` of slots with a `NonZeroU32` generation, the same rules as `exitium_core::Slab` (stale ids fail every lookup in O(1), a slot whose generation would overflow is retired). It is a separate small type, not `Slab`, because it adds `restore` (see "Stable ids and undo") and **two LIFO free lists**: one for `Authored` slots and one for `Runtime` slots (see "How the arena is shaped for binding", point 3). If another crate needs `restore`, fold it into `Slab` later; that is not a UI decision.
- **Why parallel arrays.** Paint and hit-test read `kind`, `style`, `rect` and `flags` from hot data. Layout reads `LayoutData`. Everything a paint walk never needs sits in `NodeCold`. Splitting them keeps the paint walk's working set at about 1.3 MB per 5,000 nodes instead of 6 MB. `cold[i]` and `layout[i]` are valid for every live slot `i` and are resized with the arena.
- **Memory budget:** at most 1.25 KiB per node including hot, cold and `LayoutData` and excluding heap strings; 10,000 nodes fit in 12.5 MB. Measured sizes on a 64-bit build with taffy's `std` + `flexbox` features: `Style` 248 B, `Cache` 392 B, `Layout` 76 B; with `NodeHot` at most 256 B and `NodeCold` at most 64 B that is about 1,040 B. `size_of` tests pin all four.
- **Names.** A node may have a unique name (`"score_label"`). `find(name)` is how Rust game code and agents address nodes without holding ids, and how a markup reload matches nodes. A duplicate name is `UiError::DuplicateName`.
- **Roots.** A root is a node with `parent == None` and `root: Some(RootConfig)`; `roots` is the draw order (lower `layer` first, so higher layers draw on top and are hit first). `RootConfig::layer` is limited to `-1000..=999` (`UiError::LayerRange`); layer 1000 is reserved for the built-in tooltip root, which the render mapping also fits.

Widgets:

```rust
#[non_exhaustive]
pub enum WidgetKind {
    Panel,                                     // flex container with an optional background and border
    Label(LabelData),                          // text leaf
    RichText(Box<RichData>),                   // text leaf with inline color, size and font spans
    NumLabel(NumLabelData),                    // a `Num` shown through a `NumFormat`
    Image(ImageData),                          // image leaf, plain or 9-slice
    Button(ButtonData),                        // flex container with interaction states
    Toggle(ToggleData),                        // flex container holding an on/off value
    Slider(SliderData),                        // track, fill and thumb leaf
    ProgressBar(ProgressData),                 // track plus fill leaf
    ScrollView(ScrollData),                    // clipping container that scrolls its content
    VirtualList(Box<ListData>),                // scroll container that realizes only the visible rows
    Grid(GridData),                            // equal-width columns
    Tabs(TabsData),                            // a strip of buttons and a set of pages, one shown
}
pub struct LabelData    { pub text: String, pub font_size: Option<f32>, pub color: Option<Color>, pub align: TextAlign /* Start | Center | End */, pub wrap: bool }
pub struct RichData     { pub markup: String, pub font_size: Option<f32>, pub color: Option<Color>, pub align: TextAlign }
pub struct NumLabelData { pub value: Num, pub format: NumFormat, pub prefix: String, pub suffix: String, pub font_size: Option<f32>, pub color: Option<Color>, pub align: TextAlign }
pub struct ImageData    { pub image: Guid, pub tint: Color, pub fit: ImageFit /* Stretch | Contain | NineSlice { border: Edges } (texels) */ }
pub struct ButtonData   { pub enabled: bool }
pub struct ToggleData   { pub on: bool, pub enabled: bool }
pub struct SliderData   { pub value: f32 /* clamped to min..=max on set */, pub min: f32, pub max: f32, pub step: Option<f32>, pub vertical: bool, pub enabled: bool }
pub struct ProgressData { pub value: f32 /* clamped 0..=1 on set */, pub direction: FillDirection /* LeftToRight | BottomToTop */, pub fill: Option<Color>, pub track: Option<Color> }
pub struct ScrollData   { pub horizontal: bool, pub vertical: bool, pub scrollbar: bool, pub offset: Vec2 /* live; clamped to the content on layout */ }
pub struct ListData     { pub count: u32, pub item_extent: f32 /* UI-logical px, fixed */, pub overscan: u32, pub horizontal: bool, pub offset: f32, pub item: UiNodeDesc /* the row template */ }
pub struct GridData     { pub columns: u32 /* >= 1 */ }
pub struct TabsData     { pub selected: u32 }
```

`None` colors and sizes mean "take it from the theme" (through the node's class, see "Themes, transitions and 9-slice"). Images name an asset by `Guid` (stable, serializable, already in core); the facade resolves it to a texture. A button, a toggle and a tabs strip are flex containers: their text is an ordinary child label, so a button can hold an icon and a label.

Style (flex only; no taffy types):

```rust
pub struct UiStyle {
    pub direction: Direction,            // Row | Column (default Column)
    pub justify: Justify,                // Start | Center | End | SpaceBetween | SpaceAround
    pub align_items: Align,              // Start | Center | End | Stretch
    pub align_self: Option<Align>,
    pub gap: f32,                        // both axes; also the row and column gap of `Grid`
    pub padding: Option<Edges>,          // None: class, then theme padding for Panel and Button, 0 for others
    pub margin: Edges,
    pub width: Dim, pub height: Dim,     // Dim = Auto | Px(f32) | Percent(f32)
    pub min_width: Dim, pub min_height: Dim, pub max_width: Dim, pub max_height: Dim,
    pub flex_grow: f32, pub flex_shrink: f32,
    pub visible: bool,                   // false: display none, no layout, no draw, no hit
    pub background: Option<Color>,       // None: class, then theme for Panel and Button, transparent otherwise
    pub border: Border,                  // { width: f32, color: Option<Color> }; width 0 draws nothing
    pub opacity: f32,                    // 0..=1, multiplies the alpha of this node and its descendants at paint time (no offscreen pass)
    pub transition: Option<Transition>,  // None: class, then theme
    pub pointer: PointerMode,            // Auto | Block | Pass
}
pub struct Edges { pub left: f32, pub top: f32, pub right: f32, pub bottom: f32 }
```

`PointerMode::Auto` resolves to `Block` for buttons, toggles, sliders, scroll views, virtual lists, tab headers and panels with a visible background, and to `Pass` for everything else (labels, images, progress bars, transparent containers), so a transparent full-screen HUD container does not eat gameplay clicks while a visible panel does.

Roots, anchoring and scaling:

```rust
pub struct RootConfig {
    pub anchor: Anchor,                  // TopLeft, Top, TopRight, Left, Center, Right, BottomLeft, Bottom, BottomRight
    pub pivot: Option<Vec2>,             // fraction of the root's own size, default = the anchor's fraction
    pub offset: Vec2,                    // UI-logical px, +x right, +y down
    pub size: RootSize,                  // Fill (viewport) | Fixed(Vec2) | Fit (content)
    pub layer: i16,                      // -1000..=999 (1000 is reserved for the tooltip root)
    pub modal: bool,                     // blocks input to every root below and draws a scrim (see "Input", rule 8)
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
    pub clips: Vec<Rect>,                    // physical px; an item's `clip` is an index + 1 into this table, 0 = none
}

#[non_exhaustive]
pub enum UiDrawItem {                        // every item carries its root's layer and its clip
    Quad  { layer: i16, clip: u16, rect: Rect, color: Color },
    Image { layer: i16, clip: u16, rect: Rect, uv: Rect, image: Guid, tint: Color },   // uv is normalized, (0,0)-(1,1) for the whole texture
    Text  { layer: i16, clip: u16, origin: Vec2, run: TextRun },                      // run.text is a byte range into UiDrawList::text
}
pub struct TextRun {
    pub text: Range<u32>, pub font: Option<Guid>,
    pub size: f32, pub line_height: f32,     // em size in UI-logical px; line_height is a multiplier of size (theme, default 1.2)
    pub color: Color, pub align: TextAlign, pub wrap: bool,
    pub max_width: f32,                      // the width the renderer aligns and wraps inside, UI-logical px
}
```

`rect` and `origin` are **physical** pixels, origin top-left, y down. A plain `Text` has `origin = node rectangle's top-left` and `max_width = rectangle width`, so the renderer aligns inside the node and the UI never needs a text width at paint time; text sits at the top of its node (vertical placement comes from layout: a label sized to its content fills its node). A **rich-text** line emits one `Text` per span segment, with `origin` set by `layout/rich.rs`, `wrap: false`, `align: Start` and `max_width` the segment's measured width, so the segments of a line sit side by side. `size` and `max_width` stay UI-logical because the renderer lays text out at the unscaled size and scales the result (render spec, "Shaped-buffer cache"). One shared `text` buffer means a rebuild allocates nothing once the buffers are warm. `revision` increases only when the content changes, so a consumer can keep an uploaded batch when it is unchanged.

**Clips.** A scroll view or virtual list clips its descendants. The renderer's `clip` is a field on each draw item (a scissor rectangle), not a push and pop, so the draw list does the same: each item carries a `clip` index into `clips`, which holds the rectangles in physical pixels (a pass uses a handful of distinct clips, so the table keeps `UiDrawItem` small). Clipping is applied to the **quad, image and text planes alike**, so the two planes of a root (below) stay valid inside a scroll view. The glue maps an index to the render item's `clip` rectangle (after the `dpi_scale` division).

Order in the list: ascending `layer`; per root, **all graphics (quads, images) first, then all text**. See "Painting" for why and for the authoring rule this implies.

### Themes, transitions and 9-slice

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
    pub toggle: ToggleTheme,                                       // track_off, track_on, thumb, size (default 44 x 24)
    pub slider: SliderTheme,                                       // track, fill, thumb, track_thickness, thumb_size, default length 160
    pub scrollbar: ScrollbarTheme,                                 // track, thumb, thickness (default 6)
    pub tab: ButtonColors, pub tab_selected: Color,                // tab header colors; the selected header uses tab_selected
    pub tooltip: TooltipTheme,                                     // background, text, padding, delay_ms (default 500)
    pub modal_scrim: Color,                                        // drawn under a modal root
    pub transition: Option<Transition>,                            // default for every node with a class state change; None = instant
    pub classes: Vec<UiClass>,                                     // named style classes; NodeHot::class indexes this
}

pub struct UiClass {
    pub name: String,                                              // unique in the theme ("title", "danger_button")
    pub padding: Option<Edges>, pub background: Option<Color>, pub border: Option<Border>,
    pub text_color: Option<Color>, pub font_size: Option<f32>, pub transition: Option<Transition>,
    pub hover: PaintPatch, pub pressed: PaintPatch, pub disabled: PaintPatch, pub selected: PaintPatch,  // selected: a toggle that is on, the current tab
}
pub struct PaintPatch { pub background: Option<Color>, pub border_color: Option<Color>, pub text_color: Option<Color>, pub opacity: Option<f32> }
pub struct Transition { pub duration_ms: u16, pub easing: Easing /* Linear | EaseOut | EaseInOut */ }
```

A class overrides only the properties that were already `Option` in `UiStyle` (padding, background, border color) plus the text color and font size. It cannot set `width`, `gap` or any other non-`Option` layout field, because a node's own `Dim::Auto` cannot be told apart from "not set", and a class that silently changed sizes would make layout depend on hover. The four state patches carry **paint properties only**, so a hover, press or disable never invalidates layout.

**Resolution order** for any color or size, first hit wins:

1. the node's own override (`Some` in `UiStyle` or in the widget data);
2. the class's state patch for the node's current state (precedence: disabled, pressed, hover; `selected` applies on top of the base when it is true);
3. the class's base value;
4. the widget's theme entry (`theme.button` for a `Button`, `theme.toggle` for a `Toggle`, and so on);
5. the general theme value (`text`, `font_size`, `panel_background`).

A node-level override **replaces every state** of that property: a `Button` with `background: Some(c)` stays `c` when hovered. A game that wants custom hover colors uses a class, not an override. `class_name` is stored on the node and resolved to an index when it is set and again whenever the theme changes; an unknown class name resolves to "no class" and adds an entry to `UiTree::warnings()` (shown by `ui_get_stats`), not an error, so a markup file may name a class that a later-loaded theme defines.

Changing the theme clears all layout caches (padding and font size feed layout), re-resolves every class name in one walk (O(nodes), a rare event) and bumps `paint_revision`. Changing one class field does the same. Per-node overrides stay the cheapest way to style one node.

**Transitions.** A change of resolved paint value that follows a state change (hover, press, enable, select) or a class change starts a tween from the value currently shown to the new target when a transition applies: the node's `style.transition`, else its class's, else `theme.transition`. A node that shows `opacity`, `background`, `border` color or text color changing because **game code or a binding called a setter** also tweens when a transition applies, so a health bar color may fade. Tweens animate paint properties only, never layout. Rules:

- `UiTree::advance(dt_seconds: f32)` steps every active tween and bumps `paint_revision` only if one moved. The crate reads no clock (see Threading); the plugin's `ui_animate` system passes the frame delta (see "Schedules"). With no tween active `advance` returns in O(1).
- At most `MAX_TWEENS = 512` run at once. A request past the cap snaps to the target instead of tweening (a counted `LayoutStats.tween_overflow`, never an error).
- A new target for a node and property already tweening restarts from the value currently shown, so quick hover in and out never jumps. The color is interpolated component-wise in the space `Color` is stored in.
- A transition of `duration_ms == 0` and a `None` transition are the same: instant.
- Tests use `advance` with fixed steps, so animation is deterministic.

**Borders and opacity.** `UiStyle::border` is drawn inside the rectangle as four quads (top, bottom, left, right) of `width` UI-logical px, at least one physical pixel when `width > 0`. It does not change layout: a node that wants room for a border sets its padding. `opacity` multiplies the alpha of the node's own quads, images, text and border and of all its descendants; the paint walk carries the product down. There is **no offscreen group pass**, so overlapping children of a half-transparent parent blend individually (documented limit; a render-side group opacity would be a render spec change).

**9-slice.** `ImageFit::NineSlice { border }` keeps the four corners of an image at their texel size, stretches the four edges along one axis and the center along both. One texel is one UI-logical pixel, so a 16-texel corner is 16 logical px and scales with `ui_scale` (with `snap_integer` it stays pixel-exact). It needs the texture's size, which comes from `UiMeasure::image_size`; an unknown size draws nothing. If the node is smaller than the two borders on an axis, both borders on that axis shrink proportionally. The render spec has no 9-slice (it says the UI draws nine ordinary sprites), so the paint walk emits **nine `Image` items**, each with a `uv` sub-rectangle (see "Draw data"), which the glue converts like any other sprite using `SpriteDraw::uv_rect`. Rectangles snap to physical pixels per edge, so the nine pieces stay edge-to-edge.

### The widget set

The struct definitions are under "Data layout". This section fixes behavior. Interaction results are `UiEvent`s (see "Input"); all value changes also go through setters (`set_text`, `set_num`, `set_progress`, `set_value`, `set_on`, `set_selected`, `set_count`, `set_enabled`, `set_scroll`), each with the equal-value no-op and the dirty rules of "Invalidation strategy".

| Widget | Behavior |
|---|---|
| **Panel**, **Grid** | Flex containers. A panel with a visible background blocks pointer input (`PointerMode::Auto`). |
| **Label** | One text run. Fixed-size labels never re-lay-out on `set_text`. |
| **RichText** | A label whose `markup` string carries inline spans: `[c=#rrggbb]gold[/c]` (color), `[s=24]big[/s]` (font size in UI-logical px), `[f=<guid>]...[/f]` (font), `[[` for a literal `[`. At most 64 spans and 4 nesting levels; an unknown or unclosed tag is kept as literal text and adds a warning. The render spec has no spans, so `layout/rich.rs` measures words through `UiMeasure::measure_text` and breaks lines itself, and the paint walk emits **one `Text` item per span segment per line**, positioned with `origin`. The result (segments and the measured size) is cached on the node keyed by `(markup, width)`, so a clean node costs nothing. |
| **NumLabel** | `set_num(node, Num)`: an equal `Num` is a no-op; otherwise the value is formatted with its `NumFormat` (plus prefix and suffix) into the label's buffer, and an equal resulting string is also a no-op for layout and paint. So a counter ticking at 1,000 Hz repaints only when a visible digit changes. |
| **Image** | `Stretch`, `Contain`, or `NineSlice`. An unknown size draws nothing (or, for `Stretch`, the whole rectangle). Reacts to image changes (see "Reacting to asset changes"). |
| **Button** | Click is press then release over it. Enabled state, hover, press and focus as under "Input". |
| **Toggle** | A container drawn as a track (`theme.toggle`, colored by `on`) with a thumb quad at the left or right end; children draw over it. A click or `activate` flips `on` and emits `ToggleChanged(node, bool)`. With `Auto` size and no children it measures as `theme.toggle.size`. Focusable. |
| **Slider** | A leaf: track, fill and thumb quads. Press on it, or press on its track, sets the value from the pointer position along the axis and starts a drag that follows the pointer until release, even outside the slider; the value is clamped to `min..=max` and snapped to `step`. Arrow keys (`nav_x` or `nav_y`) on a focused slider change it by `step`, or 1% of the range when `step` is `None`. Emits `SliderChanged(node, f32)` on change only. `Auto` size is `theme.slider` default length by its thumb height. Focusable. |
| **ProgressBar** | Unchanged from the Jam text; value clamped to 0..=1. |
| **ScrollView** | Clips its content to its rectangle and scrolls it. The content size is the union of its children's rectangles after layout; `offset` is clamped to `0..=(content - viewport)` per enabled axis. The wheel (`UiInputFrame::scroll`) scrolls the **innermost scroll view under the pointer that can move in that direction**; at its limit the wheel goes to the next ancestor scroll view, and only then to gameplay. With `scrollbar`, a thumb quad is drawn and can be dragged. A scroll offset change needs **no layout**: it moves the absolute rectangles of the subtree and rebuilds the hit list and draw list. Children clipped out are neither drawn nor hit-tested outside the viewport rectangle. |
| **VirtualList** | A scroll container for rows of fixed `item_extent` that realizes only the visible rows plus `overscan` on each side. See below. |
| **Tabs** | Children are `[strip, page0, page1, ...]`: `strip` is a flex container of `Button`s (one per page), and only the page `selected` points to is laid out, drawn and hit; the others are hidden by layout (`compute_hidden_layout`), keeping their state. A click on strip button `k` sets `selected = k` and emits `TabChanged(node, k)`. A selected strip button is painted with the class's `selected` patch or `theme.tab_selected`. Left and right (`nav_x`) move the selection while a strip button has focus. |
| **Tooltip** | Not a widget kind: a node's cold `tooltip` string (rich-text markup). When the pointer rests on a node with a tooltip for `theme.tooltip.delay_ms` of `advance` time, the tree shows its **built-in tooltip root** at layer 1000 (reserved, see "Roots"), near the pointer and moved to stay inside the viewport. It hides on press, on leaving the node and when the node is removed. Its nodes are `Runtime` slots, it never blocks input (`PointerMode::Pass`), and it is created lazily on the first show. |
| **Modal** | Not a widget kind: `RootConfig::modal == true`. See "Input", rule 8. |

**Virtual list.** Rows are the realization of a template, so ten thousand data items cost a few dozen nodes:

- `count` is the number of rows (set directly, or by a binding to the length of a reflected list, see "Data binding"). The content extent is `count * item_extent`; `first = floor(offset / item_extent)` and `last = ceil((offset + viewport) / item_extent)`; rows `first - overscan ..= last + overscan` (default overscan 2, clamped to `0..count`) are realized.
- A realized row is a clone of `ListData::item` built into **`Runtime` slots** under the list. When the window moves, rows leaving it go to a pool, and rows entering it take a node from the pool (a rebind, no allocation) or build one. Bound fields of a row template use `BindSource::Row` (see "Data binding"), and a row is re-applied when its index changes or the list's source changes.
- Rows are independent fixed-size layout roots (width = the list's inner width, height = `item_extent`) laid out after the list's own size is known (see "Layout of the new widgets"), so scrolling does not invalidate a clean row.
- **Identity.** A row node is recycled, so its `UiNodeId` is not an item's identity. `UiTree::row_of(node) -> Option<(list, index)>` walks up from any node inside a row, so a `Click(button)` handler finds the item with one call. When a row is newly assigned to an index, the plugin sends a reflected **`UiListRow { list, index, row }` ECS event** for code that fills rows by hand. It is an ECS event and not a `UiEvent` because the tree's event slice is cleared by every `process_input` in `First`, and rows are realized later in the frame (`PostUpdate`), so a `UiEvent` raised there would be gone before any `Update` system ran.
- Variable row heights are a non-goal. A list whose `count` is 0 draws nothing.

### How the arena is shaped for binding

This section exists because data binding is the one feature that changes the arena (see Goal). Everything below is already true of the arena built by U1 to U10, so building the layout core first costs nothing.

1. **Bindings are records outside the arena.** `BindingTable` is a slab of fixed-size `BindRecord`s (target, source, format, the last applied value, a chain link). A node holds only `NodeCold::bind_head`, the first record of its chain, and the `bound` flag in `NodeHot::flags`. The paint walk and layout never touch the table, so an unbound node pays nothing and `NodeHot` has no field for it. At most `MAX_BINDINGS_PER_NODE = 8` records per node (`UiError::TooManyBindings`). A record is at most 64 B, so 10,000 bindings are 640 KB.
2. **Bindings write only through the setters.** A binding apply is `set_text`, `set_num`, `set_progress`, `set_value`, `set_on`, `set_selected`, `set_count`, `set_enabled`, `set_visible`, `set_image` or `set_tint`, so it uses the same equal-value no-op and the same dirty flags as a hand call. There is no second dirty path to keep right, and a binding whose value did not change costs a compare.
3. **Two slot classes: `Authored` and `Runtime`.** Every arena slot has a class fixed when the slot is created, and each class has its **own LIFO free list**. Nodes made by commands, markup, a `UiNodeDesc` or Rust game code are `Authored`. Nodes the crate creates and destroys on its own (virtual-list rows, the tooltip root, the immediate-mode layer's nodes) are `Runtime`. This is what protects `restore`: it requires each slot to be the head of its free list when its turn comes ("Stable ids and undo"), and that holds only if nothing allocates from that list between a `detach` and its undo. Systems that recycle rows or the tooltip run between a command and its undo, and they allocate only from the `Runtime` list. `detach` frees a runtime descendant (a list's rows) to the runtime list and **does not record it**: `DetachedSubtree` holds the authored nodes only, and a restored list re-realizes its rows on the next pass. Commands cannot name a runtime node (`UiError::RuntimeNode`).
4. **`detach` takes the bindings with it.** `DetachedSubtree` also holds the removed nodes' binding records, so undoing a removal restores the bindings, and a removed node never leaves a record that points at a dead id.
5. **A manual edit of a bound field stays until the source next changes.** A binding is applied when its source changes (see "Data binding"), not every frame. So `ui_set_field` on a bound label's text shows the edit, and the next change of the bound value overwrites it. This is the rule for agents and the inspector: edit the binding (a command on the `bindings` field of `UiNodeDesc`) to change what a field follows.
6. **`UiNodeDesc` carries the bindings** (`bindings: Vec<UiBinding>`), so `build` creates them, `describe` returns them, markup writes them, and undo and reload see them like any other field.
7. **Lists re-apply rows without nodes changing.** A row template's records (`BindSource::Row`) are instantiated per realized row into the same table and freed when the row is recycled (Runtime class, never in a `DetachedSubtree`).

What U1 to U10 must therefore build: the class on every slot and the two free lists (U1), `bind_head` and the `bound` flag (U2), `UiNodeDesc::bindings` as an empty vector (U3). U23 fills in the table, the setters' binding calls and the `ui_bind` system.

### Layout (taffy integration)

**Dependency:** `taffy 0.15.1` (see "Wiring"). It is a pure layout library: `#![deny(unsafe_code)]`, no platform calls, builds for `wasm32-unknown-unknown`, and with `default-features = false, features = ["std", "flexbox"]` it has **no transitive dependencies** (checked with `cargo tree` and `cargo check --target wasm32-unknown-unknown`).

**Integration choice: taffy's low-level traits over our arena, not `TaffyTree`.** `TaffyTree` owns a second copy of the tree (its own storage, ids and children), which we would have to keep in sync with the arena on every add, remove and reparent. The low-level API (`TraversePartialTree`, `LayoutPartialTree`, `LayoutFlexboxContainer`, `CacheTree`, `compute_root_layout`, `compute_flexbox_layout`, `compute_leaf_layout`) is the documented route for embedding taffy in an existing widget tree, and taffy's `custom_tree_vec` example is the template. The implementer verifies the exact trait set against 0.15.1; rounding (`RoundTree`) is not used. `layout/adapter.rs` is a view struct that borrows the arena, `layout` array and a `&mut dyn UiMeasure` for the duration of one pass. Taffy `NodeId`s are the slot indices, which is safe because no structural change can happen during a pass.

Dispatch in `compute_child_layout`: `Panel` and `Button` go to `compute_flexbox_layout`; `Label`, `Image` and `ProgressBar` are leaves and go to `compute_leaf_layout` with a measure closure (the other widgets are in "Layout of the new widgets"). A node with `visible == false` goes to `compute_hidden_layout`. Every call goes through taffy's cache first (`compute_cached_layout`).

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
3. If any root was laid out, or the viewport (and so an anchor origin) changed: one depth-first walk of the affected roots computes absolute `rect`s (parent origin plus relative location, plus the root's anchor origin), rebuilds `hit_list` in paint order, and bumps `layout_revision` and `paint_revision`. Stage 5 adds scroll offsets, clip rectangles and the realized rows of virtual lists to this walk ("Layout of the new widgets").

`LayoutStats { roots_laid_out, nodes_laid_out, measure_calls, tween_overflow }` counts real (non-cached) work (`tween_overflow` is the number of tween requests that snapped because of the cap). It is what the invalidation tests assert, and the stats query exposes it to agents.

### Layout of the new widgets

All of this lives behind the same `compute_child_layout` dispatch and the same per-frame pass. Taffy types stay in `layout/`.

- **Dispatch.** `Panel`, `Button`, `Toggle` and `Tabs` go to `compute_flexbox_layout` (`Tabs` is a column; its hidden pages go to `compute_hidden_layout`). `ScrollView` is a flex container whose children are laid out **without** a size limit on each enabled scroll axis (available space `MaxContent` on that axis), so content can be larger than the view. `Label`, `NumLabel`, `RichText`, `Image`, `ProgressBar` and `Slider` are leaves measured by `compute_leaf_layout`. `Grid` and `VirtualList` use the two algorithms below.
- **Default sizes.** An `Auto` size on a leaf with no intrinsic size uses the theme: `ProgressBar` 100 x 8 (as before), `Slider` 160 x thumb height, an empty `Toggle` 44 x 24.
- **`Grid`.** `layout/grid.rs` places children in `columns` equal columns, left to right and then down. Column width is `(inner width - (columns - 1) * gap) / columns` and is definite, so each child is laid out with `compute_child_layout` at that width; a row's height is the tallest child in it; `gap` is both the column and the row gap. With an `Auto` width, the grid takes the available width. A child's `width` is ignored (it fills its cell) and its `height` is honored. There is no spanning and no taffy grid, so the crate keeps taffy's no-dependency feature set.
- **`RichText`.** The measure function splits the markup into words, measures each word with `measure_text` at its span's size and font (cached per `(markup, width)` on the node), breaks lines greedily at the offered width, and returns the block size. Layout calls `measure_text` once per distinct word and span, at most twice per dirty node per pass in the steady state, and never for a clean one.
- **Scroll content size.** After a scroll view's own layout, its content size is the union of its children's relative rectangles plus their margins; the post-layout walk stores it in the view's runtime state and re-clamps the offset (a content shrink moves the offset back into range).
- **`VirtualList` rows.** The list node is a leaf for its parent's layout: its size comes from its own style (normally `flex_grow` or a fixed size). After the root containing the list has been laid out, each realized row is laid out as an **independent fixed-size root** (`compute_root_layout` with the list's inner width and `item_extent`), so a row with a clean cache costs nothing and a scroll does not touch layout. Row `i` is placed at `list origin + (0, i * item_extent - offset)` (or along x for a horizontal list) by the absolute-rectangle walk.
- **Absolute rectangles and clips.** The post-layout walk that sets each node's absolute `rect` also (1) subtracts a scroll view's offset from its descendants, (2) records a **clip rectangle** per node, the intersection of the rectangles of all `ScrollView` and `VirtualList` ancestors (none for a node outside every scroll container), stored as an index into a per-pass clip table, and (3) writes it into each `HitEntry`, so a clipped-out button cannot be hovered or clicked. A scroll offset change re-runs only this walk for the scroll view's subtree, then rebuilds the hit list and bumps `paint_revision`; it never clears a layout cache.

### Invalidation strategy

Each node's taffy cache is the layout dirty flag. **Invariant: if a node's cache is empty, so are all its ancestors'** (taffy relies on the same one). `invalidate_from(node)` clears `node`'s cache unconditionally, then walks up and stops at the first ancestor whose cache is already empty. New nodes are created with an empty cache, so adding a child calls `invalidate_from(parent)`, never `invalidate_from(child)` (the early-out would stop immediately and leave the parent stale). Paint has one tree-level flag, `paint_revision`.

| Edit | Layout | Paint |
|---|---|---|
| add, remove, reparent, `visible` change | `invalidate_from` the affected parent(s) | yes |
| `UiStyle` set, and the converted taffy style differs | `invalidate_from(node)` | yes |
| `UiStyle` set, only `background`, `border`, `opacity`, `transition` or `pointer` differ | none | yes |
| `set_text`, label has `Auto` width or height | `invalidate_from(node)` | yes |
| `set_text`, label has fixed width and height | none | yes |
| `set_num`, formatted text differs, `Auto` size / fixed size | `invalidate_from(node)` / none | yes |
| `set_text` / `set_num` / `set_progress` / `set_value` / `set_on` / `set_enabled` / `set_count` with an equal value (or equal formatted text for `set_num`) | none | none (no-op) |
| progress value, slider value, toggle `on`, button enabled, image tint, hover, press, focus, tween step | none | yes |
| `set_selected` on tabs | `invalidate_from(tabs)` | yes |
| `set_scroll` / wheel / scrollbar drag | none (rectangle walk of the scroll subtree and hit list only) | yes |
| `set_count` on a virtual list, or the rows moving | none for the list; rows with an empty cache are laid out as their own roots | yes |
| rich-text markup changes | `invalidate_from(node)` | yes |
| image changed, size `Auto` / `Contain` / `NineSlice`: `image_changed(guid)` | `invalidate_from` each user (O(users)) | yes |
| `font_changed` | the text nodes of that font | yes |
| class, class name or `class` patch changes | the nodes whose padding or font size it resolves | yes |
| theme padding or font size changes | all caches | yes |
| a binding applying a value, a `.ui` reload, an immediate-layer frame | exactly the rows above for the setter each one calls | per setter |
| UI-logical viewport size changes | `Fill` roots | yes |
| `ui_scale` changes with the UI-logical size unchanged | none | yes (snapping) |

The class row says "the nodes whose padding or font size it resolves" because a class that sets only colors invalidates paint, not layout.

Consequences for "heavy live numbers": a counter label with a fixed size updates at the cost of one string compare and a copy into its existing capacity, with no layout. A counter that must resize re-lays-out only its ancestor chain (siblings hit their caches). The draw list is rebuilt whenever `paint_revision` moved since the last build, as one linear walk; per-subtree draw caching is a later optimization, enabled by the same flags (open question 6). The cost this table does **not** remove is text shaping for a changed string, which belongs to the renderer (see Performance).

### Painting

`UiTree::build_draw_list(&mut self, measure: &mut dyn UiMeasure, out: &mut UiDrawList)` clears and refills `out` when `paint_revision` is newer than `out.revision`, and returns immediately otherwise. The measurer is used only to get the intrinsic size of images (`image_size`, for `Contain` and `NineSlice`); text needs none, because the renderer aligns inside the node rectangle (rich text reads the per-node measure cache layout filled). The walk is depth-first from each root in layer order, children in order, skipping invisible subtrees, carrying the inherited alpha (`opacity`), the disabled flag and the current clip down. Per widget:

- **Panel, Grid:** a quad for its background, if the resolved color is not fully transparent; then its border, if any.
- **Button:** an optional focus-ring quad (the rectangle outset by 2 UI-logical px, theme `focus_ring`) when focused, then a quad colored by state with precedence disabled, pressed, hovered, normal (a class state patch or `theme.button`; a node override replaces every state). Children (the label) follow. Disabled buttons pass `text_disabled` to descendants' default label color.
- **Label, NumLabel:** one `Text` item as described under "Draw data".
- **RichText:** one `Text` item per span segment per line.
- **Image:** `Stretch` uses the full rectangle; `Contain` shrinks the rectangle to the image's aspect ratio inside the node, centered, using `measure.image_size`; `NineSlice` emits nine `Image` items with `uv` sub-rectangles (see "Themes, transitions and 9-slice"); an unknown size draws `Stretch` as a whole rectangle and draws nothing for the other two.
- **ProgressBar:** a track quad, then a fill quad whose extent is `value` of the track along `direction`.
- **Toggle:** a track quad (`on` color or `off` color), a thumb quad at the end that matches `on`, then its children.
- **Slider:** a track quad, a fill quad up to the thumb and a thumb quad.
- **ScrollView, VirtualList:** the node's own background and border, then the descendants with this node's visible rectangle added to the clip stack; then, when `scrollbar` is set and the content overflows, a scrollbar track and thumb quad (outside the clip).
- **Tabs:** the strip and the selected page only.
- **Modal root:** the scrim quad first, then the root's tree.
- **Every node:** a border (four quads) when `border.width > 0`, after its background.

Rectangles convert to physical pixels as `round(logical * ui_scale)` per edge, so adjacent widgets stay edge-to-edge. The alpha carried down is multiplied into every color, tint and text color of the subtree (there is no offscreen pass).

**Two planes per root.** Graphics and text are emitted as two planes: while walking a root, quads and images go to the list in tree order and text items go to a scratch list that is appended after the root's graphics. The reason is renderer batching: the render spec orders by `(layer, depth)` and needs one text batch for every run of text that sorts between sprite batches (capped at `MAX_TEXT_BATCHES = 64`, beyond which order is approximate). A screen with 200 buttons painted strictly in tree order alternates quad, text, quad, text and would need 200 text batches. Two planes need one text batch per root layer. **Authoring rule that follows:** inside one root, text always draws above every graphic of that root, so a panel that must cover another widget's text has to be its own root on a higher layer (modals, popups and tooltips are separate roots anyway). Roots that share a `layer` value share the planes (all their text over all their graphics), so overlapping roots need distinct layers. Rich text adds items to the text plane, not batches: all text of one layer still shares one batch.

### Render handoff (facade glue contract)

The UI does not depend on the renderer, and the renderer does not depend on the UI. The `exitium` facade owns a glue system in the render extraction schedule (after transform propagation, before the render frame; it is the facade item listed under "Implementation issues"). The seams are `UiMeasure` (in) and `UiDrawList` (out); this is what the glue must do, in terms of the render spec (#107):

1. **Measurer.** Implement `UiMeasure` over `Renderer::measure_text` (`TextStyle` built from the `TextMeasure` fields; font `Guid` mapped to the renderer's `FontId` by the font store; `None` is the engine's default font) and over the texture store for `image_size` (texture pixels, unscaled). The renderer's shaped-buffer cache is the memo; the glue adds none. Run `ui.layout(&mut measurer)`, then `ui.build_draw_list(&mut measurer, &mut list)`.
2. **Units.** The render space is `Space::Screen`: logical pixels, physical divided by `scale_factor`. The list is physical, so every `Quad`/`Image` rect and `Text` origin is **divided by `list.dpi_scale`**. Text is `TextStyle { size: run.size, line_height: run.line_height, max_width: Some(run.max_width), align, wrap }` with `TextDraw.scale = list.ui_scale / list.dpi_scale`, because the renderer lays out at the unscaled `size` and then applies `scale` (so glyphs end up `size * ui_scale` physical pixels tall, and the wrap width stays in UI-logical units). `TextAlign` maps Start, Center, End to Left, Center, Right; `wrap` maps to `Word` or `None`.
3. **Snapping survives the round trip.** The UI snaps to whole physical pixels; the glue divides by `dpi_scale` and the renderer multiplies it back, so an edge returns within one f32 ulp (at most 1e-3 px for coordinates up to 8,192 px). Fragment centers sit 0.5 px from any edge, so coverage is unchanged. A facade test asserts the round trip for several `dpi_scale` values.
4. **Order.** Render `layer = layer_base + 2 * item.layer` for graphics and `+ 1` for text (`layer_base` defaults to 10,000 and `mask` is configurable, both in the plain-data `UiRenderConfig` that this crate defines); `depth` is the item's index in the list; `space = Screen`; `clip` is the item's entry of `list.clips` (divided by `dpi_scale` like any rectangle), or `None` for index 0; an `Image` item's `uv` becomes `SpriteDraw::uv_rect`. UI sits above the game world and below editor overlays by choice of `layer_base`. A solid quad is a sprite with `texture: None`; an image is a sprite with the texture resolved from the `Guid` (an image that is not loaded yet draws nothing).
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
    pub nav: IVec2,                      // edge-triggered arrow keys this frame: x -1/0/+1 (left/right), y -1/0/+1 (up/down)
    pub scroll: Vec2,                    // wheel movement this frame in lines (platform `scroll_lines`), +y is content moving up
}
#[non_exhaustive]
pub enum UiEvent {
    Click(UiNodeId), HoverEnter(UiNodeId), HoverExit(UiNodeId), FocusGained(UiNodeId), FocusLost(UiNodeId),
    ToggleChanged(UiNodeId, bool), SliderChanged(UiNodeId, f32), TabChanged(UiNodeId, u32),
    ModalCancel(UiNodeId),               // `cancel` pressed with no focus while this modal root is topmost; the game decides whether to close it
}
pub struct UiCapture {
    pub pointer: bool,     // gameplay must not react to the mouse this frame
    pub scroll: bool,      // the UI consumed the wheel this frame: gameplay must not zoom or scroll
    pub keyboard: bool,    // the UI consumed a navigation key this frame: gameplay must ignore Tab, Enter, Space, Escape and the arrow keys
    pub text_input: bool,  // a text-input widget has focus: gameplay must ignore every key. Always false (no such widget, open question 11)
}

impl UiTree {
    pub fn process_input(&mut self, frame: &UiInputFrame) -> UiCapture;
    pub fn events(&self) -> &[UiEvent];  // everything from the last process_input
    /// Queue the events a real click on `node` would produce (what `ui_click` calls). They appear at the
    /// start of the slice returned after the next `process_input`. Errors for a node that is not an enabled interactive widget.
    pub fn inject_click(&mut self, node: UiNodeId) -> Result<(), UiError>;
}
```

`process_input` clears `events` first and returns the new capture, so every system that runs after it in the same frame reads the same slice (there is no drain). Rules, evaluated in this order each frame:

1. **Hit test.** Walk `hit_list` back to front (topmost first) and take the first entry whose absolute rectangle contains the pointer. The list holds only nodes whose resolved `PointerMode` is `Block`, plus interactive widgets (rule 9) and scroll containers, so the cost scales with the number of interactive widgets, not nodes. The hit is the *topmost blocking node*, which may be a panel covering a button, in which case the button is not hovered. Invisible subtrees and a `None` pointer hit nothing.
2. **Hover.** The hovered node is the hit if it is an enabled interactive widget (rule 9), else none. Changes emit `HoverExit` then `HoverEnter` and mark paint dirty.
3. **Press** (`primary_down` rising): the press origin is `Ui` if there was a hit, else `Game`. If the hit is an enabled button it becomes `pressed`. **Any press clears keyboard focus** (`FocusLost` if there was one): a mouse press never gives a button keyboard focus.
4. **Release** (`primary_down` falling): if the pressed button is still the hovered one, emit `Click(button)`. The pressed state always clears. Releasing elsewhere is not a click.
5. **Keyboard.** Focus is reached **only by `tab`**, which moves it to the next enabled, visible interactive widget (rule 9) in paint order (previous with `shift`), wrapping, and with no focus picks the first. `activate` with a focus emits `Click(focused)`. `cancel` with a focus clears it. Focus changes emit `FocusLost`/`FocusGained`.
6. **Capture.**
   - `pointer` follows the press origin. Origin `Ui`: true on the press frame, on every frame the button is held and **on the release frame**, so gameplay never sees the mouse-up of a UI press. Origin `Game`: false for the same span, even if the pointer drags over UI. No press in progress (including the frame after a release): true iff the pointer is over a hit-list node.
   - `keyboard` is true on a frame where the UI **consumed a key edge**: `tab` when at least one enabled button exists, or `activate`/`cancel` while a button had focus. It is **not** a level: a focused button does not capture WASD, and after a mouse click on a button (which leaves no focus) Space and Enter go to gameplay. A player who has pressed Tab and then wants to play presses Escape or clicks.
   - `text_input` is false until a text-input widget exists.
7. **Removal.** Removing a node clears hover, pressed and focus that name it, without events.
8. **Modal roots.** While at least one visible root has `modal: true`, the **topmost** modal root and the roots above it are the only input targets. The hit test stops at the modal root (a click on the scrim, or on any root below, hits the modal root's scrim and nothing else, so gameplay and lower UI see no press); Tab cycles only through focusable nodes inside the topmost modal root and focus that is outside it is cleared when the modal appears; `pointer` capture is true everywhere on screen. A `cancel` with no focus emits `ModalCancel(root)` and consumes the key. The modal's scrim is drawn as the first graphic of its root (`theme.modal_scrim`, viewport-sized).
9. **Interactive widgets.** Rules 1 to 7 are written for buttons and apply unchanged to every **interactive widget**: an enabled `Button`, `Toggle` or `Slider` (tab headers are buttons). They are hit-list entries, hover targets, press targets and focus targets. What differs is what the press and release do:
   - **Button, tab header:** `Click` on release over the same node; a tab header also sets the tabs' `selected` and emits `TabChanged`.
   - **Toggle:** the same click flips `on` and emits `ToggleChanged(node, on)`; `activate` with focus does the same.
   - **Slider:** the press sets the value from the pointer and starts a drag. While `primary_down` the value follows the pointer along the slider's axis even outside its rectangle; each change emits `SliderChanged(node, value)`. Release ends the drag with no `Click`. `nav` on a focused slider changes it by `step` (or 1% of the range) and emits the same event. A drag counts as a UI press origin for capture (rule 6).
10. **Scrolling.** `scroll` moves the innermost `ScrollView` or `VirtualList` under the pointer that can move in the wheel's direction; at its limit the wheel passes to the next scroll ancestor, and only if none can move does it fall through to gameplay. `UiCapture::scroll` is true on a frame where a scroll view consumed it. A scroll view or list is a hit-list entry for the pointer (it blocks like a panel with a background) and for the wheel. A scrollbar thumb is a press target whose drag moves the offset like a slider. Scroll changes need no layout ("The widget set"). The hit list stores each entry's clip rectangle (see "Layout of the new widgets"), and a point outside the clip hits nothing in that entry.
11. **Keyboard nav keys.** `nav` is consumed (and `keyboard` set) only when the focused node uses it: a slider (either axis), a tab header (`x`) or a focused node inside a scroll view (`y` scrolls it by one line). Otherwise arrows go to gameplay, so WASD-style arrow movement is not eaten by a visible HUD.

Disabled buttons are not hit-testable as hover targets, not focusable and ignore `activate`; they still block (a `Block` node under the pointer captures it).

**How UI consumes input before gameplay (Stage 2).** The plugin owns a `UiInputFrame` resource (default: no pointer, nothing pressed) that **the facade glue fills each frame** from the platform input state. `exitium_ui` has no edge to `exitium_platform` (C1), so the conversion is glue (`app-platform.md`, F2) and the plugin never names a platform type. The plugin's `ui_input` system reads that resource, calls `process_input` and stores the result in a `UiCapture` resource. Gameplay input code checks `UiCapture` before reacting: `pointer` for the mouse, `scroll` for the wheel, `keyboard` for Tab, Enter, Space, Escape and the arrow keys, `text_input` for everything (a one-line guard; whether the engine's input helpers apply it automatically is open question 4).

**Frame order and `FixedUpdate`.** The contract lists the schedules as Startup, FixedUpdate (60 Hz by default), Update and render extraction; the exact in-frame order is #106's. This spec assumes `FixedUpdate` runs before `Update` in a frame, and states the consequence precisely:

- Capture is computed once per frame. All `FixedUpdate` steps in that frame (zero or several) see the same value.
- **Requirement on #106, met:** `ui_input` runs after the platform input system and **before `FixedUpdate`** in the frame, in set `UI_INPUT` of `First` (`app-platform.md`; open question 5). Press and release edges seen by gameplay in either schedule use this frame's capture, so the previous-frame-capture case the Jam text had to accept does not arise. A test asserts it (U11).

The platform side is settled in `app-platform.md`: the glue reads the platform `InputState` in its frame view, and the platform never masks input (capture flags are the contract).

### Stable ids and undo

Commands must be undoable (rule 3), and a command recorded after another may refer to a node the earlier command created. So ids must survive undo and redo:

- `detach(node) -> DetachedSubtree` removes a subtree in **pre-order** and returns every **authored** node's full data (hot and cold), **its original id**, the pre-order slot list, its position in the parent, the node's **binding records** and any **mount** whose roots are in the subtree. Each authored slot is freed (generation bumped, pushed on the `Authored` free list) in that order, except that a slot whose generation was `u32::MAX` is retired instead of pushed. Runtime descendants (virtual-list rows, see "How the arena is shaped for binding", point 3) are freed to the `Runtime` free list and are not recorded.
- `restore(DetachedSubtree)` re-occupies the **same slots, with the same generations, in the reverse of the detach order**, then re-inserts the binding records and the mount and rebuilds the `names` and `image_nodes` entries for the nodes. Because the `Authored` free list is LIFO and nothing but commands allocates from it, each slot is the list head when its turn comes, so each step is an O(1) pop, with no search and no doubly linked list. A step succeeds only if the slot is the head of the `Authored` free list and its current generation is the recorded generation plus one; it then sets the generation back to the recorded one. Under the command bus's last-in, first-out undo this always holds: everything done after the detach has been undone first, which returns the free list to its state right after the detach. If any step fails (an out-of-order restore, or a slot that was **retired** at `u32::MAX`, which takes 2^32 - 1 reuses of one slot and is unreachable in practice) the whole restore is rolled back and returns `UiError::RestoreConflict`, and the command bus reports the undo as failed. A restored virtual list has no rows until the next `ui_lists` pass.
- `build(parent, &UiNodeDesc)` returns the new subtree's root id. **Its undo is `detach`, and the redo is `restore` of the `DetachedSubtree` that undo produced**, so a later command that names a node of the built subtree finds it at the same id after undo and redo. There is no "build with given ids" call. A build from a `.ui` file is the same operation plus a mount, which `detach` and `restore` carry.
- `reparent(node, new_parent, index)` returns the old `(parent, index)`; reparenting under one's own descendant is `UiError::Cycle`.
- **What is not undoable and why.** A hot reload, the immediate layer and virtual-list row recycling are not commands: they are driven by a file, by game code and by scrolling, and they work only on `Runtime` slots or through setters on authored nodes. A reload that replaces a node (a changed kind or name) gives it a new id, and an undo entry that still names the old id fails with a stale-target error rather than touching the wrong node (generations make that safe).

### Reacting to asset changes

Asset hot reload (assets spec D12 and D13, PR #166) is in the MVP. The UI side has two inputs and one rule: **the tree never reads an asset; the facade glue tells it what changed.** The glue reads `AssetEvent`s in `First` (see "Schedules"), before `UI_MARKUP` and `UI_INPUT`.

| What changed | Glue calls | Effect |
|---|---|---|
| An image's pixel size changed (a reload with different dimensions, or a first load of an image the tree already names) | `UiTree::image_changed(image: Guid) -> usize` | Looks the `Guid` up in `image_nodes`, and for each node that uses it **with a size that depends on the image** (`Auto` width or height, `Contain`, `NineSlice`) calls `invalidate_from(node)`; bumps `paint_revision`; returns the number of nodes touched. **O(users of that image)**, no tree walk. |
| An image reloaded with the **same** size | nothing | Draw items name a `Guid`, so the next frame already draws the new pixels. The glue's texture lookup is the render side's job (assets spec D13). |
| A font reloaded or loaded | `UiTree::font_changed(font: Option<Guid>) -> usize` | Clears the layout cache of every text node (`Label`, `NumLabel`, `RichText`, tooltips) whose resolved font is `font` (`None` is the theme default) and bumps `paint_revision`. This is one linear pass over the arena. Font changes are rare (a developer replaced the file), so there is no index; the cost is O(nodes) once per change. |
| A `.ui` asset loaded or reloaded | updates the `UiMarkupStore` resource | See "Markup and hot reload". |
| A reload failed (`AssetFailed { reload: true }`) | nothing for images and fonts (the old value stays, D12) | For `.ui`: the store keeps the last good text and records the error for `ui_get_stats`. |

**The image index.** `image_nodes: HashMap<Guid, Vec<UiNodeId>>` lists the nodes whose `ImageData::image` is that `Guid`. It is updated by every operation that can change a node's image or its existence: `build`, `detach` and `restore` (on the nodes of the subtree), `set_image`, and a widget-kind change. A node is added once even if the `Guid` is set twice, and an empty entry is removed. `image_changed` never allocates beyond the returned count. Test: build 5,000 nodes over 10 images, call `image_changed` for one image, and assert exactly its users were invalidated (`LayoutStats` shows only their ancestor chains laid out on the next pass) and the others were not.

Unknown size (`UiMeasure::image_size` returns `None`) draws nothing for `Contain` and `NineSlice` and the whole rectangle for `Stretch`, as before. The facade calls `image_changed` again when the image finishes loading, so a node that was waiting for its size relays out once.

### Data binding

A binding makes a node field follow a value in the game's state. It is **one-way** (state to UI; the UI reports user input as events, see "Input") and **change-only**: a value that did not change costs a tick comparison and nothing else.

```rust
pub struct UiBinding {                       // authored form, a field of UiNodeDesc (so markup, build, describe and undo all carry it)
    pub target: BindTarget,
    pub source: BindSource,
    #[reflect(default)] pub format: BindFormat,
}
#[non_exhaustive]
pub enum BindTarget { Text, Num, Progress, Value, On, Selected, Count, ListSource, Enabled, Visible, Image, Tint }
pub enum BindSource {
    Resource  { resource: String, path: String },                       // a reflected resource by registered type name, then a reflect path ("gold", "stats.hp")
    Component { entity: Entity, component: String, path: String },      // code and commands only; not allowed in a .ui file (no entity references there)
    Row       { path: String },                                         // a field of the list item at this row's index; only inside a VirtualList template
}
pub enum BindFormat { Direct, Number(NumFormat), Normalize { min: f64, max: f64 }, Not }
```

| Target | Sets | Accepts |
|---|---|---|
| `Text` | `Label` text, `RichText` markup | `String`, any integer or float (formatted with `BindFormat::Number`), `bool`, `Num` (formatted) |
| `Num` | `NumLabel` value | `Num`, or an integer or float converted to `Num` |
| `Progress`, `Value` | `ProgressBar`, `Slider` value | `f32`/`f64` (0..=1 for progress), or any number with `Normalize` |
| `On`, `Selected` | `Toggle` on, `Tabs` selected | `bool`; an unsigned integer |
| `Count` | `VirtualList` count | an unsigned integer |
| `ListSource` | `VirtualList` count and rows | a reflected list (`Vec<T>`): the count follows its length and the rows' `Row` sources index into it |
| `Enabled`, `Visible` | enabled flag, `style.visible` | `bool` (`BindFormat::Not` inverts) |
| `Image`, `Tint` | image `Guid`, tint | `Guid` (or an asset reference), `Color` |

A source whose type does not fit the target is **a binding error**, not a panic: the record stops applying, the first occurrence is logged and `ui_get_stats` lists it (`binding_errors`) with the node, target and message. It resumes when the node's bindings are edited or the source type changes. A missing resource, a despawned entity or a missing component pauses the record the same way (`SourceGone`) and resumes when the source is back.

**How change-only works.** The table groups records by source (one group per resource, one per `(entity, component)`). Each frame `ui_bind` does, in `PostUpdate` (see "Schedules"):

1. For each group, ask the ECS whether the source changed since this system last ran: a resource's `is_changed`, a component's `EntityRef::component_ticks` against the system's cursor. An unchanged group is skipped at the cost of one tick comparison, however many records it holds. Editor and agent edits stamp the tick (ECS spec), so authoring changes show up the same way.
2. For a changed group, each record reads its field through a **pre-parsed path** (parsed once when the record is created, never per frame) with `Reflect::reflect_path`, converts it, and compares with the record's `last` value. Only a different value reaches the setter.
3. The setter then applies its own equal-value check and dirty rules ("Invalidation strategy"), so a bound counter with a fixed size repaints with no layout, and one whose text length changes re-lays-out only its ancestor chain.
4. A newly created or edited record is applied once on the next pass whether or not its source changed.

The ECS read uses the registry's read guard once per pass. `ui_bind` takes the `UiTree` by `&mut`, so it is serialized with every other UI system (see Threading).

**Cost model.** O(groups) per frame plus O(records in changed groups). The bench rows are in "Performance": 10,000 unchanged bindings, 1,000 counters on one resource with a few changing, 10,000 counters with 10% changing, and **rebinding** 1,000 counters to new sources in one frame. The design choice that makes the last one cheap is that a binding is a record in a table, not a field or a system per node: rebinding rewrites a record and re-parses one path, with no structural change to the tree and no layout.

**Lists.** `ListSource` sets the list's count from the source's length and marks it as the *row source*. A row template's `Row { path }` records (instantiated per realized row, see "How the arena is shaped for binding", point 7) read element `index` of that list through the same pre-parsed path machinery, re-applied only when the list's source changed or the row's index changed. A list of 100,000 items costs the realized rows, a length read per change and nothing per unrealized item.

**What binding does not do:** no expressions or computed values (derive a field in a game system; a health ratio is a `hp_ratio` field, not `hp / max_hp` in markup), no two-way binding (a slider's `SliderChanged` event is read by a game system that writes the state), and no binding to a node style field other than `visible`.

### Markup and hot reload

**The format.** A `.ui` file is the reflection text form of a `UiDocument` (the serial spec's `%exitium-text` format, RON-shaped, comments allowed). There is no second parser: `exitium_serial` reads it, so errors carry spans, unknown fields follow the file-load rule (`UnknownFields::Warn`), and a field with an explicit `#[reflect(default)]` may be left out, which is why a file lists only what differs from the defaults.

```rust
#[derive(Reflect, Clone, PartialEq)]
pub struct UiDocument {
    #[reflect(default)] pub theme: Option<UiTheme>,      // applied only by a code mount with apply_theme (see below)
    #[reflect(default)] pub roots: Vec<UiNodeDesc>,      // every root has `root: Some(..)`; a theme file has none
}
// UiNodeDesc: name, class, tooltip, kind (WidgetKind data), style, root, bindings, children. All but `kind` are #[reflect(default)].
```

```
%exitium-text 1 root UiDocument
(
  roots: [(
    name: Some("hud"), kind: Panel, class: Some("card"),
    root: Some((anchor: TopRight, offset: (x: -16.0, y: 16.0), size: Fit, layer: 0)),
    style: (gap: 4.0, padding: Some((left: 8.0, top: 8.0, right: 8.0, bottom: 8.0))),
    children: [
      (name: Some("gold"), kind: NumLabel((value: 0, prefix: "Gold ")),
       bindings: [(target: Num, source: Resource(resource: "glutton::Wallet", path: "gold"))]),
      (name: Some("hp"), kind: ProgressBar((value: 1.0)), style: (width: Px(160.0), height: Px(10.0)),
       bindings: [(target: Progress, source: Resource(resource: "glutton::Player", path: "hp_ratio"))]),
    ],
  )],
)
```

(The example shows the shape; the serial spec owns the exact spelling of each value.)

- **Accepted roots.** The header's `root` type may be `UiDocument` or a bare `UiNodeDesc` (read as a one-root document), so a file written to match the assets spec's "text form of a `UiNodeDesc`" loads too. The UI plugin writes `UiDocument`.
- **References.** `Guid` fields (images, fonts) are strings, so the assets spec's `find_references` finds them in a `.ui` file with no UI code. Entity references are rejected (`Component` bindings and entity fields are code-only), because a file cannot name a live entity.
- **Validation after parse** (a failure is a parse error, nothing is built): names unique within the file; `RootConfig::layer` in range; class names are not checked (unknown names warn, see "Themes"); binding sources must be `Resource` or `Row`, and `Row` only inside a `VirtualList` template; widget limits (rich-text spans, `Grid.columns >= 1`).
- **A binary form** needs no UI code: `UiDocument` is reflected, so the binary serial format (#24) writes and reads it like any other type. Whether a shipped game packs `.ui` files that way is the export pipeline's choice (assets spec), not this crate's.

**Mounting.** A mount ties a markup asset to the nodes built from it:

```rust
impl UiTree {
    pub fn mount(&mut self, asset: Guid, doc: UiDocument, parent: Option<UiNodeId>, apply_theme: bool) -> Result<MountId, UiError>;
    pub fn reload(&mut self, asset: Guid, text: &str, generation: u32) -> ReloadOutcome;   // every mount of `asset`; see below
}
pub struct UiMount { id: MountId, asset: Guid, roots: Vec<UiNodeId>, doc: UiDocument /* the last good document */, apply_theme: bool, generation: u32 }
```

`mount` builds the document's roots (atomically: a duplicate name or a bad root builds nothing) and records the document, so a later reload can diff against it. With `apply_theme`, the document's `theme` replaces the tree's theme on mount and on each reload; a theme file is simply a document with a `theme` and no roots. The command `ui_build_tree` takes either a `tree: UiNodeDesc` or a `markup: Guid` (exactly one, `UiError::BuildSource` otherwise) and mounts with `apply_theme == false`, so an agent changes the theme with `ui_set_theme` and never through a file side effect. Undo of a markup build is `detach` of the mount's roots, which also removes the mount; redo is `restore`, which brings it back (`DetachedSubtree` records the mount, see "Stable ids and undo").

**The asset side.** `exitium_ui` has no edge to `exitium_assets` (C1). The plugin owns `UiMarkupStore`, a resource mapping a `.ui` asset's `Guid` to `{ text: Arc<str>, generation: u32, error: Option<UiParseError> }`. The **facade glue** fills it from `AssetEvent`s in `First`: a load stores generation 1, a reload stores the new text with `generation >= 2` (assets spec D12), and a failed reload (`AssetFailed { reload: true }`) is ignored by the store, which keeps the last good text. `ui_markup`, the plugin's system in set `UI_MARKUP` of `First`, finds the entries whose generation is newer than the mount's and calls `UiTree::reload`.

**Hot reload.** `reload(asset, text, generation)` has one swap point (assets spec D13: the swap happens in one place per frame; the UI's is `UI_MARKUP`, before `UI_INPUT`, so no input is processed against a half-applied tree):

1. **Parse.** `text` becomes a `UiDocument` or a `UiParseError { asset, line, column, message, generation }`.
2. **On error: the old tree stays untouched.** The error is stored on the store entry (cleared by the next good reload), logged once per generation, and listed by `ui_get_stats` (`markup: [{ asset, generation, ok, error }]`). The mount keeps its last good document and its nodes, so the game keeps running with the previous UI. Warnings (defaulted fields, unknown class names) are reported the same way and do not block the swap.
3. **Plan.** On success the new document is diffed against the mount's last good document, producing an edit list **without touching the tree**. A planning failure (a new name that collides with a node outside the mount) is treated like a parse error.
4. **Apply**, all or nothing, by the arena operations and the ordinary setters. The edit list is not undoable and is not recorded in the command journal: the file is the source of truth, and version control is its undo.
5. The mount stores the new document and generation.

**The diff** is by name, with subtree hashes to skip unchanged parts. Every `UiNodeDesc` carries a lazily computed 64-bit hash of its authored content and its children's hashes (std's `DefaultHasher::new()`, deterministic). `reconcile(old, new, live)`:

- If `hash(old) == hash(new)`, the whole subtree is skipped. A one-line edit therefore costs the nodes on its path and their siblings, not the document.
- If the widget kind or the name differs, the live subtree is replaced: the new subtree is built at the same parent and index and the old one is detached and dropped.
- Otherwise the node is kept (**its `UiNodeId` is stable**) and each authored field group (style, widget data, class, tooltip, root config, bindings) is merged **three-way**: it takes the new value only if the new document's value differs from the old document's value. If the file did not change a field, the live value survives, whatever game code, a binding or the user did to it.
- Children are paired by name when named, otherwise by order among the unnamed siblings. Unmatched old children are removed, unmatched new ones are built, and a changed order is applied by reordering.

**What survives a reload** (a surviving node is one that was kept above):

| State | Result |
|---|---|
| Keyboard focus, hover, press | kept on a surviving node. If the focused node is removed, focus clears with `FocusLost`; a removed pressed node cancels the press without a click. |
| Scroll offsets (`ScrollView`, `VirtualList`) | kept and re-clamped to the new content size. |
| Toggle `on`, slider `value`, tabs `selected`, text a game wrote | kept, unless the file changed that field (three-way rule). |
| Bound fields | kept. A binding added or edited by the file is applied once on the next `ui_bind` pass; an unchanged one is untouched. |
| Tweens in flight | kept on surviving nodes, dropped with a removed node. |
| Tooltip showing | hidden. |
| Virtual-list rows | kept when the row template's hash is unchanged; re-realized when the template changed. |
| `UiNodeId`s | stable for surviving nodes. An undo entry that names a replaced node fails with a stale-target error and the bus reports the undo as failed. |

**The budget.** A reload must finish within **100 ms** for a 10,000-node document on the min-spec runner, parse and swap together, **with the next layout and draw rebuild on top staying within the frame rows of "Performance"**. The planned costs: parse is O(text), reconcile is O(changed path), and a reload that changes nothing costs one parse and one hash compare of the root. If a swap takes longer than 100 ms it still completes (a half-applied reload is worse than a slow one) and `ui_get_stats` reports `last_swap_ms` so an agent sees it. The benches pin the rows below, and the 5% gate holds them.

**Layout consequences.** A reload changes the tree only through setters and arena operations, so layout is invalidated exactly as for any edit: a changed label re-lays-out its ancestor chain, a changed theme padding clears every cache. A reload that changes one style field costs the layout of one ancestor chain, not the document.

### The immediate-mode layer

For debug overlays and quick tools (frame stats, a cheat panel, a sim inspector) a game writes the UI as code that runs every frame, with no ids to hold and no tree to maintain. It is a **front end over the same tree**: the calls build a description of this frame's widgets, and the layer reconciles it into the retained tree with the code that hot reload uses, so layout, input, painting, themes and the 5% gate are shared.

```rust
pub struct UiImmediate { /* resource: previous and current frame descriptions, key table */ }
impl UiImmediate { pub fn frame(&mut self) -> ImUi<'_>; }
pub struct ImWindow { pub anchor: Anchor, pub offset: Vec2, pub width: Option<f32> }   // layer IMMEDIATE_LAYER = 900
impl ImUi<'_> {
    pub fn window(&mut self, id: &str, cfg: ImWindow, body: impl FnOnce(&mut ImUi<'_>));
    pub fn label(&mut self, text: &str);
    pub fn num(&mut self, label: &str, value: Num, format: NumFormat);
    pub fn button(&mut self, label: &str) -> bool;                                  // true when it was clicked this frame
    pub fn checkbox(&mut self, label: &str, value: &mut bool) -> bool;             // a Toggle plus a label; true on change
    pub fn slider(&mut self, label: &str, value: &mut f32, range: RangeInclusive<f32>) -> bool;
    pub fn progress(&mut self, label: &str, value: f32);
    pub fn separator(&mut self);
}
```

- **Identity.** A widget's key is its window id, its label (or its call order for `label` and `separator`) and its occurrence count. The same call at the same place is the same node frame after frame, so hover, press and focus survive.
- **Reconcile.** `UI_IMMEDIATE_END` (see "Schedules") diffs this frame's descriptions against the previous frame's with the subtree-hash skip, builds nodes for new keys, and removes nodes whose key was not called. A frame whose calls are identical to the last costs one hash compare per window and **no allocation**. The nodes live in **`Runtime` slots** (see "How the arena is shaped for binding"), under roots at layer 900, so they never disturb `restore`, never enter an undo journal and cannot be named by commands (`UiError::RuntimeNode`).
- **Input.** There is no input latency beyond the first frame a widget exists. `button` returns true if the node with its key received a `Click` in the current frame's event slice (the click happened in `First`, the node was built at the end of the previous frame). A `checkbox` or `slider` writes the node's live value into `*value` when a `ToggleChanged` or `SliderChanged` for it is in the slice, and otherwise pushes `*value` into the node, so the game's variable is the truth whenever the user is not touching it.
- **Windows** are flex columns on the theme's panel style and block input over their rectangles. They are placed by anchor and offset, not dragged, and there is no text input.
- **Frozen mode.** The calls come from `Update` systems, which a frozen editor does not run (see "Schedules"); `UI_IMMEDIATE_END` still runs and removes the nodes, so an overlay disappears while the simulation is frozen and returns when it runs. The layer is for a running game.
- **Agents** see immediate nodes in `ui_get_tree` (flagged `runtime: true`) and can read and hit-test them; they cannot edit them, because the code that runs next frame would undo the edit. This is not an editor feature, so rule 1 asks for no tool beyond that.

Budget (see "Performance"): 200 unchanged immediate widgets cost at most 100 us per frame with zero allocations.

### Schedules, and what runs while the simulation is frozen

The editor's Edit mode sets `SimulationMode::Frozen`, which skips the fixed-tick loop and `Update` but **still runs `First`, `PostUpdate`, `Extract` and `Last`** (SP3, #139; `app-platform.md` AM3, the X3 row of `editor-mcp-mvp.md`). Authoring a UI happens in exactly that mode: an agent or a human edits the tree, a bound resource, or a `.ui` file, and the result must appear without pressing Play. So **every UI system the engine needs while authoring runs in `First`, `PostUpdate` or `Extract`, and the plugin registers nothing into `Update` or `FixedUpdate`.**

| System | Set | Schedule | Order | Runs when frozen |
|---|---|---|---|---|
| `ui_markup` (apply `.ui` reloads) | `UI_MARKUP` | `First` | after the facade's asset-event system, before `UI_INPUT` | yes |
| `ui_input` | `UI_INPUT` | `First` | after platform input, before every `FixedUpdate` tick | yes |
| *game code* (builds and edits the tree, reads `UiCapture` and events) | | `Update`, `FixedUpdate` | | **no** |
| `ui_bind` | `UI_BIND` | `PostUpdate` | first of the UI systems | yes |
| `ui_lists` (realize virtual-list rows, send `UiListRow`) | `UI_LISTS` | `PostUpdate` | after `UI_BIND` | yes |
| `ui_immediate_end` | `UI_IMMEDIATE_END` | `PostUpdate` | after `UI_LISTS` | yes |
| `ui_animate` (`advance(dt)`) | `UI_ANIMATE` | `PostUpdate` | after `UI_IMMEDIATE_END` | yes |
| **`Headless` driver** (`layout`, `build_draw_list` with the fixed-advance measurer) | `UI_DRIVER` | **`PostUpdate`** | last of the UI systems | yes |
| **`External` glue** (the facade: measurer, `layout`, `build_draw_list`, conversion) | `EXTRACT` | `Extract` | after transform propagation | yes |

- **Why the Headless driver moved.** The Jam text ran it at the end of `Update`. A frozen editor skips `Update`, so a headless tool or test that expects a laid-out tree after an edit would see stale rectangles. In `PostUpdate` the same tree is laid out in Edit mode, in Play, and in a game with no renderer.
- **Edits made while frozen.** A command or an `ecs_set_resource_field` between frames is picked up by the next `ui_bind`; a `.ui` edit by the next `ui_markup`; both appear in the next `Extract`. Nothing waits for `Update`.
- **`ui_animate` delta.** It passes the app clock's real (unscaled) frame delta clamped to 0.1 s, so transitions keep running while the simulation is paused or frozen. The resource name is `app-platform.md`'s.
- **Order is a contract.** The bind pass comes before list realization (a bound count sets the rows), both before layout (they change nodes), and the animation step before the driver (the draw list shows this frame's tween values).

**Cross-spec requirements this section records:**

| Spec | Row |
|---|---|
| `app-platform.md` AM3 (SP3, #139) | `SimulationMode::Frozen` runs `First`, `PostUpdate`, `Extract`, `Last` and skips the tick loop and `Update`. This spec's `UI_MARKUP`, `UI_INPUT`, `UI_BIND`, `UI_LISTS`, `UI_IMMEDIATE_END`, `UI_ANIMATE`, `UI_DRIVER` and the `External` glue are placed to satisfy it, and U11, U17, U22, U23, U25 and U26 carry a Frozen-mode test. |
| `assets-scene-export.md` (SP4, PR #166) | D12 (a bad reload keeps the good value) and D13 (one swap point) are met by "Markup and hot reload"; `.ui` is a `Text` asset whose root type may be `UiDocument` or `UiNodeDesc`. |
| `render-v1.md` | `SpriteDraw::uv_rect` (9-slice and image sub-rectangles) and the per-item `clip` (scroll views) exist and are used; the UI emits `Text` items per rich-text span because the render spec has no spans. |
| `editor-mcp-mvp.md` AM6 | Closed by "Command and query surface". |
| `serial-text-format.md` | `UiDocument` uses the file-load rules: spans in errors, `UnknownFields::Warn`, defaults from explicit `#[reflect(default)]`. |
| `mvp-roadmap.md` | The UI full lane is Stage 5 and runs beside Stage 4 (C6: `Blocked by` intake 3); the editor edge PR is C5; the facade item F2 cannot overlap a `exitium_ui` issue (R3). |

### Threading

`UiTree` is plain owned data (`Send + Sync`, no interior mutability) and lives in a resource. Every UI system takes the resource by `&mut` (or `&` for read-only), so the scheduler serializes them. Layout, input, painting, binding and reload are single-threaded, as the app's executor is. Roots are independent layout subtrees with no shared cache, which allows a later parallel layout (splitting the arena by root) without a layout change; that is not designed here. The crate uses no threads, no clocks and no file I/O, so it builds and runs unchanged on wasm32.

### Stages

- **Stage 1 (`exitium_core` and `taffy` only).** The layout core: arena (with the two slot classes), tree, styles, widget data types, theme types, desc, layout of the five Jam widgets, painting, the render-facing mapping helpers (`UiRenderConfig` and the conversions the glue calls), input, plus unit tests and benches. Nothing in Stage 1 names the ECS, the app, the platform or the renderer. The coupling points are three plain-data seams: `UiMeasure` (in), `UiInputFrame` (in), `UiDrawList` (out). **Stage 1 builds the final arena shape** ("How the arena is shaped for binding"), so nothing in Stage 5 reshapes it.
- **Stage 2 (`plugin` module).** A `UiPlugin` that registers, using the app spec's plugin mechanism (#106; exact `App`/`Plugin` signatures are an open dependency):
  - **Resources:** `UiTree`, `UiViewport` (synced from the window), `UiCapture`, `UiDrawList`, `UiRenderConfig`, `UiInputFrame`.
  - **Systems into `Startup`:** insert the resources with defaults. Game code builds its UI in its own `Startup` systems from Rust (builder or `UiNodeDesc`).
  - **System into the input position:** `ui_input`, in set `UI_INPUT` of the `First` schedule, consuming the `UiInputFrame` resource that the facade glue fills just before it. Game systems then read `UiTree::events()` and `UiCapture` and edit the tree.
  - **Layout and draw output are driven by the facade glue**, in `Extract`, because they need the renderer's text measurer (see "Render handoff"). `UiPlugin { driver: UiDriver }` has two drivers: `External` (default; the facade runs `layout` and `build_draw_list`) and `Headless`, where the plugin adds a system in set `UI_DRIVER` of **`PostUpdate`** that runs both with a fixed-advance measurer (see "Schedules"). `Headless` is for tests, tools and games with no renderer.
  - **Nothing into `FixedUpdate` or `Update`.** Gameplay that runs at the fixed rate may call tree setters; the dirty flags make that cheap and the next layout picks it up.
  - **Reflection derives** on the data types.
- **Stage 3 (commands and queries).** U13 registers the seven commands and six queries with the command bus (see "Command and query surface"). It is `Blocked by` CB9.
- **Stage 5 (the UI full lane, 14 serial PRs, U14 to U27).** Everything marked "full" in this spec: the rest of the widgets, themes and transitions, 9-slice and clipping, change-only data binding, markup and hot reload, the immediate-mode layer, the plugin systems they need (`UI_MARKUP`, `UI_BIND`, `UI_LISTS`, `UI_IMMEDIATE_END`, `UI_ANIMATE`) and the 10,000-node and counter-rebinding benches. The lane starts beside Stage 4 and ends before the facade glue (the roadmap's R3). The facade item F2 adds the glue for the new draw data and the asset events. The edge `exitium_editor -> exitium_ui` is the roadmap's C5 PR, after the lane.
- **Crate edges added in Stage 2 and 5** are listed under "Wiring". `exitium_ui` does **not** depend on `exitium_render` or `exitium_assets`; the facade depends on all three.

## MCP / editor surface

Agent parity (rule 1): every UI capability is a command or a query registered with the command bus, with a reflected payload, so the generic agent and editor tooling that `exitium_mcp` builds from the registry exposes it with no UI-specific tool or panel. **This spec files no issue in `exitium_mcp` or `exitium_editor`;** it lists what `exitium_ui` must register. Mutation goes through the bus (rule 3) and schemas come from the reflected data types (rule 2, JSON Schema from `TypeInfo`, CB9).

### Command and query surface

This section implements AM6 of `editor-mcp-mvp.md`. **`exitium_ui` registers all seven commands and all six queries itself**, from `UiPlugin::build`, with `register_command::<C>(world)` and `register_query::<Q>(world)`. The facade does no registration, so the "or the facade" alternative of the Jam text's open question 7 is closed. Each type implements `Command` or `Query` (`Reflect + FromReflect + Clone + Send`), with `NAME` set to the tool name; undo state lives in `#[reflect(skip)]` fields. Node ids travel as the text form `"{index}v{generation}"`, like `Entity`; a node may be addressed by its unique name instead. **U13 is `Blocked by` CB9** (the schemas and the registry), and registers nothing before it.

| Command (`NAME`) | Meaning | Undo |
|---|---|---|
| `ui_build_tree { parent: Option<Node>, tree: Option<UiNodeDesc>, markup: Option<Guid> }` | a whole subtree or a new root in one step, from a description **or** from a `.ui` asset (exactly one, else `UiError::BuildSource`); a markup build also records the mount | **one command**: `undo` is `detach`, and the second `apply` (redo) is `restore` of the `DetachedSubtree` that `undo` stored in its skipped field, so ids are identical after undo and redo |
| `ui_add_node { parent, index, kind, name? }` | one node | `detach` |
| `ui_remove_node { node }` | remove a subtree | `restore` the `DetachedSubtree` |
| `ui_reparent { node, new_parent, index }` | move | reparent back |
| `ui_set_field { node, path, value }` | any reflected field of the node's shallow `UiNodeDesc`: style, widget data and live widget values (toggle `on`, slider `value`, tabs `selected`, scroll `offset`), class, tooltip, root config, name, `bindings` | previous value via `clone_boxed` |
| `ui_set_theme { path, value }` | any field of `UiTheme`, including classes | previous value |
| `ui_click { node }` | debug action: queues a `Click` (or the `ToggleChanged` or `TabChanged` a real click would produce) as if the pointer had pressed and released on the node; the events appear in the next frame's `UiTree::events()` | **`undo_kind` returns `UndoKind::NotUndoable`**: recorded in the journal only |

Commands that name a node reject a `Runtime` node (list row, tooltip, immediate-layer node) with `UiError::RuntimeNode`. `ui_set_field` is one generic command instead of one per field, built on the reflection path machinery (`apply`, `reflect_path_mut`) over the node's shallow description, so a new widget or style field is editable by agents and the inspector the day it is added. Structural edits stay explicit commands because they need ids and undo data. The seven names are the tools `exitium_mcp` derives; the facade's parity test (C3) lists them.

Read-only queries (each is a `Query`, so the bus checks it changes no observable ECS state):

| Query (`NAME`) | Result |
|---|---|
| `ui_get_tree { root?, depth?, fresh }` | the `UiNodeDesc` plus each node's id, computed rectangle, interaction flags and `runtime` flag |
| `ui_get_node { node }` | one node's shallow description, rectangle and flags |
| `ui_find { name }` | the node id for a name |
| `ui_get_theme` | the `UiTheme` |
| `ui_hit_test { x, y }` | the node under a UI-logical point |
| `ui_get_stats` | `LayoutStats`, node counts (authored and runtime), revisions, `text_changes`, active tweens, binding counts and `binding_errors`, `warnings`, and `markup: [{ asset, generation, ok, error, last_swap_ms }]` |

Computed rectangles are as of the last layout pass: a response carries `layout_revision` and `stale: true` when edits since then have not been laid out. **`fresh`** asks `ui_get_tree` to run a layout pass first, and it may do that **only through the non-stamping query path**: `QueryCx` allows a cache refresh only through `bypass_change_detection`, so the query takes `UiTree` and the plugin's `UiMeasurer` resource without stamping a change tick on either, runs `layout` (never `build_draw_list`: the draw list is the glue's output), and returns `stale: false`. A fresh query may advance `layout_revision` (a cache refresh) but changes no authored data and no ECS change tick. The measurer is the boxed `UiMeasure` the plugin keeps in `UiMeasurer`: `Headless` installs the fixed-advance one, the facade installs the renderer-backed one (F1), and with neither the query falls back to the fixed-advance measurer and says so (`measurer: "fallback"`) rather than failing. Verifying how the UI looks is the screenshot capability owned by the render and editor specs, not this one.

**Open question 7 is answered:** a non-undoable action is `undo_kind() == NotUndoable` (`ui_click`); a bulk command is one command with its own undo state (`ui_build_tree`: `detach` and `restore`); registration is from the crate. **The pointer in open question 5 to #110's ordering is recorded as settled:** `app-platform.md` places `UI_INPUT` in `First` before every `FixedUpdate` tick and `EXTRACT` after transform propagation, and this spec's "Schedules" fixes the rest.

### What the editor needs from the UI

Listed so `exitium_mcp` and the editor can build it generically; nothing here is a `exitium_ui` deliverable beyond the registrations above:

- the node tree for a hierarchy view (`ui_get_tree`), with reparent by drag (`ui_reparent`) and delete (`ui_remove_node`);
- an inspector for the selected node generated from `UiNodeDesc` reflection (ranges, tooltips and `hidden` honored), writing through `ui_set_field`, and a theme editor over `ui_set_theme`, including class editing;
- a selection outline drawn from a node's `rect` (`ui_get_node`) by the editor overlay, and node picking from `ui_hit_test`;
- a palette of the widgets (the `WidgetKind` variants, discoverable through reflection);
- viewport mouse input translated to UI-logical pixels and fed to `process_input` as in a built game;
- the **UI editor** view of Stage 5 (editor extras), which edits a `.ui` file by `ui_build_tree { markup }` and the field commands, and relies on hot reload to show saved edits. It needs the `exitium_editor -> exitium_ui` edge (C5), which is not part of this spec.

Whether the editor reads the `UiTree` directly (which needs that edge) or only through the query channel is the editor's choice; this spec needs neither.

## Performance

Budgets are p50, single thread, on the min-spec reference runner (4 cores, 8 GB). Benches live in `exitium_ui/benches/` (criterion, per #18) and are held by the 5% regression gate once baselined; budgets may be re-baselined by the first bench PR with a note to the owner. The 10,000-node tree is 100 row containers of 99 fixed-size leaves each, the shape of a Glutton resource screen. The **full-widget 10,000-node tree** (U27) is the same count with the full widget mix: 40% labels and number labels, 20% panels and grids, 15% buttons and toggles, 10% images (a tenth of them 9-slice), 5% progress bars and sliders, 5% rich text, and 5% inside scroll views, plus one virtual list.

| Operation | Budget |
|---|---|
| Full cold layout, 10,000 nodes (flex, fixed-size leaves) | <= 6 ms |
| Full cold layout, full-widget 10,000-node tree | <= 8 ms |
| Relayout after one leaf resizes, 10,000 nodes | <= 100 us |
| Relayout with nothing dirty | <= 1 us |
| `set_text`, equal text (no-op) | <= 30 ns |
| `set_text` on a fixed-size label, warm buffer, no allocation | <= 100 ns |
| `set_num`, equal `Num` (no-op) / changed `Num`, equal formatted text | <= 40 ns / <= 150 ns |
| 1,000 fixed-size counter updates in one frame (setters only, no draw) | <= 100 us |
| Draw list rebuild, 10,000 nodes, warm buffers | <= 1 ms |
| Draw list rebuild, full-widget 10,000-node tree | <= 1.5 ms |
| Draw list rebuild, 500 nodes | <= 60 us |
| Hit test, 200 interactive entries | <= 2 us |
| `process_input`, 10,000-node tree, 200 interactive entries | <= 5 us |
| Grid layout, 1,000 cells, cold | <= 300 us |
| Rich text, 200 nodes cold layout (fixed-advance measurer) | <= 1 ms |
| Scroll offset change, scroll view with 500 descendants (rectangle walk, hit list, no layout) | <= 50 us |
| Virtual list, 100,000 items, one scrolled frame (about 40 realized rows, 2 recycled; warm) | <= 150 us, 0 allocations |
| `image_changed`, 100 users among 10,000 nodes / 1 user | <= 10 us / <= 200 ns |
| `font_changed`, 10,000 nodes | <= 300 us |
| 512 active tweens, one `advance` | <= 40 us |
| **`ui_bind`, 10,000 records on one unchanged resource** | <= 1 us |
| `ui_bind`, 10,000 records on 10,000 components, none changed | <= 150 us |
| `ui_bind`, 1,000 counters on one resource, resource changed, 10 values differ | <= 80 us |
| `ui_bind`, 10,000 counters, 10% change per frame (read, compare, format, setter; **excludes text shaping**) | <= 1.5 ms |
| **Counter rebinding**: 1,000 counters rewired to new sources in one frame (record rewrite, path parse, first apply; no layout) | <= 500 us |
| Immediate layer, 200 unchanged widgets (calls and reconcile), warm | <= 100 us, 0 allocations |
| **`.ui` reload swap, 10,000-node document, one node changed** (parse, plan, apply, **hard budget**) | <= 100 ms |
| Reconcile only, 10,000 nodes: unchanged document / one node changed / every node's style changed | <= 5 us / <= 1 ms / <= 20 ms |
| Hot-path allocations in a frame with no structural change and warm buffers | 0 |
| UI-side CPU per frame, 10,000 nodes, 1,000 counters changing (setters, layout, draw rebuild; **excludes text shaping**) | <= 2 ms |
| **Text shaping for changed labels** (renderer-owned, joint row with #107's `text/*` benchmarks): one changed short label (about 8 characters), shaped-cache miss | <= 20 us |
| **UI frame with shaping**: 100 labels change to new strings in one frame, 10,000 nodes (UI-side cost plus shaping and draw conversion) | <= 4 ms |
| Text batches per frame with 200 buttons in one root (also with rich text and scroll views) | 1 per root layer |
| Memory per node (hot, cold and layout, excluding strings) | <= 1.25 KiB |
| `NodeHot` / `NodeCold` | <= 256 B / <= 64 B |
| `UiDrawItem` size | <= 112 B |

The "excludes text shaping" rows are deliberate and honest: a counter that shows a new string every frame costs a shaped-buffer cache miss in the renderer's cache (keyed by text), which dwarfs the UI's own cost. 1,000 *distinct new strings* per frame would be about 20 ms at the ceiling above, so it is **not** within the frame budget, and the Phase 3 exit ("heavy live numbers at full frame rate") is covered by the shaping rows, not by the setter rows. The mitigations that keep real screens inside them: format to the displayed precision so a 1,000 Hz counter changes its text only when a visible digit changes (`set_num` and `set_text` with equal text are free, and `NumLabel` compares the formatted text), and the stats query reports `text_changes` per frame so a benchmark or an agent can see the rate. If measurement shows the shaping ceiling is not reachable for numerals, the options (a glyph-quad path for digits, a pre-shaped digit atlas) belong to #107 and are open question 2.

Evidence for the layout rows: a throwaway probe of taffy 0.15.1 (flexbox only, rounding off, native release build on a developer machine, not min-spec) measured about 4.4 ms for the cold 10,000-node pass and about 35 us for one dirty leaf, so the budgets keep a margin for min-spec and for the arena adapter's extra indirection. The binding, reload and list rows are **proposals** from the cost models in this spec (a tick comparison, a path read and a compare per record; a hash compare per skipped subtree; a handful of rows realized per frame); the first bench PR of the issue that builds each of them replaces them with measured numbers and reports any it cannot meet to the owner. The 100 ms reload swap is the one budget that is a requirement rather than a forecast.

Complexity: layout O(dirty ancestor chain + dirty subtrees); paint O(visible nodes) per rebuild and zero when clean; hit test O(interactive entries); `set_*` O(1) amortized; bind pass O(groups + records in changed groups); `image_changed` O(users); reload O(changed path + parse).

Where the budgets are tight, the first answer is the data layout (hot/cold split, flat hit list, shared text buffer, two paint planes, records outside the arena) and the second is a smaller rebuild (subtree draw caching), never a bypass of the command bus or the dirty flags.

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
- **Stage 2:** headless app tests that insert the plugin (`Headless` driver), write a `UiInputFrame` into its resource and assert events and `UiCapture` before a game system runs, including the `FixedUpdate` ordering case above; command tests for apply, undo and redo of every command; a reflection round trip of `UiNodeDesc` through the text format. **Facade (hotspot item):** the measurer and draw conversion against a headless renderer, and the 200-button text-batch count.
- **Arena classes (U1):** a runtime node allocated and freed between a `detach` and its undo does not break `restore`; two free lists never mix; a `detach` of a list with realized rows restores without rows and re-realizes them on the next pass.
- **Widgets (Stage 5):** `Grid` goldens (columns, gaps, uneven row heights); scroll view content size, offset clamping and hit-test clipping (a button scrolled out is not hoverable); tabs show and hit only the selected page; toggle, slider (drag outside the rectangle, `step`, `nav`) and tab events; `NumLabel` equal-value and equal-text no-ops; rich text spans, wrapping, escapes and the unknown-tag warning; borders, opacity and 9-slice goldens (nine `Image` items, `uv`, shrinking borders, unknown size draws nothing); the clip table on all three item kinds.
- **Themes and transitions:** the resolution order (override, state patch, class, widget theme, general), an unknown class warning, a theme change re-resolving classes, a node override replacing hover colors; tweens with fixed `advance` steps, restart from the shown value, the 512 cap snapping, and a theme with no transition being instant.
- **Virtual list:** 100,000 items realize only the window, recycle rows on scroll with zero allocation, `row_of` maps any node in a row to `(list, index)`, `UiListRow` events carry the new index, and a list inside a modal root works.
- **Asset changes:** `image_changed` invalidates exactly the users of one image among many, once each, and a same-size reload needs no call; `font_changed` clears only text nodes of that font.
- **Binding:** an unchanged source costs a tick comparison (assert no read), a changed source applies only differing records, a counter with a fixed size repaints with no layout, a type mismatch is a reported error that resumes after an edit, a despawned entity pauses and resumes, a manual `ui_set_field` on a bound field stays until the source changes, list `Row` sources follow scroll and source changes, and removing a bound node leaves no record (undo brings it back).
- **Markup:** `describe(build(doc))` round trips through the text format for every widget; a `UiNodeDesc`-rooted file loads; a binary round trip needs no UI code; every parse and validation failure builds nothing; `ui_build_tree { markup }` undo and redo keep ids and the mount.
- **Hot reload:** a parse error keeps the old tree and reports line and column in `ui_get_stats`; a later good reload clears it; the state table (focus, scroll, toggle, slider, tabs, bound text, tweens, list rows) row by row; the three-way merge (a game-written value survives unless the file changed that field); a rename replaces the node with a new id; a collision with an outside name is reported and changes nothing; the unchanged-document hash skip performs no setter call.
- **Immediate layer:** identical frames allocate nothing, `button`, `checkbox` and `slider` results, a removed call removes the node, nodes are `Runtime` and commands reject them.
- **Frozen mode (U11, U17, U22 to U26):** a headless app with `SimulationMode::Frozen` still runs `ui_markup`, `ui_input`, `ui_bind`, `ui_lists`, `ui_immediate_end`, `ui_animate` and the `Headless` driver; an edit of a bound resource, a `.ui` reload and a command each lay out and rebuild the draw list with `Update` skipped; a schedule-graph test asserts the plugin registers no system in `Update` or `FixedUpdate`.
- **Commands and queries (U13):** every command applies, undoes and redoes with identical ids (`ui_build_tree` by `detach` and `restore`); `ui_click` is `NotUndoable` and journalled; `ui_get_tree { fresh }` lays out without moving the `UiTree` or `UiMeasurer` change tick; the bus's read-only check passes for every query; a registry test lists exactly the seven command names and six query names; runtime nodes are rejected.

## Open questions

1. **Renderer guarantees (needs #107).** The glue relies on: equal-`layer` items draw in `depth` order; all text of a layer shares a text batch when no sprite of that layer sorts between; `TextStyle.max_width` with `wrap: None` is still used as the alignment width, and an aligned label hits the same shaped-buffer cache entry in measure and draw (the cache key includes `max_width`, and the measure offers the available width while the draw passes the final one, so a second shaping can occur on the first draw after a change); `SpriteDraw::uv_rect` and the per-item `clip` behave as in the render spec (used by 9-slice and scroll views). If #107 cannot guarantee these, the glue falls back to aligning with `anchor` and may merge the two planes, at the cost of more text batches.
2. **Shaping cost of changing numerals (needs #107).** The shaping rows in Performance are proposals for #107's `text/*` benchmarks. If numerals cannot meet them, a digit fast path is a render-side feature (not a UI one).
3. **Platform input shape.** Resolved by `app-platform.md`: the facade glue converts the platform `InputState` (frame view: `cursor_logical`, mouse and key state, `scroll_lines`; no touch in the MVP) into the `UiInputFrame` resource. The arrow keys that feed `nav` are ordinary key state.
4. **Consumption.** Capture flags are the contract. Resolved by `app-platform.md`: the platform never masks its input state.
5. **System ordering. Settled.** `app-platform.md` places `UI_INPUT` in `First`, after platform input and before every `FixedUpdate` tick, and facade glue for layout and draw output in the `EXTRACT` set of `Extract`, after transform propagation; "Schedules" fixes the rest and the Frozen-mode behavior (SP3, #139). The Jam text's pointer to #110's ordering is closed by those two documents.
6. **Draw caching.** Is a full rebuild per paint change good enough at the full-widget 10,000-node screen? The first bench PR of U18 answers it; subtree caching keyed on per-node revisions is the next step if not.
7. **Command bus details. Answered (AM6).** `ui_click` is `UndoKind::NotUndoable` (journalled only); `ui_build_tree` is one command whose undo is `detach` and whose redo is `restore`; U13 registers the seven commands and six queries from the crate with `register_command` and `register_query` and is `Blocked by` CB9; the facade-registration alternative is closed; `ui_get_tree { fresh }` runs layout only through the non-stamping query path.
8. **Persistence. Answered.** A UI is saved as a `.ui` `Text` asset holding a `UiDocument` (a bare `UiNodeDesc` also loads); the game's `Startup` or `ui_build_tree { markup }` mounts it; scenes carry no UI nodes (the editor's Stop rebuilds from the file, ED6). **Cross-spec note for the architect:** `assets-scene-export.md` (line 77 and UA1) calls the file's content a `UiNodeDesc`; this spec accepts that root type, so no amendment blocks anything, but the sentence should say `UiDocument`. The one-sentence fix to `assets-scene-export.md` is outside this PR's footprint; the orchestrator files it as a follow-up.
9. **Clipping. Answered.** The per-item clip table (see "Draw data") and scroll views.
10. **Image identity.** `Guid` is used because it is stable and already in core; the facade resolves it through the asset and texture stores and decides what an unloaded image draws (nothing, in this spec). `image_changed` and `font_changed` carry reloads in.
11. **Text input. Open, owner decision.** A text field needs a text-event path (characters, IME, repeat) that `app-platform.md` does not have. The full UI leaves it out (Non-goals); `UiCapture::text_input` is reserved. If the owner wants it in the MVP, it needs an `app-platform.md` amendment first, then a lane issue after U20.
12. **Variable-height rows and spanning grid cells.** Not planned; `VirtualList` rows are fixed-extent and `Grid` cells are single. Revisit if a game needs a chat log or a spanning layout.
13. **Rounded corners. Open, owner decision.** Plain quads only; ornate or rounded frames are 9-slice images. A rounded-rectangle shader is a render-side feature.
14. **Reload swap budget. Owner to confirm.** 100 ms for a 10,000-node document (SP4's suggestion). Parse cost depends on `exitium_serial`; if its text reader cannot parse 10,000 nodes in about 60 ms on min-spec, the budget is met by splitting a large screen into several `.ui` files, not by loosening the number.
15. **Reload and undo.** A hot reload is not a command, so it is not undoable, and a node replaced by a changed kind or name gets a new id (a stale undo entry fails safely). The alternative, journalling reloads, would make a file edit pollute the editor's undo stack and is not proposed.
16. **Theme from a file.** A document's `theme` is applied only by a code `mount` with `apply_theme`, never by `ui_build_tree`. If agents need to apply a theme file through MCP, add a `theme: Option<Guid>` field to `ui_set_theme` (a payload change, not a new command).

## Implementation issues

Filed by the architect after this spec merges, with the `file-issue` skill (an existing issue that carries `needs-spec` for this work has the label lifted once this spec merges, instead of a duplicate being filed). Every issue edits at most one crate directory, fits one PR (about 600 changed lines or fewer; each split below was made to stay under that), has acceptance criteria, `Blocked by` lines and the full labelled footprint. Labels: `area:ui` and `role:impl-ui`, except F1 and F2 (`area:ui`, role per the facade owner). Because all `exitium_ui` issues edit `crates/exitium_ui/` (one in-flight issue per crate), they are a chain; U1 to U13 and F1 are the Glutton UI critical path, and U14 to U27 and F2 are the Stage 5 lane. `Cargo.toml`, `Cargo.lock` and the facade are hotspots owned by the intake PRs and the facade items.

**Stage 1** (no ECS). Each is `Blocked by` intake 1 (the Stage 1 dependency and crate-graph intake PR) and the issue before it. Every criterion list also includes `cargo xtask ci all` passing, the wasm build, and the benchmarks named for it.

- **U1 `feat(ui): node arena, tree structure and stable ids`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `NodeArena` with **two slot classes (`Authored`, `Runtime`), each with its own LIFO free list**, retirement and `restore` (a `Runtime` allocation between a `detach` and its undo never disturbs `restore`; the arena-classes tests from the Test plan); `UiTree` add, remove (`detach`), reparent with cycle check, names and `find`, roots ordered by layer; `restore` rules exactly as in "Stable ids and undo"; the arena fuzz and restore tests from the Test plan. Blocked by: intake 1.
- **U2 `feat(ui): widget data, UiStyle and UiTheme types`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `WidgetKind` and the widget data structs, `UiStyle`, `Dim`, `Edges`, the flex and pointer enums, `RootConfig` (`layer` in `-1000..=999`, `modal`) and `Anchor`, `UiTheme` with the defaults above, all `Clone + PartialEq + Debug + Default` where meaningful; the final arena shape ("How the arena is shaped for binding"): `NodeHot` and `NodeCold` with `bind_head` (an empty slot until U23), and `size_of` pins for `NodeHot` (<= 256 B) and `NodeCold` (<= 64 B) and the per-node memory budget; no behavior beyond constructors and validation (progress clamp, layer range). Blocked by: intake 1, U1.
- **U3 `feat(ui): UiNodeDesc, builder and setters`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiNodeDesc` (with the `class`, `tooltip` and `bindings` fields present and empty-defaulted until their issues) with `build` and `describe` (round-trip test, atomic duplicate-name rejection); the setters `set_text`, `set_progress`, `set_enabled`, `set_style`, `set_theme`, and `set_image` with the `image_nodes` index (Guid to nodes), with the equal-value no-op, no allocation on a warm buffer, and the dirty marking (a stub `invalidate_from` and `paint_revision` that U5 fills in); the setter benchmarks. Blocked by: intake 1, U2.
- **U4 `feat(ui): taffy style conversion and arena adapter`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiStyle` to `taffy::Style` conversion (theme padding applied per widget kind), the taffy tree traits implemented over the arena and `layout` array, `UiMeasure`, `TextMeasure` and a fixed-advance test measurer, dispatch for the five widgets, layout of a single root; flex golden tests (row, column, gap, padding, grow and shrink, percent, min and max, nested, leaves measured). Blocked by: intake 1, U3.
- **U5 `feat(ui): layout pass, absolute rectangles and invalidation`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiTree::layout` over several roots in layer order, absolute `rect`s, `LayoutStats`, `invalidate_from` and the cache-empty invariant wired into every row of the invalidation table; invalidation tests asserting `LayoutStats`; `image_changed(guid)` (relayout only the nodes using that image, through the `image_nodes` index, O(users)) and `font_changed(Option<Guid>)`, with their tests from "Asset changes"; the 10,000-node cold, one-leaf and clean layout benchmarks and the `image_changed` and `font_changed` benchmarks. Blocked by: intake 1, U4.
- **U6 `feat(ui): viewport, scaling and anchoring`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiViewport`, `ScaleMode`, `ui_scale` and UI-logical viewport math, root sizes (`Fill`, `Fixed`, `Fit`), anchor and pivot origin, the viewport-change invalidation rows; anchoring and scaling golden tests; the tests that a `ui_scale`-only change lays out nothing. Blocked by: intake 1, U5.
- **U7 `feat(ui): draw list and theme resolution`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiDrawList`, `UiDrawItem`, `TextRun`, `build_draw_list(measure, out)` for the five Stage 1 widgets (panel, label, image, button, progress bar) and button states, theme and override resolution, physical-pixel snapping, the two planes per root, `revision`; the painting golden tests; the draw benchmarks and the `UiDrawItem` size pin. Blocked by: intake 1, U6.
- **U8 `feat(ui): render-facing mapping helpers`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiRenderConfig` (`layer_base`, `mask`), pure helpers for render layer and depth from an item, the physical-to-Screen conversion and the text scale (`ui_scale / dpi_scale`), written against #107's `Space::Screen`; the render-mapping tests including the snapped round trip. Blocked by: intake 1, U7, #107 merged.
- **U9 `feat(ui): hit test, hover, press and click`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: the hit list built in the layout post-pass, `UiInputFrame`, `UiEvent`, `UiCapture`, rules 1 to 4, pointer capture as in rule 6 (all three origin cases, including the release frame), removal clearing (rule 7) for hover and pressed; the matching input tests and the hit-test benchmark. Blocked by: intake 1, U8.
- **U10 `feat(ui): keyboard focus and keyboard capture`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: rule 5 and the `keyboard` and `text_input` capture rules; a mouse press clears focus and gives none; the "click then Space" test and the Tab, shift-Tab, activate, cancel and removal tests; the `process_input` benchmark. Blocked by: intake 1, U9.

**Stage 2** (ECS, app, reflect, command bus). Each is `Blocked by` intake 2, which adds every edge below, so none of them touches `Cargo.toml` or `Cargo.lock`.

- **U11 `feat(ui): UiPlugin, resources and ui_input`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: the `plugin` module with the resources (including a `UiInputFrame` resource and the `UiMeasurer` resource), the `ui_input` system **in set `UI_INPUT` of `First`**, which reads that resource and stores `UiCapture` and reads no platform type, the `Headless` driver as a system **in set `UI_DRIVER` of `PostUpdate`** (not `Update`: `SimulationMode::Frozen` skips `Update`, SP3 #139), and headless app tests that write a `UiInputFrame` into the resource (events and capture before a game system, the `FixedUpdate` ordering case, and a `Frozen`-mode test that the input system and the driver still run and a click and a layout still happen). The plugin registers no system in `Update` or `FixedUpdate`; a schedule-graph test asserts it. Blocked by: intake 2, U10, the app and plugin issues from #106 (A1, A2).
- **U12 `feat(ui): reflection derives and UiNodeDesc text round trip`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `Reflect` on every data type, hand-written `UiNodeId` reflection with the `"{index}v{generation}"` text form, a `UiNodeDesc` round trip through the text format in a test (`exitium_serial`, which U24 later makes a normal dependency under the `plugin` feature). Blocked by: intake 2, U11, the reflect issues and the text-serialization issues (#23).
- **U13 `feat(ui): UI commands and queries with undo`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance (AM6 of `editor-mcp-mvp.md`): the **7 commands and 6 queries** of "Command and query surface" (`ui_build_tree`, `ui_add_node`, `ui_remove_node`, `ui_reparent`, `ui_set_field`, `ui_set_theme`, `ui_click`; `ui_get_tree`, `ui_get_node`, `ui_find`, `ui_get_theme`, `ui_hit_test`, `ui_get_stats`) registered with `register_command` and `register_query` **from `UiPlugin::build` in this crate, not the facade**, under exactly those `NAME`s; `ui_click` returns `UndoKind::NotUndoable`; `ui_build_tree` is **one command** (`undo` is `detach`, redo is `restore` of the stored `DetachedSubtree`, ids identical); `ui_get_tree { fresh }` runs layout **only through the non-stamping query path** (a test asserts neither the `UiTree` nor the `UiMeasurer` change tick moves); runtime nodes are rejected with `UiError::RuntimeNode`; apply, undo and redo tests for each command, and a registry test listing exactly the seven and six names. Registration adds no schedule system, so there is no schedule to name. Blocked by: intake 2, U12, **CB9** (the registry and the reflected schemas), the `exitium_command` issues from #110.
- **F1 `feat(exitium): UI to renderer glue`.** Footprint: Edits: none; Governs: none; Hotspots: `crates/exitium/`; New crate edges: none (the facade's edges to `exitium_ui` and `exitium_render` are in intake 2). Acceptance: the glue system in render extraction exactly as in "Render handoff" (the `UiMeasure` over `Renderer::measure_text` and the texture store, the unit and scale conversions, layer and depth mapping, skip when `revision` is unchanged), the facade tests for the round trip and the 200-button text-batch count, and a UI example in `crates/exitium/examples/`. High-risk review: facade API. Blocked by: intake 2, U8, U11, the render plugin issues from #107, and the facade chain (the facade runs alone).

**Stage 5, the UI full lane** (`exitium_ui` only; 14 serial PRs; roadmap C6: every issue is `Blocked by` intake 3 and the issue before it; `Edits: crates/exitium_ui/` for all of them, so they are a chain by R1). Every criterion list also includes `cargo xtask ci all`, the wasm build, and the benchmarks named for it. Each states **the schedule and set its systems run in**, and each system's Frozen-mode test follows "Schedules". The lane starts beside Stage 4 and must end before the facade glue (R3).

- **U14 `feat(ui): full widget data, widget state and setters`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `RichText`, `NumLabel`, `Toggle`, `Slider`, `ScrollView`, `VirtualList`, `Grid` and `Tabs` data types with defaults and validation (slider clamp and step, grid `columns >= 1`, tabs index); `WidgetState` in `NodeCold`; the setters `set_num`, `set_value`, `set_on`, `set_selected`, `set_count`, `set_scroll` with the equal-value no-op and dirty marking per the invalidation table; `size_of` pins still hold; setter benchmarks. No systems. Blocked by: intake 3, U13.
- **U15 `feat(ui): layout of grid, tabs, scroll and list rows`.** Footprint: as U14. Acceptance: "Layout of the new widgets": the dispatch, default sizes, `layout/grid.rs`, hidden tab pages, scroll content size and offset clamp, independent fixed-size row roots for lists, the absolute-rectangle walk with scroll offsets and the clip table, the hit list's per-entry clip; goldens and the grid, scroll-offset and full-widget cold-layout benchmarks. Runs inside `layout`, no system. Blocked by: intake 3, U14.
- **U16 `feat(ui): themes, classes and resolution order`.** Footprint: as U14. Acceptance: `UiTheme` widget themes, `UiClass`, `PaintPatch`, class resolution on set and on theme change, the five-step resolution order, the unknown-class warning, border and opacity fields resolved; tests for the order and the theme-change effects. No systems. Blocked by: intake 3, U15.
- **U17 `feat(ui): transitions and tweens`.** Footprint: as U14. Acceptance: `tween.rs`, `UiTree::advance`, the 512 cap with snapping, restart-from-shown-value, no layout invalidation, deterministic tests with fixed steps; the plugin system **`ui_animate` in set `UI_ANIMATE` of `PostUpdate`** (real, unscaled delta clamped to 0.1 s) and its Frozen-mode test; the 512-tween benchmark. Blocked by: intake 3, U16, U11.
- **U18 `feat(ui): clipping, 9-slice, borders and opacity in the draw list`.** Footprint: as U14. Acceptance: the clip table and per-item `clip` on all three item kinds, `uv` on `Image`, nine-item 9-slice, border quads, opacity carried down, scrollbar and modal-scrim painting; the render-mapping helpers for clip and `uv`; goldens for each; the full-widget draw-rebuild benchmark and the `UiDrawItem` size pin. Runs inside `build_draw_list`, no system. Blocked by: intake 3, U17.
- **U19 `feat(ui): rich text and number labels`.** Footprint: as U14. Acceptance: `layout/rich.rs` (span parsing, word measuring, greedy line breaking, the per-node `(markup, width)` cache), one `Text` item per span segment, `NumLabel` formatting through `NumFormat` with the equal-`Num` and equal-text no-ops, the warnings for unknown tags; goldens and the rich-text and `set_num` benchmarks. No systems. Blocked by: intake 3, U18.
- **U20 `feat(ui): toggle, slider, tabs and scroll input`.** Footprint: as U14. Acceptance: input rules 9 to 11, the new `UiEvent` variants and `UiInputFrame::nav` and `scroll`, `UiCapture::scroll`, slider drag outside the rectangle, wheel chaining to ancestors, scrollbar drag, clipped hit entries; table-driven input tests; extend the `process_input` benchmark. Runs inside `process_input` (the existing `UI_INPUT` system in `First`, frozen-mode tested). Blocked by: intake 3, U19.
- **U21 `feat(ui): modal roots and tooltips`.** Footprint: as U14. Acceptance: input rule 8 (hit stop, focus trap, `ModalCancel`, scrim), the built-in tooltip root at layer 1000 in `Runtime` slots with the delay driven by `advance` (so it runs in `ui_animate`), placement inside the viewport, hide rules; tests including a tooltip over a scrolled node. Blocked by: intake 3, U20.
- **U22 `feat(ui): virtual list`.** Footprint: as U14. Acceptance: `layout/virt.rs`, row realization and recycling in `Runtime` slots, `row_of`, the reflected `UiListRow` event, the `detach` and `restore` behavior of lists; the plugin system **`ui_lists` in set `UI_LISTS` of `PostUpdate`**, after `UI_BIND`, with its Frozen-mode test; the 100,000-item scroll benchmark with zero allocations. Blocked by: intake 3, U21.
- **U23 `feat(ui): change-only data binding`.** Footprint: as U14. Acceptance: `bind.rs` (`BindingTable`, groups, pre-parsed paths, the targets and formats of "Data binding", `BindSource::Row` and `ListSource`, binding errors and `SourceGone`), `bind_head`, `detach` and `restore` carrying records; the plugin system **`ui_bind` in set `UI_BIND` of `PostUpdate`**, first of the UI systems there, with a Frozen-mode test (a resource edited while frozen updates a bound label and the draw list); `ui_get_stats` gains the binding counts and `binding_errors`; the bind benchmarks and the **counter-rebinding benchmark**. Blocked by: intake 3, U22.
- **U24 `feat(ui): markup documents and mounts`.** Footprint: as U14. Acceptance: `UiDocument`, `parse` through `exitium_serial` with spans, the validation list, `UiTree::mount` (atomic, `apply_theme`), `UiMarkupStore`, `ui_build_tree { markup }` with undo and redo carrying the mount, the `UiNodeDesc`-rooted file, the text and binary round trips, parse and validation error tests, `find_references`-compatible `Guid` strings; the parse benchmark for a 10,000-node document. No system yet. Blocked by: intake 3, U23.
- **U25 `feat(ui): .ui hot reload`.** Footprint: as U14. Acceptance: `UiTree::reload` with plan then apply, subtree hashes, the by-name diff and three-way merge, **a parse error keeps the old tree and reports line and column**, the state-survival table row by row (focus, scroll, toggle, slider, tabs, bound text, tweens, list rows), `image_changed`-style stale-id behavior for replaced nodes; the plugin system **`ui_markup` in set `UI_MARKUP` of `First`, before `UI_INPUT`**, with a Frozen-mode test (a reload while frozen shows in the next frame); `ui_get_stats` gains `markup` and `last_swap_ms`; the reload benchmarks with the **100 ms** swap row as a hard assertion. Blocked by: intake 3, U24.
- **U26 `feat(ui): immediate-mode layer`.** Footprint: as U14. Acceptance: `immediate.rs` (`UiImmediate`, `ImUi`, keys, reconcile reusing U25's code, `Runtime` slots, layer 900), results for `button`, `checkbox` and `slider`; the plugin system **`ui_immediate_end` in set `UI_IMMEDIATE_END` of `PostUpdate`** with a Frozen-mode test; the 200-widget zero-allocation benchmark. Blocked by: intake 3, U25.
- **U27 `perf(ui): full-widget 10,000-node benchmarks and budgets`.** Footprint: as U14. Acceptance: the full-widget 10,000-node tree and every remaining Performance row not owned by an earlier issue, budget re-baselining with a note to the owner for any row missed, an end-to-end Frozen-mode test across every UI system and the schedule-graph test, the crate `CLAUDE.md` and rustdoc updated for the full UI. Blocked by: intake 3, U26.
- **F2 `feat(exitium): full UI glue`.** Footprint: Edits: none; Governs: none; Hotspots: `crates/exitium/`; New crate edges: none. Acceptance: an asset-event system in `First` that fills `UiMarkupStore` (generation 1 on load, `generation >= 2` on reload, failed reloads ignored), calls `image_changed` when an image's size changed and `font_changed` on a font load, installs the renderer-backed `UiMeasurer`, converts the clip table and `uv` (clip rectangles divided by `dpi_scale`), a facade test that a reloaded `.ui` file changes the next frame's draw list and that a resized image relays out only its users, the 200-button text-batch count with rich text and scroll views, and the facade parity test for the seven commands and six queries (C3). High-risk review: facade API. Blocked by: intake 3, U18, U25, F1, and the facade chain (it runs alone, and cannot overlap any `exitium_ui` issue, R3).

The editor edge `exitium_editor -> exitium_ui` is the roadmap's C5 PR and the editor's UI editor view is an editor-extras issue; neither is filed from this spec.

The real `Blocked by #N` numbers for the intake and for the other specs' implementation issues are filled in when the architect files these, because those specs and issues are written in parallel with this one. U1 to U7, U9 and U10 need no other spec and can start the moment intake 1 merges, which is why the UI core does not wait on the app, render or command-bus specs. U13 waits on CB9, and the Stage 5 lane waits on intake 3 (roadmap C6).

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
- `exitium_ui -> exitium_serial` as a dev-dependency here (the `UiNodeDesc` text-format round trip test in U12). Intake 3 promotes it to a normal dependency under the `plugin` feature, because markup parsing runs in the game (U24). Per the roadmap's rule R3, dev-dependencies count for the conflict check.
- Facade: `exitium -> exitium_ui` (already implied by "facade depends on every runtime crate"). F1 uses `exitium -> exitium_render` as well; it needs no edge between `exitium_ui` and `exitium_render`.
- Not required by this spec: `exitium_editor -> exitium_ui` (see "What the editor needs from the UI"; it is the roadmap's C5 PR, after the Stage 5 lane).
- `exitium_ui` stays out of `NATIVE_ONLY`.

**Intake 3 (Stage 5, the UI full lane):**

- `exitium_ui -> exitium_serial` becomes a **normal dependency under the `plugin` feature** (it was dev-only in intake 2). It is a workspace crate, already built for wasm32, so it adds no crates.io dependency.
- `exitium_ui -> exitium_command`, `exitium_ecs`, `exitium_app` and `exitium_reflect` are already added by intake 2. Bind-by-path uses the reflection path machinery of `exitium_reflect`, with no new edge.
- Facade (F2): no new edge; `exitium -> exitium_ui` and `exitium -> exitium_assets` already exist. `exitium_ui` still does not depend on `exitium_assets` or `exitium_render`.
- **No new crates.io dependency.** The grid is our own equal-column algorithm (taffy's `grid` feature stays off), hashing uses std's `DefaultHasher::new()`, number formatting uses `exitium_core`'s `Num`, and parsing is `exitium_serial`.

**Crates.io dependencies** (high-risk area: a new dependency; the reviewer checks this table):

| Crate | Version | Features | License | wasm | Why not std |
|---|---|---|---|---|---|
| `taffy` | `0.15.1` | `default-features = false`, `["std", "flexbox"]` | MIT (already allowed in `deny.toml`) | yes: `cargo check --target wasm32-unknown-unknown` passes; no platform calls | A CSS-conformant flexbox solver with caching is thousands of lines of subtle algorithm; std has no layout. 0.15.1 was published 2026-10-25, MSRV 1.71 (our toolchain is newer), `#![deny(unsafe_code)]`. |

- With these features taffy has **no transitive dependencies** (`grid` would pull `smallvec`, `taffy_tree` would pull `slotmap`; neither is enabled). `serde`, `parse`, `calc`, `block_layout`, `float_layout`, `content_size` and `detailed_layout_info` are also off.
- Taffy is `0.x`: a minor-version bump may break the low-level traits. The caret requirement `0.15.1` allows only patch updates; upgrading is a deliberate PR confined to `layout/`.
- Nothing else is needed: colors, rectangles and vectors come from `exitium_core`, ids from `Guid`, and text shaping and font data stay in the renderer behind `UiMeasure`, supplied by the facade. Stages 2 and 5 add only workspace crates, no further crates.io dependency.
