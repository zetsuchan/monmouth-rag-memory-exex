//! Example: Demonstrating all integrations working together
//! 
//! This example shows how EigenLayer, EigenDA, Othentic, and Lagrange work
//! together to create a verifiable AI agent infrastructure.

use monmouth_rag_memory_exex::integrations::{
    EigenLayerIntegration,
    EigenDAIntegration,
    OthenticIntegration,
    LagrangeIntegration,
    eigenlayer::{AVSTask, TaskType, MemoryOpType},
    eigenda::{BlobData, BlobMetadata, DataType, CompressionType},
    othentic::{ModelInfo, ModelFramework, ResourceRequirements, VerificationMethod, 
                InferenceTask, TaskPriority, InferenceConstraints},
    lagrange::{ProofRequest, ComputationType, MemoryOp, ProofPriority},
};
use std::sync::Arc;
use tokio;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    println!("🚀 Monmouth Integrations Demo\n");
    
    // Initialize all integrations
    let eigenlayer = Arc::new(EigenLayerIntegration::new(
        "0x1234...".to_string(),
        "0xABCD...".to_string(),
        "0x5678...".to_string(),
    ));
    
    let eigenda = Arc::new(EigenDAIntegration::new(
        "https://disperser.eigenda.xyz".to_string(),
        "https://retriever.eigenda.xyz".to_string(),
    ));
    
    let othentic = Arc::new(OthenticIntegration::new(
        "https://avs.othentic.xyz".to_string(),
        "0xDEF0...".to_string(),
        "0x9876...".to_string(),
    ));
    
    let lagrange = Arc::new(LagrangeIntegration::new(
        "https://prover.lagrange.dev".to_string(),
        "https://aggregator.lagrange.dev".to_string(),
        "0xFEDC...".to_string(),
    ));
    
    // Step 1: Register operator on EigenLayer
    println!("1️⃣ Registering as EigenLayer operator...");
    eigenlayer.register_operator(100_000_000_000_000_000_000, None).await?;
    println!("   ✅ Registered with 100 ETH stake\n");
    
    // Step 2: Register AI model on Othentic
    println!("2️⃣ Registering AI model on Othentic...");
    let model_info = ModelInfo {
        model_id: "rag_embedder_v1".to_string(),
        model_hash: [1u8; 32],
        version: "1.0.0".to_string(),
        framework: ModelFramework::ONNX,
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "maxLength": 512 }
            }
        }),
        output_schema: serde_json::json!({
            "type": "array",
            "items": { "type": "number" },
            "minItems": 384,
            "maxItems": 384
        }),
        resource_requirements: ResourceRequirements {
            min_memory_mb: 2048,
            min_compute_units: 1000,
            gpu_required: false,
            estimated_inference_ms: 50,
        },
        verification_method: VerificationMethod::ConsensusBasedThreshold { threshold: 3 },
    };
    
    othentic.register_model(model_info).await?;
    println!("   ✅ Model registered: rag_embedder_v1\n");
    
    // Step 3: Submit RAG query task to EigenLayer
    println!("3️⃣ Submitting RAG query to EigenLayer AVS...");
    let rag_task = AVSTask {
        task_id: String::new(),
        agent_id: "agent_001".to_string(),
        task_type: TaskType::RAGQuery {
            query: "What are the latest DeFi yield strategies?".to_string(),
            max_tokens: 1000,
        },
        input_data: b"query_embedding_data".to_vec(),
        timestamp: chrono::Utc::now().timestamp() as u64,
        required_stake: 10_000_000_000_000_000_000, // 10 ETH
        quorum_threshold: 67,
    };
    
    let task_id = eigenlayer.submit_task(rag_task).await?;
    println!("   ✅ Task submitted: {}\n", task_id);
    
    // Step 4: Submit embedding generation to Othentic
    println!("4️⃣ Generating embeddings via Othentic...");
    let inference_task = InferenceTask {
        task_id: String::new(),
        model_id: "rag_embedder_v1".to_string(),
        input_data: serde_json::json!({
            "text": "DeFi yield farming strategies involving stablecoins"
        }),
        requester: "agent_001".to_string(),
        priority: TaskPriority::High,
        constraints: InferenceConstraints {
            max_latency_ms: Some(100),
            required_operators: None,
            consensus_threshold: Some(3),
            deterministic_seed: Some(42),
        },
        timestamp: chrono::Utc::now().timestamp() as u64,
    };
    
    let inference_id = othentic.submit_inference(inference_task).await?;
    println!("   ✅ Inference submitted: {}\n", inference_id);
    
    // Step 5: Store embeddings in EigenDA
    println!("5️⃣ Storing embeddings in EigenDA...");
    let embedding_blob = BlobData {
        data: vec![0u8; 384 * 4], // 384 float32 values
        metadata: BlobMetadata {
            agent_id: "agent_001".to_string(),
            data_type: DataType::Embedding {
                model: "rag_embedder_v1".to_string(),
                dimensions: 384,
            },
            timestamp: chrono::Utc::now().timestamp() as u64,
            expiry: Some(chrono::Utc::now().timestamp() as u64 + 86400 * 30), // 30 days
            compression: CompressionType::Zstd,
            encryption: None,
        },
    };
    
    let blob_ref = eigenda.store_blob(embedding_blob).await?;
    println!("   ✅ Blob stored: {}\n", blob_ref.blob_id);
    
    // Step 6: Generate ZK proof for memory operation
    println!("6️⃣ Generating ZK proof for memory update...");
    let memory_root_before = [2u8; 32];
    let memory_root_after = [3u8; 32];
    
    let proof_id = lagrange.prove_memory_operation(
        MemoryOp::Store {
            key: b"agent_001_context".to_vec(),
            value: blob_ref.blob_id.as_bytes().to_vec(),
        },
        memory_root_before,
        memory_root_after,
    ).await?;
    
    println!("   ✅ Proof request submitted: {}\n", proof_id);
    
    // Step 7: Retrieve results
    println!("7️⃣ Retrieving results...\n");
    
    // Get EigenLayer task response
    if let Some(response) = eigenlayer.get_task_response(&task_id).await? {
        println!("   📊 EigenLayer Response:");
        println!("      Result: {} bytes", response.result.len());
        println!("      Operator stake: {} ETH", response.operator_stake as f64 / 1e18);
    }
    
    // Get Othentic inference result
    if let Some(result) = othentic.get_inference_result(&inference_id).await? {
        println!("\n   🤖 Othentic Inference:");
        println!("      Output: {:?}", result.output_data);
        println!("      Inference time: {}ms", result.performance_metrics.inference_time_ms);
        println!("      Signatures: {}", result.operator_signatures.len());
    }
    
    // Get Lagrange proof
    if let Some(proof) = lagrange.get_proof_result(&proof_id).await? {
        println!("\n   🔐 Lagrange Proof:");
        println!("      Proof size: {} bytes", proof.proof.proof_data.len());
        println!("      Proving time: {}ms", proof.proving_time_ms);
        println!("      System: {:?}", proof.proof.proof_system);
    }
    
    // Verify data availability
    let available = eigenda.verify_availability(&blob_ref).await?;
    println!("\n   💾 EigenDA Availability: {}", if available { "✅" } else { "❌" });
    
    println!("\n✨ Integration demo completed successfully!");
    println!("\nKey Integration Points:");
    println!("  • EigenLayer: Decentralized task coordination");
    println!("  • Othentic: Verifiable AI inference");
    println!("  • EigenDA: Data availability for embeddings");
    println!("  • Lagrange: ZK proofs for state transitions");
    
    Ok(())
}