# Local Search Resources

Run `npm run prepare:local-search` before a local release build. The release
workflow runs the same command before bundling macOS. These resources are
intentionally excluded from source control because model weights are large.

- `models/bge-small-zh-v1.5/config.json`
- `models/bge-small-zh-v1.5/tokenizer.json`
- `models/bge-small-zh-v1.5/special_tokens_map.json`
- `models/bge-small-zh-v1.5/pytorch_model.bin`
- `models/bge-small-zh-v1.5/manifest.json`
- `sqlite-vec/vec0.dylib`

The prepare script pins expected SHA-256 values and writes `manifest.json`.
The application validates that manifest before loading Candle weights.
`sqlite-vec` is loaded only from this packaged resource path and extension
loading is disabled immediately after it.
