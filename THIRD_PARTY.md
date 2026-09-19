# Attribution

Accessor uses these projects as dependencies; it is not affiliated with their authors.

- [Handy](https://github.com/cjpais/Handy), MIT: architectural reference. No Handy desktop code is bundled.
- [transcribe-rs](https://github.com/cjpais/transcribe-rs), MIT: local transcription engine.
- [NVIDIA Canary 180M Flash](https://huggingface.co/nvidia/canary-180m-flash), CC BY 4.0: original speech model.
- [istupakov Canary ONNX conversion](https://huggingface.co/istupakov/canary-180m-flash-onnx), CC BY 4.0: quantized encoder/decoder and vocabulary. Downloaded artifacts are converted/quantized versions of NVIDIA's model, not changes made by Accessor.
- [istupakov NeMo preprocessing export](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx): shared `nemo128.onnx` preprocessing graph used by transcribe-rs. See that repository's model card/license.
- [ONNX Runtime](https://github.com/microsoft/onnxruntime), MIT: CPU inference. Setup preserves the release license and notices in `runtime/`.
- [CPAL](https://github.com/RustAudio/cpal), Apache-2.0: audio capture/playback.
- [Rubato](https://github.com/HEnquist/rubato), MIT: resampling.
- [Sonora](https://crates.io/crates/sonora), BSD-3-Clause: pure Rust WebRTC audio processing / AEC3 echo cancellation, including its upstream notices.
- [Earshot](https://github.com/pykeio/earshot), MIT OR Apache-2.0: voice activity detection.

Model files and runtime binaries are downloaded separately, not committed. Keep upstream license/attribution notices when redistributing them. Other Rust dependency licenses are recorded in their package metadata.

Optional local speech:

- [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M), Apache-2.0 model weights.
- [kokoro-onnx](https://github.com/thewh1teagle/kokoro-onnx), MIT, with model-files-v1.1 ONNX/voice exports verified by SHA-256.
- The separate Kokoro environment includes phonemizer/espeak-ng dependencies, whose GPL and other upstream license requirements apply when redistributing them. Keep their package license notices; Accessor does not relicense these components.

Terminal UI uses Ratatui and Crossterm. Credential storage uses keyring with native OS backends. These Rust dependencies and their licenses are recorded in package metadata and Cargo.lock.
