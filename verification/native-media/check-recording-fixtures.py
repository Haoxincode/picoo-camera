"""Independently validate synthetic check_native_recording bundles with ffprobe.

REQ-PICOO-NEXT-018/021/022. This checker is specific to the repeated-IDR
fixture matrix, not an assertion that arbitrary recordings contain no gaps.
"""

import argparse
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import subprocess


def require(condition, message):
    if not condition:
        raise ValueError(message)


def check(root, fixtures, ffprobe):
    for wire, codec, native in ((1, "avc", "h264"), (2, "hevc", "hevc")):
        for width, height in ((1280, 720), (1920, 1080)):
            for fps in (30, 60):
                stem = f"{wire}-{height}-{fps}"
                manifests = list((root / stem).glob("recording-*/manifest.json"))
                require(len(manifests) == 1, f"{stem}: expected exactly one bundle")
                manifest = json.loads(manifests[0].read_text())
                require(manifest["state"] == "Complete", f"{stem}: incomplete")
                require(manifest["failure"] is None, f"{stem}: failure")
                require(manifest["gaps"] == [], f"{stem}: gaps")
                require(manifest["actual_start_pts_us"] == 0, f"{stem}: start PTS")
                require(len(manifest["segments"]) == 1, f"{stem}: segment count")
                segment = manifest["segments"][0]
                require(segment["file"] == "segments/000001.mp4", f"{stem}: path")
                path = manifests[0].parent / segment["file"]
                require(path.stat().st_size == segment["bytes"], f"{stem}: size")
                with path.open("rb") as media:
                    digest = hashlib.file_digest(media, "sha256").hexdigest()
                require(digest == segment["sha256"], f"{stem}: digest")
                metadata = segment["metadata"]
                for key, value in dict(codec=codec, width=width, height=height,
                                       fps=fps, rotation=0, mirrored=False).items():
                    require(metadata[key] == value, f"{stem}: metadata {key}")
                config = (fixtures / f"{stem}.config").read_bytes()
                require(metadata["configuration_sha256"] == hashlib.sha256(config).hexdigest(),
                        f"{stem}: source configuration")
                require(metadata["source"] == dict(
                    connection_generation=1, stream_epoch=1, first_au=1,
                    last_au=3 * fps + 1, first_pts_us=0, last_pts_us=3_000_000),
                    f"{stem}: source range")
                result = subprocess.run([
                    ffprobe, "-v", "error", "-count_frames", "-select_streams", "v:0",
                    "-show_entries",
                    "stream=codec_name,width,height,nb_read_frames,color_space,color_range:frame=pts_time",
                    "-of", "json", str(path),
                ], check=True, capture_output=True, text=True, timeout=30)
                require(not result.stderr.strip(), f"{stem}: decode errors: {result.stderr}")
                decoded = json.loads(result.stdout)
                streams = decoded["streams"]
                require(streams == [dict(codec_name=native, width=width, height=height,
                                         nb_read_frames=str(3 * fps + 1),
                                         color_space="bt709", color_range="tv")],
                        f"{stem}: unexpected decoded stream {streams}")
                frames = decoded["frames"]
                require(len(frames) == 3 * fps + 1, f"{stem}: frame timestamps missing")
                for index, frame in enumerate(frames):
                    require(abs(Decimal(frame["pts_time"]) - Decimal(index) / fps)
                            <= Decimal("0.000001"), f"{stem}: frame {index} PTS")
                print(f"PASS {stem}: manifest, SHA-256, source mapping, decoded frames/color/PTS")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("fixtures", type=Path)
    parser.add_argument("--ffprobe", default="ffprobe")
    args = parser.parse_args()
    check(args.output, args.fixtures, args.ffprobe)
