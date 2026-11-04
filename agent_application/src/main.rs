// glibc malloc keeps memory freed after large, short-lived allocations (such as replaying an
// aggregate's events) in its per-thread arenas instead of returning it to the system, so the
// resident memory of the agent only ever grows towards its peak. jemalloc returns unused memory.
#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// jemalloc options read at startup: freed memory is returned to the system by a background thread
/// after one second. Overridden by the `_RJEM_MALLOC_CONF` environment variable.
#[cfg(not(target_env = "msvc"))]
#[allow(non_upper_case_globals)]
#[export_name = "_rjem_malloc_conf"]
pub static malloc_conf: &[u8; 60] = b"background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:0\0";

#[tokio::main]
async fn main() -> tokio::io::Result<()> {
    agent_application::run().await
}
