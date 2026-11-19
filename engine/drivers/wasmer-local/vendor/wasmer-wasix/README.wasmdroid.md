# WASIX source pin and WasmDroid patch

The source comes from the published wasmer-wasix 0.704.2 crate, with SHA-256
8498d5a277a3e1559d8f165a1ee828572eaee6421b6404a85c20161436c0855f.
Its archive identifies upstream commit 7a48a071c7682a409d148cf37dc8da58322a123d
and source path lib/wasix. The upstream MIT license is included. The import
removed end-of-line spaces on four lines flagged by Git.

WasmDroid uses this copy to preserve an unset receive deadline on established
TCP streams. Explicit socket deadlines and the existing defaults for other
socket kinds remain in force. The policy is used by sock_recv and socket-backed
fd_read. Host forwarding and worker shutdown retain their existing ownership
and cancellation rules. Keep this patch confined to the driver and reevaluate
it when upgrading to an upstream release with equivalent behavior.
