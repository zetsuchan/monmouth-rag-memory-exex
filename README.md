# Monmouth RAG x Memory Execution Extensions

Advanced AI-powered Execution Extensions for Reth that transform Monmouth into the premier AI-agent blockchain through real-time context retrieval (RAG) and persistent agent memory systems.

## Overview

The Monmouth RAG x Memory ExEx provides two complementary execution extensions that enable sophisticated AI agent operations on-chain:

1. **RAG ExEx**: Real-time context retrieval and vectorized knowledge base for agent decision-making
2. **Memory ExEx**: Persistent agent memory with ephemeral zones and state checkpointing

These ExExs work in tandem with the [Monmouth SVM ExEx](https://github.com/zetsuchan/monmouth-svm-exex) to create a comprehensive AI agent infrastructure.

## Architecture

### RAG ExEx Components
- **Vector Store**: ChromaDB integration for embedding storage
- **Embedding Pipeline**: Real-time transaction embeddings using sentence-transformers
- **Context Retrieval**: Semantic search and relevance scoring
- **Intent Parser**: Natural language processing for agent intents
- **Knowledge Graph**: Agent relationships and reputation tracking

### Memory ExEx Components
- **Memory Store**: Persistent storage using MDBX with LRU caching
- **Ephemeral Zones**: Session-based temporary memory spaces
- **Memory Hash**: Lattice-based hash for efficient state verification
- **Checkpointing**: State snapshots for reorg handling
- **Portability**: Export/import agent memories across chains

## Features

### Cross-ExEx Communication
- Unified AI decision engine for transaction routing
- Inter-ExEx messaging protocol
- Shared state synchronization
- Coordinated reorg handling

### Performance Targets
- RAG Query Latency: <50ms
- Memory Store/Retrieve: <10ms
- Embedding Generation: <25ms per transaction
- Context Window: 32K tokens with sliding window
- Memory Capacity: 1M+ agents with 100GB+ total memory

### Neural Agent Standard
- Standardized agent interfaces and capabilities
- Agent Neural Performance Score (aNPS) metrics
- BLS-based coordination and consensus
- Multi-modal processing support

## Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/monmouth-rag-memory-exex
cd monmouth-rag-memory-exex

# Build the project
cargo build --release

# Run tests
cargo test
```

## Configuration

Create a `config.toml` file:

```toml
[rag]
vector_store_url = "http://localhost:8000"
embedding_model = "sentence-transformers/all-MiniLM-L6-v2"
max_context_tokens = 32000

[memory]
db_path = "./memory_store"
max_memory_per_agent = 10000
checkpoint_interval = 300

[integrations]
eigenlayer_avs = "0x..."
eigenda_endpoint = "https://disperser.eigenda.xyz"
composio_api_key = "your-api-key"
```

## Usage

### As a Reth ExEx

```rust
use monmouth_rag_memory_exex::run;
use reth_exex::ExExContext;

async fn main() -> Result<()> {
    let ctx = ExExContext::new(...);
    run(ctx).await
}
```

### Standalone Components

```rust
use monmouth_rag_memory_exex::{RagExEx, MemoryExEx};

// Initialize RAG ExEx
let rag = RagExEx::new(ctx.clone(), metrics.clone()).await?;

// Initialize Memory ExEx
let memory = MemoryExEx::new(ctx.clone(), metrics.clone()).await?;

// Process transactions
rag.process_transaction(&tx).await?;
memory.store_memory(&agent_id, memory_data).await?;
```

## API Reference

### RAG Operations

```rust
// Query similar contexts
let results = rag.query_context(agent_id, query, max_results).await?;

// Generate embeddings
let embedding = rag.generate_embedding(text).await?;

// Parse intent
let intent = rag.parse_intent(transaction).await?;
```

### Memory Operations

```rust
// Store memory
memory.store(agent_id, memory_data).await?;

// Retrieve memories
let memories = memory.retrieve(agent_id, filter).await?;

// Export agent memories
let package = memory.export(agent_id, format).await?;
```

## Integrations

### EigenLayer AVS
- Register as an operator for decentralized AI tasks
- Submit and validate AI computation proofs
- Participate in agent consensus

### EigenDA
- Store large embeddings and knowledge graphs
- Ensure data availability for agent memories
- Enable cross-chain memory portability

### Composio
- Connect to external tools (Firecrawl, Perplexity, etc.)
- Enable agent interactions with Web2 services
- Extend agent capabilities dynamically

## Monitoring

Prometheus metrics are exposed on port 9090:

- `blocks_processed`: Total blocks processed
- `rag_queries`: RAG queries executed
- `memories_stored`: Agent memories stored
- `anps_scores`: Agent performance scores
- `cross_exex_communications`: Inter-ExEx messages

## Security

- Memory isolation between agents
- BLS-based authentication
- At-rest encryption for sensitive data
- GDPR-compliant memory deletion

## Contributing

Please read [CONTRIBUTING.md](CONTRIBUTING.md) for details on our code of conduct and the process for submitting pull requests.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Paradigm for Reth
- EigenLayer for AVS infrastructure
- Anthropic for AI research
- The Monmouth community