#!/usr/bin/env python3
"""Setup helper: pin publisher model revision, verify authoritative LFS size and SHA-256.

The Unsloth GGUF conversion of the Qwen model is used. Only public model files are downloaded. Interview data never leave the machine.
Requires network access to huggingface.co and the CDN it redirects to.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import urllib.request

REPO = "unsloth/Qwen3-4B-Instruct-2507-GGUF"
MODEL = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf"
DEFAULT_DEST = (Path.home() / "Library/Application Support/FilmcraftShorts/ai/models") if sys.platform == "darwin" else Path("/workspace/.local-model/models")
DEST = Path(os.environ.get("FILMCRAFT_MODEL_DIR", str(DEFAULT_DEST)))


def checksum(stream):
    digest = hashlib.sha256()
    while True:
        block = stream.read(1024 * 1024)
        if not block:
            break
        digest.update(block)
    return digest.hexdigest()


def fetch():
    with urllib.request.urlopen(f"https://huggingface.co/api/models/{REPO}?blobs=true", timeout=30) as response:
        metadata = json.load(response)
    revision = metadata["sha"]
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise RuntimeError("Invalid upstream model revision")
    candidates = [entry for entry in metadata["siblings"] if entry["rfilename"] == MODEL]
    if len(candidates) != 1:
        raise RuntimeError("Expected publisher GGUF artifact not found; review model metadata")
    lfs = candidates[0]["lfs"]
    expected, size = lfs["sha256"], lfs["size"]
    if not re.fullmatch(r"[0-9a-f]{64}", expected) or not 0 < size < 5_000_000_000:
        raise RuntimeError("Invalid upstream checksum or size")
    DEST.mkdir(parents=True, exist_ok=True)
    target = DEST / MODEL
    if target.exists():
        with target.open("rb") as stream:
            if target.stat().st_size == size and checksum(stream) == expected:
                print(target)
                return
        raise RuntimeError("Existing model failed verification; preserved for investigation")
    partial = target.with_suffix(".gguf.part")
    if partial.exists():
        raise RuntimeError("Partial download exists; inspect before restarting")
    print("Downloading local AI model ({:.1f} GB). Interview data are not uploaded.".format(size / 1e9), flush=True)
    digest = hashlib.sha256()
    received = 0
    progress = -1
    try:
        with urllib.request.urlopen(f"https://huggingface.co/{REPO}/resolve/{revision}/{MODEL}", timeout=120) as response, partial.open("xb") as output:
            while chunk := response.read(1024 * 1024):
                received += len(chunk)
                if received > size:
                    raise RuntimeError("Model exceeds authoritative size")
                digest.update(chunk)
                output.write(chunk)
                current = received * 10 // size
                if current != progress:
                    progress = current
                    print("{}%".format(min(100, current * 10)), flush=True)
        if received != size or digest.hexdigest() != expected:
            raise RuntimeError("Model size or SHA-256 mismatch")
        os.link(partial, target)  # Fail rather than replace an existing user file.
        partial.unlink()
        receipt = {"repository": REPO, "revision": revision, "file": MODEL, "size": size, "sha256": expected}
        with target.with_suffix(".receipt.json").open("x") as output:
            output.write(json.dumps(receipt, indent=2) + "\n")
    except Exception:
        partial.unlink(missing_ok=True)
        raise
    print(target)


if __name__ == "__main__":
    fetch()
