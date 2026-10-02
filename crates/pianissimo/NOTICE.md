Avskrift's Rust code is licensed under the repository's MIT license.

The downloadable Swedish Pianissimo model is by KlangAI:
https://huggingface.co/KlangAI/pianissimo-sv

The INT8 ONNX export and quantization used here are by moonhouse:
https://huggingface.co/moonhouse/pianissimo-sv-onnx
Pinned revision: 72c38267654dadd538bceac7a851de00fb55f11a.

Model license: Creative Commons Attribution 4.0 International (CC BY 4.0):
https://creativecommons.org/licenses/by/4.0/

Avskrift optimizes the encoder locally with ONNX Runtime and splits that optimized
graph into twelve execution stages. Weights are copied without further quantization.
The prepared graph is a local derivative and is rebuilt for a changed runtime or CPU.
Model weights are downloaded separately and are not covered by Avskrift's MIT license.
