# Third party notices

The text input element in `src/platform/windows/input.rs` is adapted from Zed Industries' GPUI 0.2.2 `examples/input.rs`, licensed under Apache-2.0. The license is preserved in `resources/GPUI-INPUT-LICENSE-APACHE`. Changes remove keystroke history, scope Windows shortcuts, correct IME UTF-16 offsets and hit testing, add masking and bounded input, and route paste through the isolated clipboard reader.

Source: https://docs.rs/crate/gpui/0.2.2/source/examples/input.rs
