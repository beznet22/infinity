;; The host supplies memory, so every sandboxed tool is bounded by the same cap.
(module
  (import "env" "memory" (memory 1 16))
  (func (export "run") (result i32)
    i32.const 42))
