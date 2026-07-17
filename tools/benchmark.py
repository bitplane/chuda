#!/usr/bin/env python3
"""End-to-end benchmark against Chafa's high-quality truecolour symbol mode."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/chuda"


def run(command, *, stdout=None, env=None):
    started = time.perf_counter()
    subprocess.run(command, check=True, stdout=stdout, env=env)
    return time.perf_counter() - started


def chafa_command(image, width):
    return [
        "chafa", "--probe", "off", "--polite", "on", "--relative", "off",
        "-O", "5", "-f", "symbols", "-c", "full", "-w", "9",
        "--symbols", "all-wide", "-s", str(width), str(image),
    ]


def median(values):
    values = sorted(values)
    return values[len(values) // 2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("corpus", type=Path)
    parser.add_argument("--width", type=int, default=80)
    parser.add_argument("--images", type=int, default=100)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--results", type=Path, default=ROOT / "benchmark-results")
    args = parser.parse_args()

    if not BINARY.exists():
        run(["cargo", "build", "--release", "--offline"], env=os.environ | {"CARGO_TERM_COLOR": "always"})
    if not shutil.which("chafa"):
        raise SystemExit("chafa is required as the reference executable")

    images = sorted(p for p in args.corpus.rglob("*.png") if p.is_file())[:args.images]
    if not images:
        raise SystemExit(f"no PNG images under {args.corpus}")
    args.results.mkdir(parents=True, exist_ok=True)
    environment = os.environ | {"TERM": "xterm-256color"}

    with tempfile.TemporaryDirectory(prefix="chuda-bench-") as temporary:
        temp = Path(temporary)
        inputs = temp / "input"
        inputs.mkdir()
        for index, image in enumerate(images):
            (inputs / f"{index:05}.png").symlink_to(image.resolve())

        # Warm CUDA context, disk cache and Chafa once. Output is deliberately discarded.
        run([BINARY, "--size", str(args.width), images[0]], stdout=subprocess.DEVNULL)
        run(chafa_command(images[0], args.width), stdout=subprocess.DEVNULL, env=environment)

        cuda_times, chafa_times = [], []
        for repeat in range(args.repeats):
            output = temp / f"cuda-{repeat}"
            cuda_times.append(run([BINARY, "--size", str(args.width), inputs, "--output", output], stdout=subprocess.DEVNULL))

            started = time.perf_counter()
            for image in images:
                run(chafa_command(image, args.width), stdout=subprocess.DEVNULL, env=environment)
            chafa_times.append(time.perf_counter() - started)

        sample = images[0]
        with (args.results / "sample-chuda.ansi").open("wb") as output:
            cuda_single = run([BINARY, "--size", str(args.width), sample], stdout=output)
        with (args.results / "sample-chafa.ansi").open("wb") as output:
            chafa_single = run(chafa_command(sample, args.width), stdout=output, env=environment)

    cuda = median(cuda_times)
    chafa = median(chafa_times)
    report = {
        "images": len(images), "width": args.width, "repeats": args.repeats,
        "chafa_command": chafa_command("INPUT.png", args.width),
        "batch_seconds": {"chuda": cuda, "chafa": chafa},
        "images_per_second": {"chuda": len(images) / cuda, "chafa": len(images) / chafa},
        "speedup": chafa / cuda,
        "sample_seconds": {"chuda": cuda_single, "chafa": chafa_single},
        "sample_input": str(sample),
    }
    (args.results / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"{len(images)} images at width {args.width}, median of {args.repeats}")
    print(f"chuda       {cuda:8.3f}s  {len(images)/cuda:8.2f} images/s")
    print(f"chafa       {chafa:8.3f}s  {len(images)/chafa:8.2f} images/s")
    print(f"speedup     {chafa/cuda:8.2f}x")
    print(f"outputs: {args.results}")


if __name__ == "__main__":
    main()
