# EigenDA Integration

## Overview

EigenDA is a data availability layer built on EigenLayer that provides high-throughput, low-cost data storage for the Monmouth RAG x Memory ExEx. It enables storing large AI artifacts like embeddings, memory snapshots, and knowledge graphs with cryptographic guarantees of availability.

## Architecture

### Core Components

1. **Blob Management**
   - Data chunking and erasure coding
   - KZG commitment generation
   - Dispersal tracking

2. **Compression Pipeline**
   - Multiple compression algorithms (Zstd, Lz4, Snappy)
   - Automatic format selection based on data type
   - Compression ratio optimization

3. **Availability Verification**
   - Operator polling
   - Proof verification
   - Reconstruction capability

## API Reference

### Initialization

```rust
use monmouth_rag_memory_exex::integrations::EigenDAIntegration;

let eigenda = EigenDAIntegration::new(
    disperser_endpoint,   // Disperser service URL
    retriever_endpoint    // Retriever service URL
);

// With custom encoding configuration
let eigenda = EigenDAIntegration::new(disperser_endpoint, retriever_endpoint)
    .with_encoding_config(EncodingConfig {
        chunk_size: 512 * 1024,    // 512KB chunks
        coding_ratio: 2.0,         // 100% redundancy
        security_threshold: 0.5,   // 50% honest assumption
    });
```

### Storing Data

```rust
use monmouth_rag_memory_exex::integrations::eigenda::{
    BlobData, BlobMetadata, DataType, CompressionType
};

// Store embeddings
let blob = BlobData {
    data: embedding_vector,
    metadata: BlobMetadata {
        agent_id: "agent_001".to_string(),
        data_type: DataType::Embedding {
            model: "sentence-transformers/all-MiniLM-L6-v2".to_string(),
            dimensions: 384,
        },
        timestamp: current_timestamp(),
        expiry: Some(timestamp + 30 * 86400), // 30 days
        compression: CompressionType::Zstd,
        encryption: None,
    },
};

let reference = eigenda.store_blob(blob).await?;
println!("Blob ID: {}", reference.blob_id);
println!("Commitment: {:?}", reference.commitment);
```

### Data Types

#### Embeddings
```rust
DataType::Embedding {
    model: String,      // Model used for generation
    dimensions: usize,  // Vector dimensions
}
```

#### Memory Snapshots
```rust
DataType::Memory {
    memory_type: String,  // ShortTerm, LongTerm, etc.
    size_bytes: usize,    // Uncompressed size
}
```

#### Checkpoints
```rust
DataType::Checkpoint {
    block_height: u64,    // Block number
    state_root: [u8; 32], // State root hash
}
```

#### Knowledge Graphs
```rust
DataType::KnowledgeGraph {
    num_nodes: usize,  // Total nodes
    num_edges: usize,  // Total edges
}
```

### Batch Operations

```rust
use monmouth_rag_memory_exex::integrations::eigenda::{
    BatchSubmission, SubmissionPriority
};

let batch = BatchSubmission {
    blobs: vec![blob1, blob2, blob3],
    priority: SubmissionPriority::High,
};

let references = eigenda.store_batch(batch).await?;
```

### Retrieving Data

```rust
// Retrieve blob
if let Some(blob_data) = eigenda.retrieve_blob(&reference).await? {
    println!("Retrieved {} bytes", blob_data.data.len());
    println!("Agent: {}", blob_data.metadata.agent_id);
}

// Verify availability
let is_available = eigenda.verify_availability(&reference).await?;
println!("Data available: {}", is_available);
```

### Cost Estimation

```rust
// Estimate storage cost
let cost = eigenda.estimate_storage_cost(
    data_size,      // Size in bytes
    duration_blocks // Storage duration
).await?;

println!("Estimated cost: {} gwei", cost);
```

## Compression Strategies

### Algorithm Selection

- **None**: For already compressed data or when latency is critical
- **Zstd**: Best compression ratio, ideal for long-term storage
- **Lz4**: Fast compression/decompression, good for frequently accessed data
- **Snappy**: Balance between speed and ratio, default for most cases

### Automatic Compression

The integration automatically selects compression based on:
1. Data type (embeddings vs. raw memory)
2. Size threshold (>1KB recommended)
3. Access patterns (hot vs. cold storage)

## Security Features

### Data Integrity

- KZG commitments ensure data hasn't been tampered with
- Merkle proofs for efficient subset verification
- Cryptographic binding between blob ID and content

### Encryption Support

```rust
use monmouth_rag_memory_exex::integrations::eigenda::EncryptionInfo;

metadata: BlobMetadata {
    // ... other fields
    encryption: Some(EncryptionInfo {
        algorithm: "AES-256-GCM".to_string(),
        key_id: "agent_key_001".to_string(),
    }),
}
```

## Performance Optimization

### Blob Size Guidelines

- Minimum: 1KB (smaller data should be batched)
- Optimal: 256KB - 1MB (best throughput/cost ratio)
- Maximum: 16MB (hard limit per blob)

### Batching Strategy

```rust
// Efficient batching for small items
let mut batch_blobs = Vec::new();
let mut current_size = 0;

for item in items {
    if current_size + item.len() > 256 * 1024 {
        // Submit current batch
        eigenda.store_batch(BatchSubmission {
            blobs: batch_blobs.clone(),
            priority: SubmissionPriority::Normal,
        }).await?;
        
        batch_blobs.clear();
        current_size = 0;
    }
    
    batch_blobs.push(create_blob(item));
    current_size += item.len();
}
```

## Integration with ExEx

### RAG ExEx Integration

```rust
// Store embeddings from RAG pipeline
let embeddings = rag_exex.generate_embeddings(text).await?;
let blob_ref = eigenda.store_blob(BlobData {
    data: embeddings.to_bytes(),
    metadata: create_embedding_metadata(),
}).await?;

// Update index with blob reference
rag_exex.update_index(doc_id, blob_ref).await?;
```

### Memory ExEx Integration

```rust
// Store memory checkpoint
let checkpoint = memory_exex.create_checkpoint().await?;
let blob_ref = eigenda.store_blob(BlobData {
    data: checkpoint.serialize(),
    metadata: create_checkpoint_metadata(checkpoint.block_height),
}).await?;
```

## Monitoring

Key metrics:
- `eigenda_blobs_stored`: Total blobs stored
- `eigenda_bytes_stored`: Total bytes stored
- `eigenda_retrieval_latency`: Blob retrieval time
- `eigenda_availability_rate`: Percentage of available blobs
- `eigenda_compression_ratio`: Average compression achieved

## Production Configuration

```toml
[eigenda]
disperser_endpoint = "https://disperser.eigenda.xyz"
retriever_endpoint = "https://retriever.eigenda.xyz"
max_blob_size_mb = 16
default_compression = "zstd"
availability_check_interval_s = 300
cache_size_mb = 1024
```

## Error Handling

```rust
match eigenda.store_blob(blob).await {
    Ok(reference) => {
        // Success
    }
    Err(e) if e.to_string().contains("size exceeds") => {
        // Split into smaller blobs
    }
    Err(e) if e.to_string().contains("disperser unavailable") => {
        // Retry with exponential backoff
    }
    Err(e) => {
        // Log and handle other errors
    }
}
```

## Future Enhancements

- [ ] Streaming blob upload/download
- [ ] Automatic replication across regions
- [ ] Smart compression selection using ML
- [ ] Hybrid storage with local caching
- [ ] Cross-shard blob aggregation