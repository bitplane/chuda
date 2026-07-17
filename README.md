# chuda

A CUDA-only, high-quality truecolour ANSI renderer for image-training
pipelines. It implements the expensive part of Chafa's effort-9 symbol mode:
exhaustive foreground/background fitting and error scoring over the narrow
symbol atlas. PNG decode, high-quality resize and stateful ANSI emission stay
in Rust; independent cell/symbol evaluation runs on CUDA.

RGBA images are optimized jointly for either a detailed opaque foreground and
background cell or a composable foreground-only cell. Alpha-mask agreement is
part of symbol scoring, so antialiased sprite edges do not require two renders
and a cell-level merge pass. `--transparent-threshold` controls the bias toward
opaque interior detail and defaults to `0.10`.

The atlas is generated from the vendored Chafa reference source and checked
into the Rust binary. Chafa is not a build-time or runtime dependency.

## Requirements

- Rust 1.85 or newer
- NVIDIA driver
- CUDA Toolkit (`nvcc` and `libcudart`; Ubuntu package: `nvidia-cuda-toolkit`)

## Build and run

```sh
cargo build --release
cargo run --release -- --size 80 image.png > image.ansi
```

Directory mode recursively mirrors PNG paths and changes their suffix to
`.ansi`:

```sh
cargo run --release -- --size 80 corpus --output rendered
```

Only ANSI is written. Directory mode does not leave resized images or other
intermediates behind.

## Architecture note

The production scorer is implemented directly in `cuda/renderer.cu` and
exposed to Rust through a small C ABI.

## Updating the symbol atlas

After updating the Chafa sources in `vendor/chafa`, run:

```sh
python3 tools/generate_symbols.py
cargo fmt
```

The generated atlas is LGPL-derived and this project is correspondingly
licensed LGPL-3.0-or-later. See `LICENSE` and `NOTICE`.

## Benchmark against Chafa

The benchmark excludes compilation, warms both programs once, and reports the
median end-to-end batch time. It also writes one output from each renderer for
visual inspection:

```sh
python3 tools/benchmark.py ../ansi-scaler/data/artifacts/rasters \
  --width 80 --images 100 --repeats 3
less -R benchmark-results/sample-chuda.ansi
less -R benchmark-results/sample-chafa.ansi
```

Machine-readable measurements are saved in `benchmark-results/report.json`.
