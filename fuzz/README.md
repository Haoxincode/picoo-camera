# Fuzz targets — REQ-PICOO-PROTOCOL-007/013

独立 workspace（不加入根 `Cargo.toml` members），需 nightly + `cargo-fuzz`：

```bash
cargo install cargo-fuzz --version 0.13.2 --locked
cargo +nightly fuzz run video-packet-decode
cargo +nightly fuzz run control-envelope
cargo +nightly fuzz run pairing-transcript
cargo +nightly fuzz run reassembly-fec
```

`corpus/` 保存可审查的固定 regression seed；`hex:` 文件在 target 内解码成二进制输入。日常 CI
仍运行协议的轻量非 panic/门禁回归，夜间 workflow 对四个 target 执行有时限 fuzz。
