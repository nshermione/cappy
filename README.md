# cappy

A small GUI framework with damage-region repainting, a retained widget tree, reactive signals,
taffy-based layout and themeable styles. Frames are drawn either on the GPU with
[wgpu](https://wgpu.rs/) (the default) or on the CPU with
[tiny-skia](https://github.com/linebender/tiny-skia) and
[softbuffer](https://github.com/rust-windowing/softbuffer) when memory matters more.

> [!WARNING]
> **This is a personal learning project.** It exists to help the author learn more
> about Rust. It is **not production-ready**, the API can change or break at any time,
> and it is **not recommended for use in real projects**.
>
> The author has **no plans to publish any of these crates** to crates.io.

## Workspace layout

| Path | Description |
| --- | --- |
| `crates/cappy` | Facade crate that re-exports the framework |
| `crates/cappy-core` | Retained widget tree, reactive signals and damage tracking |
| `crates/cappy-style` | Typed styles, design tokens and themes |
| `crates/cappy-layout` | Geometry and taffy-based layout |
| `crates/cappy-text` | Fonts, text shaping and measurement shared by the renderers |
| `crates/cappy-host` | Renderer-independent window host and renderer selection |
| `crates/cappy-render` | wgpu renderer with damage-region repainting |
| `crates/cappy-render-cpu` | CPU renderer (tiny-skia + softbuffer) for low memory use |
| `crates/cappy-components` | Built-in components (planned) |
| `crates/cappy-testing` | Headless UI driver and GPU / CPU snapshot helpers |
| `macros/cappy-dsl` | `macro_rules!` DSL (`ui!`, `style!`, `theme!`) |
| `macros/cappy-macros` | Procedural macros (`derive(Props)`, `derive(ClassName)`) |
| `integrations/cappy-winit` | [winit](https://github.com/rust-windowing/winit) integration |
| `integrations/cappy-tao` | [tao](https://github.com/tauri-apps/tao) integration |
| `playground/*` | Shared demo scenes and the winit / tao demo hosts |

Both renderers consume the same `RenderFrame` (primitives plus damage) from `cappy-core` and
implement the `WindowRenderer` trait from `cappy-host`, so the integrations do not care which
one draws the window.

## Getting started

The toolchain is pinned in `rust-toolchain.toml`, so `rustup` picks it up automatically.

```sh
# Run the playground with winit
cargo run -p winit-demo

# Run the playground with tao
cargo run -p tao-demo

# Outline the regions each frame repaints (or press F12 in the window)
cargo run -p tao-demo -- --debug-damage

# Run the tests
cargo test --workspace
```

## Choosing a renderer

| | GPU (`gpu`, default) | CPU (`cpu`) |
| --- | --- | --- |
| Drawing | wgpu | tiny-skia + softbuffer |
| Memory | GPU driver loaded into the process | No GPU driver |
| Best for | Animation, large or high-DPI windows, effects | Tools and mostly static UIs |

On the author's machine (Windows, AMD RX 6600, release build of `tao-demo`), the window used
about 174 MB with the GPU renderer and about 10 MB with the CPU renderer (Task Manager,
private working set). The CPU renderer pays for this with CPU time per repainted pixel, which
damage-region repainting keeps small for typical UIs.

- Enable the `cpu` feature of `cappy-tao` or `cappy-winit` (or of the demos) to compile it in.
- With both backends compiled in, pick one in code or with the `CAPPY_RENDERER=gpu|cpu`
  environment variable, which takes precedence. Without either, the GPU renderer is used.
- With `default-features = false, features = ["cpu"]`, wgpu is not compiled at all, which
  also makes the binary smaller and the build faster.
- Asking for a backend that is not compiled in is an error; there is no silent fallback.

```rust
use cappy_tao::{RendererKind, WindowOptions, run};

let options = WindowOptions::new("My app").with_renderer(RendererKind::Cpu);
run(options, ui).expect("app failed");
```

```sh
# Both backends, CPU chosen at runtime (PowerShell: $env:CAPPY_RENDERER="cpu")
CAPPY_RENDERER=cpu cargo run -p tao-demo --features cpu --release

# CPU only, without wgpu
cargo run -p tao-demo --no-default-features --features cpu --release
```

The two renderers blend colors differently (wgpu in linear light, tiny-skia in sRGB), so
anti-aliased edges and translucent colors can differ very slightly between them.

### Facade features

| Feature | Contents |
| --- | --- |
| `basic` | Core, styles, layout, text, window host, DSL and derive macros |
| `gpu` (default) | `basic` plus the wgpu renderer as `cappy::render` |
| `cpu` | `basic` plus the CPU renderer as `cappy::render_cpu` |
| `full` | `basic` and `gpu` plus the built-in components |

## Testing

```sh
# Unit and harness tests
cargo test --workspace

# Pixel tests on a real or software GPU
cargo test -p cappy-testing --features gpu-tests

# Pixel tests with the CPU renderer (no GPU needed)
cargo test -p cappy-testing --features cpu-tests

# Everything, including the GPU / CPU parity check
cargo test --workspace --all-features
```

Snapshots live in `crates/cappy-testing/tests/snapshots` (GPU) and `.../snapshots/cpu` (CPU).
A missing snapshot is written on first run; set `CAPPY_UPDATE_SNAPSHOTS=1` to rewrite them all.
On a mismatch the actual image is saved next to the snapshot as `<name>.actual.png`.

## Environment variables

| Variable | Effect |
| --- | --- |
| `CAPPY_RENDERER` | `gpu` or `cpu`; overrides the renderer chosen in code |
| `CAPPY_DEBUG_DAMAGE` | `1` outlines repainted regions in the playground |
| `CAPPY_UPDATE_SNAPSHOTS` | `1` rewrites pixel test snapshots |
| `WGPU_BACKEND` | Forces a wgpu backend such as `dx12` or `vulkan` (GPU renderer only) |
| `RUST_LOG` | Log filter; the playground logs per-frame repaints from `cappy_host` |

## Support, issues and pull requests

The author is fairly lazy and **will not handle issues or pull requests** in this
repository. Please do not expect replies, fixes or reviews.

If you would like to contribute seriously or take over maintenance of this project,
please get in touch by email: **[ns.hermione@gmail.com](mailto:ns.hermione@gmail.com)**.

## License

This project is free to use. It is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual
licensed as above, without any additional terms or conditions.
