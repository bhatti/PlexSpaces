;; Wrapper for snapshot_state to provide multi-value return
;; This wraps the Rust functions to provide proper (i32, i32) return
(module
  ;; Import the Rust functions
  (import "env" "snapshot_state_ptr" (func $ptr (result i32)))
  (import "env" "snapshot_state_len" (func $len (result i32)))

  ;; Export a function with multi-value return
  (func (export "snapshot_state") (result i32 i32)
    call $ptr
    call $len
  )
)
