;; Minimal traditional WASM module for testing
;; This is NOT a component - it's a traditional WASM module that works with the runtime
(module
    (memory (export "memory") 1)
    (func (export "init") (param i32 i32) (result i32)
        (i32.const 0)
    )
    (func (export "handle_message") 
          (param $from_ptr i32) (param $from_len i32)
          (param $msg_type_ptr i32) (param $msg_type_len i32)
          (param $payload_ptr i32) (param $payload_len i32)
          (result i32)
        (i32.const 0)
    )
    (func (export "snapshot_state") (result i32 i32)
        (i32.const 0)
        (i32.const 0)
    )
)

