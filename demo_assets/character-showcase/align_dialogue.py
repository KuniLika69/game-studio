"""Align Mira's Russian dialogue to the existing PCM and build editable mouth curves."""

import argparse
import json
import math
from pathlib import Path
import re
import subprocess
import wave

import kaldi_native_fbank as kaldi
import numpy as np
import onnxruntime as ort


ASSETS = Path(__file__).resolve().parent
TEXT = {
    "quay": "Фонарь у входа ещё горит. Занесу инструмент в мастерскую. "
            "Сначала осторожно пройду через лужу, потом отмечусь у верстака.",
    "delta": "Слышишь, как свистит ветер? Значит, мы близко. "
             "Сначала проверю медный пост, потом маяк.",
}
MOUTH = [
    "jawOpen", "mouthClose", "mouthPucker", "mouthFunnel",
    "mouthRollLower", "mouthStretchLeft", "mouthStretchRight",
]
SHAPES = {
    # Jaw, closure, rounding, funnel, lower lip under teeth, corners.
    "а": (0.78, 0.0, 0.0, 0.0, 0.0, 0.11),
    "я": (0.68, 0.0, 0.0, 0.0, 0.0, 0.20),
    "у": (0.28, 0.0, 0.79, 0.24, 0.0, 0.0),
    "ю": (0.29, 0.0, 0.71, 0.31, 0.0, 0.0),
    "о": (0.52, 0.0, 0.43, 0.48, 0.0, 0.0),
    "ё": (0.52, 0.0, 0.43, 0.47, 0.0, 0.0),
    "э": (0.48, 0.0, 0.0, 0.0, 0.0, 0.33),
    "е": (0.43, 0.0, 0.0, 0.0, 0.0, 0.39),
    "и": (0.21, 0.0, 0.0, 0.0, 0.0, 0.54),
    "ы": (0.27, 0.0, 0.0, 0.0, 0.0, 0.37),
}
VOWELS = set(SHAPES)
CLOSE = set("бмп")
ACTING_BEATS = {
    "quay": {"фонарь": 0.24, "горит": 0.11, "осторожно": 0.21, "верстака": 0.16},
    "delta": {"слышишь": 0.31, "ветер": 0.15, "близко": 0.23, "маяк": 0.18},
}
DOWNWARD_GLANCES = {"quay": "лужу", "delta": "медный"}


def audio_features(path):
    args = ["ffmpeg", "-v", "error", "-i", str(path), "-ac", "1", "-ar", "16000",
            "-f", "f32le", "-"]
    audio = np.frombuffer(subprocess.check_output(args), dtype="<f4")
    options = kaldi.FbankOptions()
    options.frame_opts.dither = 0
    options.frame_opts.remove_dc_offset = False
    options.frame_opts.preemph_coeff = 0
    options.frame_opts.window_type = "hann"
    options.frame_opts.round_to_power_of_two = False
    options.mel_opts.low_freq = 0
    options.mel_opts.high_freq = 8000
    options.mel_opts.num_bins = 64
    fbank = kaldi.OnlineFbank(options)
    fbank.accept_waveform(16000, audio)
    features = np.stack([fbank.get_frame(i) for i in range(fbank.num_frames_ready)])
    return audio, features.astype(np.float32)


def vocabulary(path):
    tokens = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if line:
            label, index = line.rsplit(maxsplit=1) if len(line.split()) == 2 else (" ", line.strip())
            tokens[int(index)] = " " if label == "|" else label
    return tokens


def forced_path(scores, labels, blank):
    """Maximum-likelihood CTC path with legal skips and repeated-letter boundaries."""
    extended = np.full(2 * len(labels) + 1, blank, np.int32)
    extended[1::2] = labels
    count = len(extended)
    prev = np.full(count, -np.inf, np.float32)
    prev[0] = scores[0, blank]
    prev[1] = scores[0, extended[1]]
    back = np.zeros((len(scores), count), np.uint8)
    may_skip = np.zeros(count, dtype=bool)
    may_skip[2:] = (extended[2:] != blank) & (extended[2:] != extended[:-2])
    for t in range(1, len(scores)):
        one = np.r_[-np.inf, prev[:-1]]
        two = np.where(may_skip, np.r_[-np.inf, -np.inf, prev[:-2]], -np.inf)
        alternatives = np.stack((prev, one, two))
        back[t] = np.argmax(alternatives, axis=0)
        prev = alternatives.max(axis=0) + scores[t, extended]
    state = count - (1 if prev[-1] >= prev[-2] else 2)
    if not np.isfinite(prev[state]):
        raise ValueError("The transcript cannot be aligned with the recording.")
    trace = np.empty(len(scores), np.int32)
    for t in range(len(scores) - 1, -1, -1):
        trace[t] = state
        state -= int(back[t, state])
    return [np.flatnonzero(trace == 2 * i + 1) for i in range(len(labels))]


def visible_shape(char):
    if char in SHAPES:
        return SHAPES[char]
    if char in CLOSE:
        return (-0.24, 0.96, 0.0, 0.0, 0.0, 0.0)
    if char in "вф":
        return (0.09, 0.17, 0.0, 0.0, 0.63, 0.09)
    if char in "жшщч":
        return (0.23, 0.0, 0.26, 0.21, 0.0, 0.10)
    return (0.17, 0.0, 0.0, 0.0, 0.0, 0.14)


def simplify(values, times, tolerance):
    """Preserve visible peaks while meeting the project's limits on editable keys."""
    keep = {0, len(values) - 1}
    pending = [(0, len(values) - 1)]
    while pending:
        first, last = pending.pop()
        if last <= first + 1:
            continue
        expected = np.interp(
            times[first + 1:last], [times[first], times[last]], [values[first], values[last]]
        )
        errors = np.abs(values[first + 1:last] - expected)
        chosen = int(np.argmax(errors))
        if errors[chosen] > tolerance:
            index = first + 1 + chosen
            keep.add(index)
            pending.extend([(first, index), (index, last)])
    return [
        {"ms": int(times[index]), "weight": round(float(values[index]), 4)}
        for index in sorted(keep)
    ]


def mouth_at(lane, channel, at):
    keys = next((row["keys"] for row in lane["channels"] if row["target"] == channel), [])
    if not keys:
        return 0.0
    return float(np.interp(at, [key["ms"] for key in keys], [key["weight"] for key in keys]))


def expression_curves(kind, baseline, events, duration):
    replaced = set(MOUTH) | {"browInnerUp", "browOuterUpRight", "eyeLookDownLeft", "eyeLookDownRight"}
    tracks = [row for row in baseline["channels"] if row["target"] not in replaced]
    words = {}
    offset = 0
    for word in "".join(event["char"] for event in events).split():
        span = events[offset:offset + len(word)]
        words[word] = (span[0]["center_ms"], span[-1]["center_ms"])
        offset += len(word) + 1

    def add(target, items):
        keys = {0: 0.0, duration: 0.0}
        keys.update({at: round(weight, 4) for at, weight in items if 0 < at < duration})
        tracks.append({"target": target, "keys": [
            {"ms": time, "weight": keys[time]} for time in sorted(keys)
        ]})

    for target, gain in [("browInnerUp", 1.0), ("browOuterUpRight", 0.48)]:
        keys = []
        for word, height in ACTING_BEATS[kind].items():
            start, end = words[word]
            keys.extend([(start - 115, 0.01), (start - 22, height * gain),
                         (end + 130, 0.0)])
        add(target, keys)
    start, end = words[DOWNWARD_GLANCES[kind]]
    add("eyeLookDownLeft", [(start - 120, 0.0), (start + 20, 0.19), (end + 220, 0.0)])
    add("eyeLookDownRight", [(start - 109, 0.0), (start + 31, 0.17), (end + 228, 0.0)])
    return tracks


def build_curves(events, audio, duration, expressive):
    times = np.arange(0, duration, 15)
    times = np.append(times, duration)
    signal = np.zeros((len(times), 6), np.float32)
    influence = np.zeros(len(times), np.float32)
    for i, event in enumerate(events):
        char = event["char"]
        if char in " ьъ":
            continue
        at = event["center_ms"]
        previous = events[i - 1]["center_ms"] if i else at - 80
        following = events[i + 1]["center_ms"] if i + 1 < len(events) else at + 80
        lead = min(85, max(32, (at - previous) * 0.64))
        decay = min(96, max(33, (following - at) * 0.63))
        if char in CLOSE:
            at -= 18  # Make the lip seal lead the consonant release.
            lead = 46
            decay = 42
        distance = times - at
        weight = np.maximum(0, 1 - np.abs(distance) / np.where(distance < 0, lead, decay))
        signal += weight[:, None] * visible_shape(char)
        influence += weight
    signal /= np.maximum(1.0, influence[:, None])
    for index, ms in enumerate(times):
        center = int(ms) * 16
        area = audio[max(0, center - 192):min(len(audio), center + 192)]
        rms = float(np.sqrt(np.mean(area * area))) if len(area) else 0.0
        speech = min(1.0, max(0.0, (rms - 0.006) * 45))
        closure = signal[index, 1]
        signal[index, 0] *= speech * (1 - min(0.99, closure * 1.35))
        signal[index, 2:] *= max(0.15, speech)
    curves = np.clip(signal[:, [0, 1, 2, 3, 4, 5, 5]], 0.0, 1.0)
    curves[:, -1] *= 0.85
    curves[0, :] = 0
    curves[-1, :] = 0
    expression_keys = sum(len(row["keys"]) for row in expressive)
    tolerance = 0.022
    while True:
        mouth = [
            {"target": channel, "keys": simplify(curves[:, index], times, tolerance)}
            for index, channel in enumerate(MOUTH)
        ]
        if (all(len(row["keys"]) <= 192 for row in mouth)
                and sum(len(row["keys"]) for row in mouth) + expression_keys <= 768):
            return mouth + expressive, tolerance
        tolerance *= 1.25


def distance(a, b):
    previous = list(range(len(b) + 1))
    for i, left in enumerate(a, 1):
        row = [i]
        for j, right in enumerate(b, 1):
            row.append(min(row[-1] + 1, previous[j] + 1, previous[j - 1] + (left != right)))
        previous = row
    return previous[-1]


def align(kind, session, tokens, baseline, report_dir):
    wav = ASSETS / f"mira-{kind}.wav"
    with wave.open(str(wav), "rb") as recording:
        duration = round(1000 * recording.getnframes() / recording.getframerate())
    audio, features = audio_features(wav)
    scores = session.run(None, {
        "features": features.T[None, ...].copy(),
        "feature_lengths": np.array([len(features)], np.int64),
    })[0][0]
    encoded = {label: i for i, label in tokens.items()}
    blank = encoded["<blk>"]
    text = re.sub(r"[^а-яё]+", " ", TEXT[kind].lower()).strip()
    target = text.replace("ё", "е")
    frames = forced_path(scores, [encoded[char] for char in target], blank)
    greedy = []
    previous = blank
    for index in scores.argmax(axis=1):
        if index != blank and index != previous:
            greedy.append(tokens[index])
        previous = index
    recognition = "".join(greedy).strip()
    edits = distance(target, recognition)
    if edits > max(4, len(target) * 0.15):
        raise ValueError(f"The recording differs from the transcript: {recognition!r}")
    hop = 10 * int(session.get_modelmeta().custom_metadata_map["subsampling_factor"])
    events = []
    for char, label, start_frames in zip(text, target, frames):
        center = int(start_frames[np.argmax(scores[start_frames, encoded[label]])])
        events.append({
            "char": char,
            "center_ms": min(duration, round(center * hop + 12.5)),
            "emission": round(math.exp(float(scores[center, encoded[label]])), 4),
        })

    expressive = expression_curves(kind, baseline, events, duration)
    curves, tolerance = build_curves(events, audio, duration, expressive)
    revised = {"duration_ms": duration, "channels": curves}
    closures = [max(0, event["center_ms"] - 18) for event in events if event["char"] in CLOSE]

    def closure_score(lane):
        weights = [max(mouth_at(lane, "mouthClose", t + offset) for offset in [-15, 0, 15])
                   for t in closures]
        return {"matched": sum(weight >= 0.45 for weight in weights),
                "mean": round(sum(weights) / len(weights), 3)}

    write_json(ASSETS / f"mira-{kind}.face.json", revised)
    report = {
        "recording_ms": duration, "acoustic_frame_ms": hop, "text": TEXT[kind],
        "greedy_text": recognition, "edit_distance": edits, "characters": len(events),
        "closure_events": len(closures), "before": closure_score(baseline),
        "after": closure_score(revised), "key_tolerance": tolerance,
        "keys": sum(len(row["keys"]) for row in curves), "segments": events,
    }
    write_json(report_dir / f"{kind}-alignment.json", report)
    print(kind, {key: report[key] for key in ("edit_distance", "closure_events", "before", "after", "keys")})


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("model_dir", type=Path)
    parser.add_argument("before_projects", type=Path)
    parser.add_argument("report_dir", type=Path)
    args = parser.parse_args()
    args.report_dir.mkdir(parents=True, exist_ok=True)
    options = ort.SessionOptions()
    options.intra_op_num_threads = 4
    options.inter_op_num_threads = 1
    session = ort.InferenceSession(str(args.model_dir / "model.int8.onnx"), options,
                                   providers=["CPUExecutionProvider"])
    tokens = vocabulary(args.model_dir / "tokens.txt")
    for kind in TEXT:
        project = json.loads((args.before_projects / f"mira-{kind}.robot.json").read_text(encoding="utf-8"))
        conversation = project["story"]["conversations"][0]
        node = next(node for node in conversation["nodes"] if node["id"] == conversation["entry"])
        align(kind, session, tokens, node["face"], args.report_dir)


if __name__ == "__main__":
    main()
