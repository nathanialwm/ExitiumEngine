# Spec: Game UI (retained tree on taffy)

- Issue: #109. Amended by #142 (SP6: the full UI, and AM6 of `editor-mcp-mvp.md` for the command bus).
- Crates: exitium_ui. Stage 1 depends on exitium_core and taffy only. The `plugin` module (also a cargo feature of that name, per the roadmap's crate graph) adds exitium_ecs, exitium_app, exitium_reflect, exitium_command and exitium_serial. There is **no edge** to exitium_render, exitium_platform or exitium_assets (C1: no edges between leaf crates); the facade holds all cross-crate glue.
- Status: draft (revised after AI review rounds 1 and 2 and the owner decisions of 2026-11-25; amended for the full MVP by SP6)

## Goal

Specify `exitium_ui`: a retained widget tree with flex layout on [taffy](https://crates.io/crates/taffy), the full widget set of epic #5 (panel, label, rich text, number label, image with 9-slice, button, toggle, slider, progress bar, scroll view, virtualized list, grid, tabs, tooltip, modal) plus the owner's additions of 2026-11-25 (**dropdown (select), text input, drag and drop**), screen-space anchoring and scaling, pointer input and **keyboard and gamepad focus navigation** that the UI consumes before gameplay, themes with named classes, transitions, **data binding that updates only on change**, **text markup files (`.ui`) with hot reload**, and an **immediate-mode layer** for debug overlays. An agent builds and edits the tree through commands, a human refines it in the editor, and the same tree runs on Windows and in the browser (wasm32).

The Jam-era text of this spec specified five widgets and left the rest as later work. The owner's full-MVP direction (`mvp-roadmap.md`, SP6) brings all of it back, and **the whole design is decided here, once**, because data binding changes the arena shape (see "How the arena is shaped for binding"): deciding it after the layout core is built would mean reshaping the arena under ten merged issues. The implementation order does not change. U1 to U10 build the layout core on the final arena shape; the rest of the widgets, binding, markup, themes and the immediate layer arrive as the Stage 5 UI lane (see "Implementation issues").

This spec also settles PLAN Phase 3 item 1, "ECS entities or a dedicated arena": **a dedicated arena** (see "Node storage decision"). PLAN says "decided by ADR"; the owner-approved issue records it here, with alternatives, to keep `docs/decisions/` (a hotspot) out of this PR.

Design in one paragraph: `UiTree` owns a generational arena of nodes in three parallel arrays: **hot** (what paint and hit-test read: widget data, authored style, rectangle, flags), **cold** (what only tools and bookkeeping read: names, class, the node's first binding, the runtime state of big widgets, and an optional box of rarely used fields: tooltip, focus links, drag and drop settings) and **layout** (taffy's style, cache and result). Layout runs through taffy's low-level traits directly on the arena, one independent layout per root, with anchoring applied after layout. Edits clear exactly the caches they invalidate, and edits that cannot change geometry (counters with a fixed size, progress values, hover state, scroll offsets, tween steps) skip layout entirely. **Data binding** is a table of records outside the arena that pushes game state in through the same setters, driven by ECS change detection, so only values that changed cost anything. **Markup** files are the reflection text form of a `UiDocument`; a hot reload diffs the new document against the mounted tree and touches only what changed. Painting turns the laid-out tree into a flat `UiDrawList` of quads, images, text runs and clip pushes in physical pixels. Input is a small plain-data frame in, `UiEvent`s and a `UiCapture` out. An **immediate-mode layer** builds a throwaway tree per frame on the same code. **Stage 1 is the layout core with no ECS, no render and no platform dependency.** **Stage 2** wraps it in a plugin that registers resources and systems into the app schedules; the facade feeds the plain draw data to the renderer and supplies the text measurer. **Stage 3** registers the commands and queries. The **Stage 5 lane** adds the rest of the widgets, binding, markup, themes and the immediate layer.

## Scope

What the Jam-era text left out, and where this spec now specifies it:

| Item | Where |
|---|---|
| Widgets beyond panel, label, image, button and progress bar: rich text, number label, toggle, slider, scroll view, list and virtualized list, grid, tabs, tooltip, modal | "The widget set" |
| **Dropdown (select), text input, drag and drop** (owner decisions, 2026-11-25) | "The widget set" (`Dropdown`, `TextInput`), "Drag and drop", "Input" rules 12 to 14 |
| **Keyboard and gamepad focus navigation** (tab order, spatial navigation, explicit links, initial focus) | "Input" (rule 5 and "Focus navigation") |
| Data binding that updates only on change; the `Num` display widget | "Data binding"; "The widget set" (`NumLabel`) |
| Markup files and hot reload | "Markup and hot reload" |
| Named style classes, widget themes, 9-slice images, borders, opacity, transitions and tweens | "Themes, transitions and 9-slice" |
| Clipping and scrolling | "The widget set" (`ScrollView`), "Painting" (clip items) |
| Relayout when an image or font changes on disk | "Reacting to asset changes" |
| The immediate-mode layer for debug overlays | "The immediate-mode layer" |
| A binary form of markup | `UiDocument` is reflected, so the binary serial format (#24) writes it with no UI code (see "Markup and hot reload") |

## Non-goals

Not in this spec. Each can be added later without breaking the layout below (design rule 5), and `WidgetKind` and `UiDrawItem` are `#[non_exhaustive]` for that reason:

- **Text input is in the MVP** (owner decision, open question 11) and this spec owns the widget. What it leaves out: **multi-line fields**, caret movement by grapheme cluster (the caret moves by Unicode scalar value, so backspace can split a combining sequence or an emoji ZWJ sequence; std has no segmentation and no new dependency is justified), type-ahead search in a dropdown, a UI-drawn on-screen keyboard (a gamepad player types through the platform's overlay keyboard, which arrives as committed text), and rich text inside a field. The platform side (committed text, IME, clipboard, key repeat) is the separate `app-platform.md` amendment #177 (see the cross-spec table).
- Rounded corners and other per-pixel effects on quads. Quads are plain rectangles; rounded or ornate frames are 9-slice images, which also suit pixel art. A rounded-rectangle shader is a render-side feature. **Accepted by the owner, 2026-11-25** (open question 13).
- Taffy's CSS grid and block layout. Only flexbox is compiled in; `Grid` has its own equal-column algorithm (see "Layout of the new widgets").
- Keyframe timelines and animated layout. Tweens animate paint properties only, so an animation never invalidates layout.
- Includes or templates in markup (a file embedding another). A game reuses a subtree by building a `UiNodeDesc` in code; an `Include(Guid)` variant can be added later.
- Variable-height rows in the virtualized list, a two-way (UI to game state) binding, touch gestures and kinetic scrolling, right-to-left text, localization, accessibility. Also not here: dragging several items at once, dropping files from the operating system, and dragging between windows (drag and drop is in-tree only).
- World-space UI (health bars over entities). Those are sprites and text drawn by the renderer, not a retained tree.
- Luau bindings for the UI. The scripting spec (SP9) generates bindings from reflection and the command bus; this crate hand-writes none.
- Parallel layout. The executor is single-threaded and so is the layout pass; independent roots leave room for it (see Threading).
- Per-node image filtering. Nearest or linear sampling is a property of the texture (`TextureDesc.filter` in the render spec), chosen at import, so pixel-art images are imported with `Nearest`.

## Design

### Module layout

```
exitium_ui/src/
  lib.rs
  arena.rs        NodeArena: slots, generations, free list, reserve, restore  safe
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
  focus.rs        tab order, spatial navigation, focus links, initial focus    safe
  dragdrop.rs     drag state, ghost, drop targets, keyboard drag, auto-scroll  safe
  textedit.rs     TextEditOp, caret, selection, preedit, edit history          safe
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
| Stable ids under undo | despawn then restore gives a new `Entity` | arena reserves the slot and restores the exact id (see "Stable ids and undo") | both |
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
    cold: Vec<NodeCold>,            // parallel array: names, class, binding head, runtime widget state, the rare-fields box
    layout: Vec<LayoutData>,        // parallel array indexed by slot index, see below
    roots: Vec<UiNodeId>,           // sorted by RootConfig::layer, then creation order
    names: HashMap<Box<str>, UiNodeId>,
    image_nodes: HashMap<Guid, Vec<UiNodeId>>,   // image -> the nodes that show it, see "Reacting to asset changes"
    bindings: BindingTable,         // see "Data binding"
    mounts: Vec<UiMount>,           // markup files mounted into the tree, see "Markup and hot reload"
    tweens: Vec<ActiveTween>,       // in-flight transitions, capped, see "Themes, transitions and 9-slice"
    theme: UiTheme, viewport: UiViewport,
    input: InputState,              // hover, pressed, focus, the drag state, prev button, press origin, popup and ghost roots
    hit_list: Vec<HitEntry>,        // interactive nodes in paint order, rebuilt after layout
    events: Vec<UiEvent>,           // valid until the next process_input
    capture: UiCapture,
    release: ReleaseQueue,          // slots whose DetachedSubtree was dropped, see "Stable ids and undo"
    tree_id: u64,                   // unique per tree: a DetachedSubtree restores only into the tree that made it
    layout_revision: u64, paint_revision: u64, asset_refs_revision: u64,
}

struct NodeHot {                    // pinned at the measured size, ceiling 320 B, excluding heap data (see the size note below)
    parent: Option<UiNodeId>,
    children: Vec<UiNodeId>,        // paint order: later children draw on top
    kind: WidgetKind,               // authored widget data; the big variants are boxed, ceiling 96 B for the enum
    style: UiStyle,                 // authored, compared on set to skip no-op edits
    class: u16,                     // index into theme.classes, 0 = none; resolved from NodeCold::class_name
    root: Option<Box<RootConfig>>,  // Some only for roots
    rect: Rect,                     // absolute, UI-logical px, set by the post-layout pass
    flags: NodeFlags,               // hovered, pressed, focused, bound, tweening, runtime, draggable, droppable, drag_over
}

struct NodeCold {                   // pinned <= 64 B (about 52 B); never loaded by the paint walk, the hit test or layout
    name: Option<Box<str>>,
    class_name: Option<Box<str>>,   // the authored name; re-resolved when the theme changes
    bind_head: u32,                 // first of this node's records in BindingTable, NONE when unbound
    state: Option<Box<WidgetState>>,// runtime state of the big widgets, see "The widget set"
    extra: Option<Box<NodeExtra>>,  // None for most nodes
}
struct NodeExtra {                  // authored fields that most nodes do not have
    tooltip: Option<Box<str>>,      // rich-text markup, shown by the built-in tooltip root
    focus: FocusLinks,              // explicit up, down, left, right targets by node name (see "Focus navigation")
    drag: Option<DragSource>, drop: Option<DropTarget>,   // see "Drag and drop"
}

struct LayoutData {                 // about 720 B per node with taffy's flexbox-only build
    style: taffy::Style,            // derived from UiStyle + theme + widget kind (248 B)
    cache: taffy::Cache,            // 392 B
    unrounded: taffy::Layout,       // parent-relative, 76 B
}
```

- **Arena.** `NodeArena` is a `Vec` of slots with a `NonZeroU32` generation, the same rules as `exitium_core::Slab` (stale ids fail every lookup in O(1), a freed slot's generation is bumped, a slot whose generation would overflow is retired). It is a separate small type, not `Slab`, because a slot has **three states**, not two: `Occupied`, `Free` (on the single LIFO free list) and `Reserved(token)` (a node removed through a command: off every list, generation unchanged, lookups of its id fail, waiting for its undo or for its history entry to be dropped). `restore` re-occupies a reserved slot; see "Stable ids and undo". If another crate needs reserve and restore, fold them into `Slab` later; that is not a UI decision.
- **Why parallel arrays.** Paint and hit-test read `kind`, `style`, `rect` and `flags` from hot data. Layout reads `LayoutData`. Everything a paint walk never needs sits in `NodeCold`. Splitting them keeps the paint walk's working set at about 1.6 MB per 5,000 nodes (a 320 B ceiling per hot node) instead of 6 MB. `cold[i]` and `layout[i]` are valid for every live slot `i` and are resized with the arena.
- **Memory budget:** at most 1.25 KiB per node including hot, cold and `LayoutData` and excluding heap strings; 10,000 nodes fit in 12.5 MB. Measured sizes on a 64-bit build with taffy's `std` + `flexbox` features: `Style` 248 B, `Cache` 392 B, `Layout` 76 B (716 B of `LayoutData`). **`NodeHot` is capped at 320 B and `NodeCold` at 64 B**, which is about 1,100 B per node. The 320 B comes from counting the listed fields with the real core types (`Color` is 4 x `f32`, so `Option<Color>` is 20 B; `UiStyle` is about 155 B; `parent` 8, `children` 24, `rect` 16, `root` 8, `class` 2 and `flags` 4 add about 62 B; `kind` up to 96 B; 155 + 62 + 96 = 313 B, so the 320 B ceiling leaves 7 B for alignment), not from the Jam-era 192 B plus a hope. `WidgetKind` is capped at 96 B: `RichText`, `VirtualList`, `Dropdown` and `TextInput` are boxed, while `NumLabel` (about 90 B with two `String`s, a `Num`, a `NumFormat` and an `Option<Color>`) and `Image` (a 16-byte-aligned `Guid`) stay inline. **The pin is the measured number:** U2 runs `size_of` and pins what it finds, U14 re-measures after the new widgets are added and updates the pin in the same PR, so growth is always a visible diff. If a measurement exceeds a ceiling, the order of relief is: box `NumLabel`, then move `UiStyle::transition` and `UiStyle::border` into a boxed `StyleExtra` (compared on set like the rest). `size_of` tests pin all four sizes.
- **Names.** A node may have a unique name (`"score_label"`). `find(name)` is how Rust game code and agents address nodes without holding ids, and how a markup reload matches nodes. A duplicate name is `UiError::DuplicateName`.
- **Roots.** A root is a node with `parent == None` and `root: Some(RootConfig)`; `roots` is the draw order (lower `layer` first, so higher layers draw on top and are hit first). `RootConfig::layer` is limited to `-1000..=999` (`UiError::LayerRange`). Three layers above that are reserved for the built-in roots, which the render mapping also fits: **1000 the tooltip, 1001 the open dropdown popup, 1002 the drag ghost** (so a popup opened inside a modal draws over it, and a dragged item draws over everything). Because 1001 and 1002 sit above every modal, a popup or drag that was already open when a modal root appeared would stay on top of it: input rule 8 closes the popup and cancels the drag whose owner lies below the new modal.

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
    Dropdown(Box<DropdownData>),               // a closed box showing the chosen option; opens a popup list
    TextInput(Box<TextInputData>),             // a single-line editable text field
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
pub struct DropdownData { pub options: Vec<DropdownOption>, pub selected: Option<u32> /* index into options, None shows the placeholder */, pub placeholder: String, pub max_visible: u32 /* rows before the popup scrolls, default 8 */, pub enabled: bool }
pub struct DropdownOption { pub label: String, pub value: String /* stable key for game code, may be empty */, pub enabled: bool }
pub struct TextInputData  { pub text: String, pub placeholder: String, pub max_chars: Option<u32>, pub filter: TextFilter /* Any | Integer | Decimal */, pub password: bool, pub read_only: bool, pub enabled: bool }
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

`PointerMode::Auto` resolves to `Block` for buttons, toggles, sliders, dropdowns, text inputs, scroll views, virtual lists, tab headers, **nodes with a `drag` or `drop` setting** and panels with a visible background, and to `Pass` for everything else (labels, images, progress bars, transparent containers), so a transparent full-screen HUD container does not eat gameplay clicks while a visible panel does.

Roots, anchoring and scaling:

```rust
pub struct RootConfig {
    pub anchor: Anchor,                  // TopLeft, Top, TopRight, Left, Center, Right, BottomLeft, Bottom, BottomRight
    pub pivot: Option<Vec2>,             // fraction of the root's own size, default = the anchor's fraction
    pub offset: Vec2,                    // UI-logical px, +x right, +y down
    pub size: RootSize,                  // Fill (viewport) | Fixed(Vec2) | Fit (content)
    pub layer: i16,                      // -1000..=999 (1000 to 1002 are reserved for the built-in roots)
    pub modal: bool,                     // blocks input to every root below and draws a scrim (see "Input", rule 8)
    pub autofocus: bool,                 // a modal root always does: the first nav or tab edge with no focus enters this root (see "Focus navigation")
    pub initial_focus: Option<String>,   // name of the node that edge focuses; None = the first focusable node in paint order
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
    pub clips: Vec<Rect>,                    // physical px; `clip == 0` is no clip, `clip == k` is `clips[k - 1]` (see "Clips")
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

**Clips.** A scroll view or virtual list clips its descendants. The renderer's `clip` is a field on each draw item (a scissor rectangle), not a push and pop, so the draw list does the same: each item carries a `clip` that selects an entry of `clips`, which holds the rectangles in physical pixels (a pass uses a handful of distinct clips, so the table keeps `UiDrawItem` small). **The one wording, copied by the render spec: `clip == 0` means no clip, and `clip == k` (k >= 1) means `clips[k - 1]`.** Index 0 of the vector is therefore the first real rectangle, never a "none" entry. Clipping is applied to the **quad, image and text planes alike**, so the two planes of a root (below) stay valid inside a scroll view. The glue maps `clip == 0` to `None` and `clip == k` to `Some(clips[k - 1])` divided by `dpi_scale`, for `SpriteDraw.clip` (quads and images) and for `TextDraw.clip` (a `Text` item carries its own `clip`, which becomes that text item's own `TextBounds`). The render spec's batch plan `Run.clip` is therefore `None` for a text run, and a clip change never splits a text batch (render-v1.md, guarantee 8).

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
    pub dropdown: DropdownTheme,                                   // box: ButtonColors, chevron color, popup background and border, option: ButtonColors, option_selected, row_height (default 28)
    pub text_input: TextInputTheme,                                // background, border, border_focused, text, placeholder, selection, caret, preedit underline, caret_width (default 1 physical px minimum), blink_ms (530), double_click_ms (400), default size
    pub drag: DragTheme,                                           // threshold (6 UI-logical px), ghost_opacity (0.7), auto_scroll_edge (24), auto_scroll_speed (600 px/s)
    pub modal_scrim: Color,                                        // drawn under a modal root
    pub transition: Option<Transition>,                            // default for every node with a class state change; None = instant
    pub classes: Vec<UiClass>,                                     // named style classes; NodeHot::class indexes this
}

pub struct UiClass {
    pub name: String,                                              // unique in the theme ("title", "danger_button")
    pub padding: Option<Edges>, pub background: Option<Color>, pub border: Option<Border>,
    pub text_color: Option<Color>, pub font_size: Option<f32>, pub transition: Option<Transition>,
    pub hover: PaintPatch, pub pressed: PaintPatch, pub disabled: PaintPatch, pub selected: PaintPatch,  // selected: a toggle that is on, the current tab
    pub drag_over: PaintPatch, pub dragging: PaintPatch,           // a drop target that would accept the item being dragged; the source while it is dragged
}
pub struct PaintPatch { pub background: Option<Color>, pub border_color: Option<Color>, pub text_color: Option<Color>, pub opacity: Option<f32> }
pub struct Transition { pub duration_ms: u16, pub easing: Easing /* Linear | EaseOut | EaseInOut */ }
```

A class overrides only the properties that were already `Option` in `UiStyle` (padding, background, border color) plus the text color and font size. It cannot set `width`, `gap` or any other non-`Option` layout field, because a node's own `Dim::Auto` cannot be told apart from "not set", and a class that silently changed sizes would make layout depend on hover. The six state patches carry **paint properties only**, so a hover, press, disable or drag never invalidates layout.

**Resolution order** for any color or size, first hit wins:

1. the node's own override (`Some` in `UiStyle` or in the widget data);
2. the class's state patch for the node's current state (precedence: disabled, dragging, pressed, drag_over, hover; `selected` applies on top of the base when it is true);
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

The struct definitions are under "Data layout". This section fixes behavior. Interaction results are `UiEvent`s (see "Input"); all value changes also go through setters (`set_text`, `set_num`, `set_progress`, `set_value`, `set_on`, `set_selected`, `set_count`, `set_enabled`, `set_scroll`, `set_choice`, `set_options`, `set_input_text`, `set_drag_payload`), each with the equal-value no-op and the dirty rules of "Invalidation strategy".

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
| **Dropdown** | A closed box (drawn like a button: `theme.dropdown` colors, the chosen option's label or the placeholder, a chevron quad). **Open state is runtime** (`WidgetState`: `open`, `highlighted`). A click, or `activate` with focus, opens a **built-in popup root** at layer 1001 (reserved), placed under the box (above it when there is no room below) and at least as wide as the box: a `ScrollView` of at most `max_visible` rows, one row per option (`theme.dropdown.row_height`), built into `Runtime` slots on open and removed on close (O(options) per open, not a hot path). A disabled option is drawn with `text_disabled` and cannot be chosen. **Choosing** an option (click, or `activate` on the highlighted row) sets `selected`, closes the popup and emits `DropdownChanged(node, Option<u32>)` on change only. **Closing:** `cancel` closes and keeps focus on the box, and so does a keyboard choice (`activate` on the highlighted row). A close **by pointer** (a click on a row, or a press outside the popup) **clears the focus** (`FocusLost`), so after a mouse choice Space and Escape reach gameplay again (the "click then Space" rule of input rule 6; the press that focused the box was only a way to drive the open popup). A press outside the popup **is consumed** (a UI press origin, so it neither clicks what is under it nor reaches gameplay). The popup also closes (without `DropdownChanged`, with `DropdownToggled(node, false)`) when the box is hidden, disabled or removed, and when a modal root appears over the box (input rule 8). While open, `nav` y moves the highlight, and **`nav` x and `tab` do nothing**, so focus cannot escape a popup. `Auto` width is the widest option label plus the chevron (each label measured once and cached per `(label, font, size)`; `set_options` re-measures). **`set_options` keeps `selected` when its index is still in range, otherwise sets it to `None` (the placeholder), and raises no event** (like every setter, only a user choice does); it also closes an open popup. `set_choice(Some(i))` with `i >= options.len()` stores `None`. Focusable. Not for variable-height or searchable lists (those are a `VirtualList` of buttons). |
| **TextInput** | A single-line editable field (all rules under "Text input" below). It is a leaf drawn as a box with the text, a selection, a caret and, during IME composition, the preedit text. Its size comes from its style or `theme.text_input` (default 200 px wide, one line plus padding high), **never from its content, so typing re-lays-out nothing**. Focusable. |
| **Tooltip** | Not a widget kind: a node's `tooltip` string (rich-text markup, in `NodeExtra`). When the pointer rests on a node with a tooltip for `theme.tooltip.delay_ms` of `advance` time, the tree shows its **built-in tooltip root** at layer 1000 (reserved, see "Roots"), near the pointer and moved to stay inside the viewport. It hides on press, on leaving the node and when the node is removed. Its nodes are `Runtime` slots, it never blocks input (`PointerMode::Pass`), and it is created lazily on the first show. |
| **Modal** | Not a widget kind: `RootConfig::modal == true`. See "Input", rule 8. |

**Virtual list.** Rows are the realization of a template, so ten thousand data items cost a few dozen nodes:

- `count` is the number of rows (set directly, or by a binding to the length of a reflected list, see "Data binding"). The content extent is `count * item_extent`; `first = floor(offset / item_extent)` and `last = ceil((offset + viewport) / item_extent)`; rows `first - overscan ..= last + overscan` (default overscan 2, clamped to `0..count`) are realized.
- A realized row is a clone of `ListData::item` built into **`Runtime` slots** under the list. When the window moves, rows leaving it go to a pool, and rows entering it take a node from the pool (a rebind, no allocation) or build one. Bound fields of a row template use `BindSource::Row` (see "Data binding"), and a row is re-applied when its index changes or the list's source changes. **`ui_lists` applies the `Row` records of every row it realizes or re-indexes itself, with the same apply code `ui_bind` uses, before it returns**, so a scrolled row never shows the previous item's data for a frame (`ui_lists` runs after `ui_bind` in `PostUpdate`, and `ui_bind` would otherwise only reach the row next frame).
- Rows are independent fixed-size layout roots (width = the list's inner width, height = `item_extent`) laid out after the list's own size is known (see "Layout of the new widgets"), so scrolling does not invalidate a clean row.
- **Identity.** A row node is recycled, so its `UiNodeId` is not an item's identity. `UiTree::row_of(node) -> Option<(list, index)>` walks up from any node inside a row, so a `Click(button)` handler finds the item with one call. When a row is newly assigned to an index, the plugin sends a reflected **`UiListRow { list, index, row }` ECS event** for code that fills rows by hand. It is an ECS event and not a `UiEvent` because the tree's event slice is cleared by every `process_input` in `First`, and rows are realized later in the frame (`PostUpdate`), so a `UiEvent` raised there would be gone before any `Update` system ran. **One-frame lag for hand-filled rows:** the event is sent in `PostUpdate` and read by an `Update` system next frame, so a row that game code fills by hand shows the template's content (or the recycled row's old content) for one frame. Rows filled by `Row` bindings have no lag (see above); a game that cares uses bindings, not `UiListRow`.
- Variable row heights are a non-goal. A list whose `count` is 0 draws nothing.

### Text input

The `TextInput` widget, this spec's own (owner decision 2026-11-25). The platform side (committed text, IME, clipboard, key repeat) is `app-platform.md` amendment #177; the UI never names a platform type, and the facade glue (F3) converts.

```rust
pub enum TextFilter { Any, Integer, Decimal }   // Integer: an optional leading '-' and digits; Decimal: also one '.'
#[non_exhaustive]
pub enum TextEditOp {                            // the same ops come from keys (through the glue), from IME commits and from the `ui_text_edit` command
    Insert(String),                              // committed text: typed characters, an IME result, a paste
    Backspace, Delete,
    Move { to: CaretMove, select: bool },        // CaretMove: Left, Right, WordLeft, WordRight, Home, End; select extends the selection
    SelectAll, Copy, Cut, Undo, Redo, Submit,
}
#[non_exhaustive]
pub enum UiTextEvent { Edit(TextEditOp), Preedit { text: String, cursor: Option<(u32, u32)> } }   // an empty Preedit text ends composition
```

- **Runtime state** (in `WidgetState`, never serialized): `caret: u32` and `anchor: u32` (byte offsets on `char` boundaries; the selection is `anchor..caret` either way round), `scroll_x: f32` (the text's horizontal scroll inside the box), `preedit: Option<Preedit>` (composition text, drawn inline at the caret with an underline), blink phase, the click count for double-click, and a bounded edit history (64 steps, consecutive inserts or consecutive backspaces merged into one step). It lives in the widget, **not in the command bus**: Ctrl+Z inside a field undoes typing, and the editor's undo stack never sees keystrokes.
- **Editing.** `Insert` replaces the selection, drops control characters (a `\n` or `\r` in pasted text is dropped, since the field is single-line), applies `filter` and `max_chars` per character (what does not fit is cut), and clears any preedit. `Backspace` and `Delete` remove the selection, else one `char` (backspace before the caret, delete after it). Word movement uses std character classes (alphanumeric, whitespace, other), not Unicode word segmentation. `Copy` and `Cut` put the selection text in `UiTree::take_clipboard_write()` for the glue to send to the platform clipboard; they do nothing in a `password` field. A paste is an `Insert` the glue builds from the platform's paste event. `Submit` (Enter) emits `TextSubmitted(node)`. `read_only` allows only movement, selection and `Copy`; `enabled == false` takes no focus.
- **Events.** `TextChanged(node)` after a user edit changed the text (read it with `UiTree::input_text(node)`), `TextSubmitted(node)`. Neither carries a `String`, so `UiEvent` stays small and plain. `set_input_text(node, &str)` (game code, bindings, `ui_set_field`) is an equal-value no-op, keeps the caret where it still fits and clamps it otherwise, and raises **no** event (only a user edit does).
- **Caret and selection geometry** use `UiMeasure::measure_text` only, so the renderer needs no new API: the caret x is the width of the text before it (measured as a prefix), a click maps x to a byte index by a binary search over `char` boundaries (O(log n) measures, once per click or drag step), and selection is one quad from the anchor x to the caret x. A prefix measure ignores kerning and ligatures across the cut by at most a fraction of a pixel; accepted. A `password` field draws and measures one bullet (`U+2022`) per `char`.
- **Painting.** The field's inner rectangle is one entry of the clip table, so long text and the caret are clipped to it (the clip applies to text, as for scroll views). Order: box and border (the border uses `border_focused` while focused), selection quad, text item shifted by `-scroll_x`, caret quad (one physical pixel wide at least, drawn only in the visible half of the blink), an underline quad under the preedit. The placeholder draws, with `placeholder` color, while the text is empty and there is no preedit. Selection and caret are graphics, so under the two-planes rule they draw below the text of the same root, which is correct for a one-pixel caret and a translucent selection color.
- **Blink and scrolling.** `advance(dt)` toggles the blink phase every `blink_ms` while a field has focus and bumps `paint_revision` only when the phase flips; any edit or caret move resets the phase to visible. After every edit or move the box scrolls just enough to keep the caret inside it. Nothing here touches layout.
- **Pointer.** A press inside the field gives it focus (one of two exceptions to "a mouse press never gives keyboard focus", rule 3), places the caret (a press on the field that is **already focused** keeps focus and emits no `FocusLost` or `FocusGained`), and a drag extends the selection (the field is a UI press origin for capture). A second press within `theme.text_input.double_click_ms` of `advance` time, at nearly the same x, selects the word under it.
- **IME.** `Preedit` shows composition inline; a following `Edit(Insert(..))` commit replaces it. `UiTree::ime_area() -> Option<Rect>` is the caret rectangle in physical pixels (the draw list's space) while a field has focus, else `None`; the glue divides by `dpi_scale` and gives it to the platform as the IME cursor area (so the candidate window opens beside the caret). `UiCapture::text_input` is true while a text field has focus, and the glue turns the platform's IME and text events on and off by it.
- **Keys while a field has focus.** `UiCapture::text_input` is true, so gameplay ignores every key. The field consumes edit ops and `nav` x (caret, one `char`, exactly as `Edit(Move { Left | Right, select: false })`). **A key press reaches the field once, never as both**: the glue sends the keyboard's Left and Right arrows (and their repeats, and Home, End and the shifted forms) to a focused field **as `Edit(Move)` only and does not put them in `nav`**, while the D-pad and the left stick, which have no text event, arrive as `nav` x (F3 decides by the previous frame's `UiCapture::text_input`; an arrow pressed in the very frame a field gains focus follows the old value, accepted). So one arrow press moves the caret once. `activate` is **not** a `Click` here (Enter arrives as `Edit(Submit)`; Space is committed text), `cancel` clears focus (the field is left as it is; there is no revert), `tab` moves focus on (a tab character is never inserted), and `nav` y moves focus spatially (see "Focus navigation"), so a gamepad or arrow keys can leave a field.
- **Bindings and reload.** `BindTarget::Text` on a `TextInput` calls `set_input_text`; a field bound to game state is **state-wins** (a binding apply overwrites a half-typed value), so a game that binds a field writes its state from `TextChanged`, which makes the apply an equal-value no-op. A hot reload keeps caret, selection and history on a surviving node and re-clamps the caret if the file changed the text (three-way rule).
- **Agents.** `ui_set_field` edits `text` like any field (undoable, no event). `ui_text_edit { node, op }` runs one `TextEditOp` as a user would (focuses the field first, emits the same events, `NotUndoable`, journalled); `ui_get_node` returns caret, selection and preedit. The platform's injected input (`input_inject`, app-platform) reaches the same path through the glue.
- **Cost.** One edit is O(text length) for the string change, up to three `measure_text` calls (caret, scroll, selection edge; each a shaped-buffer cache hit or one miss of about 20 us) and one paint revision; no layout. The benchmarks are under "Performance".

### Drag and drop

Pointer-driven, in-tree drag and drop of a small reflected payload, with a keyboard and gamepad path for the same operation. Typical uses: moving an inventory item between slots, reordering a list, dropping a building onto a slot.

```rust
pub struct DragPayload { pub kind: Box<str>, pub id: u64, pub text: String }   // reflected; `kind` is the game's tag ("item", "building"), `id` its key, `text` free data
pub struct DragSource { pub payload: DragPayload, pub ghost: DragGhost /* Clone | Image(Guid) | None */, pub enabled: bool }
pub struct DropTarget { pub accepts: Vec<Box<str>> /* payload kinds; empty = every kind */, pub enabled: bool }
```

Both are optional authored fields of `UiNodeDesc` (`drag`, `drop`), stored in `NodeExtra`; the hot `flags` carry `draggable`, `droppable` and `drag_over` so the hit test and paint never open the box. Because they are reflected fields they are edited by `ui_set_field`, written in `.ui` files and carried by undo and hot reload like any other field. `set_drag_payload(node, DragPayload)` is the setter (equal-value no-op). In a virtual-list row template the payload id is bound with `BindTarget::DragId` and `BindSource::Row`, so a recycled row always carries the right item.

**Pointer drag.**
1. **Start.** A press on a node with an enabled `drag` setting (or on a descendant that is not itself an interactive widget: the nearest draggable ancestor counts) arms a drag. When the pointer has moved `theme.drag.threshold` UI-logical px from the press point while the button is held, the drag starts: the payload is **copied** into the tree's `DragState` (so a recycled list row cannot change it), the source gets the `dragging` class patch, the ghost root is built, and `DragStarted(source)` is emitted. A button that is also draggable gives a `Click` when released before the threshold. A drag is a UI press origin for capture (rule 6), through the release frame.
2. **Ghost.** The default `Clone` ghost is a **snapshot**: `describe` of the source subtree with every `name`, `bindings`, `drag`, `drop`, `focus` link and `tooltip` **stripped** from the copy (a ghost must not follow a recycled row's `Row` bindings, start a drag or accept a drop, and its names would collide with the live ones; a `VirtualList` inside the source becomes an empty `Panel` of the same style), built into `Runtime` slots under a root at layer 1002 (reserved), `PointerMode::Pass`, drawn with `theme.drag.ghost_opacity`, kept under the pointer at the offset where it was grabbed. `Image(guid)` is one image node, and its `Guid` is part of `referenced_assets` (so FA1 requests it before the first drag); `None` has no ghost (the game draws its own). The ghost is not hit-testable, so the hit test sees what is under it.
3. **Over.** Each frame the topmost hit is walked up through its ancestors to the **nearest droppable that accepts the payload kind** (an enabled `drop` whose `accepts` is empty or names `payload.kind`; a non-accepting droppable is skipped, not a barrier). A change emits `DragLeave(old)` then `DragEnter(new)`; the target carries the `drag_over` class patch while it is the current target. Only nodes inside the topmost modal root are candidates when a modal is up.
4. **Drop.** Releasing over an accepting target emits `Dropped { source, target }`; elsewhere, or on the source itself, `DragCancelled(source)`. `cancel` during a drag cancels it. Removing the source cancels the drag without a drop; removing the target clears the over state. The game reads the payload with `UiTree::drag() -> Option<&DragState>` (`source`, `payload`, `source_row: Option<(list, index)>`, `over`) while handling the event. The state stays readable for the rest of the frame in which the drag ended and is **cleared at the start of the next `process_input`** (the same moment as the event slice). **The source `UiNodeId` can name a recycled row by then, so game code uses `payload.id`.**
5. **Auto-scroll.** While a drag is active and the pointer is within `theme.drag.auto_scroll_edge` of the edge of the innermost `ScrollView` or `VirtualList` under it, `advance(dt)` scrolls it toward that edge at up to `auto_scroll_speed` px/s (faster nearer the edge), through `set_scroll` (no layout), and the over target is recomputed, so a long inventory can be dragged across.

**Keyboard and gamepad drag.** A node with an enabled `drag` setting is **focusable whatever its kind** (input rule 9), so a draggable `Panel` or `Image`, such as an inventory slot, can take focus; without that the path below could never start from one.
- **Start.** `activate` on a focused draggable starts a keyboard drag (the same `DragState`, the ghost is drawn next to the focus ring), but **only when at least one enabled droppable that accepts the payload kind is visible and not clipped out**; otherwise nothing starts.
- **A draggable `Button`.** With the keyboard, **drag wins and no `Click` is emitted** (the pointer keeps the other split: released before the threshold is a `Click`, past it a drag). When no accepting droppable exists, `activate` on a draggable `Button` emits its `Click`, and on any other draggable node does nothing, so a focused button is never dead. A game that needs a keyboard "use" and a keyboard "move" on one slot puts the "use" on a second control.
- **Candidates.** While the keyboard drag lasts, `nav` moves focus **among the enabled droppables that accept the payload kind, whether or not they are otherwise focusable** (an `Image` or `Panel` with a `drop` setting qualifies; they are **exempt from the "interactive widget" rule of focus**), inside the topmost modal root when one is up, by the same spatial scoring as "Focus navigation". `tab` does nothing during the drag. `activate` drops on the focused target, `cancel` cancels.

`DragStarted`, `DragEnter`, `DragLeave`, `Dropped` and `DragCancelled` are identical to the pointer path, so game code handles one path. A keyboard drag sets `UiCapture::keyboard` while it lasts.

**A modal root appearing** over an open drag (the source is below the new modal root) **cancels the drag** (`DragCancelled`, no drop), and one over an open dropdown popup closes it (see "The widget set"); the layers 1001 and 1002 would otherwise keep both above the modal. A drag that **starts inside** a modal root is unaffected.

**Events and capture.** `UiEvent::{DragStarted, DragEnter, DragLeave, Dropped, DragCancelled}` as above. `UiCapture::pointer` is true for the whole drag, including the release frame; `scroll` is true while auto-scroll moves a view. **Agents:** `ui_drag { source, target? }` runs a whole drag as a user would (start, enter, drop or cancel when the target does not accept), emitting the same events (`NotUndoable`, journalled); `ui_set_field` edits `drag` and `drop`; `ui_get_node` shows the live `DragState`. A drag survives a hot reload when its source survives.

### How the arena is shaped for binding

This section exists because data binding is the one feature that changes the arena (see Goal). Everything below is already true of the arena built by U1 to U10, so building the layout core first costs nothing.

1. **Bindings are records outside the arena.** `BindingTable` is a slab of fixed-size `BindRecord`s (target, source, format, the last applied value, a chain link). A node holds only `NodeCold::bind_head`, the first record of its chain, and the `bound` flag in `NodeHot::flags`. The paint walk and layout never touch the table, so an unbound node pays nothing and `NodeHot` has no field for it. At most `MAX_BINDINGS_PER_NODE = 8` records per node (`UiError::TooManyBindings`). A record is at most 64 B, so 10,000 bindings are 640 KB.
2. **Bindings write only through the setters.** A binding apply is `set_text`, `set_num`, `set_progress`, `set_value`, `set_on`, `set_selected`, `set_count`, `set_enabled`, `set_visible`, `set_image` or `set_tint`, so it uses the same equal-value no-op and the same dirty flags as a hand call. There is no second dirty path to keep right, and a binding whose value did not change costs a compare.
3. **A `runtime` flag, and reserved slots for undo.** A node is `runtime` when the crate creates and destroys it on its own: virtual-list rows, the tooltip, dropdown popup and drag ghost roots, and the immediate-mode layer's nodes. Every other node (commands, markup, `UiNodeDesc` builds, Rust game code) is authored. The flag is a bit in `NodeHot::flags`, set at creation, and decides only two things: **commands cannot name a runtime node** (`UiError::RuntimeNode`), and **`detach` frees a runtime descendant at once and does not record it**, so `DetachedSubtree` holds the authored nodes only and a restored list re-realizes its rows on the next pass. It plays no part in `restore`: undo safety comes from reserving the removed slots, not from separating who allocates ("Stable ids and undo"). The Jam-era design with two LIFO free lists is gone, because a free list cannot restore slot generations after other allocations (round 1, finding 1).
4. **`detach` takes the bindings with it.** `DetachedSubtree` also holds the removed nodes' binding records, so undoing a removal restores the bindings, and a removed node never leaves a record that points at a dead id.
5. **A manual edit of a bound field stays until the source next changes.** A binding is applied when its source changes (see "Data binding"), not every frame. So `ui_set_field` on a bound label's text shows the edit, and the next change of the bound value overwrites it. This is the rule for agents and the inspector: edit the binding (a command on the `bindings` field of `UiNodeDesc`) to change what a field follows.
6. **`UiNodeDesc` carries the bindings** (`bindings: Vec<UiBinding>`), so `build` creates them, `describe` returns them, markup writes them, and undo and reload see them like any other field.
7. **Lists re-apply rows without nodes changing.** A row template's records (`BindSource::Row`) are instantiated per realized row into the same table and freed when the row is recycled (runtime nodes are never in a `DetachedSubtree`).

What U1 to U10 must therefore build: the slot states, the reserve and restore operations, the release queue and the `runtime` flag (U1), `bind_head`, `NodeExtra` and the `bound` flag (U2), `UiNodeDesc::bindings`, `drag`, `drop`, `focus` and `tooltip` as empty-defaulted fields (U3). U23 fills in the table, the setters' binding calls and the `ui_bind` system.

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

- **Dispatch.** `Panel`, `Button`, `Toggle` and `Tabs` go to `compute_flexbox_layout` (`Tabs` is a column; its hidden pages go to `compute_hidden_layout`). `ScrollView` is a flex container whose children are laid out **without** a size limit on each enabled scroll axis (available space `MaxContent` on that axis), so content can be larger than the view. `Label`, `NumLabel`, `RichText`, `Image`, `ProgressBar`, `Slider`, `Dropdown` and `TextInput` are leaves measured by `compute_leaf_layout`. `Grid` and `VirtualList` use the two algorithms below.
- **Default sizes.** An `Auto` size on a leaf with no intrinsic size uses the theme: `ProgressBar` 100 x 8 (as before), `Slider` 160 x thumb height, an empty `Toggle` 44 x 24, `TextInput` `theme.text_input` size (200 x one line plus padding; independent of the text), `Dropdown` the widest option label plus the chevron by one line plus padding.
- **`Grid`.** `layout/grid.rs` places children in `columns` equal columns, left to right and then down. Column width is `(inner width - (columns - 1) * gap) / columns` and is definite, so each child is laid out with `compute_child_layout` at that width; a row's height is the tallest child in it; `gap` is both the column and the row gap. With an `Auto` width, the grid takes the available width. A child's `width` is ignored (it fills its cell) and its `height` is honored. There is no spanning and no taffy grid, so the crate keeps taffy's no-dependency feature set.
- **`RichText`.** The measure function splits the markup into words, measures each word with `measure_text` at its span's size and font (cached per `(markup, width)` on the node), breaks lines greedily at the offered width, and returns the block size. Layout calls `measure_text` once per distinct word and span, at most twice per dirty node per pass in the steady state, and never for a clean one.
- **Scroll content size.** After a scroll view's own layout, its content size is the union of its children's relative rectangles plus their margins; the post-layout walk stores it in the view's runtime state and re-clamps the offset (a content shrink moves the offset back into range).
- **`VirtualList` rows.** The list node is a leaf for its parent's layout: its size comes from its own style (normally `flex_grow` or a fixed size). After the root containing the list has been laid out, each realized row is laid out as an **independent fixed-size root** (`compute_root_layout` with the list's inner width and `item_extent`), so a row with a clean cache costs nothing and a scroll does not touch layout. Row `i` is placed at `list origin + (0, i * item_extent - offset)` (or along x for a horizontal list) by the absolute-rectangle walk.
- **Absolute rectangles and clips.** The post-layout walk that sets each node's absolute `rect` also (1) subtracts a scroll view's offset from its descendants, (2) records a **clip rectangle** per node, the intersection of the rectangles of all `ScrollView` and `VirtualList` ancestors (none for a node outside every scroll container), stored as a 1-based index into a per-pass clip table (0 = none, the same convention as `UiDrawList::clips`), and (3) writes it into each `HitEntry`, so a clipped-out button cannot be hovered or clicked. A scroll offset change re-runs only this walk for the scroll view's subtree, then rebuilds the hit list and bumps `paint_revision`; it never clears a layout cache.

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
| `set_choice` on a dropdown, `Auto` width / fixed width | none (the width is the widest option, not the chosen one) / none | yes |
| `set_options` on a dropdown, `Auto` width / fixed width | `invalidate_from(node)` / none | yes |
| text input edit, caret or selection move, blink flip, `set_input_text`, preedit | none (the size never depends on the text) | yes (a blink flip only bumps `paint_revision`) |
| drag start, over change, drop, ghost move, auto-scroll step | none (the ghost is its own root; a scroll step is `set_scroll`) | yes |
| dropdown open or close | none for the box; the popup root is built or removed as its own root | yes |
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
- **Any focused interactive widget** (button, toggle, slider, dropdown, text input, tab header, or a draggable node of any kind): a focus-ring quad (the rectangle outset by 2 UI-logical px, theme `focus_ring`) drawn before the widget's own quads. A text input draws it as its `border_focused` instead. While a **keyboard drag** is active, the ring marks the focused drop target, which may be a node that is not otherwise focusable.
- **Dropdown:** the box like a button (disabled, pressed, hovered, normal), the chosen label or the placeholder as a `Text` item, and a chevron quad. The open popup is its own root (layer 1001): a background quad, then per row a quad (highlighted or `option_selected` colors) and a `Text` item.
- **TextInput:** as described under "Text input" (box, border, selection quad, text clipped to the inner rectangle, caret quad, preedit underline).
- **Drag ghost:** its own root at layer 1002, painted like any other root with `theme.drag.ghost_opacity` applied through the opacity rule.
- **Button:** a quad colored by state with precedence disabled, pressed, hovered, normal (a class state patch or `theme.button`; a node override replaces every state). Children (the label) follow. Disabled buttons pass `text_disabled` to descendants' default label color.
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
4. **Order.** Render `layer = layer_base + 2 * item.layer` for graphics and `+ 1` for text (`layer_base` defaults to 10,000 and `mask` is configurable, both in the plain-data `UiRenderConfig` that this crate defines); `depth` is the item's index in the list; `space = Screen`; `clip` is `None` when the item's `clip == 0` and `Some(list.clips[clip - 1])` otherwise (divided by `dpi_scale` like any rectangle; the same wording as under "Clips"), for `SpriteDraw.clip` (quads and images) and for `TextDraw.clip` (a `Text` item carries its own `clip`, mapped to that item's `TextBounds`, so the batch plan's `Run.clip` is `None` for a text run: render guarantee 8); an `Image` item's `uv` becomes `SpriteDraw::uv_rect`. UI sits above the game world and below editor overlays by choice of `layer_base`. A solid quad is a sprite with `texture: None`; an image is a sprite with the texture resolved from the `Guid` (an image that is not loaded yet draws nothing).
5. **What the renderer must guarantee** (a requirement on #107, open question 1): items of equal `layer` draw in `depth` order, and all text of one `layer` goes in one text batch when no sprite of that layer sorts between its items. Expected count: one text batch per root layer, so even 60 root layers stay under `MAX_TEXT_BATCHES = 64`.
6. **Skip work when clean.** If `list.revision` is unchanged since the last frame, the glue reuses what it built last frame instead of reconverting.

### Input

Stage 1 input is data in, events out. The tree never reads a window, so it needs no platform dependency and tests are table-driven.

```rust
pub struct UiInputFrame {
    pub pointer: Option<Vec2>,           // UI-logical px (window px / ui_scale); None: not over the window
    pub primary_down: bool,              // level, not edge; the tree derives press and release
    pub tab: bool, pub activate: bool,   // edge-triggered: Tab, Enter or Space, or the pad's confirm button, went down this frame
    pub cancel: bool, pub shift: bool,   // cancel (Escape, or the pad's back button) is an edge; shift is a level that reverses tab
    pub nav: IVec2,                      // edge-triggered this frame: arrow keys, the D-pad, or the left stick crossing its threshold; x -1/0/+1 (left/right), y -1/0/+1 (up/down). Key repeat is the glue's job (the UI reads no clock)
    pub page: i8,                        // edge-triggered -1/0/+1: the pad's shoulder buttons (or PageUp/PageDown); changes the page of the focused node's `Tabs`
    pub scroll: Vec2,                    // wheel movement this frame in lines (platform `scroll_lines`), +y is content moving up
    pub text: Vec<UiTextEvent>,          // committed text, edit keys and IME preedit this frame, in order; cleared by the glue each frame, capacity kept
}
#[non_exhaustive]
pub enum UiEvent {
    Click(UiNodeId), HoverEnter(UiNodeId), HoverExit(UiNodeId), FocusGained(UiNodeId), FocusLost(UiNodeId),
    ToggleChanged(UiNodeId, bool), SliderChanged(UiNodeId, f32), TabChanged(UiNodeId, u32),
    DropdownChanged(UiNodeId, Option<u32>), DropdownToggled(UiNodeId, bool /* open */),
    TextChanged(UiNodeId), TextSubmitted(UiNodeId),
    DragStarted(UiNodeId), DragEnter(UiNodeId /* target */), DragLeave(UiNodeId /* target */),
    Dropped { source: UiNodeId, target: UiNodeId }, DragCancelled(UiNodeId),
    ModalCancel(UiNodeId),               // `cancel` pressed with no focus while this modal root is topmost; the game decides whether to close it
}
pub struct UiCapture {
    pub pointer: bool,     // gameplay must not react to the mouse this frame
    pub scroll: bool,      // the UI consumed the wheel this frame: gameplay must not zoom or scroll
    pub keyboard: bool,    // the UI consumed a navigation key or button this frame: gameplay must ignore Tab, Enter, Space, Escape, the arrow keys, the D-pad and the pad's confirm, back and shoulder buttons
    pub text_input: bool,  // a text-input widget has focus: gameplay must ignore every key. The glue also enables platform IME and text events by this flag
}

impl UiTree {
    pub fn process_input(&mut self, frame: &UiInputFrame) -> UiCapture;
    pub fn events(&self) -> &[UiEvent];  // everything from the last process_input
    pub fn drag(&self) -> Option<&DragState>;             // the live drag, valid while handling DragStarted/DragEnter/DragLeave/Dropped/DragCancelled
    pub fn input_text(&self, node: UiNodeId) -> Option<&str>;            // the text of a TextInput
    pub fn ime_area(&self) -> Option<Rect>;               // caret rectangle in physical px while a TextInput has focus
    pub fn take_clipboard_write(&mut self) -> Option<String>;           // set by Copy and Cut; the glue sends it to the platform clipboard
    /// Queue the events a real click on `node` would produce (what `ui_click` calls). They appear at the
    /// start of the slice returned after the next `process_input`. Errors for a node that is not an enabled interactive widget.
    pub fn inject_click(&mut self, node: UiNodeId) -> Result<(), UiError>;
}
```

`process_input` clears `events` first and returns the new capture, so every system that runs after it in the same frame reads the same slice (there is no drain). Rules, evaluated in this order each frame:

1. **Hit test.** Walk `hit_list` back to front (topmost first) and take the first entry whose absolute rectangle contains the pointer. The list holds only nodes whose resolved `PointerMode` is `Block`, plus interactive widgets (rule 9) and scroll containers, so the cost scales with the number of interactive widgets, not nodes. The hit is the *topmost blocking node*, which may be a panel covering a button, in which case the button is not hovered. Invisible subtrees and a `None` pointer hit nothing.
2. **Hover.** The hovered node is the hit if it is an enabled interactive widget (rule 9), else none. Changes emit `HoverExit` then `HoverEnter` and mark paint dirty.
3. **Press** (`primary_down` rising): the press origin is `Ui` if there was a hit, else `Game`. If the hit is an enabled button it becomes `pressed`. **Any press clears keyboard focus** (`FocusLost` if there was one): a mouse press never gives a button keyboard focus. Two widgets are the exception, because a pointer user expects them to take the keyboard: a press on a **text input** (rule 14) or a **dropdown** (rule 12) focuses it; the dropdown keeps that focus only while its popup is open (a close by pointer clears it, rule 12). A press on the widget that **already holds** the focus keeps it and emits no `FocusLost` or `FocusGained`.
4. **Release** (`primary_down` falling): if the pressed button is still the hovered one, emit `Click(button)`. The pressed state always clears. Releasing elsewhere is not a click.
5. **Keyboard and gamepad focus.** Focus is reached by `tab` and by `nav` (see "Focus navigation" below): `tab` moves it to the next focusable node in tab order (previous with `shift`), wrapping, `nav` moves it to the neighbour in that direction. **With no focus the two edges differ** (the one rule, repeated in "Focus navigation", "Entering the UI"): `tab` (and `shift+tab`) **always enters** the UI when a focusable node exists, which keeps the Jam behaviour (the first focusable node in tab order, the last with `shift`; an up `autofocus` or modal root is entered through its `initial_focus` instead), while `nav` enters **only** an `autofocus` or modal root and otherwise goes to gameplay, so arrow-key movement is not eaten by a plain menu or HUD. `activate` with a focus emits `Click(focused)` (or the widget's own result, rule 9, or starts a keyboard drag on a draggable node, "Drag and drop"). `cancel` with a focus clears it. Focus changes emit `FocusLost`/`FocusGained`. A **focusable** node is an enabled, visible, clipped-in interactive widget (rule 9) that is not under a lower root than the topmost modal; **a node with an enabled `drag` setting is interactive and so focusable, whatever its kind** (rule 9).
6. **Capture.**
   - `pointer` follows the press origin. Origin `Ui`: true on the press frame, on every frame the button is held and **on the release frame**, so gameplay never sees the mouse-up of a UI press. Origin `Game`: false for the same span, even if the pointer drags over UI. No press in progress (including the frame after a release): true iff the pointer is over a hit-list node.
   - `keyboard` is true on a frame where the UI **consumed a key or button edge**: `tab` or `page` when at least one focusable node exists, a `nav` edge that moved focus, that entered an `autofocus` or modal root, or that a focused widget used (rule 11), or `activate`/`cancel` while a node had focus. A `nav` edge with no focus and no such root up is **not** consumed. It is **not** a level: a focused button does not capture WASD, and after a mouse click on a button (which leaves no focus) Space and Enter go to gameplay. A player who has pressed Tab and then wants to play presses Escape or clicks. A keyboard drag (rule 13) holds `keyboard` true for its whole length.
   - `text_input` is a **level**: true on every frame a text input has focus (including the frame it gains and the frame it loses focus), so gameplay ignores every key while someone types. It is false otherwise.
7. **Removal.** Removing a node clears hover, pressed and focus that name it, without events.
8. **Modal roots.** While at least one visible root has `modal: true`, the **topmost** modal root and the roots above it are the only input targets. The hit test stops at the modal root (a click on the scrim, or on any root below, hits the modal root's scrim and nothing else, so gameplay and lower UI see no press); Tab cycles only through focusable nodes inside the topmost modal root and focus that is outside it is cleared when the modal appears; **an open dropdown popup whose box lies below the new modal root closes (no `DropdownChanged`) and a drag whose source lies below it is cancelled (`DragCancelled`, no drop), in the same pass**, because the reserved layers 1001 and 1002 would otherwise keep them above the modal; `pointer` capture is true everywhere on screen. A `cancel` with no focus emits `ModalCancel(root)` and consumes the key. The modal's scrim is drawn as the first graphic of its root (`theme.modal_scrim`, viewport-sized).
9. **Interactive widgets.** Rules 1 to 7 are written for buttons and apply unchanged to every **interactive widget**: an enabled `Button`, `Toggle`, `Slider`, `Dropdown` or `TextInput` (tab headers are buttons), **and any node, of any kind, with an enabled `drag` setting** (a draggable `Panel` or `Image`; its `PointerMode::Auto` is already `Block`). They are hit-list entries, hover targets, press targets and focus targets. A **droppable that is not otherwise interactive** is only a drop candidate (pointer hover over it during a drag, and the keyboard drag path of "Drag and drop"); it is not focusable on its own. What differs is what the press and release do:
   - **Button, tab header:** `Click` on release over the same node; a tab header also sets the tabs' `selected` and emits `TabChanged`.
   - **Toggle:** the same click flips `on` and emits `ToggleChanged(node, on)`; `activate` with focus does the same.
   - **Slider:** the press sets the value from the pointer and starts a drag. While `primary_down` the value follows the pointer along the slider's axis even outside its rectangle; each change emits `SliderChanged(node, value)`. Release ends the drag with no `Click`. `nav` on a focused slider changes it by `step` (or 1% of the range) and emits the same event. A drag counts as a UI press origin for capture (rule 6).
10. **Scrolling.** `scroll` moves the innermost `ScrollView` or `VirtualList` under the pointer that can move in the wheel's direction; at its limit the wheel passes to the next scroll ancestor, and only if none can move does it fall through to gameplay. `UiCapture::scroll` is true on a frame where a scroll view consumed it. A scroll view or list is a hit-list entry for the pointer (it blocks like a panel with a background) and for the wheel. A scrollbar thumb is a press target whose drag moves the offset like a slider. Scroll changes need no layout ("The widget set"). The hit list stores each entry's clip rectangle (see "Layout of the new widgets"), and a point outside the clip hits nothing in that entry.
11. **Nav edges.** With a focus, a `nav` edge is first offered to the focused widget, which **uses** the axes it needs: a slider (its own axis), a tab header (`x`: moves the selection and the focus to the neighbouring header), an open dropdown (`y`), a text input (`x`: the caret, for the D-pad and the stick; the keyboard's arrows arrive as `Edit(Move)` and **not** as `nav`, so one press moves the caret once, see "Text input"), a focused node inside a scroll view that is not itself navigating (`y` scrolls it by one line only when no focusable neighbour lies that way). An axis the widget does not use **moves focus** ("Focus navigation"). With **no** focus, a `nav` edge goes to gameplay unless an `autofocus` or modal root is up (then it enters that root and is consumed), so arrow-key movement is not eaten by a visible HUD. (`tab` with no focus is different: it always enters, rule 5.)
12. **Dropdown.** A press on a closed dropdown focuses it and opens the popup (`DropdownToggled(node, true)`); on release over the box nothing else happens. The press focuses the box only so that `nav` y, `activate` and `cancel` can drive the open popup: **when the popup closes by pointer (a click on a row, or a press outside) the focus clears** (`FocusLost`), and when it closes by the keyboard (`cancel`, or `activate` on the highlighted row) the focus stays on the box (see "The widget set"). `activate` or `nav` y on a focused closed dropdown opens it with the highlight on the chosen option. Open: the popup root is the only hit target besides the box; a press on a row chooses it, a press anywhere else closes the popup and **is consumed** (neither clicks nor reaches gameplay; the frame is a UI press origin through its release). The wheel scrolls the popup's own scroll view. A row hover moves the highlight.
13. **Drag and drop.** Rules 1 to 4 decide the press; "Drag and drop" above gives the drag start, over, drop, cancel, auto-scroll and the keyboard path. During a pointer drag, hover on other widgets is suspended (no `HoverEnter` for them); `Click` is not emitted for the source on release after a drag started.
14. **Text input.** A press inside a text input focuses it and places the caret; it is a drag-select press origin. Text events (`UiInputFrame::text`) are applied **after** the pointer and key rules of the same frame, in order, to the focused field only; with no focused text input they are ignored (the glue should not send them, since `UiCapture::text_input` was false, but a stale frame is harmless).

Disabled buttons are not hit-testable as hover targets, not focusable and ignore `activate`; they still block (a `Block` node under the pointer captures it).

#### Focus navigation (keyboard and gamepad)

In scope because gamepad is in the Stage 2 exit and Steam is in the MVP: a menu must be fully usable with a D-pad and two buttons, and with arrow keys, Tab and Enter. The UI reads no device; the glue maps the keyboard, the D-pad and the left stick to `nav` edges (stick: crossing 0.5 emits an edge, with a repeat the glue owns), the confirm and back buttons to `activate` and `cancel`, and the shoulder buttons to `page`.

```rust
pub struct FocusLinks { pub up: Option<String>, pub down: Option<String>, pub left: Option<String>, pub right: Option<String>, pub order: Option<i16> }   // node names; reflected, in NodeExtra
```

- **Tab order.** `tab` and `shift+tab` walk the focusable nodes in tab order: nodes with `focus.order` set come first, ascending (ties in paint order), then the rest in paint order. It wraps. Inside the topmost modal root only (rule 8).
- **Spatial navigation.** A `nav` edge that the focused widget does not use moves focus to the best candidate in that direction. A candidate is a focusable node whose rectangle **centre** lies in the half-plane of the direction from the focused rectangle's centre. Its score is `along + 2 * across`, with both distances measured **between rectangle edges, not centres**: `along` is the gap on the movement axis between the focused rectangle's facing edge and the candidate's nearest edge (0 when they overlap on that axis), and `across` is the gap on the other axis between the two rectangles (0 when their spans overlap, so a candidate that overlaps the focused rectangle on the cross axis gets no cross penalty). The lowest score wins, ties going to paint order. If a node has an explicit **link** for that direction (`focus.up` and so on, a node name resolved at navigation time), the link wins over the score; a link to a missing, hidden or disabled node falls back to the score. No candidate: focus stays and the edge is **not** consumed, except inside a modal root, which swallows it so the game under it never sees it. Candidates inside a scroll view that are scrolled out of view are allowed, and landing on one **scrolls it into view** (`set_scroll`, no layout).
- **Entering the UI (the one rule; input rule 5 and U10 and U27 repeat it).** With no focus the two edges differ:

  | Edge | An `autofocus` or modal root is up | No such root |
  |---|---|---|
  | `tab` (`shift+tab` takes the last) | enters the topmost such root: focus goes to the node named by its `initial_focus`, else its first (last) focusable node in tab order; consumed | **enters the first (last with `shift`) focusable node in tab order** of the whole tree; consumed when a focusable node exists (the Jam behaviour, so a plain menu can always be tabbed into) |
  | `nav` | the same entry as `tab`; consumed | **not consumed**: it goes to gameplay, so arrow keys and the D-pad still move the player under a plain HUD or menu |

  A game that wants focus at once (a pause menu opening under a pad) calls `UiTree::focus_first_in(root)`, which `ui_focus` also does. Only `nav` needs an `autofocus` root to enter; `tab` never does.
- **Leaving.** `cancel` clears focus (rule 5). A modal root's `cancel` with no focus emits `ModalCancel`. Focus also clears when its node is removed, hidden or disabled (rule 7), and a modal appearing clears focus outside it (rule 8).
- **Pages.** `page` changes the selected page of the nearest `Tabs` ancestor of the focused node (else the only `Tabs` in the topmost root) by one, wrapping off, and emits `TabChanged`; it needs no focus on a tab header.
- **Dropdowns and drags** narrow the candidates as described in their sections: an open popup limits `nav` to its rows; during a keyboard drag the candidates are the enabled droppables that accept the payload, **exempt from the focusable-only rule above** (an `Image` with a `drop` setting qualifies), and `tab` does nothing. A node with an enabled `drag` setting is focusable like any interactive widget (input rule 9), so a draggable slot can be reached and started.
- **Cost.** One navigation step is a linear scan of the hit list's focusable entries: at most 20 us for 200 interactive entries, only on a `nav` or `tab` edge.
- **Agents.** `ui_focus { node?, step? }` focuses a node, clears focus, or applies one `FocusStep` (`Next`, `Prev`, `Up`, `Down`, `Left`, `Right`, `Page(i8)`) exactly as the edge would (`NotUndoable`, journalled); `ui_get_tree` and `ui_get_node` report the focused flag; `ui_set_field` edits `focus` links and the root's `autofocus` and `initial_focus`.

**How UI consumes input before gameplay (Stage 2).** The plugin owns a `UiInputFrame` resource (default: no pointer, nothing pressed) that **the facade glue fills each frame** from the platform input state (and after `process_input`, the glue reads `UiTree::ime_area()` and `take_clipboard_write()` and hands them to the platform). `exitium_ui` has no edge to `exitium_platform` (C1), so the conversion is glue (`app-platform.md`, F2 and F3 here) and the plugin never names a platform type. The plugin's `ui_input` system reads that resource, calls `process_input` and stores the result in a `UiCapture` resource. Gameplay input code checks `UiCapture` before reacting: `pointer` for the mouse, `scroll` for the wheel, `keyboard` for Tab, Enter, Space, Escape and the arrow keys, `text_input` for everything (a one-line guard; whether the engine's input helpers apply it automatically is open question 4).

**Frame order and `FixedUpdate`.** The contract lists the schedules as Startup, FixedUpdate (60 Hz by default), Update and render extraction; the exact in-frame order is #106's. This spec assumes `FixedUpdate` runs before `Update` in a frame, and states the consequence precisely:

- Capture is computed once per frame. All `FixedUpdate` steps in that frame (zero or several) see the same value.
- **Requirement on #106, met:** `ui_input` runs after the platform input system and **before `FixedUpdate`** in the frame, in set `UI_INPUT` of `First` (`app-platform.md`; open question 5). Press and release edges seen by gameplay in either schedule use this frame's capture, so the previous-frame-capture case the Jam text had to accept does not arise. A test asserts it (U11).

The platform side is settled in `app-platform.md`: the glue reads the platform `InputState` in its frame view, and the platform never masks input (capture flags are the contract).

### Stable ids and undo

Commands must be undoable (rule 3), and a command recorded after another may refer to a node the earlier command created. So ids must survive undo and redo. The mechanism is **reserve until the history entry is discarded**: a node removed by a command keeps its arena slots, out of every free list, so nothing can take them and undo is a plain re-occupy. (The Jam-era text restored the free-list order instead. That was wrong: restoring the order does not restore slot generations, and "remove X, add Y, undo, undo" failed with `RestoreConflict` under ordinary LIFO undo. It also assumed nothing but commands allocates, which hot reload and Rust game code break. Round 1, findings 1 and 2.)

- **Slot states.** `Occupied`, `Free` (on the one LIFO free list) and `Reserved(token)`. A reserved slot keeps its generation, is on no list, and fails every id lookup (the node is not there), exactly like a stale id.
- **`remove_subtree(node)`** is for game code, hot reload and the immediate layer. It frees the whole subtree at once (generation bumped, slots to the free list, bindings and mount dropped) and returns nothing to restore. Its ids fail from then on.
- **`detach(node) -> DetachedSubtree`** is for commands. In **pre-order** it records every **authored** node's full data (hot and cold) with its id, the pre-order slot list, the subtree root's parent id and index, the removed nodes' **binding records** and any **mount** whose roots are in the subtree, and sets each of those slots to `Reserved(token)` with the generation unchanged. Runtime descendants (virtual-list rows, see "How the arena is shaped for binding", point 3) are freed at once and are not recorded.
- **Release.** A `DetachedSubtree` holds an `Arc` of its slot list. When the last clone drops, it pushes `(slot, token)` onto the tree's `ReleaseQueue` (a small `Arc<Mutex<Vec<..>>>` shared with the tree; the one piece of interior mutability, see Threading). The tree drains the queue at the start of every operation that allocates a slot and at the start of `layout`: an entry whose slot is still `Reserved(token)` is freed for real (generation bumped, a slot at `u32::MAX` is retired instead, then pushed to the free list); any other entry (the slot was restored, or the token differs) is skipped. The command bus drops a command's undo state exactly when it discards the history entry (a new command clears the redo stack, the 1,000-entry and 16 MiB cap drops the oldest, `clear_history` on Stop), so **no hook in the bus is needed**. `Command` is `Clone`: clones share the `Arc`, the slots release when the last clone goes, and the token makes a drop after a restore harmless. **The journal copy does not hold undo state.** The bus journals a `clone_boxed` copy of every command (editor-mcp-mvp.md, "Journal"), which is a reflected clone (skipped fields come back as their `Default`, see the cross-spec table); the `DetachedSubtree` lives in a `#[reflect(skip)]` field of type `Option<DetachedSubtree>`, so the journal copy has `None` there and holds no `Arc`. Otherwise a 1,000-entry journal would keep reserved slots alive after the history entry that owns them was discarded. A test asserts `Arc::strong_count == 1` after a journalled `ui_remove_node`, and that discarding the history entry releases the slots while the journal entry still exists.
- **`restore(&mut self, &DetachedSubtree) -> Result<UiNodeId, UiError>`** validates everything first, then commits, so a failure changes nothing. Checks, in this order: the handle's `tree_id` is this tree's (`UiError::ForeignTree`); every recorded slot is `Reserved(token)` (`UiError::RestoreConflict`: only a released handle or a bug); the recorded parent id is a live node, or the subtree was a root (`UiError::ParentGone`); no name in the subtree is used by a live node outside it (`UiError::DuplicateName`). Commit: each slot goes back to `Occupied` with its **own generation, so every id is identical**, the data is written back, the subtree is re-attached at `min(index, parent.children.len())`, the binding records and the mount are re-inserted, `names` and `image_nodes` are rebuilt for the nodes, and `invalidate_from(parent)` runs. O(subtree), no search. A restored virtual list has no rows until the next `ui_lists` pass.
- **No stale id can alias.** A slot leaves `Reserved` in exactly two ways: `restore` (the old id resolves again, to its own node) or release (the generation is bumped first, so every id minted before it is dead). While a handle is alive nothing can allocate the slot, so no other node ever holds that `(index, generation)`. An id therefore resolves to the node it named or fails; it never names another node.
- **Cost and the memory bound.** Reserved slots live as long as their history entry, so the arena holds at most the removed nodes of 1,000 entries beyond its live size. **Each reserved node costs about 1.1 KiB** (its hot, cold and layout slots stay allocated in the parallel arrays, plus the copy of its data in the `DetachedSubtree`, plus any strings), so one `ui_remove_node` of a 10,000-node screen holds about 11 MB until its entry is discarded. The bus bounds history at 1,000 entries **and 16 MiB estimated from `size_of_val` and snapshot sizes** (editor-mcp-mvp.md, "History"), and `size_of_val` of a command cannot see that heap, so left alone the 16 MiB cap would not bound this. **This spec therefore asks the command bus for one hook** (cross-spec table): `Command::undo_bytes(&self) -> usize`, default `size_of_val(self)`, which the bus adds to its history estimate. `ui_remove_node` and `ui_build_tree` return `DetachedSubtree::byte_size()` (`reserved nodes * 1.1 KiB` plus the recorded strings and binding records) from it, so the bus drops the oldest entries when the sum passes 16 MiB, and dropping an entry releases its slots (see "Release"). U13 is `Blocked by` the issue that adds the hook, and a test asserts a 1,000-node remove reports at least 1 MiB. A fixed bench row pins `detach`, `restore` and release (see "Performance").
- `build(parent, &UiNodeDesc)` returns the new subtree's root id. **Its undo is `detach`, and the redo is `restore` of the `DetachedSubtree` that undo produced**, so a later command that names a node of the built subtree finds it at the same id after undo and redo. There is no "build with given ids" call. A build from a `.ui` file is the same operation plus a mount, which `detach` and `restore` carry.
- `reparent(node, new_parent, index)` returns the old `(parent, index)`; reparenting under one's own descendant is `UiError::Cycle`.
- **What is not undoable, and what that means (owner decision 2026-11-25, open question 15).** A **UI hot reload cannot be undone**. Neither can the immediate layer, virtual-list row recycling, the tooltip, popup and ghost roots, or text typed into a field (the field keeps its own history): they are driven by a file, by game code, by scrolling and by the user, and they use `remove_subtree`, builds and setters on live nodes, so they never touch a reserved slot. The consequences are specified, not left to chance:
  - An undo entry that names a node a reload **replaced** (changed kind or name, so a new id) fails with a stale-target error, and the bus reports the undo as failed; generations make that safe.
  - `ui_remove_node X`, then the author saves the `.ui` file (a reload), then Ctrl-Z: the restore **succeeds** when X's parent is still there and no node with X's name was built in the meantime. It fails with `ParentGone` or `DuplicateName`, changing nothing, when the reload replaced X's parent or built a node with X's name. A failed undo is reported and the history carries on.
  - The reload's own edit is never in the journal; the file is the source of truth and version control is its undo.

### Reacting to asset changes

Asset hot reload (assets spec D12 and D13, PR #166) is in the MVP. The UI side has two inputs and one rule: **the tree never reads an asset; the facade glue tells it what changed.** The glue reads the assets spec's `AssetLoaded { guid, kind, generation }` and `AssetFailed` events in `First` (see "Schedules"), before `UI_MARKUP` and `UI_INPUT`. **Exactly one glue issue owns each call, and the UI plugin never touches the asset server** (see "Who calls what" below).

| What changed | Glue calls | Effect |
|---|---|---|
| An image's pixel size changed (a reload with different dimensions, or a first load of an image the tree already names) | `UiTree::image_changed(image: Guid) -> usize` | Looks the `Guid` up in `image_nodes`, and for each node that uses it **with a size that depends on the image** (`Auto` width or height, `Contain`, `NineSlice`) calls `invalidate_from(node)`; bumps `paint_revision`; returns the number of nodes touched. **O(users of that image)**, no tree walk. |
| An image reloaded with the **same** size | nothing | Draw items name a `Guid`, so the next frame already draws the new pixels. The glue's texture lookup is the render side's job (assets spec D13). |
| A font reloaded or loaded | `UiTree::font_changed(font: Option<Guid>) -> usize` | Clears the layout cache of every text node (`Label`, `NumLabel`, `RichText`, tooltips) whose resolved font is `font` (`None` is the theme default) and bumps `paint_revision`. This is one linear pass over the arena. Font changes are rare (a developer replaced the file), so there is no index; the cost is O(nodes) once per change. |
| A `.ui` asset loaded or reloaded | fills the `UiMarkupStore` resource (the facade glue takes the payload with `take_loaded_guid` and writes the store) | See "Markup and hot reload". |
| A reload failed (`AssetFailed { reload: true }`, an I/O or decode failure) | nothing for images and fonts (the old value stays, D12) | For `.ui`: **the store ignores it** and keeps the last good text. The UI records nothing: the asset system reports its own failure (`AssetStatus.failed_list`, assets spec). A `.ui` file that was read fine but does not parse is a different case, a **parse error**, which belongs to the mount (see "Markup and hot reload"). |

**Who calls what (asset glue ownership).** Both this spec and the assets spec (PR #166: FA1 and FA5) had claimed the `image_changed` and `font_changed` calls. The split below is by **what happened**, so no call has two owners and none falls in a gap. `AssetLoaded.generation == 1` is a first load, `>= 2` a reload that replaced a value.

| Job | Owner | When and how |
|---|---|---|
| **Request** the images and fonts the tree names | **FA1** (assets, `AssetGluePlugin`) | `PostUpdate`, when `UiTree::asset_refs_revision()` moved: reads `referenced_assets` and requests each `Guid` once. It does **not** walk the draw list or `paint_revision`. |
| **Request** `.ui` assets | **FA1** | Drains `UiMarkupStore::wanted` each frame into requests. Needs U24 (the store), so FA1 is blocked by U24 as well as U11. |
| Image **first load** (generation 1): `image_changed(guid)` | **F2** (this spec) | `First`, after FA1's delivery has inserted the texture: the tree may have laid out with `image_size == None` (an `Auto` or `Contain` node measured 0), so each user relayouts once. O(users of that image); a no-op when the tree does not name it. |
| Image **replace** (generation >= 2) whose size changed: `image_changed(guid)` | **FA5** | `First`, after the swap, when `TextureStore::size(guid)` differs across the replace. A same-size replace calls nothing. |
| Font **first load** (generation 1): `font_changed(Some(guid))` | **F2** | `First`, after FA1's delivery, **only when the font is in `referenced_assets`** (the call is an O(nodes) pass, so F2 filters). |
| Font **replace** (generation >= 2): `font_changed(Some(guid))` | **FA5** | On `RenderAssetReplaced { kind: Font }`. The signature takes `Option<Guid>` (`None` is the theme default font), so the call passes `Some(guid)`. |
| `.ui` payloads: `take_loaded_guid`, fill `UiMarkupStore` (generation 1 on load, `>= 2` on reload; a failed reload is ignored) | **F2** | `First`, before `UI_MARKUP`. This is a facade call: `exitium_ui` has no edge to `exitium_assets` (C1). Assets rows "Text" in the delivery and hot-reload tables name "the UI plugin" for this and must be corrected (see the cross-spec table). |
| Clip table and `uv` conversion, the units of the draw list | **F2** | `Extract` (see "Render handoff"). |

F2 never requests an asset, and FA1 and FA5 never touch a `.ui` payload. The first-load row exists because FA5 is a *replace* rule: nothing else tells the tree that a `Guid` it already named has become measurable.

**The image index.** `image_nodes: HashMap<Guid, Vec<UiNodeId>>` lists the nodes whose `ImageData::image` is that `Guid`. It is updated by every operation that can change a node's image or its existence: `build`, `detach` and `restore` (on the nodes of the subtree), `set_image`, and a widget-kind change. A node is added once even if the `Guid` is set twice, and an empty entry is removed. `image_changed` never allocates beyond the returned count. Test: build 5,000 nodes over 10 images, call `image_changed` for one image, and assert exactly its users were invalidated (`LayoutStats` shows only their ancestor chains laid out on the next pass) and the others were not.

**Which assets the tree names.** The assets spec's glue (FA1) must request the images and fonts a tree uses, and this crate has to be able to say which. `UiTree::asset_refs_revision() -> u64` increases whenever the set could have changed (build, `detach`, `restore`, `remove_subtree`, `set_image`, a theme or class change, a rich-text edit, an edit of a `drag` setting's ghost), and `UiTree::referenced_assets(&self, out: &mut Vec<UiAssetRef>)` appends every `UiAssetRef::Image(Guid)` (the keys of `image_nodes`) and `UiAssetRef::Font(Guid)` (the theme font and the fonts of rich-text spans), plus the `Guid` of every `DragGhost::Image` in the tree (an `Image` ref), deduplicated. It is O(distinct assets plus rich-text nodes), and the glue calls it only when the revision moved. **`.ui` assets are requested the other way round:** `ui_build_tree { markup }` for a `Guid` with no entry in an existing `UiMarkupStore` fails with `UiError::MarkupNotLoaded` and pushes the `Guid` onto `UiMarkupStore::wanted`, which FA1 drains each frame into asset requests (see "Markup and hot reload"); a game's own request goes through the same `wanted` list. Test: the three lists after build, undo, redo, a theme font change and a `DragGhost::Image` edit.

Unknown size (`UiMeasure::image_size` returns `None`) draws nothing for `Contain` and `NineSlice` and the whole rectangle for `Stretch`, as before. F2 calls `image_changed` when the image finishes its first load (generation 1, see "Who calls what"), so a node that was waiting for its size relays out once.

### Data binding

A binding makes a node field follow a value in the game's state. It is **one-way** (state to UI; the UI reports user input as events, see "Input") and **change-only**: a value that did not change costs a tick comparison and nothing else.

```rust
pub struct UiBinding {                       // authored form, a field of UiNodeDesc (so markup, build, describe and undo all carry it)
    pub target: BindTarget,
    pub source: BindSource,
    #[reflect(default)] pub format: BindFormat,
}
#[non_exhaustive]
pub enum BindTarget { Text, Num, Progress, Value, On, Selected, Count, ListSource, Enabled, Visible, Image, Tint, Choice, Options, DragId }
pub enum BindSource {
    Resource  { resource: String, path: String },                       // a reflected resource by registered type name, then a reflect path ("gold", "stats.hp")
    Component { entity: Entity, component: String, path: String },      // code and commands only; not allowed in a .ui file (no entity references there). See the note on despawn undo below
    Row       { path: String },                                         // a field of the list item at this row's index; only inside a VirtualList template
}
pub enum BindFormat { Direct, Number(NumFormat), Normalize { min: f64, max: f64 }, Not }
```

| Target | Sets | Accepts |
|---|---|---|
| `Text` | `Label` text, `RichText` markup, `TextInput` text (`set_input_text`, state wins) | `String`, any integer or float (formatted with `BindFormat::Number`), `bool`, `Num` (formatted) |
| `Num` | `NumLabel` value | `Num`, or an integer or float converted to `Num` |
| `Progress`, `Value` | `ProgressBar`, `Slider` value | `f32`/`f64` (0..=1 for progress), or any number with `Normalize` |
| `On`, `Selected` | `Toggle` on, `Tabs` selected | `bool`; an unsigned integer |
| `Count` | `VirtualList` count | an unsigned integer |
| `ListSource` | `VirtualList` count and rows | a reflected list (`Vec<T>`): the count follows its length and the rows' `Row` sources index into it |
| `Choice` | `Dropdown` selected option (`set_choice`) | an unsigned integer, or `Option<u32>`; an index out of range shows the placeholder |
| `Options` | `Dropdown` options (`set_options`; `value` is the label, enabled) | a reflected `Vec<String>`; a change replaces the options |
| `DragId` | `DragSource::payload.id` (`set_drag_payload`) | an unsigned integer; with `Row` it keeps a recycled row's payload right |
| `Enabled`, `Visible` | enabled flag, `style.visible` | `bool` (`BindFormat::Not` inverts) |
| `Image`, `Tint` | image `Guid`, tint | `Guid` (or an asset reference), `Color` |

A source whose type does not fit the target is **a binding error**, not a panic: the record stops applying, the first occurrence is logged and `ui_get_stats` lists it (`binding_errors`) with the node, target and message. It resumes when the node's bindings are edited or the source type changes. A missing resource, a despawned entity or a missing component pauses the record the same way (`SourceGone`) and resumes when the source is back.

**How change-only works.** The table groups records by source (one group per resource, one per `(entity, component)`). Each frame `ui_bind` does, in `PostUpdate` (see "Schedules"):

1. Take one cursor for the pass, `this = world.increment_change_cursor()`. For each group, ask the ECS whether the source changed **since the group's own stored cursor** (`None` until its first poll, which counts as changed), then store `this` in the group. A resource group asks `ReflectResource::is_changed_since(&self, world: &World, since: ChangeCursor) -> Option<bool>` (a new API of `ecs-scheduler.md`, PR #168, see the cross-spec table): `None` means the resource is absent (the group pauses as `SourceGone`), and an **expired cursor counts as changed**. A component group reads `EntityRef::component_ticks` for the component against `world.resolve_cursor(since)` and treats `Err(CursorExpired)` as changed. An unchanged group is skipped at the cost of one tick comparison, however many records it holds. Editor and agent edits stamp the tick (ECS spec), so authoring changes show up the same way.
2. For a changed group, each record reads its field through a **pre-parsed path** (parsed once when the record is created, never per frame) with `Reflect::reflect_path`, converts it, and compares with the record's `last` value. Only a different value reaches the setter.
3. The setter then applies its own equal-value check and dirty rules ("Invalidation strategy"), so a bound counter with a fixed size repaints with no layout, and one whose text length changes re-lays-out only its ancestor chain.
4. A newly created or edited record is applied once on the next pass whether or not its source changed.

**How `ui_bind` declares its world access.** The ECS scheduler has no `&World` system parameter, and a system cannot hold `ResMut<UiTree>` and read other resources through the same world. So `ui_bind` (and `ui_lists`, which reads `Row` sources the same way) is an **exclusive system** (`fn(&mut World)`, `System::is_exclusive`): the executor runs it alone at a sync point in `PostUpdate`, and its access claim is the whole world. Inside it works in two phases over a scratch buffer kept in a `UiBindScratch` resource (taken out with `std::mem::take` at the start and put back at the end, so its capacity is reused and a warm pass allocates nothing): **phase 1** has only `&World` (the `UiTree`'s table is read through `world.resource::<UiTree>()`, the registry's read guard is taken once, the cursor is taken, groups are polled, changed records are read through their pre-parsed paths, converted, compared with `last`, and the differing ones pushed to the scratch as `(node, target, value)`); **phase 2** takes `world.resource_mut::<UiTree>()` and applies the scratch through the setters, storing the new `last` values. This is also what the ECS requires for correctness: `is_changed_since` reports a change exactly once only if **nothing writes the resource between this pass's cursor mint and its polls** (a write in that window is stamped after the mint, is seen by this poll, and is seen again by the next one), which an exclusive system guarantees: while it runs no other system runs. The pass therefore never runs beside a writer, in `Frozen` mode or in play. **The cost is real, and is not "no parallelism lost":** `PostUpdate` is a `Parallel` schedule (app-platform.md) and the ECS spec's executor (PR #168) is parallel in the MVP, so each exclusive system is a **barrier** (about 1 to 1.5 us of dispatch each, ecs-scheduler.md "Performance"), and every other `PostUpdate` system waits for the pass's duration. Two exclusive systems, `ui_bind` and `ui_lists`, are two barriers per frame; the pass durations are the bind rows of "Performance" (1 us when nothing changed, at most about 1.5 ms in the 10,000-counter row), and the barriers are in the UI-side CPU rows. Correctness does not depend on the cost; if the barriers show up in a profile, merging the two systems into one exclusive pass is the first answer.

**A `Component` source and despawn undo.** A `BindSource::Component { entity }` holds an `Entity`. The bus's `rewrite_live_references` walks registered types and does not see `UiTree`, so after the undo of an `ecs_despawn_entity` (which respawns with a **new** id) a binding set through `ui_set_field` still holds the old id and stays `SourceGone` until its author re-points it. This is documented rather than remapped: a tree bound to scene entities is better served by `Resource` sources, which have no ids, and `.ui` files cannot name entities at all. (The alternative, a `remap` hook that rewrites the tree's records, is a bus extension this spec does not ask for.)

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
// UiNodeDesc: name, class, tooltip, kind (WidgetKind data), style, root, bindings, drag, drop, focus, children. All but `kind` are #[reflect(default)].
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
    pub fn mount(&mut self, asset: Guid, doc: UiDocument, stamp: MarkupStamp, parent: Option<UiNodeId>, apply_theme: bool) -> Result<MountId, UiError>;
    pub fn reload(&mut self, asset: Guid, text: &str, generation: u32) -> ReloadOutcome;   // every mount of `asset`; see below
}
pub struct MarkupStamp { pub generation: u32, pub text_hash: u64 }   // of the text `doc` was parsed from; `MarkupStamp::NONE` (generation 0) for a document built in code
pub struct UiMount {
    id: MountId, asset: Guid, roots: Vec<UiNodeId>, apply_theme: bool,
    doc: UiDocument,                  // the last good document, with its subtree hashes
    generation: u32,                  // of the last GOOD document
    attempted: u32,                   // of the last parse attempt, good or bad: the retry guard
    text_hash: u64,                   // hash of the last attempted text
    error: Option<UiParseError>,      // of the last attempt; cleared by the next good one
}
```

`mount` builds the document's roots (atomically: a duplicate name or a bad root builds nothing) and records the document, so a later reload can diff against it. **The initial retry state comes from the stamp:** `generation` and `attempted` both start at `stamp.generation`, and `text_hash` at `stamp.text_hash` (the hash of the text with the same hasher `reload` uses), so the first `ui_markup` pass after a mount finds `store generation == attempted` and does **not** reparse the document it was just built from. `ui_build_tree { markup }` takes the stamp from the `UiMarkupStore` entry it parsed. A document built in code (no store entry) has no subtree hashes and no stamp: `mount` runs the same bottom-up hash pass over it before storing it, and uses `MarkupStamp::NONE` (generation 0), so the store's first entry for that asset (generation >= 1) triggers one reload that finds the text equal or different and reconciles. With `apply_theme`, the document's `theme` replaces the tree's theme on mount and on each reload; a theme file is simply a document with a `theme` and no roots. The command `ui_build_tree` takes either a `tree: UiNodeDesc` or a `markup: Guid` (exactly one, `UiError::BuildSource` otherwise) and mounts with `apply_theme == false`. **A theme loaded from a file applies only when game code mounts it with `apply_theme` (owner decision 2026-11-25, open question 16): `ui_set_theme` takes no theme and no `Guid`, and an agent changes the theme with `ui_set_theme { path, value }` field by field.** Undo of a markup build is `detach` of the mount's roots, which also removes the mount; redo is `restore`, which brings it back (`DetachedSubtree` records the mount, see "Stable ids and undo").

**`ui_build_tree { markup }` across U13 and U24.** U13 registers the command with its final schema (both arms, so the schema never changes later: `tree`, `markup`), and the `BuildSource` check is live from U13. **The two errors mean two different things, and only these:** `UiError::MarkupUnavailable` means **the `UiMarkupStore` resource is absent** (the plugin never inserted it); `UiError::MarkupNotLoaded(guid)` means **the store exists and has no entry for the `Guid`** (the normal case before the asset has loaded). There is no third state: the store maps a `Guid` to loaded text, so an entry "still loading" cannot exist (a loading asset has no entry yet and is `MarkupNotLoaded`). Until U24 the `UiMarkupStore` type does not exist and no mount exists, so in U13 the `markup` arm always returns `MarkupUnavailable` (a `CommandError`, world unchanged, nothing journalled as applied). U24 adds the store (inserted by `UiPlugin::build`) and fills the arm in without touching the schema; from then on `MarkupUnavailable` is only seen if a game removed the resource. With the store present, a `Guid` with no entry returns `MarkupNotLoaded(guid)`, changes nothing, and pushes the `Guid` onto `UiMarkupStore::wanted` so FA1 requests it; the caller retries once `asset_*` reports it ready (or a later frame). A parse failure of an entry that exists carries line and column. All are command errors, not panics, and a test pins each.

**The asset side.** `exitium_ui` has no edge to `exitium_assets` (C1). The plugin owns `UiMarkupStore`, a resource mapping a `.ui` asset's `Guid` to `{ text: Arc<str>, generation: u32 }`, plus `wanted: Vec<Guid>` (assets the UI wants loaded; the glue drains it). The **facade glue** (F2, which takes the payload with `take_loaded_guid`; the plugin never touches the asset server) fills the map from `AssetLoaded` events in `First`: a load stores generation 1, a reload stores the new text with `generation >= 2` (assets spec D12), and a failed reload (`AssetFailed { reload: true }`) is ignored by the store, which keeps the last good text. The store holds **no error**: a parse error belongs to the mount (`UiMount::error`), so the one type that can hold it (`UiParseError`) is the only thing stored there, and I/O failures stay with the asset system. `ui_markup`, the plugin's system in set `UI_MARKUP` of `First`, finds the mounts whose store generation is newer than their **`attempted`** generation and calls `UiTree::reload`.

**Hot reload.** `reload(asset, text, generation)` has one swap point (assets spec D13: the swap happens in one place per frame; the UI's is `UI_MARKUP`, before `UI_INPUT`, so no input is processed against a half-applied tree):

0. **Retry guard.** `ui_markup` calls `reload` only when the store's generation is newer than the mount's `attempted`, and `reload` sets `attempted = generation` **before** parsing, whatever the outcome. So a bad file is parsed **once per generation**, not once per frame, until a newer generation arrives (a fix saved). If the new text is byte-identical to the last attempted text (`text_hash` equal, a touched file), nothing else runs: the cost is one text hash.
1. **Parse and hash.** `text` becomes a `UiDocument` with the **subtree hashes of every `UiNodeDesc` filled in one bottom-up pass right after the parse** (O(nodes), counted in the "parse and hash" row, not in reconcile), or a `UiParseError { asset, line, column, message, generation }`.
2. **On error: the old tree stays untouched.** The error is stored in `UiMount::error` (cleared by the next good reload), logged once per generation, and listed by `ui_get_stats` (`markup: [{ asset, generation, attempted, ok, error, last_swap_ms }]`). The mount keeps its last good document and its nodes, so the game keeps running with the previous UI. Warnings (defaulted fields, unknown class names) are reported the same way and do not block the swap.
3. **Plan.** On success the new document is diffed against the mount's last good document, producing an edit list **without touching the tree**. A planning failure (a new name that collides with a node outside the mount) is treated like a parse error.
4. **Apply**, all or nothing, by the arena operations and the ordinary setters. The edit list is not undoable and is not recorded in the command journal: the file is the source of truth, and version control is its undo.
5. The mount stores the new document, `generation` and `text_hash`.

**The diff** is by name, with subtree hashes to skip unchanged parts. Every `UiNodeDesc` of a **parsed** document carries a 64-bit hash of its authored content and its children's hashes (std's `DefaultHasher::new()`, deterministic), filled in by the parse-and-hash pass above; the mount's old document already has them from its own parse. So `reconcile` only **compares** hashes and never computes one. `reconcile(old, new, live)`:

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
| Text input caret, selection, preedit and edit history | kept on a surviving node; the caret is re-clamped if the file changed the text. |
| Dropdown popup open | kept open on a surviving node; closed (without `DropdownChanged`) if the node is replaced or its options changed. |
| Drag in progress | kept while its source survives; cancelled (`DragCancelled`) if the source is replaced or removed. |
| Virtual-list rows | kept when the row template's hash is unchanged; re-realized when the template changed. |
| `UiNodeId`s | stable for surviving nodes. An undo entry that names a replaced node fails with a stale-target error and the bus reports the undo as failed. |

**The budget.** A reload must finish within **100 ms** for a 10,000-node document on the min-spec runner, parse and swap together, **with the next layout and draw rebuild on top staying within the frame rows of "Performance"**. The planned costs: parse is O(text), the hash pass is O(nodes) (about 3 to 8 ms for 10,000 nodes with SipHash; it is **inside** the budget and has its own row), reconcile is O(changed path and its siblings), and a reload that changes nothing costs one parse, one hash pass and one compare of the root. **The owner confirmed the hard 100 ms budget on 2026-11-25 (open question 14).** If a swap takes longer than 100 ms it still completes (a half-applied reload is worse than a slow one) and `ui_get_stats` reports `last_swap_ms` so an agent sees it. The benches pin the rows below, and the 5% gate holds them.

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
- **Windows** are flex columns on the theme's panel style and block input over their rectangles. They are placed by anchor and offset, not dragged. The layer has **no text input, dropdown or drag and drop**: an overlay that needs them builds a retained tree (or a `.ui` file).
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
| `ui_bind` (**exclusive**, see "Data binding") | `UI_BIND`, in `UI` | `PostUpdate` | first of the UI systems | yes |
| `ui_lists` (**exclusive**; realize virtual-list rows and apply their `Row` records, send `UiListRow`) | `UI_LISTS`, in `UI` | `PostUpdate` | after `UI_BIND` | yes |
| `ui_immediate_end` | `UI_IMMEDIATE_END`, in `UI` | `PostUpdate` | after `UI_LISTS` | yes |
| `ui_animate` (`advance(dt)`: tweens, tooltip delay, caret blink, double-click timer, drag auto-scroll) | `UI_ANIMATE`, in `UI` | `PostUpdate` | after `UI_IMMEDIATE_END` | yes |
| **`Headless` driver** (`layout`, `build_draw_list` with the fixed-advance measurer) | `UI_DRIVER`, in `UI` | **`PostUpdate`** | last of the UI systems | yes |
| **`External` glue** (the facade: measurer, `layout`, `build_draw_list`, conversion) | `EXTRACT` | `Extract` | after transform propagation | yes |

- **The public system set `UI`.** The plugin declares a public set `UI` in `PostUpdate`, **ordered `after_set(PROPAGATE)`** (the transform propagation set of `app-platform.md`), and puts the `PostUpdate` UI systems in it (`UI_BIND`, `UI_LISTS`, `UI_IMMEDIATE_END`, `UI_ANIMATE`, `UI_DRIVER`, chained in that order as sub-sets). Layout and the draw list are therefore computed from this frame's transforms, a system in another crate orders itself against all of the UI with one `before_set(UI)` or `after_set(UI)`, and `app-platform.md` (PR #167) names the `Headless` driver as a member of `UI`. Edges the UI declares and nothing else: `UI` after `PROPAGATE`; `UI_BIND` first; `UI_DRIVER` last.
- **Why the Headless driver moved.** The Jam text ran it at the end of `Update`. A frozen editor skips `Update`, so a headless tool or test that expects a laid-out tree after an edit would see stale rectangles. In `PostUpdate` the same tree is laid out in Edit mode, in Play, and in a game with no renderer.
- **Edits made while frozen.** A command or an `ecs_set_resource_field` between frames is picked up by the next `ui_bind`; a `.ui` edit by the next `ui_markup`; both appear in the next `Extract`. Nothing waits for `Update`.
- **`ui_animate` delta.** It passes the app clock's real (unscaled) frame delta clamped to 0.1 s, so transitions keep running while the simulation is paused or frozen. The resource name is `app-platform.md`'s.
- **Order is a contract.** The bind pass comes before list realization (a bound count sets the rows), both before layout (they change nodes), and the animation step before the driver (the draw list shows this frame's tween values).

**Cross-spec requirements this section records:**

| Spec | Row |
|---|---|
| `app-platform.md` AM3 (SP3, #139) | `SimulationMode::Frozen` runs `First`, `PostUpdate`, `Extract`, `Last` and skips the tick loop and `Update`. This spec's `UI_MARKUP`, `UI_INPUT`, `UI_BIND`, `UI_LISTS`, `UI_IMMEDIATE_END`, `UI_ANIMATE`, `UI_DRIVER` and the `External` glue are placed to satisfy it, and U11, U17, U22, U23, U25 and U26 carry a Frozen-mode test. |
| `assets-scene-export.md` (SP4, PR #166) | D12 (a bad reload keeps the good value) and D13 (one swap point) are met by "Markup and hot reload"; `.ui` is a `Text` asset whose content type is **`UiDocument`** (which may be a bare `UiNodeDesc` root, so the assets spec's wording is also accepted). The call ownership is the **"Who calls what"** table of "Reacting to asset changes": FA1 requests (reads `referenced_assets` when `asset_refs_revision` moved, drains `UiMarkupStore::wanted`); FA5 calls `image_changed` on a size change across a replace and `font_changed(Some(guid))` on a font replace; **this spec's F2** owns the first-load `image_changed` and `font_changed`, `take_loaded_guid` for `.ui` and the `UiMarkupStore` fill, and the clip and `uv` conversion. The UI plugin never touches the asset server (C1). **Three changes in PR #166 are required, and #166 is frozen, so the orchestrator files them as one follow-up issue against `assets-scene-export.md`:** (1) rows "Text" of the delivery table (L222) and the hot-reload table (L480) say the UI plugin calls `take_loaded_guid`; they must say the **facade glue (ui-mvp.md F2)** does, and fills `UiMarkupStore`; (2) FA1 is `Blocked by` **ui-mvp.md U24** as well as U11 (`UiMarkupStore::wanted` exists from U24), and its UI request step reads `referenced_assets` when `asset_refs_revision` moved (the glue row at L616 and FA1's fourth criterion say it walks the `UiDrawList` when `paint_revision` moved, which is not what this spec offers) and drains `wanted`; (3) FA5's text at L478, L1125 and L1130 passes `Some(guid)` to `font_changed(Option<Guid>)`, and its UI line says "replace" explicitly (the first-load calls are F2's). |
| `render-v1.md` (PR #171) | `SpriteDraw::uv_rect` (9-slice and image sub-rectangles) and the per-item `clip` (scroll views) exist and are used; the UI emits `Text` items per rich-text span because the render spec has no spans. **Alignment requested of #171:** copy this one wording into its UI glue text (L607 says "index 0 is `None`", which can be read as `clips[0]` being a none entry): **`clip == 0` means no clip, and `clip == k` means `clips[k - 1]`** (divided by `dpi_scale`). A `Text` item carries its own `clip`, which the glue maps to `TextDraw.clip` (per-item `TextBounds`), so the batch plan's `Run.clip` is `None` for a text run (render guarantee 8). |
| **`ecs-scheduler.md` (PR #168, ECS spec issue #138)** | **`ReflectResource::is_changed_since(&self, world: &World, since: ChangeCursor) -> Option<bool>`**: `None` when the resource is absent, and an expired cursor counts as changed. Used by `ui_bind` (U23 is `Blocked by #138`). Also used: `World::increment_change_cursor`, `World::resolve_cursor`, `EntityRef::component_ticks` (storage spec), exclusive systems (`System::is_exclusive`). `ui_bind` and `ui_lists` are exclusive so that no writer runs between a poll and the next cursor (see "Data binding"). |
| **`app-platform.md` amendment #177 (text input, this spec's widget)** | The platform side of `TextInput`, which this spec's issue U31 is `Blocked by #177` for: (1) **committed text** per frame (typed characters and IME results) as events in the frame view; (2) **IME**: enable and disable by request (this spec drives it from `UiCapture::text_input`), **preedit** events (`text`, optional cursor range) and **commit** events, and an **IME cursor area** setter taking a rectangle in logical pixels (this spec's `UiTree::ime_area`, divided by `dpi_scale`); (3) **clipboard**: a text write (`set_clipboard_text(&str)`) and a **paste delivered as an event** carrying the text (the browser clipboard is asynchronous and permission-gated, so a synchronous read cannot exist on wasm32); (4) **key repeat**: a press-or-repeat edge for Backspace, Delete, arrows, Home and End; (5) the text and edit keys reach the same injected-input path (`input_inject`) so an agent can type. The facade glue (F3) converts these to `UiInputFrame::text`. The UI names no platform type. |
| **`app-platform.md` (SP3 gamepad, #139) and #177** | **Gamepad** state for the glue: D-pad and left-stick edges with a threshold, and the confirm, back and two shoulder buttons as edges. If SP3's gamepad addition lacks any of these, #177 adds it. The glue (F3) maps them to `UiInputFrame::{nav, activate, cancel, page}`. |
| **`app-platform.md` (PR #167): the `UI` set** | `PostUpdate` set `UI`, `after_set(PROPAGATE)`; the `Headless` driver is a member (see "Schedules"). |
| `editor-mcp-mvp.md` AM6 | Closed by "Command and query surface". **One addition is requested of the command bus:** `Command::undo_bytes(&self) -> usize`, default `size_of_val(self)`, added to the history's 16 MiB estimate (L177 there), so a command whose undo state owns heap (here `DetachedSubtree`) is bounded by the cap instead of escaping it ("Stable ids and undo"). **This spec also relies on** the journal's `clone_boxed` copy being a reflected clone, in which a `#[reflect(skip)]` field comes back as its `Default`, and asks the bus spec to state it (its text only says "a `clone_boxed` copy"). The orchestrator files the request against the command-bus issue (CB9 adds the trait); U13 is `Blocked by` it. |
| `app-platform.md` (SP3, PR #167), stale text | L29 and L503 still say pad-driven UI navigation is a UI non-goal ("Spatial UI navigation by pad is `ui-mvp.md`'s call" and "an amendment to `ui-mvp.md`, which lists it as a non-goal"). It is in scope here ("Focus navigation (keyboard and gamepad)"). Both sentences should say the UI owns focus navigation and the facade glue (this spec's F3) maps the pad. Follow-up for **#177**, the amendment that already edits that file. |
| `app-platform.md` F2 (UI input and viewport glue) | This spec's F3 extends the system that app-platform.md F2 creates (the `First` system that turns `InputState` into `UiInputFrame`), so **F3 is `Blocked by` app-platform.md F2**, besides #177. |
| `serial-text-format.md` | `UiDocument` uses the file-load rules: spans in errors, `UnknownFields::Warn`, defaults from explicit `#[reflect(default)]`. |
| `mvp-roadmap.md` | The UI full lane is Stage 5 and runs beside Stage 4 (C6: `Blocked by` intake 3); the editor edge PR is C5; the facade item F2 cannot overlap a `exitium_ui` issue (R3). |

### Threading

`UiTree` is plain owned data (`Send + Sync`) and lives in a resource. **Its one piece of interior mutability is the `ReleaseQueue`** (a small `Arc<Mutex<Vec<..>>>` that a dropped `DetachedSubtree` pushes to, see "Stable ids and undo"); it is locked only on that drop and when the tree drains it before an allocation, never in a per-frame walk. Every UI system takes the resource by `&mut` (or `&` for read-only), so the scheduler serializes them. Layout, input, painting, binding and reload are single-threaded, as the app's executor is. Roots are independent layout subtrees with no shared cache, which allows a later parallel layout (splitting the arena by root) without a layout change; that is not designed here. The crate uses no threads, no clocks and no file I/O, so it builds and runs unchanged on wasm32.

### Stages

- **Stage 1 (`exitium_core` and `taffy` only).** The layout core: arena (with the reserved-slot state and the release queue), tree, styles, widget data types, theme types, desc, layout of the five Jam widgets, painting, the render-facing mapping helpers (`UiRenderConfig` and the conversions the glue calls), input, plus unit tests and benches. Nothing in Stage 1 names the ECS, the app, the platform or the renderer. The coupling points are three plain-data seams: `UiMeasure` (in), `UiInputFrame` (in), `UiDrawList` (out). **Stage 1 builds the final arena shape** ("How the arena is shaped for binding"), so nothing in Stage 5 reshapes it.
- **Stage 2 (`plugin` module).** A `UiPlugin` that registers, using the app spec's plugin mechanism (#106; exact `App`/`Plugin` signatures are an open dependency):
  - **Resources:** `UiTree`, `UiViewport` (synced from the window), `UiCapture`, `UiDrawList`, `UiRenderConfig`, `UiInputFrame`.
  - **Systems into `Startup`:** insert the resources with defaults. Game code builds its UI in its own `Startup` systems from Rust (builder or `UiNodeDesc`).
  - **System into the input position:** `ui_input`, in set `UI_INPUT` of the `First` schedule, consuming the `UiInputFrame` resource that the facade glue fills just before it. Game systems then read `UiTree::events()` and `UiCapture` and edit the tree.
  - **Layout and draw output are driven by the facade glue**, in `Extract`, because they need the renderer's text measurer (see "Render handoff"). `UiPlugin { driver: UiDriver }` has two drivers: `External` (default; the facade runs `layout` and `build_draw_list`) and `Headless`, where the plugin adds a system in set `UI_DRIVER` of **`PostUpdate`** that runs both with a fixed-advance measurer (see "Schedules"). `Headless` is for tests, tools and games with no renderer.
  - **Nothing into `FixedUpdate` or `Update`.** Gameplay that runs at the fixed rate may call tree setters; the dirty flags make that cheap and the next layout picks it up.
  - **Reflection derives** on the data types.
- **Stage 3 (commands and queries).** U13 registers the base commands and six queries with the command bus (see "Command and query surface"); U27 to U31 add the focus, choice, drag and text-edit simulation commands with their widgets. It is `Blocked by` CB9.
- **Stage 5 (the UI full lane, 18 serial PRs, U14 to U31).** Everything marked "full" in this spec: the rest of the widgets, themes and transitions, 9-slice and clipping, change-only data binding, markup and hot reload, the immediate-mode layer, focus navigation, the dropdown, drag and drop, text input, the plugin systems they need (`UI_MARKUP`, `UI_BIND`, `UI_LISTS`, `UI_IMMEDIATE_END`, `UI_ANIMATE`) and the 10,000-node and counter-rebinding benches. The lane starts beside Stage 4 and ends before the facade glue (the roadmap's R3). The facade item F2 adds the glue for the new draw data and the asset events, and F3 the text and gamepad glue. The edge `exitium_editor -> exitium_ui` is the roadmap's C5 PR, after the lane.
- **Crate edges added in Stage 2 and 5** are listed under "Wiring". `exitium_ui` does **not** depend on `exitium_render` or `exitium_assets`; the facade depends on all three.

## MCP / editor surface

Agent parity (rule 1): every UI capability is a command or a query registered with the command bus, with a reflected payload, so the generic agent and editor tooling that `exitium_mcp` builds from the registry exposes it with no UI-specific tool or panel. **This spec files no issue in `exitium_mcp` or `exitium_editor`;** it lists what `exitium_ui` must register. Mutation goes through the bus (rule 3) and schemas come from the reflected data types (rule 2, JSON Schema from `TypeInfo`, CB9).

### Command and query surface

This section implements AM6 of `editor-mcp-mvp.md`. **`exitium_ui` registers all eleven commands and all six queries itself**, from `UiPlugin::build`, with `register_command::<C>(world)` and `register_query::<Q>(world)`. The facade does no registration, so the "or the facade" alternative of the Jam text's open question 7 is closed. Each type implements `Command` or `Query` (`Reflect + FromReflect + Clone + Send`), with `NAME` set to the tool name; undo state lives in `#[reflect(skip)]` fields. Node ids travel as the text form `"{index}v{generation}"`, like `Entity`; a node may be addressed by its unique name instead. **U13 is `Blocked by` CB9** (the schemas and the registry), and registers nothing before it.

| Command (`NAME`) | Meaning | Undo |
|---|---|---|
| `ui_build_tree { parent: Option<Node>, tree: Option<UiNodeDesc>, markup: Option<Guid> }` | a whole subtree or a new root in one step, from a description **or** from a `.ui` asset (exactly one, else `UiError::BuildSource`); a markup build also records the mount. With `markup`, the text comes from `UiMarkupStore` (see "Markup and hot reload"): **no store resource is `UiError::MarkupUnavailable`, a store with no entry for the guid is `UiError::MarkupNotLoaded`** (and the guid is queued in `wanted`), and a parse failure carries line and column; none of them builds anything | **one command**: `undo` is `detach`, and the second `apply` (redo) is `restore` of the `DetachedSubtree` that `undo` stored in its skipped field, so ids are identical after undo and redo |
| `ui_add_node { parent, index, kind, name? }` | one node | `detach` |
| `ui_remove_node { node }` | remove a subtree | `restore` the `DetachedSubtree` (held in a `#[reflect(skip)]` field, so the journal's `clone_boxed` copy does not carry it, and reported to the bus by `Command::undo_bytes`, see "Stable ids and undo") |
| `ui_reparent { node, new_parent, index }` | move | reparent back |
| `ui_set_field { node, path, value }` | any reflected field of the node's shallow `UiNodeDesc`: style, widget data and live widget values (toggle `on`, slider `value`, tabs `selected`, scroll `offset`, dropdown `selected` and `options`, text-input `text`), class, tooltip, `drag`, `drop`, `focus` links, root config (including `autofocus` and `initial_focus`), name, `bindings` | previous value via `clone_boxed` |
| `ui_set_theme { path, value }` | any field of `UiTheme`, including classes. It edits the live theme only: **a theme file is applied only when game code mounts it** (owner decision, open question 16), so there is no theme argument | previous value |
| `ui_click { node }` | debug action: queues a `Click` (or the `ToggleChanged` or `TabChanged` a real click would produce) as if the pointer had pressed and released on the node; the events appear in the next frame's `UiTree::events()` | **`undo_kind` returns `UndoKind::NotUndoable`**: recorded in the journal only |
| `ui_focus { node?, step? }` | focus a node, clear focus (neither given), or apply one `FocusStep` (`Next`, `Prev`, `Up`, `Down`, `Left`, `Right`, `Page(i8)`) exactly as the keyboard or pad edge would | `NotUndoable`, journalled |
| `ui_choose { node, index: Option<u32> }` | choose an option of a `Dropdown` as a click on its row would (opens nothing; emits `DropdownChanged` when it changed). An out-of-range or disabled option is `UiError::BadChoice` | `NotUndoable`, journalled |
| `ui_drag { source, target? }` | run a whole drag as a user would: start, enter the target, drop (or cancel when the target does not accept or none is given); emits the same events | `NotUndoable`, journalled |
| `ui_text_edit { node, op: TextEditOp }` | run one text edit op on a `TextInput` as a user would (focuses it first); emits `TextChanged` or `TextSubmitted` | `NotUndoable`, journalled |

The four simulation commands exist because a user's focus move, choice, drag and typing are not edits of authored data: they are what an agent must be able to *do* to test a screen (agent parity, rule 1). Commands that name a node reject a `Runtime` node (list row, tooltip, popup, ghost, immediate-layer node) with `UiError::RuntimeNode`. `ui_set_field` is one generic command instead of one per field, built on the reflection path machinery (`apply`, `reflect_path_mut`) over the node's shallow description, so a new widget or style field is editable by agents and the inspector the day it is added. Structural edits stay explicit commands because they need ids and undo data. The eleven names are the tools `exitium_mcp` derives; the facade's parity test (C3) lists them.

**Saving a tree to a `.ui` file through MCP** needs no UI command: an agent calls `ui_get_tree` (the `UiNodeDesc` of a root), wraps it in a `UiDocument` (or writes the bare `UiNodeDesc`), renders it with `serial_write_text` (the serial spec's tool), writes the file through the editor's file tool and registers it with `asset_import`. The text a reload then parses is exactly what `describe` produced, so a saved tree reloads to itself (the round-trip test).

Read-only queries (each is a `Query`, so the bus checks it changes no observable ECS state):

| Query (`NAME`) | Result |
|---|---|
| `ui_get_tree { root?, depth?, fresh }` | the `UiNodeDesc` of the **authored** nodes (runtime nodes are listed as a count per parent, never as descriptions, since they have no description to save) plus each node's id, computed rectangle, interaction flags (hovered, pressed, focused, open, drag-over) and `runtime` flag. Bounded: `depth` caps the walk, and the 10,000-node reply is a bench row |
| `ui_get_node { node }` | one node's shallow description, rectangle and flags, and the widget's live state: dropdown `open` and highlight, text-input caret, selection and preedit, the live `DragState` while a drag runs |
| `ui_find { name }` | the node id for a name |
| `ui_get_theme` | the `UiTheme` |
| `ui_hit_test { x, y }` | the node under a UI-logical point |
| `ui_get_stats` | `LayoutStats`, node counts (authored and runtime), revisions, `text_changes`, active tweens, binding counts and `binding_errors`, `warnings`, and `markup: [{ asset, generation, attempted, ok, error, last_swap_ms }]` |

Computed rectangles are as of the last layout pass: a response carries `layout_revision` and `stale: true` when edits since then have not been laid out. **`fresh`** asks `ui_get_tree` to run a layout pass first, and it may do that **only through the non-stamping query path**: `QueryCx` allows a cache refresh only through `bypass_change_detection`, so the query takes `UiTree` and the plugin's `UiMeasurer` resource without stamping a change tick on either, runs `layout` (never `build_draw_list`: the draw list is the glue's output), and returns `stale: false`. A fresh query may advance `layout_revision` (a cache refresh) but changes no authored data and no ECS change tick. The measurer is the boxed `UiMeasure` the plugin keeps in `UiMeasurer`: `UiPlugin::build` installs a fixed-advance default, `Headless` keeps it, the facade replaces it with the renderer-backed one (F1), and while the fixed-advance default is still installed (no facade glue has run) a fresh query says so (`measurer: "fallback"`) rather than failing. Verifying how the UI looks is the screenshot capability owned by the render and editor specs, not this one.

**Open question 7 is answered:** a non-undoable action is `undo_kind() == NotUndoable` (`ui_click` and the four simulation commands); a bulk command is one command with its own undo state (`ui_build_tree`: `detach` and `restore`); registration is from the crate. **The pointer in open question 5 to #110's ordering is recorded as settled:** `app-platform.md` places `UI_INPUT` in `First` before every `FixedUpdate` tick and `EXTRACT` after transform propagation, and this spec's "Schedules" fixes the rest.

### What the editor needs from the UI

Listed so `exitium_mcp` and the editor can build it generically; nothing here is a `exitium_ui` deliverable beyond the registrations above:

- the node tree for a hierarchy view (`ui_get_tree`), with reparent by drag (`ui_reparent`) and delete (`ui_remove_node`);
- an inspector for the selected node generated from `UiNodeDesc` reflection (ranges, tooltips and `hidden` honored), writing through `ui_set_field`, and a theme editor over `ui_set_theme`, including class editing;
- a selection outline drawn from a node's `rect` (`ui_get_node`) by the editor overlay, and node picking from `ui_hit_test`;
- a palette of the widgets (the `WidgetKind` variants, discoverable through reflection);
- **drag and drop** in the hierarchy view: reparenting by drag is `ui_reparent`, and the editor's own drag of a palette entry onto the viewport is `ui_add_node` or `ui_build_tree` at the node under `ui_hit_test`;
- **dropdown, text input and focus** in the inspector: the generated inspector edits `options`, `selected`, `text`, `drag`, `drop` and `focus` links through `ui_set_field` like any field, and a "preview" mode drives `ui_choose`, `ui_text_edit`, `ui_drag` and `ui_focus` so a designer can try a screen without writing code;
- viewport mouse input translated to UI-logical pixels and fed to `process_input` as in a built game;
- the **UI editor** view of Stage 5 (editor extras), which edits a `.ui` file by `ui_build_tree { markup }` and the field commands, and relies on hot reload to show saved edits. It needs the `exitium_editor -> exitium_ui` edge (C5), which is not part of this spec.

Whether the editor reads the `UiTree` directly (which needs that edge) or only through the query channel is the editor's choice; this spec needs neither.

## Performance

Budgets are p50, single thread, on the min-spec reference runner (4 cores, 8 GB). Benches live in `exitium_ui/benches/` (criterion, per #18) and are held by the 5% regression gate once baselined; budgets may be re-baselined by the first bench PR with a note to the owner. The 10,000-node tree is 100 row containers of 99 fixed-size leaves each, the shape of a Glutton resource screen. The **full-widget 10,000-node tree** (U30) is the same count with the full widget mix: 40% labels and number labels, 20% panels and grids, 15% buttons and toggles, 10% images (a tenth of them 9-slice), 5% progress bars and sliders, 5% rich text, and 5% inside scroll views, plus one virtual list, 50 dropdowns (closed) and 50 text inputs, and 200 draggable and 200 droppable nodes among the buttons. **Build order:** U30 (before the text input exists) builds this tree **without** the 50 text inputs (the other widgets still make up the 10,000 nodes), and baselines the two full-widget rows on that tree; **U31 adds the 50 text inputs to the bench tree and re-baselines the two full-widget rows** (cold layout and draw rebuild), with a note to the owner for any row that moves by more than the 5% gate.

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
| Focus navigation step (one `nav` or `tab` edge), 200 focusable entries | <= 20 us |
| Dropdown open (popup of 50 options built into runtime slots, then laid out) / close | <= 100 us / <= 30 us |
| Drag frame (200 droppable entries, pointer moving, no target change) / start (payload copy, ghost of a 20-node source) | <= 5 us / <= 60 us |
| Text input: one typed character (edit, caret, scroll, one paint revision; **excludes shaping**) / paste of 1,000 characters | <= 5 us / <= 60 us |
| Arena: allocate then free one slot / `detach` then `restore` of a 100-node subtree / release of that subtree | <= 40 ns / <= 25 us / <= 5 us |
| Setters: `set_value`, `set_on`, `set_scroll`, `set_count`, `set_selected`, `set_choice` (equal value / changed value, fixed-size node) | <= 30 ns / <= 80 ns |
| `ui_get_tree` of a 10,000-node tree (describe all, no layout) | <= 3 ms |
| Grid layout, 1,000 cells, cold | <= 300 us |
| Rich text, 200 nodes cold layout (fixed-advance measurer) | <= 1 ms |
| Scroll offset change, scroll view with 500 descendants (rectangle walk, hit list, no layout) | <= 50 us |
| Virtual list, 100,000 items, one scrolled frame (about 40 realized rows, 2 recycled; warm) | <= 150 us, 0 allocations |
| `image_changed`, 100 users among 10,000 nodes / 1 user | <= 10 us / <= 200 ns |
| `referenced_assets`, 10,000-node tree, cached (no change) / rebuilt after a node edit | <= 100 ns / <= 300 us |
| `font_changed`, 10,000 nodes | <= 300 us |
| 512 active tweens, one `advance` | <= 40 us |
| **`ui_bind`, 10,000 records on one unchanged resource** | <= 1 us |
| `ui_bind`, 10,000 records on 10,000 components, none changed | <= 150 us |
| `ui_bind`, 1,000 counters on one resource, resource changed, 10 values differ | <= 80 us |
| `ui_bind`, 10,000 counters, 10% change per frame (read, compare, format, setter; **excludes text shaping**) | <= 1.5 ms |
| **Counter rebinding**: 1,000 counters rewired to new sources in one frame (record rewrite, path parse, first apply; no layout) | <= 500 us |
| Immediate layer, 200 unchanged widgets (calls and reconcile), warm | <= 100 us, 0 allocations |
| **`.ui` reload swap, 10,000-node document, one node changed** (hash text, parse, plan, apply, **hard budget, owner-confirmed 2026-11-25**) | <= 100 ms |
| Parse and subtree-hash of a 10,000-node document (the part of the swap that is O(document)) | <= 60 ms |
| Reconcile only (hash compares, no parse, no hashing), 10,000 nodes: unchanged document / one node changed / every node's style changed | <= 5 us / <= 1 ms / <= 20 ms |
| Hot-path allocations in a frame with no structural change and warm buffers | 0 |
| UI-side CPU per frame, 10,000 nodes, 1,000 hand-set counters changing (setters, layout, draw rebuild; **excludes text shaping**) | <= 2 ms |
| UI-side CPU per frame, 10,000 nodes, 1,000 **bound** counters changing (`ui_bind` and `ui_lists` included; **excludes text shaping**) | <= 3 ms |
| **Text shaping for changed labels** (renderer-owned, joint row with #107's `text/*` benchmarks): one changed short label (about 8 characters), shaped-cache miss | <= 20 us |
| **UI frame with shaping**: 100 labels change to new strings in one frame, 10,000 nodes (UI-side cost plus shaping and draw conversion) | <= 4 ms |
| Text batches per frame with 200 buttons in one root (also with rich text and scroll views) | 1 per root layer |
| Memory per node (hot, cold and layout, excluding strings) | <= 1.25 KiB |
| `NodeHot` / `NodeCold` (`size_of` pins, set at the measured value by U2 and re-measured by U14) | <= 320 B / <= 64 B |
| `UiDrawItem` size | <= 112 B |

The "excludes text shaping" rows are deliberate and honest: a counter that shows a new string every frame costs a shaped-buffer cache miss in the renderer's cache (keyed by text), which dwarfs the UI's own cost. 1,000 *distinct new strings* per frame would be about 20 ms at the ceiling above, so it is **not** within the frame budget, and the Phase 3 exit ("heavy live numbers at full frame rate") is covered by the shaping rows, not by the setter rows. The mitigations that keep real screens inside them: format to the displayed precision so a 1,000 Hz counter changes its text only when a visible digit changes (`set_num` and `set_text` with equal text are free, and `NumLabel` compares the formatted text), and the stats query reports `text_changes` per frame so a benchmark or an agent can see the rate. If measurement shows the shaping ceiling is not reachable for numerals, the options (a glyph-quad path for digits, a pre-shaped digit atlas) belong to #107 and are open question 2.

Evidence for the layout rows: a throwaway probe of taffy 0.15.1 (flexbox only, rounding off, native release build on a developer machine, not min-spec) measured about 4.4 ms for the cold 10,000-node pass and about 35 us for one dirty leaf, so the budgets keep a margin for min-spec and for the arena adapter's extra indirection. The binding, reload and list rows are **proposals** from the cost models in this spec (a tick comparison, a path read and a compare per record; a hash compare per skipped subtree; a handful of rows realized per frame); the first bench PR of the issue that builds each of them replaces them with measured numbers and reports any it cannot meet to the owner. The 100 ms reload swap is the one budget that is a requirement rather than a forecast.

Complexity: layout O(dirty ancestor chain + dirty subtrees); paint O(visible nodes) per rebuild and zero when clean; hit test O(interactive entries); `set_*` O(1) amortized; bind pass O(groups + records in changed groups); `image_changed` O(users); reload O(parse + hash) for the document plus O(changed path and its siblings) to reconcile (the parse and hash pass is O(nodes) and is its own row); focus step O(focusable entries); `detach` and `restore` O(subtree); release O(subtree).

Where the budgets are tight, the first answer is the data layout (hot/cold split, flat hit list, shared text buffer, two paint planes, records outside the arena) and the second is a smaller rebuild (subtree draw caching), never a bypass of the command bus or the dirty flags.

## Test plan

- **Arena:** stale ids fail every lookup, LIFO reuse of free slots, generation retirement at `u32::MAX`. A reserved slot fails every lookup and is never allocated. **The round 1 sequence: remove X, add Y, undo (the add), undo (the remove), then redo both**: every id of X's subtree is identical after the second undo, Y's id is dead after the first, and nothing aliases. `restore` of a multi-node subtree, of a subtree under a parent that was reparented meanwhile (re-attached by id), and of a subtree removed in any order relative to other removes. `restore` returns `ParentGone`, `DuplicateName`, `ForeignTree` and `RestoreConflict` (a released handle) and **leaves the arena unchanged on each failure**. Release: dropping the last clone of a `DetachedSubtree` frees its slots with a bumped generation (old ids dead), dropping one of two clones frees nothing, and a drop after a restore is a no-op (token mismatch). A seeded random sequence of add, remove, reparent, undo, redo and history-cap eviction checks parent/child consistency and id stability after every step.
- **Reload between remove and undo:** `ui_remove_node X`, a hot reload that leaves X's parent alone, undo: X is restored at its id. The same with a reload that replaces X's parent (`ParentGone`) and one that builds a node named X (`DuplicateName`): the undo fails, changes nothing and the history carries on. An undo entry naming a node the reload replaced fails with the stale-target error.
- **Layout goldens:** known trees (row, column, gap, padding, grow and shrink, `Percent`, min/max, nested) compared to exact rectangles using a fixed-advance measurer. Anchoring and scaling: every anchor at three viewport sizes, a non-default pivot, `snap_integer`, and physical-pixel snapping of adjacent edges.
- **Invalidation:** each row of the invalidation table asserts `LayoutStats` (a fixed-size counter edit does zero layout; a resizing counter lays out only its ancestor chain; a clean pass does none; a `ui_scale` change with an unchanged UI-logical size lays out nothing). A test for the empty-cache invariant covers adding a child to a clean parent.
- **Painting:** golden draw lists for a panel with children, each button state and the focus ring, `Contain` images (known and unknown size), progress fill in both directions, layer order across roots, two planes per root (all text after all graphics), an invisible subtree, and `revision` stability when nothing changed.
- **Render mapping:** pure tests of the `UiRenderConfig` helpers: layer and depth from the list, the `dpi_scale` division and `ui_scale / dpi_scale` text scale, and the round trip of snapped edges for `dpi_scale` in {1, 1.25, 1.5, 2} (within 1e-3 px).
- **Input:** table-driven sequences of `UiInputFrame`s: press and release on a button (click), press then release elsewhere (no click), press on gameplay then drag over UI (not captured, including the release frame), press on UI then release (captured on the release frame, not on the next frame unless hovering), a transparent root passing clicks, a blocking panel over a button, disabled buttons, tab and shift-tab wrap, **Tab with no focus focuses the first focusable node of a plain menu root (shift-Tab the last) and is consumed**, **`nav` with no focus and no `autofocus` or modal root is not consumed** (`keyboard` stays false), activate and cancel, `keyboard` capture only on consumed key edges, **click a button with the mouse and then press Space: no `Click` and `keyboard` is false**, and removing the hovered, pressed and focused node mid-sequence.
- **Description round trip:** `describe(build(desc)) == desc`, including names and roots; duplicate names are rejected atomically (no partial subtree).
- **Benchmarks** for every UI-side row of the performance table; a `size_of` test pins `UiDrawItem` and the per-node memory budget.
- **wasm:** `cargo build --workspace --target wasm32-unknown-unknown` covers the crate (it is not `NATIVE_ONLY`).
- **Stage 2:** headless app tests that insert the plugin (`Headless` driver), write a `UiInputFrame` into its resource and assert events and `UiCapture` before a game system runs, including the `FixedUpdate` ordering case above; command tests for apply, undo and redo of every command; a reflection round trip of `UiNodeDesc` through the text format. **Facade (hotspot item):** the measurer and draw conversion against a headless renderer, and the 200-button text-batch count.
- **Runtime nodes and reserved slots (U1):** a runtime node (list row, popup row, ghost, tooltip) allocated and freed between a `detach` and its undo cannot take a reserved slot and does not break `restore`; `Command` clones share one release (the `Clone` test); a `detach` of a list with realized rows restores without rows and re-realizes them on the next pass; a command that `detach`es a subtree holding a mount carries the mount through `restore`.
- **Widgets (Stage 5):** `Grid` goldens (columns, gaps, uneven row heights); scroll view content size, offset clamping and hit-test clipping (a button scrolled out is not hoverable); tabs show and hit only the selected page; toggle, slider (drag outside the rectangle, `step`, `nav`) and tab events; `NumLabel` equal-value and equal-text no-ops; rich text spans, wrapping, escapes and the unknown-tag warning; borders, opacity and 9-slice goldens (nine `Image` items, `uv`, shrinking borders, unknown size draws nothing); the clip table on all three item kinds.
- **Themes and transitions:** the resolution order (override, state patch, class, widget theme, general), an unknown class warning, a theme change re-resolving classes, a node override replacing hover colors; tweens with fixed `advance` steps, restart from the shown value, the 512 cap snapping, and a theme with no transition being instant.
- **Virtual list:** 100,000 items realize only the window, recycle rows on scroll with zero allocation, `row_of` maps any node in a row to `(list, index)`, `UiListRow` events carry the new index, and a list inside a modal root works.
- **Asset changes:** `image_changed` invalidates exactly the users of one image among many, once each, and a same-size reload needs no call; `font_changed` clears only text nodes of that font. `referenced_assets` lists every image and font guid of a tree (node images, theme images, 9-slice, the theme font, rich-text span fonts, `DragGhost::Image` guids), is cached against `asset_refs_revision`, and changes after an edit, a reload and a restore.
- **Binding:** an unchanged source costs a tick comparison (assert no read), a changed source applies only differing records, a counter with a fixed size repaints with no layout, a type mismatch is a reported error that resumes after an edit, a despawned entity pauses and resumes, a manual `ui_set_field` on a bound field stays until the source changes, list `Row` sources follow scroll and source changes, and removing a bound node leaves no record (undo brings it back). **`ui_bind` and `ui_lists` are exclusive systems**: a resource written by a system that runs between two `ui_bind` runs is seen exactly once (the `is_changed_since` cursor test), an expired cursor counts as changed, an absent resource reports `SourceGone`, and a row scrolled into view in `ui_lists` shows its own item's data in the same frame (no one-frame lag for `Row` bindings). **A component binding whose entity despawn is undone stays `SourceGone`** (the respawn has a new id and the tree's records are not remapped, see "A `Component` source and despawn undo"), and **resumes only after `ui_set_field` re-points it** at the new entity; a `Resource` binding in the same tree is unaffected.
- **Markup:** `describe(build(doc))` round trips through the text format for every widget; a `UiNodeDesc`-rooted file loads; a binary round trip needs no UI code; every parse and validation failure builds nothing; `ui_build_tree { markup }` undo and redo keep ids and the mount; with **no `UiMarkupStore` resource** it fails with `MarkupUnavailable`, and with the store present but **no entry** for the `Guid` it fails with `MarkupNotLoaded` and queues the `Guid` in `wanted` (once, however many calls), building nothing in both cases; `mount` takes its `attempted` and `text_hash` from the stamp, so the first `ui_markup` pass after a mount reparses nothing, and a code-built document is hashed by `mount`; a saved tree (`ui_get_tree`, wrapped in a `UiDocument`) reloads to an equal tree.
- **Hot reload:** a parse error keeps the old tree and reports line and column in `ui_get_stats`, and is **parsed once per generation** (a counter on the parser: ten frames with the bad file parse once, a newer generation parses again); a byte-identical resave (same `text_hash`) runs nothing; a later good reload clears the error; a 10,000-node swap meets the 100 ms row; the state table (focus, scroll, toggle, slider, tabs, bound text, tweens, list rows) row by row; the three-way merge (a game-written value survives unless the file changed that field); a rename replaces the node with a new id; a collision with an outside name is reported and changes nothing; the unchanged-document hash skip performs no setter call.
- **Immediate layer:** identical frames allocate nothing, `button`, `checkbox` and `slider` results, a removed call removes the node, nodes are `Runtime` and commands reject them.
- **Dropdown (U28):** open on click and on `activate`, popup at layer 1001 built in runtime slots and removed on close, placement above when there is no room below, a disabled option cannot be chosen, `DropdownChanged` on change only, `cancel` closes and keeps focus, **a keyboard choice keeps focus on the box while a mouse choice clears it (`FocusLost`), so after a mouse choice Space and Escape reach gameplay (`keyboard` false)**, a press outside closes and is consumed (no click underneath, no gameplay press), focus cannot leave an open popup with `nav` x or `tab`, hidden, disabled or removed box closes the popup, **a modal root appearing over the box closes the popup with no `DropdownChanged`**, `Auto` width from the widest label, `set_options` re-measures, **keeps `selected` when its index is still in range, otherwise sets `None`, raises no event and closes an open popup**, `set_choice` out of range stores `None`, a press on the already focused box emits no focus events, a dropdown in a modal root works, `ui_choose` equals a click on the row, and the bench rows.
- **Drag and drop (U29):** the threshold (a release before it is a `Click`), payload copied at start (the source recycled mid-drag does not change it), accept and reject by `kind`, a non-accepting droppable is skipped not a barrier, `DragEnter` and `DragLeave` order, `Dropped` on accept and `DragCancelled` on reject, on the source itself and on `cancel`, removing the source or target mid-drag, auto-scroll of a list under the pointer, capture through the release frame, the keyboard and gamepad path (`activate`, `nav` among accepting droppables, `activate` or `cancel`) producing the same events, **a draggable `Panel` and a draggable `Image` are focusable and start a keyboard drag, a droppable `Image` that is not otherwise focusable is a keyboard-drag candidate, `tab` does nothing during a keyboard drag, a draggable `Button` starts a drag on `activate` with no `Click`, and with no accepting droppable `activate` on it emits `Click` (on a draggable `Panel` nothing)**, **a modal root appearing over a running drag cancels it (`DragCancelled`, no drop)**, **the ghost of a list row carries no `Row` bindings, `drag`, `drop`, names or tooltip (a recycled row cannot change it), and a `DragGhost::Image` guid is in `referenced_assets`**, **`UiTree::drag()` is readable until the next `process_input` and cleared then**, a drag inside a modal, survival across a hot reload, `ui_drag`, and the bench rows.
- **Focus navigation (U27):** tab order with `focus.order`, wrap, shift-tab, spatial scoring on a grid of buttons (including a cross-axis overlap and a tie), an explicit link wins and a broken link falls back, no candidate leaves focus and does not consume the edge outside a modal and does consume it inside one, **`tab` with no focus on a plain (non-autofocus, non-modal) menu root focuses its first focusable node and `shift+tab` its last, and `nav` with no focus there is not consumed**, entering an `autofocus` or modal root with `tab` and with `nav` honors `initial_focus`, a draggable node is focusable, the score's edge distances on a staggered grid (gap measured between edges, zero on overlap), `page` on tabs, a candidate scrolled out of view scrolls into view, a slider and a text input keep their own axes, an open dropdown narrows candidates, `ui_focus` equals the edge, and the 200-entry bench row.
- **Gamepad mapping (F3):** the glue test maps D-pad and stick-threshold edges to `nav`, confirm and back to `activate` and `cancel`, shoulders to `page`, with key repeat in the glue (the UI crate has no clock), and a menu is fully driven by those inputs alone. **With `UiCapture::text_input` true, a keyboard Left or Right press (and its repeat) produces one `Edit(Move)` and `nav.x == 0`, while a D-pad press produces `nav.x` and no text op**, so the caret never moves twice for one press.
- **Text input (U31):** insert, backspace, delete, selection by keys and by pointer drag, double-click word select, word movement, `Home` and `End`, `max_chars` and each `TextFilter`, a pasted newline dropped, `password` draws bullets and refuses `Copy`, `Copy`, `Cut` and `take_clipboard_write`, field-local undo and redo (the editor's undo stack never sees keystrokes), IME preedit then commit, `ime_area` follows the caret, `UiCapture::text_input` as a level, `set_input_text` raises no event, a binding is state-wins, caret survives a reload that did not change the text, `ui_text_edit` equals real input, no layout on typing (`LayoutStats`), **one arrow press moves the caret exactly once (the UI test feeds `Edit(Move)` with `nav` zero and one `nav.x` edge alone, each moving it by one `char`; the facade test of F3 checks the key path sends only the text op and the D-pad path only `nav`)**, a press on the already focused field emits no focus events, and the bench rows (the 50 text inputs join the full-widget bench tree here, and the two full-widget rows are re-baselined).
- **Frozen mode (U11, U17, U22 to U26, U28 to U31):** a headless app with `SimulationMode::Frozen` still runs `ui_markup`, `ui_input`, `ui_bind`, `ui_lists`, `ui_immediate_end`, `ui_animate` and the `Headless` driver (and so the focus, dropdown, drag and text paths that run inside them); an edit of a bound resource, a `.ui` reload and a command each lay out and rebuild the draw list with `Update` skipped; a schedule-graph test asserts the plugin registers no system in `Update` or `FixedUpdate`.
- **Commands and queries (U13):** every command applies, undoes and redoes with identical ids (`ui_build_tree` by `detach` and `restore`); `ui_click`, `ui_focus`, `ui_choose`, `ui_drag` and `ui_text_edit` are `NotUndoable` and journalled (U27 to U31 add the last four with their widgets); `ui_get_tree { fresh }` lays out without moving the `UiTree` or `UiMeasurer` change tick; the bus's read-only check passes for every query; a registry test lists exactly the eleven command names and six query names; runtime nodes are rejected; `ui_build_tree { markup }` returns `MarkupUnavailable` when the store resource is absent (always, in U13) and `MarkupNotLoaded` when the store has no entry (from U24), and builds nothing; **a journalled `ui_remove_node` holds no `DetachedSubtree` in its journal copy (`Arc::strong_count == 1`), `undo_bytes` of a 1,000-node remove is at least 1 MiB, and the bus dropping the history entry releases the slots**; a 10,000-node `ui_get_tree` reply is bounded by `depth`.

## Open questions

1. **Renderer guarantees (needs #107).** The glue relies on: equal-`layer` items draw in `depth` order; all text of a layer shares a text batch when no sprite of that layer sorts between; `TextStyle.max_width` with `wrap: None` is still used as the alignment width, and an aligned label hits the same shaped-buffer cache entry in measure and draw (the cache key includes `max_width`, and the measure offers the available width while the draw passes the final one, so a second shaping can occur on the first draw after a change); `SpriteDraw::uv_rect` and the per-item `clip` behave as in the render spec (used by 9-slice and scroll views). If #107 cannot guarantee these, the glue falls back to aligning with `anchor` and may merge the two planes, at the cost of more text batches.
2. **Shaping cost of changing numerals (needs #107).** The shaping rows in Performance are proposals for #107's `text/*` benchmarks. If numerals cannot meet them, a digit fast path is a render-side feature (not a UI one).
3. **Platform input shape.** Resolved by `app-platform.md`: the facade glue converts the platform `InputState` (frame view: `cursor_logical`, mouse and key state, `scroll_lines`; no touch in the MVP) into the `UiInputFrame` resource. The arrow keys that feed `nav` are ordinary key state.
4. **Consumption.** Capture flags are the contract. Resolved by `app-platform.md`: the platform never masks its input state.
5. **System ordering. Settled.** `app-platform.md` places `UI_INPUT` in `First`, after platform input and before every `FixedUpdate` tick, and facade glue for layout and draw output in the `EXTRACT` set of `Extract`, after transform propagation; "Schedules" fixes the rest and the Frozen-mode behavior (SP3, #139). The Jam text's pointer to #110's ordering is closed by those two documents.
6. **Draw caching.** Is a full rebuild per paint change good enough at the full-widget 10,000-node screen? The first bench PR of U18 answers it; subtree caching keyed on per-node revisions is the next step if not.
7. **Command bus details. Answered (AM6).** `ui_click` is `UndoKind::NotUndoable` (journalled only); `ui_build_tree` is one command whose undo is `detach` and whose redo is `restore`; U13 registers the base commands and six queries from the crate with `register_command` and `register_query` and is `Blocked by` CB9 (U27 to U31 add the four simulation commands, eleven in all); the facade-registration alternative is closed; `ui_get_tree { fresh }` runs layout only through the non-stamping query path.
8. **Persistence. Answered.** A UI is saved as a `.ui` `Text` asset whose content type is **`UiDocument`** (a bare `UiNodeDesc` root also loads, so `assets-scene-export.md`'s wording "a `UiNodeDesc`" stays correct and no amendment blocks anything); the game's `Startup` or `ui_build_tree { markup }` mounts it; scenes carry no UI nodes (the editor's Stop rebuilds from the file, ED6). Saving through MCP is described under "Command and query surface". The one-sentence wording fix in `assets-scene-export.md` is outside this PR's footprint and optional; the orchestrator may file it.
9. **Clipping. Answered.** The per-item clip table (see "Draw data") and scroll views.
10. **Image identity.** `Guid` is used because it is stable and already in core; the facade resolves it through the asset and texture stores and decides what an unloaded image draws (nothing, in this spec). `image_changed` and `font_changed` carry reloads in.
11. **Text input. Answered (owner, 2026-11-25): in the MVP.** This spec owns the widget ("Text input"; issue U31). The platform side (committed text, IME preedit and commit, IME cursor area, clipboard, key repeat) is the `app-platform.md` amendment #177; U31 and F3 are `Blocked by #177`. See the cross-spec table.
12. **Variable-height rows and spanning grid cells.** Not planned; `VirtualList` rows are fixed-extent and `Grid` cells are single. Revisit if a game needs a chat log or a spanning layout.
13. **Rounded corners. Answered (owner, accepted).** Plain quads only; rounded or ornate frames are 9-slice images. A rounded-rectangle shader is a render-side feature, not planned.
14. **Reload swap budget. Answered (owner, confirmed).** A hard 100 ms for swapping a 10,000-node document. Parse cost depends on `exitium_serial`: the parse-and-hash row budgets 60 ms of it, and if the text reader cannot meet that on min-spec, the answer is to split a large screen into several `.ui` files, not to loosen the number.
15. **Reload and undo. Answered (owner, accepted).** A hot reload is not a command and **cannot be undone**; the file is the source of truth. The consequences (a stale undo entry fails safely, a remove-then-reload-then-undo succeeds or fails with `ParentGone` or `DuplicateName`, nothing changes on failure) are specified under "Stable ids and undo".
16. **Theme from a file. Answered (owner, accepted).** A theme loaded from a file applies only when game code mounts it (`mount` with `apply_theme`). `ui_set_theme` edits fields of the live theme and takes no theme file; `ui_build_tree` never applies one.
17. **Gamepad text entry. Owner to confirm.** A pad user typing into a `TextInput` relies on the platform's overlay keyboard (Steam Deck and Big Picture provide one) opened by the glue when `UiCapture::text_input` turns true. The UI draws **no on-screen keyboard** of its own. If a game wants one, it is a widget built from buttons on top of `ui_text_edit`, not part of this spec. **Recommended default: the platform overlay keyboard** (nothing to build; F3 opens it). **If the owner picks a UI-drawn keyboard instead:** it becomes a new widget (a grid of buttons on `ui_text_edit`, its own theme and focus rules) and one more issue after U31, and F3 drops the overlay-keyboard step. Platforms without an overlay keyboard (a bare Windows pad session) have no gamepad text entry until then.
18. **Text input limits. Owner to confirm.** Single-line only; the caret moves by `char`, not grapheme cluster, so a combined emoji or accent sequence takes several presses (std has no segmentation and a segmentation crate is not added for the MVP); no bidirectional text; no type-ahead search in a dropdown (a searchable list is a `VirtualList` of buttons). Each is a later addition, not a blocker for Glutton. **Recommended default: exactly this list.** **If the owner picks otherwise:** a grapheme caret needs a segmentation crate (a new dependency, so high-risk review and a licence and wasm check) and a new `CaretMove` granularity; multi-line needs a new field kind, vertical caret movement and line scroll; type-ahead needs a typing buffer driven by `advance` and a clock-free timeout; each adds one issue after U31 and none changes the U31 API shape for single-line fields.

## Implementation issues

Filed by the architect after this spec merges, with the `file-issue` skill (an existing issue that carries `needs-spec` for this work has the label lifted once this spec merges, instead of a duplicate being filed). Every issue edits at most one crate directory, fits one PR (about 600 changed lines or fewer; each split below was made to stay under that), has acceptance criteria, `Blocked by` lines and the full labelled footprint. Labels: `area:ui` and `role:impl-ui`, except F1 and F2 (`area:ui`, role per the facade owner). Because all `exitium_ui` issues edit `crates/exitium_ui/` (one in-flight issue per crate), they are a chain; U1 to U13 and F1 are the Glutton UI critical path, and U14 to U31, F2 and F3 are the Stage 5 lane (the owner's full-MVP direction, `mvp-roadmap.md`, puts every widget in the MVP, so nothing here is optional). `Cargo.toml`, `Cargo.lock` and the facade are hotspots owned by the intake PRs and the facade items.

**Stage 1** (no ECS). Each is `Blocked by` intake 1 (the Stage 1 dependency and crate-graph intake PR) and the issue before it. Every criterion list also includes `cargo xtask ci all` passing, the wasm build, and the benchmarks named for it.

- **U1 `feat(ui): node arena, tree structure and stable ids`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `NodeArena` with one LIFO free list, three slot states (`Occupied`, `Free`, `Reserved(token)`), generation retirement, the `runtime` flag, `ReleaseQueue`, `DetachedSubtree` (an `Arc` of its slot list; the last drop queues the release), `ForeignTree` (`tree_id`) and `restore` exactly as in "Stable ids and undo"; `UiTree` add, `remove_subtree`, `detach` (hot and cold data, bindings and mount slots empty until U23 and U24), reparent with cycle check, names and `find`, roots ordered by layer; the arena tests of the Test plan, including the **remove X, add Y, undo, undo** sequence, the `Clone` and release tests and the seeded fuzz with history-cap eviction; the arena benchmark rows (allocate and free, `detach` and `restore`, release). Blocked by: intake 1.
- **U2 `feat(ui): widget data, UiStyle and UiTheme types`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `WidgetKind` and the widget data structs, `UiStyle`, `Dim`, `Edges`, the flex and pointer enums, `RootConfig` (`layer` in `-1000..=999`, `modal`) and `Anchor`, `UiTheme` with the defaults above, all `Clone + PartialEq + Debug + Default` where meaningful; the final arena shape ("How the arena is shaped for binding"): `NodeHot` and `NodeCold` with `bind_head` (an empty slot until U23) and `extra: Option<Box<NodeExtra>>` (`tooltip`, `focus` links, `drag`, `drop`), and `size_of` pins set **at the measured value** for `NodeHot` (<= 320 B) and `NodeCold` (<= 64 B) and the per-node memory budget (the derivation under "Data layout" is the target; if the measured size differs, the PR re-pins and reports it); no behavior beyond constructors and validation (progress clamp, layer range). Blocked by: intake 1, U1.
- **U3 `feat(ui): UiNodeDesc, builder and setters`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiNodeDesc` (with the `class`, `tooltip`, `bindings`, `drag`, `drop` and `focus` fields present and empty-defaulted until their issues) with `build` and `describe` (round-trip test, atomic duplicate-name rejection); the setters `set_text`, `set_progress`, `set_enabled`, `set_style`, `set_theme`, and `set_image` with the `image_nodes` index (Guid to nodes), with the equal-value no-op, no allocation on a warm buffer, and the dirty marking (a stub `invalidate_from` and `paint_revision` that U5 fills in); the setter benchmarks. Blocked by: intake 1, U2.
- **U4 `feat(ui): taffy style conversion and arena adapter`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiStyle` to `taffy::Style` conversion (theme padding applied per widget kind), the taffy tree traits implemented over the arena and `layout` array, `UiMeasure`, `TextMeasure` and a fixed-advance test measurer, dispatch for the five widgets, layout of a single root; flex golden tests (row, column, gap, padding, grow and shrink, percent, min and max, nested, leaves measured). Blocked by: intake 1, U3.
- **U5 `feat(ui): layout pass, absolute rectangles and invalidation`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiTree::layout` over several roots in layer order, absolute `rect`s, `LayoutStats`, `invalidate_from` and the cache-empty invariant wired into every row of the invalidation table; invalidation tests asserting `LayoutStats`; `image_changed(guid)` (relayout only the nodes using that image, through the `image_nodes` index, O(users)) and `font_changed(Option<Guid>)`, with their tests from "Asset changes"; **`referenced_assets()` and `asset_refs_revision`** for node images and the theme font (U16 adds theme images, U19 rich-text span fonts), cached and bumped as the Test plan says; the 10,000-node cold, one-leaf and clean layout benchmarks and the `image_changed`, `font_changed` and `referenced_assets` benchmarks. Blocked by: intake 1, U4.
- **U6 `feat(ui): viewport, scaling and anchoring`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiViewport`, `ScaleMode`, `ui_scale` and UI-logical viewport math, root sizes (`Fill`, `Fixed`, `Fit`), anchor and pivot origin, the viewport-change invalidation rows; anchoring and scaling golden tests; the tests that a `ui_scale`-only change lays out nothing. Blocked by: intake 1, U5.
- **U7 `feat(ui): draw list and theme resolution`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiDrawList`, `UiDrawItem`, `TextRun`, `build_draw_list(measure, out)` for the five Stage 1 widgets (panel, label, image, button, progress bar) and button states, theme and override resolution, physical-pixel snapping, the two planes per root, `revision`; the painting golden tests; the draw benchmarks and the `UiDrawItem` size pin. Blocked by: intake 1, U6.
- **U8 `feat(ui): render-facing mapping helpers`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `UiRenderConfig` (`layer_base`, `mask`), pure helpers for render layer and depth from an item, the physical-to-Screen conversion and the text scale (`ui_scale / dpi_scale`), written against #107's `Space::Screen`; the render-mapping tests including the snapped round trip. Blocked by: intake 1, U7, #107 merged.
- **U9 `feat(ui): hit test, hover, press and click`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: the hit list built in the layout post-pass, `UiInputFrame`, `UiEvent`, `UiCapture`, rules 1 to 4, pointer capture as in rule 6 (all three origin cases, including the release frame), removal clearing (rule 7) for hover and pressed; the matching input tests and the hit-test benchmark. Blocked by: intake 1, U8.
- **U10 `feat(ui): keyboard focus and keyboard capture`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: rule 5 in its tab-and-activate form (spatial `nav`, `autofocus` and modal entry are U27) and the `keyboard` capture rule (`text_input` is U31); **`tab` with no focus focuses the first focusable node in tab order and `shift+tab` the last (the Jam behaviour, needing no `autofocus` root), and `nav` with no focus is not consumed**; a mouse press clears focus and gives none; the "click then Space" test and the Tab, shift-Tab, activate, cancel and removal tests; the `process_input` benchmark. Blocked by: intake 1, U9.

**Stage 2** (ECS, app, reflect, command bus). Each is `Blocked by` intake 2, which adds every edge below, so none of them touches `Cargo.toml` or `Cargo.lock`.

- **U11 `feat(ui): UiPlugin, resources and ui_input`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: the `plugin` module with the resources (including a `UiInputFrame` resource and the `UiMeasurer` resource, whose default is a fixed-advance measurer), **the public system set `UI` in `PostUpdate`, `after_set(PROPAGATE)`**, which holds the layout and draw-list systems (`app-platform.md` places the `Headless` driver in it), the `ui_input` system **in set `UI_INPUT` of `First`**, which reads that resource and stores `UiCapture` and reads no platform type, the `Headless` driver as a system **in set `UI_DRIVER` of `PostUpdate`** (not `Update`: `SimulationMode::Frozen` skips `Update`, SP3 #139), and headless app tests that write a `UiInputFrame` into the resource (events and capture before a game system, the `FixedUpdate` ordering case, and a `Frozen`-mode test that the input system and the driver still run and a click and a layout still happen). The plugin registers no system in `Update` or `FixedUpdate`; a schedule-graph test asserts it. Blocked by: intake 2, U10, the app and plugin issues from #106 (A1, A2).
- **U12 `feat(ui): reflection derives and UiNodeDesc text round trip`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `Reflect` on every data type, hand-written `UiNodeId` reflection with the `"{index}v{generation}"` text form, a `UiNodeDesc` round trip through the text format in a test (`exitium_serial`, which U24 later makes a normal dependency under the `plugin` feature). Blocked by: intake 2, U11, the reflect issues and the text-serialization issues (#23).
- **U13 `feat(ui): UI commands and queries with undo`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance (AM6 of `editor-mcp-mvp.md`): the **7 base commands and 6 queries** of "Command and query surface" (`ui_build_tree`, `ui_add_node`, `ui_remove_node`, `ui_reparent`, `ui_set_field`, `ui_set_theme`, `ui_click`; `ui_get_tree`, `ui_get_node`, `ui_find`, `ui_get_theme`, `ui_hit_test`, `ui_get_stats`; U27 to U31 add `ui_focus`, `ui_choose`, `ui_drag` and `ui_text_edit`, eleven in all) registered with `register_command` and `register_query` **from `UiPlugin::build` in this crate, not the facade**, under exactly those `NAME`s; `ui_click` returns `UndoKind::NotUndoable`; `ui_build_tree` is **one command** (`undo` is `detach`, redo is `restore` of the stored `DetachedSubtree`, ids identical); its `markup` arm always returns `UiError::MarkupUnavailable` (there is no `UiMarkupStore` until U24 adds it, and `MarkupNotLoaded` appears with the store), building nothing; `ui_remove_node` and `ui_build_tree` keep their `DetachedSubtree` in a `#[reflect(skip)]` field and report it through `Command::undo_bytes` (the journal copy holds none; tests as in the Test plan); `ui_get_tree` reports authored nodes only and takes `depth`; `ui_get_tree { fresh }` runs layout **only through the non-stamping query path** (a test asserts neither the `UiTree` nor the `UiMeasurer` change tick moves); runtime nodes are rejected with `UiError::RuntimeNode`; apply, undo and redo tests for each command, and a registry test listing exactly the seven base and six query names (the registry test grows with U27 to U31). Registration adds no schedule system, so there is no schedule to name. Blocked by: intake 2, U12, **CB9** (the registry and the reflected schemas), the `exitium_command` issues from #110, and the `exitium_command` issue that adds `Command::undo_bytes` (cross-spec table).
- **F1 `feat(exitium): UI to renderer glue`.** Footprint: Edits: none; Governs: none; Hotspots: `crates/exitium/`; New crate edges: none (the facade's edges to `exitium_ui` and `exitium_render` are in intake 2). Acceptance: the glue system in render extraction exactly as in "Render handoff" (the `UiMeasure` over `Renderer::measure_text` and the texture store, **installed into the `UiMeasurer` resource, replacing the fixed-advance default**, the unit and scale conversions, layer and depth mapping, skip when `revision` is unchanged), the facade tests for the round trip and the 200-button text-batch count, and a UI example in `crates/exitium/examples/`. High-risk review: facade API. Blocked by: intake 2, U8, U11, the render plugin issues from #107, and the facade chain (the facade runs alone).

**Stage 5, the UI full lane** (`exitium_ui` only; 18 serial PRs, U14 to U31; roadmap C6: every issue is `Blocked by` intake 3 and the issue before it; `Edits: crates/exitium_ui/` for all of them, so they are a chain by R1). Every criterion list also includes `cargo xtask ci all`, the wasm build, and the benchmarks named for it. Each states **the schedule and set its systems run in**, and each system's Frozen-mode test follows "Schedules". The lane starts beside Stage 4 and must end before the facade glue (R3).

- **U14 `feat(ui): full widget data, widget state and setters`.** Footprint: Edits: `crates/exitium_ui/`; Governs: none; Hotspots: none; New crate edges: none. Acceptance: `RichText`, `NumLabel`, `Toggle`, `Slider`, `ScrollView`, `VirtualList`, `Grid` and `Tabs` data types with defaults and validation (slider clamp and step, grid `columns >= 1`, tabs index); `WidgetState` in `NodeCold`; the setters `set_num`, `set_value`, `set_on`, `set_selected`, `set_count`, `set_scroll` with the equal-value no-op and dirty marking per the invalidation table; **`size_of` re-measured with the full `WidgetKind` and re-pinned (the pins from U2 still hold or are changed here with a note to the owner)**; setter benchmarks. No systems. Blocked by: intake 3, U13.
- **U15 `feat(ui): layout of grid, tabs, scroll and list rows`.** Footprint: as U14. Acceptance: "Layout of the new widgets": the dispatch, default sizes, `layout/grid.rs`, hidden tab pages, scroll content size and offset clamp, independent fixed-size row roots for lists, the absolute-rectangle walk with scroll offsets and the clip table, the hit list's per-entry clip; goldens and the grid, scroll-offset and full-widget cold-layout benchmarks. Runs inside `layout`, no system. Blocked by: intake 3, U14.
- **U16 `feat(ui): themes, classes and resolution order`.** Footprint: as U14. Acceptance: `UiTheme` widget themes (including `dropdown`, `text_input` and `drag`), `UiClass` (with `drag_over` and `dragging`), `PaintPatch`, class resolution on set and on theme change, the five-step resolution order, the unknown-class warning, border and opacity fields resolved; theme images join `referenced_assets`; tests for the order and the theme-change effects. No systems. Blocked by: intake 3, U15.
- **U17 `feat(ui): transitions and tweens`.** Footprint: as U14. Acceptance: `tween.rs`, `UiTree::advance`, the 512 cap with snapping, restart-from-shown-value, no layout invalidation, deterministic tests with fixed steps; the plugin system **`ui_animate` in set `UI_ANIMATE` of `PostUpdate`** (real, unscaled delta clamped to 0.1 s) and its Frozen-mode test; the 512-tween benchmark. Blocked by: intake 3, U16, U11.
- **U18 `feat(ui): clipping, 9-slice, borders and opacity in the draw list`.** Footprint: as U14. Acceptance: the clip table and per-item `clip` on all three item kinds, `uv` on `Image`, nine-item 9-slice, border quads, opacity carried down, scrollbar and modal-scrim painting; the render-mapping helpers for clip and `uv`; goldens for each; the full-widget draw-rebuild benchmark and the `UiDrawItem` size pin. Runs inside `build_draw_list`, no system. Blocked by: intake 3, U17.
- **U19 `feat(ui): rich text and number labels`.** Footprint: as U14. Acceptance: `layout/rich.rs` (span parsing, word measuring, greedy line breaking, the per-node `(markup, width)` cache), one `Text` item per span segment, `NumLabel` formatting through `NumFormat` with the equal-`Num` and equal-text no-ops, the warnings for unknown tags, **rich-text span fonts added to `referenced_assets`** (and `font_changed` reaching them); goldens and the rich-text and `set_num` benchmarks. No systems. Blocked by: intake 3, U18.
- **U20 `feat(ui): toggle, slider, tabs and scroll input`.** Footprint: as U14. Acceptance: input rules 9 to 11 for toggle, slider, tabs and scroll (the dropdown, drag and text rules come with U28, U29 and U31, and spatial focus with U27), the toggle, slider, tab and scroll `UiEvent` variants and `UiInputFrame::nav`, `page` and `scroll`, `UiCapture::scroll`, slider drag outside the rectangle, wheel chaining to ancestors, scrollbar drag, clipped hit entries; table-driven input tests; extend the `process_input` benchmark. Runs inside `process_input` (the existing `UI_INPUT` system in `First`, frozen-mode tested). Blocked by: intake 3, U19.
- **U21 `feat(ui): modal roots and tooltips`.** Footprint: as U14. Acceptance: input rule 8 (hit stop, focus trap, `ModalCancel`, scrim), the built-in tooltip root at layer 1000 in `Runtime` slots with the delay driven by `advance` (so it runs in `ui_animate`), placement inside the viewport, hide rules; tests including a tooltip over a scrolled node. Blocked by: intake 3, U20.
- **U22 `feat(ui): virtual list`.** Footprint: as U14. Acceptance: `layout/virt.rs`, row realization and recycling in `Runtime` slots, `row_of`, the reflected `UiListRow` event, the `detach` and `restore` behavior of lists; the plugin system **`ui_lists` in set `UI_LISTS` of `PostUpdate`**, after `UI_BIND`, with its Frozen-mode test (it is an exclusive system from U23 on, when it starts applying `Row` records; until then it takes `ResMut<UiTree>`); the 100,000-item scroll benchmark with zero allocations. Blocked by: intake 3, U21.
- **U23 `feat(ui): change-only data binding`.** Footprint: as U14. Acceptance: `bind.rs` (`BindingTable`, groups, pre-parsed paths, the targets and formats of "Data binding", `BindSource::Row` and `ListSource`, binding errors and `SourceGone`), `bind_head`, `detach` and `restore` carrying records; the **`ui_bind` and `ui_lists` row-apply path as exclusive systems** (there is no `&World` system parameter, so they read the world in a first phase into a `UiBindScratch` resource and apply to `UiTree` in a second, which also satisfies the rule that nothing writes a polled resource between the poll and the next `increment_change_cursor`); change detection through the ECS spec's `ReflectResource::is_changed_since(&World, ChangeCursor) -> Option<bool>` (`None` is `SourceGone`, an expired cursor counts as changed); the Component despawn-undo note; the plugin system **`ui_bind` in set `UI_BIND` of `PostUpdate`**, first of the UI systems there, with a Frozen-mode test (a resource edited while frozen updates a bound label and the draw list); `ui_get_stats` gains the binding counts and `binding_errors`; the bind benchmarks and the **counter-rebinding benchmark** and the bound frame row. Blocked by: intake 3, U22, **#138** (the ECS spec's `is_changed_since`, PR #168).
- **U24 `feat(ui): markup documents and mounts`.** Footprint: as U14. Acceptance: `UiDocument`, `parse` through `exitium_serial` with spans, the validation list, `UiTree::mount` (atomic, `apply_theme`), `UiMarkupStore`, `ui_build_tree { markup }` with undo and redo carrying the mount, **filling in the `markup` arm U13 left returning `MarkupUnavailable`** (the `UiMarkupStore` resource of `{ text, generation }` entries plus the `wanted` set, inserted by `UiPlugin::build`; from here `MarkupUnavailable` means the resource is absent and `MarkupNotLoaded` means the store has no entry for the `Guid`, which also queues it in `wanted`), `UiMount { attempted, text_hash, error }` with `mount` taking a `MarkupStamp` (so `attempted` and `text_hash` start from the store entry, and a code-built document is hashed by `mount`), the `UiNodeDesc`-rooted file, the text and binary round trips, parse and validation error tests, `find_references`-compatible `Guid` strings; the parse benchmark for a 10,000-node document. No system yet. Blocked by: intake 3, U23.
- **U25 `feat(ui): .ui hot reload`.** Footprint: as U14. Acceptance: `UiTree::reload` with the **retry guard** (`attempted` set before the parse, so a bad file parses once per generation), the **text-hash short-circuit**, the **parse-and-hash pass** (hashes filled in right after the parse, so `reconcile` only compares), plan then apply, the by-name diff and three-way merge, **a parse error keeps the old tree and reports line and column**, the state-survival table row by row (focus, scroll, toggle, slider, tabs, bound text, tweens, list rows), `image_changed`-style stale-id behavior for replaced nodes; the plugin system **`ui_markup` in set `UI_MARKUP` of `First`, before `UI_INPUT`**, with a Frozen-mode test (a reload while frozen shows in the next frame); `ui_get_stats` gains `markup` and `last_swap_ms`; the reload benchmarks with the **100 ms** swap row as a hard assertion and the parse-and-hash row; the reload-between-remove-and-undo tests of the Test plan (a reload never touches a reserved slot). Blocked by: intake 3, U24.
- **U26 `feat(ui): immediate-mode layer`.** Footprint: as U14. Acceptance: `immediate.rs` (`UiImmediate`, `ImUi`, keys, reconcile reusing U25's code, `Runtime` slots, layer 900), results for `button`, `checkbox` and `slider`; the plugin system **`ui_immediate_end` in set `UI_IMMEDIATE_END` of `PostUpdate`** with a Frozen-mode test; the 200-widget zero-allocation benchmark. Blocked by: intake 3, U25.
- **U27 `feat(ui): focus navigation for keyboard and gamepad`.** Footprint: as U14. Acceptance: "Focus navigation": `FocusLinks` in `NodeExtra` (reflected, in `.ui` files), `RootConfig::autofocus` and `initial_focus`, tab order with `order`, spatial scoring (edge gaps as defined) with explicit links and fallback, the entering table ("Entering the UI": `tab` with no focus always enters, `nav` only an `autofocus` or modal root), a draggable node being focusable, scroll-into-view of a scrolled-out candidate, `page` on tabs, the nav-edge consumption rules (input rules 5 and 11), `UiTree::focus_first_in`; the **`ui_focus` command**; table-driven tests and the focus-step benchmark (<= 20 us at 200 entries). Runs inside `process_input` (the `UI_INPUT` system, frozen-mode tested). Blocked by: intake 3, U26.
- **U28 `feat(ui): dropdown`.** Footprint: as U14. Acceptance: the `Dropdown` widget as in "The widget set" and input rule 12 (`DropdownData`, `DropdownOption`, `set_choice`, `set_options`, the popup root at layer 1001 in runtime slots, placement, highlight, consumed outside press, `DropdownChanged` and `DropdownToggled`, `Auto` width with the label-measure cache), `BindTarget::Choice` and `Options` (records dispatch only; the bind systems exist from U23), the `dropdown` theme and `ui_get_node` live state; the **`ui_choose` command**; the tests and benchmark rows of the Test plan. Blocked by: intake 3, U27.
- **U29 `feat(ui): drag and drop`.** Footprint: as U14. Acceptance: "Drag and drop" in full: `DragPayload`, `DragSource`, `DropTarget` in `NodeExtra`, `set_drag_payload`, `DragState` and `UiTree::drag`, the threshold, ghost root at layer 1002, nearest accepting droppable, `DragStarted`, `DragEnter`, `DragLeave`, `Dropped` and `DragCancelled`, auto-scroll through `advance` (the `ui_animate` system), the keyboard and gamepad path (a draggable node of any kind is focusable; a draggable `Button` drags on `activate` and emits no `Click`, or `Click` when no accepting droppable exists; non-interactive droppables are candidates; `tab` does nothing), the ghost snapshot that strips names, bindings, `drag`, `drop`, `focus` and tooltip, `DragGhost::Image` guids in `referenced_assets`, a modal appearing over a popup or drag (U28 and U29 close and cancel), `DragState` cleared at the next `process_input`, input rule 13, `BindTarget::DragId`, the `drag_over` and `dragging` class patches; the **`ui_drag` command**; the tests and benchmark rows of the Test plan. Blocked by: intake 3, U28.
- **U30 `perf(ui): full-widget 10,000-node benchmarks and budgets`.** Footprint: as U14. Acceptance: the full-widget 10,000-node tree (its mix **without the 50 text inputs**, which do not exist yet: U31 adds them and re-baselines the two full-widget rows; the dropdowns and the draggable and droppable nodes are in) and every remaining Performance row not owned by an earlier issue or by U31, budget re-baselining with a note to the owner for any row missed, an end-to-end Frozen-mode test across every UI system and the schedule-graph test, the crate `CLAUDE.md` and rustdoc updated for the full UI. Blocked by: intake 3, U29.
- **U31 `feat(ui): text input`.** Footprint: as U14. Acceptance: "Text input" in full: `TextInputData`, `TextFilter`, `TextEditOp`, `UiTextEvent`, `UiInputFrame::text`, the caret, selection, scroll, preedit and field-local undo, painting with the clip table, input rule 14, `UiCapture::text_input` as a level, `UiTree::input_text`, `ime_area` and `take_clipboard_write`, `TextChanged` and `TextSubmitted`, the `text_input` theme, `BindTarget::Text` on a text input (state-wins); arrows reach a focused field as `Edit(Move)` only and `nav` x is for the D-pad and stick (the caret moves once per press, tested); a press on the focused field emits no focus events; **the 50 text inputs are added to U30's full-widget bench tree and its two full-widget rows are re-baselined here** (with a note to the owner for any row that moves by more than the 5% gate); the **`ui_text_edit` command**, which completes the **eleven** commands (the registry test is updated to eleven and six); the tests and benchmark rows of the Test plan. It uses `UiMeasure::measure_text` only and **adds no crates.io dependency**. Blocked by: intake 3, U30, **#177** (the `app-platform.md` amendment that adds committed text, IME preedit and commit, the IME cursor area and clipboard; the UI never names those platform types, but F3 and this issue's acceptance depend on the API being specified).
- **F2 `feat(exitium): full UI glue`.** Footprint: Edits: none; Governs: none; Hotspots: `crates/exitium/`; New crate edges: none. Acceptance: the glue half of the "Who calls what" ownership table, and **no asset requests** (FA1 requests; F2 requests nothing). An asset-event system in `First` that **owns first loads** (`AssetLoaded.generation == 1`): it calls `image_changed` with the image size and `font_changed(Some(guid))` for a font in `referenced_assets` (replace-time calls, `generation >= 2`, are FA5's with `Some(guid)`), and for a `.ui` asset it calls `take_loaded_guid` (a facade call, C1) and fills `UiMarkupStore` (generation 1 on load, `generation >= 2` on reload, failed reloads ignored, each entry stamped for `mount`). It converts the clip table and `uv` (clip rectangles divided by `dpi_scale`). Facade tests: a first-load image relayouts only its users in the next frame (the first-load gap), a `.ui` first load fills the store and a `mount` of it succeeds, a reloaded `.ui` file changes the next frame's draw list, a resized image relays out only its users, and no call is made twice for one event (F2 and FA5 do not both fire on a replace). Also the 200-button text-batch count with rich text and scroll views, and the facade parity test for the base commands and six queries (C3; F3 extends it to eleven). High-risk review: facade API. Blocked by: intake 3, U18, U25, F1, and the facade chain (it runs alone, and cannot overlap any `exitium_ui` issue, R3).
- **F3 `feat(exitium): UI text and gamepad glue`.** Footprint: Edits: none; Governs: none; Hotspots: `crates/exitium/`; New crate edges: none. Acceptance: the glue that fills `UiInputFrame::text` from the platform's committed-text, edit-key and IME events (#177), turns platform IME and text events on and off by `UiCapture::text_input`, passes `UiTree::ime_area()` (divided by `dpi_scale`) as the IME cursor area, sends `take_clipboard_write()` to the clipboard and builds paste `Insert` ops from it, maps D-pad and left-stick edges to `nav` (stick threshold 0.5) and keyboard arrows to `nav` **only while no text field is focused**: when the previous frame's `UiCapture::text_input` was true, a keyboard arrow goes to the field as `Edit(Move)` and is never also sent as `nav` x (a test presses one arrow in a focused field and the caret moves once; a D-pad press in the same field still moves focus), maps confirm and back to `activate` and `cancel` and shoulders to `page`, **owns key repeat** (the UI reads no clock), and opens the platform's overlay keyboard on a gamepad when `text_input` turns true; the facade tests of the Test plan ("Gamepad mapping") and the parity test for all eleven commands. High-risk review: facade API. Blocked by: intake 3, U31, F2, **#177**, **`app-platform.md` F2** (the UI input and viewport glue system this issue extends) and the facade chain (it runs alone, and cannot overlap any `exitium_ui` issue, R3).

The editor edge `exitium_editor -> exitium_ui` is the roadmap's C5 PR and the editor's UI editor view is an editor-extras issue; neither is filed from this spec.

The real `Blocked by #N` numbers for the intake and for the other specs' implementation issues are filled in when the architect files these, because those specs and issues are written in parallel with this one. U1 to U7, U9 and U10 need no other spec and can start the moment intake 1 merges, which is why the UI core does not wait on the app, render or command-bus specs. U13 waits on CB9, U23 on #138 (the ECS spec's `is_changed_since`), U31 and F3 on #177 (the platform text, IME and clipboard amendment), and the Stage 5 lane waits on intake 3 (roadmap C6).

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
- `exitium_ui -> exitium_command`: registering commands and queries (the roadmap's expected wiring lists `ui -> plugin: command`; open question 7 records the decision to register from the crate). `exitium_command` depends on core, ecs and reflect, so there is no cycle.
- `exitium_ui -> exitium_serial` as a dev-dependency here (the `UiNodeDesc` text-format round trip test in U12). Intake 3 promotes it to a normal dependency under the `plugin` feature, because markup parsing runs in the game (U24). Per the roadmap's rule R3, dev-dependencies count for the conflict check.
- Facade: `exitium -> exitium_ui` (already implied by "facade depends on every runtime crate"). F1 uses `exitium -> exitium_render` as well; it needs no edge between `exitium_ui` and `exitium_render`.
- Not required by this spec: `exitium_editor -> exitium_ui` (see "What the editor needs from the UI"; it is the roadmap's C5 PR, after the Stage 5 lane).
- `exitium_ui` stays out of `NATIVE_ONLY`.

**Intake 3 (Stage 5, the UI full lane):**

- `exitium_ui -> exitium_serial` becomes a **normal dependency under the `plugin` feature** (it was dev-only in intake 2). It is a workspace crate, already built for wasm32, so it adds no crates.io dependency.
- `exitium_ui -> exitium_command`, `exitium_ecs`, `exitium_app` and `exitium_reflect` are already added by intake 2. Bind-by-path uses the reflection path machinery of `exitium_reflect`, with no new edge.
- Facade (F2): no new edge; `exitium -> exitium_ui` and `exitium -> exitium_assets` already exist. `exitium_ui` still does not depend on `exitium_assets` or `exitium_render`.
- **No new crates.io dependency.** The grid is our own equal-column algorithm (taffy's `grid` feature stays off), hashing uses std's `DefaultHasher::new()`, number formatting uses `exitium_core`'s `Num`, and parsing is `exitium_serial`. **The dropdown, drag and drop, focus navigation and text input add none either:** the popup and ghost are tree nodes, spatial scoring is arithmetic over the hit list, caret and selection use `UiMeasure::measure_text`, the caret moves by `char` (std; no grapheme-segmentation crate, see open question 18) and word movement uses std character classes. No new workspace-crate edge is needed: the platform side of text input reaches the UI as plain data (`UiInputFrame::text`) through the facade glue (F3), so `exitium_ui` still has no edge to `exitium_platform`. The `ReleaseQueue`'s `Mutex` is `std::sync`, which wasm32 supports.

**Crates.io dependencies** (high-risk area: a new dependency; the reviewer checks this table):

| Crate | Version | Features | License | wasm | Why not std |
|---|---|---|---|---|---|
| `taffy` | `0.15.1` | `default-features = false`, `["std", "flexbox"]` | MIT (already allowed in `deny.toml`) | yes: `cargo check --target wasm32-unknown-unknown` passes; no platform calls | A CSS-conformant flexbox solver with caching is thousands of lines of subtle algorithm; std has no layout. 0.15.1 was published 2026-10-25, MSRV 1.71 (our toolchain is newer), `#![deny(unsafe_code)]`. |

- With these features taffy has **no transitive dependencies** (`grid` would pull `smallvec`, `taffy_tree` would pull `slotmap`; neither is enabled). `serde`, `parse`, `calc`, `block_layout`, `float_layout`, `content_size` and `detailed_layout_info` are also off.
- Taffy is `0.x`: a minor-version bump may break the low-level traits. The caret requirement `0.15.1` allows only patch updates; upgrading is a deliberate PR confined to `layout/`.
- Nothing else is needed: colors, rectangles and vectors come from `exitium_core`, ids from `Guid`, and text shaping and font data stay in the renderer behind `UiMeasure`, supplied by the facade. Stages 2 and 5 add only workspace crates, no further crates.io dependency.
