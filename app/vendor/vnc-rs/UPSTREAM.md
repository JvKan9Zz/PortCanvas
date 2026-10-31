# vnc-rs 0.6.0 (PortCanvas patch)

Upstream: https://github.com/HsuJv/vnc-rs / crates.io vnc-rs 0.6.0.
Archive SHA-256: `0b757c55558e81e08e60f1a3153ba5bcffe7196719bc885e7e620400b0d81a4d`.
License: MIT OR Apache-2.0; both original license files and author metadata are retained.

This copy contains the original Rust library and tests. PortCanvas adds only:

- Optional validated Tight quality/compression levels, sent with SetEncodings before the first refresh.
- Optional FrameEnd events after complete framebuffer updates, for first-frame validation and one outstanding refresh.
- Standard clipboard text is encoded/decoded as Latin-1 as required by RFB. Unsupported Unicode is rejected before queueing, instead of corrupting text. UTF-8 server names are unchanged.
- A local input-write barrier lets the native owner flush queued key/button releases before closing its SSH stream. It does not claim remote acknowledgement.

Both options default off. Existing connector behavior and desktop-size observation remain unchanged. The native application does not request X11 display resizes.
