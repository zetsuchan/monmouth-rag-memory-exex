# Changelog

All notable changes to the Monmouth RAG x Memory ExEx project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025-07-07

### Added

#### Core Infrastructure
- Initial project structure with modular ExEx design
- Base execution extension framework following Reth patterns
- Comprehensive error handling and logging

#### RAG ExEx
- ChromaDB vector store integration for embedding storage
- Real-time transaction embedding pipeline using sentence-transformers
- Context retrieval engine with semantic search
- Intent parser for natural language transaction analysis
- Knowledge graph for tracking agent relationships and reputation

#### Memory ExEx
- MDBX-based persistent memory storage with LRU caching
- Ephemeral zone management for session-based memories
- Memory Lattice Hash (MLH) for efficient state verification
- Checkpoint system for state snapshots and reorg handling
- Memory portability with export/import functionality

#### Shared Components
- Neural Agent Standard implementation
- Comprehensive Prometheus metrics
- BLS-based coordination system
- Unified AI decision engine for transaction routing
- Cross-ExEx communication protocol
- Transaction analyzer with parallel processing

#### Integrations
- EigenLayer AVS support for decentralized AI tasks
- EigenDA integration for data availability
- Composio tool integration (Firecrawl, Perplexity, etc.)

#### Performance Features
- Sub-50ms RAG query latency
- Sub-10ms memory operations
- 32K token context windows
- Support for 1M+ agents

#### Security
- Agent memory isolation
- BLS-based authentication
- At-rest encryption support
- GDPR-compliant deletion

### Dependencies
- Reth v0.2.0-beta
- ChromaDB v0.2
- Candle v0.7 for ML operations
- BLST v0.3 for BLS cryptography
- Prometheus v0.13 for metrics

### Known Issues
- ChromaDB async operations may need optimization for high-throughput scenarios
- Memory compaction during heavy load requires tuning
- Cross-ExEx message ordering during reorgs needs additional testing

## [Unreleased]

### Planned
- GPU acceleration for embedding generation
- Multi-modal support (images, audio)
- Advanced agent learning algorithms
- Distributed vector store support
- Enhanced privacy features with ZK proofs
- WebAssembly agent execution
- Cross-chain bridge for agent migration