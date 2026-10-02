"""Checks for benchmark scoring, without loading speech models."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
import wave

spec = importlib.util.spec_from_file_location("probe", Path(__file__).with_name("pianissimo-probe.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class ScoringTests(unittest.TestCase):
    def test_swedish_letters_and_punctuation(self):
        self.assertEqual(probe.word_errors("Åsa, Östen!", "åsa östen")["wer"], 0)

    def test_negation_cannot_disappear_unscored(self):
        self.assertEqual(probe.word_errors("har inte feber", "har feber")["word_errors"], 1)

    def test_silence_reference_has_no_division_by_zero(self):
        result = probe.word_errors("", "påhittad text")
        self.assertIsNone(result["wer"])
        self.assertEqual(result["insertions_on_empty_reference"], 2)

    def test_missing_speech_is_complete_failure(self):
        self.assertEqual(probe.word_errors("hela yttrandet saknas", "")["wer"], 1)

    def test_number_formatting_is_not_silently_normalized(self):
        self.assertEqual(probe.word_errors("plan två", "plan 2")["word_errors"], 1)

    def test_wrong_audio_rate_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "test.wav"
            with wave.open(str(path), "wb") as output:
                output.setparams((1, 2, 8000, 0, "NONE", "not compressed"))
                output.writeframes(bytes(16000))
            with self.assertRaisesRegex(ValueError, "16 kHz"):
                probe.read_audio(path)

    def test_cache_is_reused_and_invalidated_by_configuration_or_corruption(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            files = {key: root / f"source-{key}.onnx" for key in ("encoder", "decoder_joint")}
            for key, path in files.items():
                path.write_bytes(key.encode())
            hashes = {key: probe.file_hash(path) for key, path in files.items()}
            args = SimpleNamespace(cache_dir=root / "cache", threads=8, optimization="all", avx2_precision=True)

            def serialize(source, *, sess_options, providers):
                Path(sess_options.optimized_model_filepath).write_bytes(Path(source).read_bytes())

            with patch.object(probe.ort, "InferenceSession", side_effect=serialize) as create:
                cached, _ = probe.cached_files(files, hashes, args)
                self.assertEqual(create.call_count, 2)
                self.assertEqual(probe.cached_files(files, hashes, args)[1], 0)
                self.assertEqual(create.call_count, 2)
                args.avx2_precision = False
                probe.cached_files(files, hashes, args)
                self.assertEqual(create.call_count, 4)
                cached["encoder"].write_bytes(b"interrupted or corrupted file")
                probe.cached_files(files, hashes, args)
                self.assertEqual(create.call_count, 6)


if __name__ == "__main__":
    unittest.main()
