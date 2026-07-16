use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use facet::Facet;
use wasmtime::{Config, Engine, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

#[derive(Debug, Clone, Facet)]
pub struct MetricPayload {
    pub label: String,
    pub value: String,
    pub percentage: f32,
    pub icon: String,
}

struct HostState {
    limits: StoreLimits,
}

pub struct WasmPluginHost {
    engine: Engine,
}

impl WasmPluginHost {
    pub fn new() -> Result<Self, String> {
        let mut config = Config::new();
        // Enable instruction fuel tracking to prevent infinite loops
        config.consume_fuel(true);

        let engine =
            Engine::new(&config).map_err(|e| format!("Failed to create Wasmtime engine: {}", e))?;

        Ok(Self { engine })
    }

    /// Loads and runs a plugin WASM file, returning its current metric payload
    pub fn execute_plugin(&self, wasm_path: &Path) -> Result<MetricPayload, String> {
        if !wasm_path.exists() {
            return Err(format!("WASM plugin path does not exist: {:?}", wasm_path));
        }

        let module = Module::from_file(&self.engine, wasm_path)
            .map_err(|e| format!("Failed to compile WASM module: {}", e))?;

        // Limit linear memory allocation to max 8MB
        let limits = StoreLimitsBuilder::new()
            .memory_size(8 * 1024 * 1024)
            .build();

        let mut store = Store::new(&self.engine, HostState { limits });
        store.limiter(|state| &mut state.limits);

        // Provision fuel for execution frame (e.g. 50,000 instructions limit)
        store
            .set_fuel(50_000)
            .map_err(|e| format!("Failed to set WASM fuel: {}", e))?;

        let mut linker = Linker::new(&self.engine);

        // Expose host functions (Imports)
        linker
            .func_wrap(
                "env",
                "nacre_log",
                |mut caller: wasmtime::Caller<'_, HostState>, ptr: i32, len: i32| {
                    let mem = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return,
                    };
                    let data = mem.data(&caller);
                    if let Some(slice) = data.get(ptr as usize..(ptr + len) as usize) {
                        if let Ok(msg) = std::str::from_utf8(slice) {
                            println!("[WASM Plugin Log] {}", msg);
                        }
                    }
                },
            )
            .map_err(|e| format!("Failed to link nacre_log: {}", e))?;

        linker
            .func_wrap("env", "nacre_get_time", || -> u64 {
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            })
            .map_err(|e| format!("Failed to link nacre_get_time: {}", e))?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| format!("Failed to instantiate WASM module: {}", e))?;

        // Call the plugin update function
        // The plugin writes its serialized JSON MetricPayload into its memory and
        // returns the pointer and length. We'll read the JSON from linear
        // memory and parse it.
        let update_fn = instance
            .get_typed_func::<(), i32>(&mut store, "nacre_update")
            .map_err(|e| format!("WASM plugin is missing 'nacre_update' export: {}", e))?;

        let packed_result = update_fn
            .call(&mut store, ())
            .map_err(|e| format!("Failed to call 'nacre_update': {}", e))?;

        // Result encodes pointer (high 16 bits) and length (low 16 bits)
        let ptr = ((packed_result >> 16) & 0xffff) as usize;
        let len = (packed_result & 0xffff) as usize;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| "WASM plugin is missing 'memory' export".to_string())?;

        let data = memory.data(&store);
        let json_bytes = data
            .get(ptr..ptr + len)
            .ok_or_else(|| "Invalid pointer returned from WASM plugin update".to_string())?;

        let payload: MetricPayload = facet_json::from_slice(json_bytes)
            .map_err(|e| format!("Failed to deserialize MetricPayload JSON: {}", e))?;

        Ok(payload)
    }
}
