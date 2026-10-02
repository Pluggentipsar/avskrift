Avskrift's Rust code is licensed under the repository's MIT license.

The downloadable Swedish Pianissimo model is by KlangAI:
https://huggingface.co/KlangAI/pianissimo-sv

The INT8 ONNX export used here is KlangAI's own (SmoothQuant, local attention kept):
https://huggingface.co/KlangAI/pianissimo-sv-onnx
Pinned revision: 63730c6021234f26b9bbae9a07a04fec39e7a52e.
Versions up to 0.8.0-beta.1 used the community export by moonhouse:
https://huggingface.co/moonhouse/pianissimo-sv-onnx

Model license: Creative Commons Attribution 4.0 International (CC BY 4.0):
https://creativecommons.org/licenses/by/4.0/

Avskrift runs the export as published; nothing is re-quantized or converted locally.
Model weights are downloaded separately and are not covered by Avskrift's MIT license.
