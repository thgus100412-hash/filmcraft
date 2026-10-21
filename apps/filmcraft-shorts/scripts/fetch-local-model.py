#!/usr/bin/env python3
"""Setup helper: pin official model revision, verify authoritative LFS size and SHA-256.

Only public model files are downloaded. Interview data never leave the machine.
Requires network access to huggingface.co and the CDN it redirects to.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import urllib.request

REPO = "Qwen/Qwen3-4B-Instruct-2507-GGUF"
MODEL = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf"
DEST = Path("/workspace/.local-model/models")


def fetch():
    with urllib.request.urlopen(f"https://huggingface.co/api/models/{REPO}?blobs=true", timeout=30) as response:
        metadata = json.load(response)
    revision = metadata["sha"]
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise RuntimeError("Invalid upstream model revision")
    candidates = [entry for entry in metadata["siblings"] if entry["rfilename"] == MODEL]
    if len(candidates) != 1:
        raise RuntimeError("Expected official GGUF artifact not found; review model metadata")
    lfs = candidates[0]["lfs"]
    expected, size = lfs["sha256"], lfs["size"]
    if not re.fullmatch(r"[0-9a-f]{64}", expected) or not 0 < size < 5_000_000_000:
        raise RuntimeError("Invalid upstream checksum or size")
    DEST.mkdir(parents=True, exist_ok=True)
    target = DEST / MODEL
    if target.exists():
        with target.open("rb") as stream:
            if target.stat().st_size == size and hashlib.file_digest(stream, "sha256").hexdigest() == expected:
                print(target)
                return
        raise RuntimeError("Existing model failed verification; preserved for investigation")
    partial = target.with_suffix(".gguf.part")
    if partial.exists():
        raise RuntimeError("Partial download exists; inspect before restarting")
    digest = hashlib.sha256()
    received = 0
    try:
        with urllib.request.urlopen(f"https://huggingface.co/{REPO}/resolve/{revision}/{MODEL}", timeout=120) as response, partial.open("xb") as output:
            while chunk := response.read(1024 * 1024):
                received += len(chunk)
                if received > size:
                    raise RuntimeError("Model exceeds authoritative size")
                digest.update(chunk)
                output.write(chunk)
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
