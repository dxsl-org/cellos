"""Measure a sine tone in a QEMU `-audiodev wav` recording (Tier 3 Chrome sound check).

QEMU writes the RIFF sizes only when it quits cleanly; a recording cut short has zero
sizes, so the data chunk is taken to run to the end of the file. Reports the loud span's
length, its frequency (zero crossings of the first channel) and the longest silence
inside it.

Usage: python3 scripts/tier3_wav_tone.py FILE.wav  (prints JSON)
"""
import json
import struct
import sys
from pathlib import Path

# A sample this far from zero counts as sound (16-bit; the test tone is at half scale).
LOUD = 1000


def pcm(data: bytes):
    """(channels, rate, first-channel samples) of a 16-bit PCM WAV."""
    fmt = data.find(b"fmt ")
    chunk = data.find(b"data")
    if data[:4] != b"RIFF" or fmt < 0 or chunk < 0:
        raise ValueError("not a WAV recording")
    tag, channels, rate, _, _, bits = struct.unpack("<HHIIHH", data[fmt + 8:fmt + 24])
    if tag != 1 or bits != 16 or channels < 1:
        raise ValueError(f"unsupported format tag={tag} bits={bits}")
    size = struct.unpack("<I", data[chunk + 4:chunk + 8])[0]
    body = data[chunk + 8:]
    if size:
        body = body[:size]
    frame = 2 * channels
    body = body[:len(body) - len(body) % frame]
    samples = struct.unpack(f"<{len(body) // 2}h", body)[::channels]
    return channels, rate, samples


def measure(samples, rate: int) -> dict:
    loud = [i for i, value in enumerate(samples) if abs(value) > LOUD]
    if not loud:
        return {"loud_s": 0.0, "freq_hz": 0.0, "longest_gap_ms": 0.0}
    first, last = loud[0], loud[-1]
    span = samples[first:last + 1]
    crossings = sum(1 for a, b in zip(span, span[1:]) if a < 0 <= b)
    gap = longest = 0
    # A sine passes zero twice a period: a "gap" is a run of quiet samples longer
    # than one period at the lowest audible tone would need.
    for value in span:
        gap = gap + 1 if abs(value) <= LOUD else 0
        longest = max(longest, gap)
    seconds = (last - first) / rate
    return {"loud_s": round(seconds, 3), "freq_hz": round(crossings / seconds, 1) if seconds else 0.0,
            "longest_gap_ms": round(longest * 1000 / rate, 1)}


def main() -> int:
    channels, rate, samples = pcm(Path(sys.argv[1]).read_bytes())
    print(json.dumps({"channels": channels, "rate": rate, **measure(samples, rate)}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
