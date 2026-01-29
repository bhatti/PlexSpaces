// wasmCloud Actor (Rust/WASM)
// Demonstrates WASM actors with capability providers

use wasmcloud_actor::*;
use wasmcloud_actor_http::*;
use wasmcloud_actor_keyvalue::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body: Vec<u8>,
}

#[no_mangle]
pub fn wapc_init() {
    // Register handlers
    register_function("handle_request", handle_request);
}

fn handle_request(msg: &[u8]) -> CallResult {
    let request: HttpRequest = deserialize(msg)?;
    
    // Use HTTP capability provider
    let http_client = HttpSender::new();
    let response = http_client.request(&HttpRequest {
        method: "GET".to_string(),
        path: "/api/data".to_string(),
        body: vec![],
    })?;
    
    // Use KeyValue capability provider
    let kv = KeyValueSender::new();
    kv.set("key", "value")?;
    let value = kv.get("key")?;
    
    Ok(serialize(&HttpResponse {
        status_code: 200,
        body: value.into_bytes(),
    })?)
}
