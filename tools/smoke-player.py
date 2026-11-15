#!/usr/bin/env python3
"""Open hidden player windows with a synthetic file, without using an account."""
import os
import subprocess
import tempfile
import time
import wave
from pathlib import Path

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory() as directory:
    audio_path = Path(directory) / "silence.wav"
    with wave.open(str(audio_path), "wb") as audio:
        audio.setnchannels(1)
        audio.setsampwidth(2)
        audio.setframerate(16000)
        audio.writeframes(bytes(32000))
    env = os.environ.copy()
    env.update(
        BETTER_AUDIBLE_SMOKE_TEST="1",
        BETTER_AUDIBLE_SMOKE_AUDIO=str(audio_path),
        BETTER_AUDIBLE_SMOKE_REOPEN="1",
    )
    process = subprocess.Popen(
        [str(root / "target/debug/better-audible")],
        env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    time.sleep(6)
    alive = process.poll() is None
    if alive:
        process.terminate()
    out, err = process.communicate(timeout=10)
    print(out, end="")
    print(err, end="")
    expected = ("mini player window opened", "main player window reopened")
    if not alive or any(message not in err for message in expected):
        raise SystemExit("Player window smoke test failed")
    print("Player window smoke test passed")
