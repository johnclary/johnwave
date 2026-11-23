# johnwave

A wave-morphing synth plugin (CLAP + VST3), built in Rust with [nice-plug](https://crates.io/crates/nice-plug) and an egui UI.

![synth-image](/docs/images/johnwave_1.jpg)

## Getting started

* Check out the [user manual](/docs/user_manual.md)

## Requirements

- Stable Rust, installed via [rustup](https://rustup.rs)

## Test

```sh
cargo test
```

Unit tests live alongside the code in `src/*.rs`.

## Build

```sh
cargo xtask bundle johnwave --release
```

This writes the plugin bundles to:

- `target/bundled/johnwave.vst3`
- `target/bundled/johnwave.clap`

Leave off `--release` for a faster, unoptimised debug build.

## Deploy (macOS)

Copy the bundles into your user plug-in folders:

```sh
mkdir -p ~/Library/Audio/Plug-Ins/VST3 ~/Library/Audio/Plug-Ins/CLAP
cp -R target/bundled/johnwave.vst3 ~/Library/Audio/Plug-Ins/VST3/
cp -R target/bundled/johnwave.clap ~/Library/Audio/Plug-Ins/CLAP/
```

Then rescan in your DAW. In Ableton: **Settings → Plug-Ins → Rescan**.

For quick iteration you can skip copying: point Ableton's custom VST3
folder at `target/bundled` and just rebuild + rescan.

## Todo

- readout bugginess in mod mode
- second lfo and second mod env (and/or use amp env as a mod source)
- ability to type in param values (with left click?)
- a second filter
- 2× oversampling around the filter, to cut back on Drive's aliasing turns
- small line in center of polar knobs (inside the arc ring)
- play with the colours. They're all in `src/editor.rs`:
  - `cyan()` and `CYAN_HUE`: the main neon cyan (knob arcs, title, chips)
  - `neon_color()`, `HUE_DRIFT`, `SATURATION`: how the scope's lines shift from cyan as Osc 1's Shape goes up
  - `OUT_COLOR`, `OUT_GLOW`: the scope's soft white OUT line (and the FREQ view's spectrum, which uses the same white)
  - `SPECTRUM_FILL`, `SPECTRUM_EDGE`: how bright the spectrum's fill and its top edge are
  - `COMPONENT_STRENGTH`, `TRAIL_STRENGTH`: how faint the thin Osc 1/Osc 2 lines and the trail are
  - `mod_pink()`: the mod colour (mod arcs, mod mode)
  - `panel_gray()`: panel outlines and titles
