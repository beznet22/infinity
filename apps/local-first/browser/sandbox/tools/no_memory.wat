;; Declares its own memory instead of importing the capped one: must be refused.
(module
  (memory 1)
  (func (export "run") (result i32)
    i32.const 7))
