;; Demo WASM module for wasm_showcase example
;; This module demonstrates a simple counter actor that processes messages and returns results

(module
    ;; Import host functions
    (import "plexspaces" "log" (func $log (param i32 i32)))
    
    ;; Export memory for host-guest communication
    (memory (export "memory") 1)
    
    ;; Global state: counter value
    (global $counter (mut i32) (i32.const 0))
    
    ;; Initialize function
    (func (export "init") (param i32 i32) (result i32)
        ;; Return 0 (success)
        (i32.const 0)
    )
    
    ;; Handle message function
    ;; Parameters: from_ptr, from_len, msg_type_ptr, msg_type_len, payload_ptr, payload_len
    ;; Returns: result_ptr (pointer to response in memory, or 0 for no response)
    (func (export "handle_message") 
          (param $from_ptr i32) (param $from_len i32)
          (param $msg_type_ptr i32) (param $msg_type_len i32)
          (param $payload_ptr i32) (param $payload_len i32)
          (result i32)
        (local $msg_type_start i32)
        (local $response_ptr i32)
        
        ;; Store message type at offset 0 for reading
        (local.set $msg_type_start (i32.const 0))
        
        ;; Read first few bytes of message type to determine action
        ;; For simplicity, we'll check the first character
        (if (i32.eq (i32.load8_u (local.get $msg_type_ptr)) (i32.const 105))  ;; 'i' for "increment"
            (then
                ;; Increment counter
                (global.set $counter 
                    (i32.add (global.get $counter) (i32.const 1)))
                
                ;; Log the increment
                (call $log (i32.const 100) (i32.const 18))  ;; "Counter incremented"
                
                ;; Write response: counter value as 4 bytes at offset 200
                (i32.store (i32.const 200) (global.get $counter))
                
                ;; Return pointer to response
                (return (i32.const 200))
            )
        )
        
        (if (i32.eq (i32.load8_u (local.get $msg_type_ptr)) (i32.const 103))  ;; 'g' for "get_count"
            (then
                ;; Log the query
                (call $log (i32.const 120) (i32.const 15))  ;; "Getting count"
                
                ;; Write response: counter value as 4 bytes at offset 200
                (i32.store (i32.const 200) (global.get $counter))
                
                ;; Return pointer to response
                (return (i32.const 200))
            )
        )
        
        (if (i32.eq (i32.load8_u (local.get $msg_type_ptr)) (i32.const 114))  ;; 'r' for "reset"
            (then
                ;; Reset counter
                (global.set $counter (i32.const 0))
                
                ;; Log the reset
                (call $log (i32.const 140) (i32.const 13))  ;; "Counter reset"
                
                ;; Write response: 0 at offset 200
                (i32.store (i32.const 200) (i32.const 0))
                
                ;; Return pointer to response
                (return (i32.const 200))
            )
        )
        
        ;; Unknown message type - return 0 (no response)
        (i32.const 0)
    )
    
    ;; Snapshot state function
    ;; Returns: (ptr, len) tuple
    (func (export "snapshot_state") (result i32 i32)
        ;; Write counter value to memory at offset 300
        (i32.store (i32.const 300) (global.get $counter))
        
        ;; Return pointer and length (4 bytes for i32)
        (i32.const 300)
        (i32.const 4)
    )
    
    ;; Data section with log messages
    (data (i32.const 100) "Counter incremented")
    (data (i32.const 120) "Getting count")
    (data (i32.const 140) "Counter reset")
)

