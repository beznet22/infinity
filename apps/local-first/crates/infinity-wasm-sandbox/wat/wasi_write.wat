;; A tool that wants the WASI filesystem. The sandbox provides no imports, so it
;; must be refused at instantiation rather than allowed to run.
(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (func (export "run") (result i32)
    i32.const 0))
