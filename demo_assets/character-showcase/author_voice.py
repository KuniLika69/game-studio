"""Create editable face lanes from the two finished Mira dialogue takes."""

import array
import json
import math
from pathlib import Path
import wave


ROOT = Path(__file__).resolve().parent
TAKES = {
    "quay": {
        "rounded": [310, 780, 1350, 2490, 3340, 4540, 5200, 6410, 7240, 8270],
        "closed": [80, 710, 1830, 2620, 3060, 3640, 5350, 6160, 7660, 8150],
        "blinks": [1920, 4110, 6980],
        "brows": [(0, 0.04), (260, 0.27), (920, 0.12), (2300, 0.06),
                  (3380, 0.32), (4470, 0.02), (6610, 0.24), (7420, 0.10), (8810, 0.0)],
        "smile": [(0, 0.0), (3750, 0.13), (4390, 0.08), (4920, 0.0)],
    },
    "delta": {
        "rounded": [350, 1230, 1730, 2430, 3000, 4250, 5050, 5530, 6140],
        "closed": [220, 940, 2890, 3600, 3990, 4670, 5300, 5820],
        "blinks": [1990, 3490, 5670],
        "brows": [(0, 0.02), (285, 0.34), (1325, 0.27), (1865, 0.12),
                  (2700, 0.05), (3560, 0.12), (5500, 0.17), (6790, 0.0)],
        "smile": [(0, 0.04), (1830, 0.08), (2780, 0.22), (3450, 0.09), (5900, 0.0)],
    },
}


def pulse(ms, marks, width):
    return max((max(0.0, 1.0 - abs(ms - mark) / width) for mark in marks), default=0.0)


def author(name, take):
    path = ROOT / f"mira-{name}.wav"
    with wave.open(str(path), "rb") as recording:
        assert recording.getnchannels() == 1 and recording.getsampwidth() == 2
        rate = recording.getframerate()
        pcm = array.array("h", recording.readframes(recording.getnframes()))
    duration = round(1000 * len(pcm) / rate)
    channels = []

    def add(target, items):
        keys = {0: 0.0, duration: 0.0}
        keys.update({at: round(weight, 4) for at, weight in items if 0 < at < duration})
        channels.append({
            "target": target,
            "keys": [{"ms": at, "weight": keys[at]} for at in sorted(keys)],
        })

    samples = []
    for ms in range(45, duration, 55):
        start = max(0, round((ms - 13) * rate / 1000))
        end = min(len(pcm), round((ms + 13) * rate / 1000))
        rms = math.sqrt(sum(x * x for x in pcm[start:end]) / max(end - start, 1)) / 32768.0
        articulation = min(0.76, max(0.0, math.sqrt(rms) * 2.2 - 0.12))
        close = pulse(ms, take["closed"], 63)
        rounded = pulse(ms, take["rounded"], 122)
        samples.append((
            ms,
            articulation * (1.0 - 0.78 * close),
            close * min(0.55, 0.19 + articulation),
            rounded * min(0.46, articulation * 0.85),
        ))
    for index, target in enumerate(["jawOpen", "mouthClose", "mouthPucker"], 1):
        add(target, ((sample[0], sample[index]) for sample in samples))
    for side in ("Left", "Right"):
        lag = 0 if side == "Left" else 13
        add("eyeBlink" + side, (
            (center + lag + offset, weight)
            for center in take["blinks"]
            for offset, weight in [(-73, 0.0), (-19, 0.68), (0, 0.96), (80, 0.0)]
        ))
        add("mouthSmile" + side, ((at, value * (1.0 if side == "Left" else 0.72))
                                  for at, value in take["smile"]))
    add("browInnerUp", take["brows"])
    add("eyeLookDownLeft", [(320, 0.13), (760, 0.0), (duration // 2, 0.18),
                           (duration // 2 + 320, 0.0)])
    add("eyeLookDownRight", [(333, 0.12), (770, 0.0), (duration // 2, 0.17),
                            (duration // 2 + 333, 0.0)])
    (ROOT / f"mira-{name}.face.json").write_text(
        json.dumps({"duration_ms": duration, "channels": channels}, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )


for take_name, settings in TAKES.items():
    author(take_name, settings)
