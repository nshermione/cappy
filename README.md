# cappy

cappy is a retained-mode GUI framework. It keeps a widget tree, tracks which signals changed, lays the tree out with [taffy](https://docs.rs/taffy), and repaints only the damaged regions. When nothing changed and no scroll animation is running, the host stays idle.

Frames are drawn on the GPU with [wgpu](https://wgpu.rs/) by default, or on the CPU with [tiny-skia](https://github.com/linebender/tiny-skia) and [softbuffer](https://github.com/rust-windowing/softbuffer) when memory matters more. cappy does not own a window. A host (today, [winit](https://github.com/rust-windowing/winit)) feeds input, calls `Ui::update`, and presents the frame.

> [!WARNING]
> **This is a personal learning project.** It exists to help the author learn more
> about Rust. It is **not production-ready**, the API can change or break at any time,
> and it is **not recommended for use in real projects**.
>
> The author has **no plans to publish any of these crates** to crates.io.

## Contents

- [Key features](#key-features)
- [Tech stack](#tech-stack)
- [Workspace layout](#workspace-layout)
- [Prerequisites](#prerequisites)
- [Getting started](#getting-started)
- [Using cappy from another crate](#using-cappy-from-another-crate)
- [Architecture](#architecture)
- [Writing UI](#writing-ui)
- [Choosing a renderer](#choosing-a-renderer)
- [Crate features](#crate-features)
- [Testing](#testing)
- [Environment variables](#environment-variables)
- [Continuous integration](#continuous-integration)
- [Troubleshooting](#troubleshooting)
- [Support, issues and pull requests](#support-issues-and-pull-requests)
- [License](#license)

## Key features

- Retained widget tree. Nodes stay mounted and are updated in place.
- Reactive signals. A read during a binding records a subscription; a write wakes the host.
- `element!` builds trees. `show` hides nodes, `if` disposes the other branch, `for` keeps rows by key.
- `#[component]` structs with state, props, events and slots. `#[store]` structs are one value per context.
- Typed styles. Design tokens, themes, and `stylesheet!` classes with `:hover` and `:active`.
- Damage-region repainting. Unchanged frames do not redraw.
- Two renderers behind one `WindowRenderer` trait. Both consume the same `RenderFrame`.
- Headless `TestUi` plus optional GPU and CPU pixel snapshots.

## Tech stack

| Piece | Choice |
| --- | --- |
| Language | Rust, edition 2024 |
| Toolchain | `1.99.1`, pinned in `rust-toolchain.toml` (dependencies need at least 1.89) |
| Layout | [taffy](https://docs.rs/taffy) 0.15 |
| Text | [cosmic-text](https://github.com/pop-os/cosmic-text) 0.19, shared by both renderers. The GPU path draws text through [glyphon](https://github.com/grovesNL/glyphon) 0.12 |
| GPU renderer | [wgpu](https://wgpu.rs/) 30 |
| CPU renderer | [tiny-skia](https://github.com/linebender/tiny-skia) 0.11 and [softbuffer](https://github.com/rust-windowing/softbuffer) 0.4 |
| Windowing | [winit](https://github.com/rust-windowing/winit) 0.30, in `cappy-winit` |
| Pixel tests | [insta](https://insta.rs/) is a workspace dependency; snapshot helpers live in `cappy-testing` |
| Logging | `log` + `env_logger` in the playground |

`wgpu` and `cosmic-text` stay on the versions glyphon uses. Bump them together.

## Workspace layout

```
├── crates/
│   ├── cappy/               Facade. Re-exports the framework and the prelude
│   ├── cappy-core/          Retained tree, signals, input, scroll, damage, paint list
│   ├── cappy-style/         Tokens, themes, stylesheets, resolved Style
│   ├── cappy-layout/        Geometry and the taffy tree
│   ├── cappy-text/          Fonts, shaping and measurement shared by the renderers
│   ├── cappy-host/          Window host, renderer selection, resize pacing
│   ├── cappy-render/        wgpu renderer
│   ├── cappy-render-cpu/    tiny-skia + softbuffer renderer
│   ├── cappy-components/    Built-in widgets. Empty for now; enabled by the `full` feature
│   └── cappy-testing/       Headless driver and GPU / CPU snapshot helpers
├── macros/
│   ├── cappy-dsl/           element! and style!
│   └── cappy-macros/        #[component], #[store], stylesheet!, derives
├── integrations/
│   └── cappy-winit/         winit event loop
├── playground/
│   ├── scenes/              Showcase scene shared by hosts
│   ├── winit-demo/          Binary: cargo run -p winit-demo
│   └── web-demo/            Browser: trunk serve (from this directory)
└── .github/workflows/rust.yml
```

Both renderers implement `WindowRenderer` from `cappy-host` and paint the same `RenderFrame` from `cappy-core`. Integrations do not branch on which backend draws the window.

## Prerequisites

- [rustup](https://rustup.rs/). The repo pins the toolchain, so the first `cargo` command in this directory installs `1.99.1` plus `rustfmt` and `clippy`.
- A C toolchain and the platform libraries wgpu and winit need:
  - **Windows:** a recent GPU driver. The default backend is DXGI (DirectX 12). Vulkan works via `WGPU_BACKEND`.
  - **Linux:** a Vulkan driver, or a software Vulkan layer if you have no GPU. Windowing needs the usual winit system packages for your distro.
  - **macOS:** Metal, which ships with the OS.
- The CPU renderer needs none of the GPU stack. Build with `--no-default-features --features cpu` when you want that path only.
- **Web:** the `wasm32-unknown-unknown` target and [trunk](https://trunkrs.dev/). A Chromium browser (WebGPU, with WebGL as fallback). The CPU renderer does not run in the browser.

There is no database, no `.env` file, and no separate asset build.

## Getting started

Clone the repository and run the playground. The showcase is a header with a theme switcher and four tabs: counter, tiles, checklist and paint.

```sh
git clone <this-repo>
cd rs-gui

# GPU renderer, the default
cargo run -p winit-demo

# Outline the regions each frame repaints. F12 toggles the same overlay
cargo run -p winit-demo -- --debug-damage

# Tests that do not need a GPU
cargo test --workspace
```

The same showcase in a browser. Trunk builds the wasm crate and serves it. The first build compiles wgpu for wasm, which is slow.

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk

cd playground/web-demo
trunk serve --open
```

The native window opens at 960×640 logical pixels and stays hidden until the first frame has been presented. The browser canvas fills the page as soon as it is created and follows the viewport. The event loop then sleeps until input arrives, a signal changes, or an animation frame is due.

A minimal window, using the same pieces the playground uses:

```rust
use cappy::prelude::*;
use cappy_winit::{Run, WindowOptions};

fn main() {
    let application = Application::new();
    let cx = application.context();
    let count = cx.reactive(0_i32);
    let root = element!((cx) {
        div(class = "app") {
            text(:format!("Clicked {} times", count.get()))
            div(class = "btn btn-primary", on_click = move |_| count.update(|n| *n += 1)) {
                text("Click")
            }
        }
    });
    let ui = Ui::new(cx, root, Theme::light());
    application
        .run(WindowOptions::new("My app"), ui)
        .expect("app failed");
}
```

`element!` without a `(cx)` argument reads `Context::current`. That is set by `Context::enter` and by a component's `IntoElement`. Calling it outside either of those panics when the tree is built.

The same tree can be driven with no window. `Ui::update` returns `UpdateOutcome::Idle` or `UpdateOutcome::Redraw { damage }`. `Ui::prepare_frame` then yields the display list.

```rust
use cappy::prelude::*;

let application = Application::new();
let cx = application.context();
let mut ui = Ui::new(cx, div().child(text("Hello")), Theme::light());
assert!(ui.update(FrameInput::new(800, 600, 1.0)).needs_redraw());
```

## Using cappy from another crate

These crates are not published. Depend on them by path.

```toml
[dependencies]
cappy = { path = "../rs-gui/crates/cappy" }
cappy-winit = { path = "../rs-gui/integrations/cappy-winit" }
```

`cappy`'s default features are `basic` and `gpu`. `cappy-winit`'s default feature is `gpu`. See [Crate features](#crate-features) for the CPU-only combination.

Most application code wants `use cappy::prelude::*`. That brings in `Application`, `Ui`, `Theme`, `element!`, `style!`, `div`, `text`, `image`, the reactive helpers, and the macros (`component`, `store`, `stylesheet`, `style_token`, `Props`, `ClassName`, `Reactive`).

## Architecture

### What one frame does

`Ui::update` is the whole runtime. It does not know about windows or GPUs.

1. Apply the viewport (`FrameInput`: physical size and scale factor).
2. Flush bindings. A binding is a closure that read signals last time; if any of those signals changed, it runs again and may mount, hide, dispose or rewrite nodes.
3. Resolve styles top-down for dirty nodes. A node’s style is the theme, its classes (including `:hover` and `:active`), its component stylesheet, and its inline refinement.
4. Run taffy layout.
5. Hover can change because layout moved boxes, so style and layout run once more.
6. Step scroll. A wheel event moves the target offset; the visible offset eases toward it. Dragging a scrollbar writes the offset directly.
7. Run layout again, collect paint damage, and rebuild the display list where it is stale.
8. Release nodes, bindings and signals that this frame disposed.
9. Return `Idle` when damage is empty and scroll is not animating. Otherwise return `Redraw` with the damaged rectangles.

The host calls `Ui::prepare_frame`, which packages the display list and the damage into a `RenderFrame`, then asks the renderer to paint those regions and present.

A `RenderFrame` is a flat list of primitives:

| Primitive | What it draws |
| --- | --- |
| `ShadowPrimitive` | A drop shadow behind a box |
| `QuadPrimitive` | A rounded rectangle: fill (solid or linear gradient), border, radius |
| `TextPrimitive` | One text block, already measured |
| `ImagePrimitive` | A bitmap fitted into a box (`ObjectFit`) |

Each primitive carries the scrollport clips of its ancestors.

### How a host uses that

`cappy-winit` builds a winit window and a `WindowHost`.

- Input is translated into `InputEvent` and given to the host.
- A signal write from outside the event (a `set_waker` callback) asks winit to wake.
- Live resize follows the pointer. Presents stay on a resize interval, then one more frame is presented when the size settles.
- On Windows, the GPU path drops the DWM redirection bitmap so the swapchain is the window content during a drag. The CPU path still blits through GDI and keeps the bitmap.
- F12 toggles the damage overlay. The playground starts with it on when `--debug-damage` or `CAPPY_DEBUG_DAMAGE=1` is set.

`select_renderer` picks the backend: `CAPPY_RENDERER` first, then `WindowOptions::with_renderer`, then the first backend compiled in. GPU is listed before CPU. Asking for a backend that was not compiled in is an error. There is no silent fallback.

### Reactivity

A `Signal<T>` is the storage. `Reactive<T>` is the handle application code holds. `cx.reactive(value)` creates one.

Reading a signal during a binding subscribes that binding. `update`, `set` and similar writes enqueue the subscribers. The next `Ui::update` flushes them. The winit host also wakes immediately so the frame is not delayed until the next input event.

`Context` is the owner of signals, listeners and singletons. `Application::new` creates one. `Ui` must be built from that same context; `Run::run` returns `RunError::ContextMismatch` otherwise.

### Styles

A theme holds token values and named classes.

Built-in token sets, defined with `#[style_token]` in `cappy-style`:

| Token | Slots | Unit |
| --- | --- | --- |
| `ColorToken` | `Background`, `Surface`, `SurfaceAlt`, `SurfaceHover`, `Border`, `Text`, `TextMuted`, `Primary`, `PrimaryHover`, `PrimaryActive`, `OnPrimary`, `Success`, `Warning`, `Danger` | `Color` |
| `Space` | `Xs` 4, `Sm` 8, `Md` 12, `Lg` 16, `Xl` 24 | logical pixels |
| `Radius` | `Sm` 4, `Md` 6, `Lg` 10, `Full` 9999 | logical pixels |
| `FontSize` | `Sm`, `Md`, `Lg`, `Xl`, `Xxl` | logical pixels |

`Theme::light()` and `Theme::dark()` fill the color slots. `Theme::patch_tokens` overwrites individual slots. A custom `#[style_token]` set is stored on the theme the first time it is overwritten.

`stylesheet!` defines class names and their rules together. `@include Name` copies another class in the same sheet. `:hover` and `:active` start from that class’s own properties. `class`, `style` and `key` on an element are applied to the element `into_element` returns.

`style!` builds an inline `StyleRefinement` for one node.

Switching themes dirties style. Classes refer to tokens, so one stylesheet serves every theme.

## Writing UI

The playground scene under `playground/scenes/src/showcase/` is the worked example: tokens and a sheet in `style.rs`, a `#[store]` for the open tab and the shared counter, and a `Badge` component with state, a prop, an event and two slots.

### `element!`

```rust
use cappy::prelude::*;

let application = Application::new();
let cx = application.context();
let count = cx.reactive(0_i32);

let tree = element!((cx) {
    div(class = "app") {
        text("Counter", class = "title"),
        div(class = "row", style = style! { gap: Space::Sm }) {
            text(:format!("Count: {}", count.get()), key = "label"),
            div(class = "btn btn-primary", on_click = move |_| count.update(|n| *n += 1)) {
                text("+1")
            }
        }
    }
});
```

- `div`, `text` and `image` open `Container`, `Text` and `Image`. A leading value is content, so `text("Title")` calls `.content("Title")`.
- A leading `:` makes the value reactive: `:format!(...)` becomes a `Dynamic` that reruns when a signal it read changes. `self.field` inside that expression is copied out before the closure.
- `class = [a, b]` is shorthand for two `class` setters. Items can mix strings and typed class names.
- `show (:cond) { ... }` keeps its nodes and hides them while `cond` is false.
- `if (:cond) { ... } else { ... }` disposes the branch that is not selected. `else` may be omitted or written `else if`.
- `for (item in :list, key = item.field) { ... }` repeats the block and keeps a row when its key stays. `item` inside the block is the row signal.
- `{ expression }` embeds one element. `{ ..iterator }` embeds every element the iterator yields.
- Children may be separated by commas. Put a comma before a `{ expression }` child that follows an element with arguments, or it is read as that element’s children.

`show`, `if` and `for` build their elements in a `'static` closure. `slot(field)` stays outside those blocks: a parent fills slots before mount.

### Components and stores

```rust
use std::sync::Arc;

use cappy::ClickEvent;
use cappy::prelude::*;

#[store]
struct Counter {
    count: i64,
}

impl Counter {
    fn increment(&self) {
        self.count.update(|count| *count += 1);
    }
}

#[component]
struct Badge {
    hits: i64,
    #[prop]
    label: String,
    #[event]
    on_click: String,
    #[slots]
    icon: Element,
    tail: Element,
}

stylesheet! {
    enum Class {
        Root {
            direction: FlexDirection::Row,
            align: Align::Center,
            padding_x: Space::Sm,
            padding_y: Space::Sm,
            gap: Space::Sm,
            radius: Radius::Full,
            background: ColorToken::Primary,
            text_color: ColorToken::OnPrimary,
            font_size: FontSize::Sm,
        }
    }
}

impl Component for Badge {
    fn stylesheet() -> Option<Arc<Stylesheet>> {
        Some(Class::sheet())
    }

    fn render(&self) -> Element {
        let counter = use_store(Counter);
        element! {
            Container(class = Class::Root, on_click = on(Self::press)) {
                slot(icon)
                Text(:format!("{} ({})", self.label, self.hits), key = "badge-label")
                slot(tail)
            }
        }
    }
}

impl Badge {
    fn press(&self, _event: &ClickEvent) {
        self.hits.update(|hits| *hits += 1);
        use_store(Counter).increment();
        self.on_click.emit(self.label.get());
    }
}
```

`#[component]` rewrites the struct. Plain fields become reactive state. `#[prop]` fields are parent inputs. `#[event]` fields are listeners. `#[slot]` fields are positions for parent children and must be `Element`. `#[props]`, `#[events]`, `#[slots]` and `#[state]` start a section that runs through the following fields. The macro adds `new`, `open` and `Clone`. `render` stays a handwritten `Component` impl. Do not also derive `Default` or apply `#[store]`. `class`, `style` and `key` are reserved for the root element.

`#[store]` rewrites each field into its tracked form and implements `Singleton`. `use_store(Counter)` returns the one value of that type in the current context. Fields start from `Default`. `use_local_store(Counter)` builds another value the same way and does not touch the singleton. A context reused for another scene still holds the previous singletons; the showcase resets them in `showcase()`.

`on(Self::press)` adapts a method to a listener. The method receives `&self` and the event.

### Themes

```rust
use cappy::prelude::*;

#[style_token(f32)]
enum Depth {
    Near,
    Far,
}

fn ocean() -> Theme {
    let mut theme = Theme::dark();
    theme.set_name("ocean");
    theme.patch_tokens((
        ColorToken::patch()
            .background(hex!("#0b1d2a"))
            .surface(hex!("#10293b"))
            .primary(hex!("#0ea5e9")),
        Radius::patch().md(10.0),
        Depth::patch().far(20.0),
    ));
    theme
}
```

Install the sheets, then build the `Ui` with a starting theme. `cx.set_themes` takes the themes; each one receives the same stylesheet. `Ui`’s theme argument selects which installed theme is active.

```rust
cx.set_themes([Theme::light(), Theme::dark()].into_iter().map(Class::apply));
let ui = Ui::new(cx, root(), Theme::light());
```

`Class::apply` is what `stylesheet!` generates. `Class::sheet()` is the sheet alone, which a component returns from `Component::stylesheet`.

## Choosing a renderer

| | GPU (`gpu`, default) | CPU (`cpu`) |
| --- | --- | --- |
| Drawing | wgpu | tiny-skia + softbuffer |
| Memory | GPU driver loaded into the process | No GPU driver |
| Best for | Animation, large or high-DPI windows, effects | Tools and mostly static UIs |

On the author's machine (Windows, AMD RX 6600, release build of `winit-demo`), the window used about 174 MB with the GPU renderer and about 10 MB with the CPU renderer (Task Manager, private working set). The CPU renderer pays for this with CPU time per repainted pixel, which damage-region repainting keeps small for typical UIs.

- Enable the `cpu` feature of `cappy-winit` (or of the demo) to compile it in.
- With both backends compiled in, pick one in code or with `CAPPY_RENDERER=gpu|cpu`. The environment variable wins. Without either, the GPU renderer is used.
- With `default-features = false` and `features = ["cpu"]`, wgpu is not compiled at all. The binary is smaller and the build is faster.
- Asking for a backend that is not compiled in is an error.

```rust
use cappy::prelude::*;
use cappy_winit::{RendererKind, Run, WindowOptions};

let application = Application::new();
let ui = Ui::new(application.context(), div().child(text("Hello")), Theme::light());
let options = WindowOptions::new("My app").with_renderer(RendererKind::Cpu);
application.run(options, ui).expect("app failed");
```

```sh
# Both backends, CPU chosen at runtime (PowerShell: $env:CAPPY_RENDERER="cpu")
CAPPY_RENDERER=cpu cargo run -p winit-demo --features cpu --release

# CPU only, without wgpu
cargo run -p winit-demo --no-default-features --features cpu --release
```

The two renderers blend colors differently (wgpu in linear light, tiny-skia in sRGB), so anti-aliased edges and translucent colors can differ slightly.

## Crate features

### `cappy`

| Feature | Contents |
| --- | --- |
| `basic` (default) | Core, styles, layout, text, window host, `element!` / `style!`, and the derive / attribute macros |
| `gpu` (default) | `basic` plus the wgpu renderer as `cappy::render` |
| `cpu` | `basic` plus the CPU renderer as `cappy::render_cpu` |
| `full` | `basic` and `gpu` plus `cappy::components` |

`cappy-components` is a placeholder. The feature exists so the facade shape stays stable; no widgets ship in it yet.

### `cappy-winit`

| Feature | Contents |
| --- | --- |
| `gpu` (default) | wgpu backend |
| `cpu` | tiny-skia + softbuffer backend |

### `winit-demo`

| Feature | Contents |
| --- | --- |
| `gpu` (default) | Playground linked to the GPU backend |
| `cpu` | Playground linked to the CPU backend |

The demo depends on `cappy` with the `basic` feature only. The renderer crate comes in through `cappy-winit`, not through `cappy::render`.

### `cappy-testing`

| Feature | Contents |
| --- | --- |
| `gpu-tests` | Render frames on a real or software GPU and compare PNGs |
| `cpu-tests` | The same comparison with the CPU renderer |

The headless `TestUi` driver needs neither feature.

## Testing

```sh
# Unit and harness tests. This is what CI runs
cargo test --workspace

# Pixel tests on a real or software GPU
cargo test -p cappy-testing --features gpu-tests

# Pixel tests with the CPU renderer (no GPU needed)
cargo test -p cappy-testing --features cpu-tests

# Everything, including the GPU / CPU parity check
cargo test --workspace --all-features

# Clippy, matching the workspace lints (unsafe_code is denied)
cargo clippy --workspace --all-targets --all-features

cargo fmt --all --check
```

`TestUi` mounts a tree in an 800×600 viewport at scale 1 and drives it like a host: click, hover, and look nodes up by `key`. Lookups panic with a message that names the missing key.

```rust
use cappy::prelude::*;
use cappy_testing::TestUi;

let application = Application::new();
let cx = application.context();
let count = cx.reactive(0_i32);
let root = element!((cx) {
    div(key = "button", on_click = move |_| count.update(|n| *n += 1)) {
        text(:format!("{}", count.get()), key = "label")
    }
});
let mut harness = TestUi::new(cx, root, Theme::light());
harness.click("button");
```

Snapshots live in `crates/cappy-testing/tests/snapshots` (GPU) and `crates/cappy-testing/tests/snapshots/cpu` (CPU). A missing snapshot is written on first run. Set `CAPPY_UPDATE_SNAPSHOTS=1` to rewrite them all. On a mismatch the actual image is saved next to the snapshot as `<name>.actual.png`.

`macros/cappy-macros/tests/compile_fail/` holds trybuild tests for bad `stylesheet!`, `#[style_token]` and `#[component]` inputs.

## Environment variables

| Variable | Effect |
| --- | --- |
| `CAPPY_RENDERER` | `gpu` or `cpu`. Overrides the renderer chosen in code. Any other value is an error |
| `CAPPY_DEBUG_DAMAGE` | `1` outlines repainted regions in the playground. `--debug-damage` does the same |
| `CAPPY_UPDATE_SNAPSHOTS` | `1` rewrites pixel-test snapshots |
| `WGPU_BACKEND` | Forces a wgpu backend such as `dx12` or `vulkan`. GPU renderer only |
| `RUST_LOG` | Log filter. The playground defaults to `info`. Per-frame repaint lines stay off until `WindowOptions::with_log_frames(true)`. Process-memory samples stay off until `WindowOptions::with_log_memory(true)` |

## Continuous integration

`.github/workflows/rust.yml` runs on pushes and pull requests to `master`, on `ubuntu-latest`:

```sh
cargo build --verbose
cargo test --verbose
```

That build uses default features, so it compiles the GPU renderer and skips `gpu-tests`, `cpu-tests` and `--all-features`. Pixel tests stay a local step.

## Troubleshooting

### The CPU renderer was not compiled in

**Error:** `the cpu renderer is not compiled in; enable the cpu cargo feature`

**Fix:** pass the feature, and turn default features off when wgpu should not be in the build.

```sh
cargo run -p winit-demo --no-default-features --features cpu
```

`CAPPY_RENDERER=cpu` does not enable the feature. It only selects a backend that is already compiled.

### `CAPPY_RENDERER` is set to something else

**Error:** `invalid CAPPY_RENDERER`

**Fix:** the value must be `gpu` or `cpu`. Unset it to use the in-code choice.

```sh
# PowerShell
Remove-Item Env:CAPPY_RENDERER
```

### The UI context does not match the application

**Error:** `ui was built with a different context than the application`

**Fix:** build the `Ui` with `application.context()`, and call `application.run(...)`. A second `Application::new()` is a different context.

### `element!` panics while building the tree

**Error:** a panic from `Context::current`

**Fix:** write `element!((cx) { ... })`, or call `element! { ... }` inside `cx.enter(|| ...)` or inside `Component::render`.

### Toolchain mismatch

**Error:** a dependency fails to compile, often around `cosmic-text` or edition 2024 syntax.

**Fix:** run cargo from this directory so `rust-toolchain.toml` selects `1.99.1`. `rustup show` should list that version as the active toolchain for the repo.

### wgpu cannot find an adapter

**Error:** a `RendererError` while creating the GPU surface, or a hang looking for an adapter.

**Fix:**

- Update the GPU driver.
- Force a backend: `WGPU_BACKEND=vulkan` or `WGPU_BACKEND=dx12` (PowerShell: `$env:WGPU_BACKEND="dx12"`).
- Or skip the GPU: `cargo run -p winit-demo --no-default-features --features cpu --release`.

### Pixel snapshots differ across machines

GPU snapshots depend on the adapter and the driver. A mismatch writes `<name>.actual.png` beside the committed snapshot. Inspect that file before updating.

```sh
# PowerShell
$env:CAPPY_UPDATE_SNAPSHOTS = "1"
cargo test -p cappy-testing --features gpu-tests
```

CPU snapshots do not need a GPU, but they are not required to match the GPU files. The renderers blend in different color spaces. `cargo test -p cappy-testing --features cpu-tests` compares against `snapshots/cpu`.

### Scroll or hover looks one frame late in a test

`Ui::update` applies bindings, then style, then layout, and may restyle after layout moves the hovered node. Drive the tree through `TestUi` or call `update` again after changing a signal. Scroll easing uses the time between updates; tests that need a settled offset should step time rather than sleep.

## Support, issues and pull requests

The author is fairly lazy and **will not handle issues or pull requests** in this repository. Please do not expect replies, fixes or reviews.

If you would like to contribute seriously or take over maintenance of this project, please get in touch by email: **[ns.hermione@gmail.com](mailto:ns.hermione@gmail.com)**.

## License

This project is free to use. It is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
