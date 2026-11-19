;; A tool that never returns. Fuel metering must stop it.
(module
  (func (export "run") (result i32)
    (loop $again (br $again))
    i32.const 0))
