# kokin-ai

Optional local inference, subordinate to source evidence and human decisions.

Implemented: cited-output validation, untrusted evidence envelopes, append-only
encrypted suggestions/review, native hardware metadata, pinned Qwen registry and
measurement-gated profiles, CPU bge-small int8 via bundled ONNX Runtime, and
384-dimensional encrypted/rebuildable sqlite-vec retrieval behind provider/index
traits. ONNX telemetry and automatic model fetching are disabled.

The desktop AI workspace provides separate resource consent/import and local
Ollama setup. Only `kokin-net` opens sockets, through closed public resource and
literal-loopback capabilities. No remote pull or analytical tools are exposed.

Real BGE retrieval passed 8/8 synthetic paraphrase queries versus 0/8 for the same
lexical compiler. This is bounded English test evidence, not universal retrieval
quality or analytical confidence. Ollama remains absent on the reference Mac;
Qwen inference/benchmarks are not claimed as tested and no recommendations are
fabricated. See docs/increments/35-local-ai-and-semantic.md.
