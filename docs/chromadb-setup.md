# ChromaDB Setup Guide

## Overview

The Monmouth RAG Memory ExEx uses ChromaDB as its vector database for storing and searching embeddings. ChromaDB provides efficient similarity search capabilities and persistent storage for RAG (Retrieval-Augmented Generation) operations.

## Prerequisites

- Docker (recommended) or Python 3.8+
- At least 4GB of available RAM
- Port 8000 available (default ChromaDB port)

## Installation Options

### Option 1: Using Docker (Recommended)

```bash
# Pull the ChromaDB image
docker pull chromadb/chroma

# Run ChromaDB
docker run -d \
  --name chromadb \
  -p 8000:8000 \
  -v ./chroma-data:/chroma/chroma \
  chromadb/chroma
```

### Option 2: Using Docker Compose

Create a `docker-compose.yml` file:

```yaml
version: '3.8'

services:
  chromadb:
    image: chromadb/chroma
    container_name: chromadb
    ports:
      - "8000:8000"
    volumes:
      - ./chroma-data:/chroma/chroma
    environment:
      - ANONYMIZED_TELEMETRY=false
      - PERSIST_DIRECTORY=/chroma/chroma
    restart: unless-stopped
```

Then run:
```bash
docker-compose up -d
```

### Option 3: Local Python Installation

```bash
# Create a virtual environment
python -m venv chroma-env
source chroma-env/bin/activate  # On Windows: chroma-env\Scripts\activate

# Install ChromaDB
pip install chromadb

# Run ChromaDB server
chroma run --host 0.0.0.0 --port 8000 --path ./chroma-data
```

## Configuration

The RAG Memory ExEx can be configured using environment variables:

```bash
# ChromaDB server URL (default: http://localhost:8000)
export CHROMA_URL=http://localhost:8000

# Collection name for RAG embeddings (default: rag_embeddings)
export CHROMA_COLLECTION=rag_embeddings

# Embedding dimension (default: 384)
export EMBEDDING_DIMENSION=384

# Distance metric: cosine, l2, or ip (default: cosine)
export DISTANCE_METRIC=cosine

# Connection timeout in seconds (default: 30)
export CHROMA_TIMEOUT=30

# Maximum retries for failed requests (default: 3)
export CHROMA_MAX_RETRIES=3

# Size of embedding cache (default: 10000)
export EMBEDDING_CACHE_SIZE=10000
```

## Verification

To verify ChromaDB is running correctly:

```bash
# Check if ChromaDB is responding
curl http://localhost:8000/api/v1/heartbeat

# Expected response:
# {"nanosecond heartbeat": <timestamp>}
```

## ChromaDB Collections

The RAG Memory ExEx creates and manages the following collections:

1. **rag_embeddings**: Main collection for transaction and document embeddings
   - Stores transaction embeddings with metadata (tx_hash, sender, block_number)
   - Stores document embeddings with metadata (doc_id, doc_type, timestamp)

2. **agent_memory**: Agent-specific memory storage (optional)
   - Stores agent memory snapshots
   - Indexed by agent_id and timestamp

## Performance Tuning

### Memory Configuration

For optimal performance with large datasets:

```bash
# Increase Docker memory limit
docker run -d \
  --name chromadb \
  -p 8000:8000 \
  -v ./chroma-data:/chroma/chroma \
  --memory="4g" \
  --memory-swap="4g" \
  chromadb/chroma
```

### Persistence

ChromaDB data is persisted in the `./chroma-data` directory. Ensure this directory:
- Has sufficient disk space (at least 10GB recommended)
- Has proper read/write permissions
- Is backed up regularly

## Monitoring

### Check Collection Status

```bash
# List all collections
curl http://localhost:8000/api/v1/collections

# Get specific collection info
curl http://localhost:8000/api/v1/collections/rag_embeddings
```

### Monitor Performance

```bash
# Check collection count
curl http://localhost:8000/api/v1/collections/rag_embeddings/count

# Monitor Docker container
docker stats chromadb
```

## Troubleshooting

### Common Issues

1. **Connection Refused**
   - Ensure ChromaDB is running: `docker ps | grep chromadb`
   - Check port availability: `lsof -i :8000`
   - Verify firewall settings

2. **Out of Memory**
   - Increase Docker memory allocation
   - Reduce embedding dimension if possible
   - Enable swap space

3. **Slow Queries**
   - Check collection size: may need to implement pagination
   - Consider adding more RAM
   - Optimize embedding dimension

### Debug Mode

Enable debug logging:

```bash
docker run -d \
  --name chromadb \
  -p 8000:8000 \
  -v ./chroma-data:/chroma/chroma \
  -e CHROMA_LOG_CONFIG=debug \
  chromadb/chroma
```

## Integration with RAG Memory ExEx

The ExEx automatically connects to ChromaDB on startup. If ChromaDB is not available, the ExEx will:
1. Log a warning
2. Attempt to reconnect every 30 seconds
3. Queue embeddings for later storage (up to 1000 items)

## Maintenance

### Backup

```bash
# Stop ChromaDB
docker stop chromadb

# Backup data directory
tar -czf chroma-backup-$(date +%Y%m%d).tar.gz ./chroma-data

# Restart ChromaDB
docker start chromadb
```

### Reset Collection

If you need to reset the collection:

```bash
# Using curl
curl -X DELETE http://localhost:8000/api/v1/collections/rag_embeddings

# The ExEx will recreate the collection on next startup
```

## Security Considerations

1. **Network Security**
   - Run ChromaDB on a private network
   - Use firewall rules to restrict access
   - Consider TLS termination with a reverse proxy

2. **Authentication** (if needed)
   - ChromaDB supports token-based authentication
   - Configure via environment variables

3. **Data Privacy**
   - Embeddings don't contain raw data
   - Metadata should not include sensitive information

## Resources

- [ChromaDB Documentation](https://docs.trychroma.com/)
- [ChromaDB GitHub](https://github.com/chroma-core/chroma)
- [ChromaDB API Reference](https://docs.trychroma.com/reference/Client)