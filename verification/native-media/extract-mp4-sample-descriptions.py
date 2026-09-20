"""Extract native synthetic stsd reference boxes; never generate production MP4."""
import argparse
from pathlib import Path
import struct


def extract(data, path):
    position = 0
    while position + 8 <= len(data):
        size, kind = struct.unpack_from(">I4s", data, position)
        header = 8
        if size == 1:
            size = struct.unpack_from(">Q", data, position + 8)[0]
            header = 16
        if size == 0:
            size = len(data) - position
        if size < header or position + size > len(data):
            raise ValueError(f"invalid atom {kind!r}")
        if kind == path[0]:
            if len(path) == 1:
                return data[position : position + size]
            return extract(data[position + header : position + size], path[1:])
        position += size
    raise ValueError(f"missing atom {path[0]!r}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    args.destination.mkdir(parents=True, exist_ok=True)
    for codec in (1, 2):
        for height in (720, 1080):
            for fps in (30, 60):
                stem = f"{codec}-{height}-{fps}"
                data = (args.source / f"{stem}.mp4").read_bytes()
                description = extract(data, [b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"])
                # Do not overwrite an unrelated previous probe artifact.
                with (args.destination / f"{stem}.stsd").open("xb") as output:
                    output.write(description)


if __name__ == "__main__":
    main()
