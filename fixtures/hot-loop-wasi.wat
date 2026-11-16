(module
  ;; Identify the WASI ABI without invoking any host call inside the hot loop.
  (import "wasi_snapshot_preview1" "fd_write"
    (func (param i32 i32 i32 i32) (result i32)))
  ;; WASIX reserves a 1 MiB stack when preparing its main context.
  (memory (export "memory") 32)
  (global (export "__stack_pointer") (mut i32) (i32.const 1048576))
  (func (export "_start")
    (loop $hot (br $hot))))
