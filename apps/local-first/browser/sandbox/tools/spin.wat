(module
  (import "env" "memory" (memory 1 16))
  (func (export "run") (result i32)
    (loop $again (br $again))
    i32.const 0))
