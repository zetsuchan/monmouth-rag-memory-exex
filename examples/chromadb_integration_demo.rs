use monmouth_rag_memory_exex::rag_exex::{
    chroma_store::ChromaStore,
    vector_store::VectorStore,
};
use alloy::primitives::{Address, B256};
use eyre::Result;
use std::collections::HashMap;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== ChromaDB Integration Demo ===\n");

    // Initialize ChromaDB connection
    println!("1. Connecting to ChromaDB...");
    let start = Instant::now();
    
    // Create vector store (uses ChromaDB backend)
    let vector_store = match VectorStore::new().await {
        Ok(store) => {
            println!("✓ Connected to ChromaDB in {:?}", start.elapsed());
            store
        }
        Err(e) => {
            println!("✗ Failed to connect to ChromaDB: {}", e);
            println!("\nMake sure ChromaDB is running:");
            println!("  docker run -p 8000:8000 chromadb/chroma");
            return Err(e);
        }
    };

    // Example 1: Store transaction embeddings
    println!("\n2. Storing transaction embeddings...");
    let tx_embeddings = vec![
        (B256::random(), generate_random_embedding(384)),
        (B256::random(), generate_random_embedding(384)),
        (B256::random(), generate_random_embedding(384)),
    ];

    for (tx_hash, embedding) in &tx_embeddings {
        let start = Instant::now();
        vector_store.store_embedding(&tx_hash.to_string(), embedding.clone()).await?;
        println!("  ✓ Stored embedding for tx {} in {:?}", 
            &tx_hash.to_string()[..8], start.elapsed());
    }

    // Example 2: Search for similar embeddings
    println!("\n3. Searching for similar embeddings...");
    let query_embedding = generate_random_embedding(384);
    
    let start = Instant::now();
    let results = vector_store.search(query_embedding.clone(), 5).await?;
    println!("  ✓ Search completed in {:?}", start.elapsed());
    
    println!("\n  Top {} results:", results.len());
    for (i, (id, score)) in results.iter().enumerate() {
        println!("    {}. {} (similarity: {:.3})", i + 1, &id[..8], score);
    }

    // Example 3: Using ChromaStore directly for advanced features
    println!("\n4. Advanced ChromaDB features...");
    let chroma = ChromaStore::new(None, "advanced_demo".to_string()).await?;

    // Store document with metadata
    let doc_id = B256::random();
    let doc_embedding = generate_document_embedding();
    
    chroma.store_document_embedding(
        &doc_id,
        doc_embedding.clone(),
        "protocol_documentation",
        Some("This is a sample protocol documentation for Uniswap V3"),
    ).await?;
    println!("  ✓ Stored document with metadata");

    // Search with filters
    let mut filter = HashMap::new();
    filter.insert("doc_type".to_string(), serde_json::json!("protocol_documentation"));
    
    let filtered_results = chroma.search(doc_embedding, 10, Some(filter)).await?;
    println!("  ✓ Found {} documents matching filter", filtered_results.len());

    // Example 4: Batch operations
    println!("\n5. Batch operations...");
    let batch_size = 100;
    let mut batch_embeddings = Vec::new();
    
    for i in 0..batch_size {
        batch_embeddings.push((
            format!("batch_item_{}", i),
            generate_random_embedding(384),
        ));
    }

    let start = Instant::now();
    for (id, embedding) in batch_embeddings {
        vector_store.store_embedding(&id, embedding).await?;
    }
    let elapsed = start.elapsed();
    
    println!("  ✓ Stored {} embeddings in {:?}", batch_size, elapsed);
    println!("  Average: {:?} per embedding", elapsed / batch_size);

    // Example 5: Memory usage for agents
    println!("\n6. Agent memory storage...");
    let agent_address = Address::random();
    let agent_memory_embedding = generate_agent_memory_embedding();
    
    // Create agent memory in ChromaDB
    let agent_chroma = ChromaStore::new(None, "agent_memory".to_string()).await?;
    
    let mut metadata = HashMap::new();
    metadata.insert("agent_id".to_string(), serde_json::json!(agent_address.to_string()));
    metadata.insert("memory_type".to_string(), serde_json::json!("action_history"));
    metadata.insert("timestamp".to_string(), serde_json::json!(chrono::Utc::now().to_rfc3339()));
    
    agent_chroma.store_embedding(
        &format!("agent_memory_{}", B256::random()),
        agent_memory_embedding,
        metadata,
    ).await?;
    
    println!("  ✓ Stored agent memory");

    // Search agent memories
    let agent_filter = HashMap::from([
        ("agent_id".to_string(), serde_json::json!(agent_address.to_string())),
    ]);
    
    let agent_memories = agent_chroma.search(
        generate_random_embedding(384),
        10,
        Some(agent_filter),
    ).await?;
    
    println!("  ✓ Found {} memories for agent", agent_memories.len());

    // Example 6: Performance metrics
    println!("\n7. Performance metrics...");
    let count = vector_store.count().await?;
    println!("  Total embeddings in main collection: {}", count);
    
    // Benchmark search performance
    let iterations = 10;
    let mut search_times = Vec::new();
    
    for _ in 0..iterations {
        let query = generate_random_embedding(384);
        let start = Instant::now();
        let _ = vector_store.search(query, 10).await?;
        search_times.push(start.elapsed());
    }
    
    let avg_search_time = search_times.iter().sum::<std::time::Duration>() / iterations;
    println!("  Average search time ({} iterations): {:?}", iterations, avg_search_time);
    
    // Cleanup (optional)
    println!("\n8. Cleanup...");
    println!("  Collections created:");
    println!("    - rag_embeddings (main)");
    println!("    - advanced_demo");
    println!("    - agent_memory");
    println!("\n  To reset: docker restart chromadb");

    println!("\n✓ Demo completed successfully!");
    
    Ok(())
}

/// Generate random embedding for testing
fn generate_random_embedding(dim: usize) -> Vec<f32> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    
    let mut embedding: Vec<f32> = (0..dim)
        .map(|_| rng.gen_range(-1.0..1.0))
        .collect();
    
    // Normalize
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut embedding {
            *x /= norm;
        }
    }
    
    embedding
}

/// Generate document embedding with some structure
fn generate_document_embedding() -> Vec<f32> {
    let mut embedding = generate_random_embedding(384);
    // Add some structure to make it distinctive
    for i in 0..10 {
        embedding[i] = 0.9;
    }
    embedding
}

/// Generate agent memory embedding
fn generate_agent_memory_embedding() -> Vec<f32> {
    let mut embedding = generate_random_embedding(384);
    // Add pattern for agent memories
    for i in (0..384).step_by(10) {
        embedding[i] = 0.5;
    }
    embedding
}