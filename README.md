# johnwave

A simple wave-morphing synth plugin (CLAP + VST3), built in Rust with
[nice-plug](https://crates.io/crates/nice-plug) and an egui UI.

![synth-image](/docs/images/johnwave_1.jpg)

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

- second lfo and second mod env (and/or use amp env as a mod source)
- lfo fade/tail
- filter scale 
- ability to type in param values (with left click?)
- a second filter
- 2× oversampling around the filter, if Drive's aliasing turns out audible
- small line in center of polar knobs
