# Fuzz targets — REQ-PICOO-PROTOCOL-007

独立 workspace（不加入根 `Cargo.toml` members），需 nightly + `cargo-fuzz`：

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run video_packet_decode
```

日常 CI 仍依赖 `picoo-protocol` 内的随机字节解码非 panic 测试作为轻量回归。
