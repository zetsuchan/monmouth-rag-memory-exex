pub mod realtime;
pub mod batch;

pub use realtime::{RealtimeEmbeddingPipeline, StreamingConfig};
pub use batch::{BatchProcessor, BatchConfig};