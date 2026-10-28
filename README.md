# cappy

A small GUI framework built on top of [wgpu](https://wgpu.rs/), with damage-region
repainting, a retained widget tree, reactive signals, taffy-based layout and themeable styles.

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
| `crates/cappy-render` | wgpu renderer with damage-region repainting |
| `crates/cappy-components` | Built-in components (planned) |
| `crates/cappy-testing` | Headless UI driver and GPU snapshot helpers |
| `macros/cappy-dsl` | `macro_rules!` DSL (`ui!`, `style!`, `theme!`) |
| `macros/cappy-macros` | Procedural macros (`derive(Props)`, `derive(ClassName)`) |
| `integrations/cappy-winit` | [winit](https://github.com/rust-windowing/winit) integration |
| `integrations/cappy-tao` | [tao](https://github.com/tauri-apps/tao) integration |
| `playground/*` | Shared demo scenes and the winit / tao demo hosts |

## Getting started

The toolchain is pinned in `rust-toolchain.toml`, so `rustup` picks it up automatically.

```sh
# Run the playground with winit
cargo run -p winit-demo

# Run the playground with tao
cargo run -p tao-demo

# Run the tests
cargo test --workspace
```

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
