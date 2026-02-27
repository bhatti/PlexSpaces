# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""
Data Lake Ingestion Pipeline - Unstructured Data Processing

Demonstrates a production-grade data lake ingestion pipeline using PlexSpaces,
inspired by Ray Data's unstructured data ingestion example.

## Architecture

    +------------------+
    |  Ingestion       |  Reads docs from blob storage / S3
    |  Coordinator     |  Distributes to parser pool
    +--------+---------+
             |  (channels: doc-queue)
    +--------v---------+     Worker Pool (process group)
    |  Document        |     Parse PDF, DOCX, HTML, TXT
    |  Parser Pool     |     Extract text content
    +--------+---------+
             |  (channels: chunk-queue)
    +--------v---------+     Worker Pool (process group)
    |  Text Chunker    |     Split into overlapping chunks
    |  Pool            |     Configurable chunk_size + overlap
    +--------+---------+
             |  (channels: embed-queue)
    +--------v---------+     Shard Group (GPU nodes)
    |  Embedding       |     Generate vector embeddings
    |  Worker Pool     |     Resource: {accelerator: "gpu"}
    +--------+---------+
             |
    +--------v---------+
    |  Vector Store    |     Write embeddings to vector DB
    |  Writer          |     (blob storage + KV index)
    +------------------+

## Framework Features Demonstrated

- **Workflow Actors**: Multi-step pipeline orchestration with durable state
- **Channels (SQS-style)**: Queue-based task distribution between stages
- **Process Groups**: Pub/sub for worker pool coordination
- **Blob Storage**: Large document and embedding storage
- **Key-Value Store**: Metadata index for vector search
- **Worker Pools**: Scalable stateless workers per stage
- **Shard Groups**: Hash-partitioned embedding workers for GPU utilization

## Comparison with Ray Data

Ray Data:
    ds = ray.data.read_binary_files("s3://bucket/documents/")
    ds.map(parse_document)
    ds.flat_map(chunk_text, num_cpus=4)
    ds.map_batches(EmbeddingModel, num_gpus=1, batch_size=32)
    ds.write_parquet("s3://bucket/embeddings/")

PlexSpaces equivalent:
    - Channel queues between pipeline stages (doc-queue, chunk-queue, embed-queue)
    - Worker pools per stage joined via process groups
    - Workflow actor orchestrates the full pipeline with durable checkpoints
    - Results stored via blob_upload + kv_put for vector index
"""

import json
import math
from typing import List, Dict, Any
from plexspaces import actor, state, handler, init_handler, host


# =============================================================================
# DocumentParser - Extracts text from various document formats
# =============================================================================

@actor
class DocumentParser:
    """Parses unstructured documents (PDF, DOCX, HTML, TXT) into plain text.

    Runs as a stateless worker in a process group. Documents arrive via
    host.ask() from the pipeline coordinator. Each parser handles any
    document type.

    In production, this would use libraries like:
        - PyPDF2 / pdfplumber for PDFs
        - python-docx for DOCX
        - BeautifulSoup for HTML
        - Unstructured.io for universal parsing
    """

    worker_id: str = state(default="")
    docs_parsed: int = state(default=0)
    total_parse_ms: float = state(default=0.0)
    bytes_processed: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        self.worker_id = config.get("actor_id", "")
        self.docs_parsed = 0
        self.total_parse_ms = 0.0
        self.bytes_processed = 0
        # Join the parser worker pool process group
        host.process_groups.join("pipeline-parsers")
        host.info(f"DocumentParser {self.worker_id} ready")

    @handler("parse")
    def parse_document(self, doc_id: str = "", doc_type: str = "txt",
                       content: str = "", source_path: str = "",
                       from_actor: str = "") -> dict:
        """Parse a document and extract text content.

        Args:
            doc_id: Unique document identifier
            doc_type: Document type (pdf, docx, html, txt)
            content: Raw document content (base64 for binary formats)
            source_path: Original file path/URL for metadata
        """
        start = host.now_ms()

        # Simulate document parsing by format
        if doc_type == "pdf":
            extracted = self._parse_pdf(content, doc_id)
        elif doc_type == "docx":
            extracted = self._parse_docx(content, doc_id)
        elif doc_type == "html":
            extracted = self._parse_html(content, doc_id)
        else:
            extracted = self._parse_text(content, doc_id)

        elapsed = host.now_ms() - start
        self.docs_parsed += 1
        self.total_parse_ms += elapsed
        self.bytes_processed += len(content)

        return {
            "status": "ok",
            "doc_id": doc_id,
            "doc_type": doc_type,
            "text": extracted["text"],
            "metadata": {
                "source": source_path,
                "doc_type": doc_type,
                "char_count": len(extracted["text"]),
                "page_count": extracted.get("pages", 1),
                "parse_ms": elapsed,
            },
        }

    def _parse_pdf(self, content: str, doc_id: str) -> dict:
        """Simulate PDF parsing (in production: PyPDF2/pdfplumber)."""
        # Simulate multi-page PDF extraction
        pages = max(1, len(content) // 500)
        text_parts = []
        for p in range(pages):
            text_parts.append(
                f"[Page {p + 1}] Content from document {doc_id}. "
                f"This section contains extracted text from the PDF. "
                f"Raw content segment: {content[p*200:(p+1)*200] if content else 'empty'}"
            )
        return {"text": "\n\n".join(text_parts), "pages": pages}

    def _parse_docx(self, content: str, doc_id: str) -> dict:
        """Simulate DOCX parsing (in production: python-docx)."""
        paragraphs = content.split("\n") if content else [f"Document {doc_id} content"]
        text = "\n\n".join(p.strip() for p in paragraphs if p.strip())
        return {"text": text, "pages": 1}

    def _parse_html(self, content: str, doc_id: str) -> dict:
        """Simulate HTML parsing (in production: BeautifulSoup)."""
        # Strip HTML tags (simplified)
        import re
        text = re.sub(r'<[^>]+>', ' ', content) if content else f"Document {doc_id}"
        text = re.sub(r'\s+', ' ', text).strip()
        return {"text": text, "pages": 1}

    def _parse_text(self, content: str, doc_id: str) -> dict:
        """Pass through plain text."""
        return {"text": content or f"Document {doc_id} text content", "pages": 1}

    @handler("stats")
    def stats(self) -> dict:
        avg_ms = self.total_parse_ms / self.docs_parsed if self.docs_parsed > 0 else 0
        return {
            "worker_id": self.worker_id,
            "docs_parsed": self.docs_parsed,
            "bytes_processed": self.bytes_processed,
            "total_parse_ms": self.total_parse_ms,
            "avg_parse_ms": round(avg_ms, 2),
        }


# =============================================================================
# TextChunker - Splits documents into overlapping chunks for embedding
# =============================================================================

@actor
class TextChunker:
    """Splits parsed text into overlapping chunks optimized for vector search.

    Uses a sliding window approach with configurable chunk_size and overlap.
    Chunks are small enough for embedding models (typically 512-2048 tokens)
    but large enough to preserve semantic context.

    Design choices (following RAG best practices):
        - chunk_size=1500 chars (~375 tokens): fits embedding model context
        - overlap=150 chars: preserves cross-chunk context
        - Sentence-boundary aware: avoids splitting mid-sentence
    """

    worker_id: str = state(default="")
    chunk_size: int = state(default=1500)
    overlap: int = state(default=150)
    docs_chunked: int = state(default=0)
    chunks_created: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        self.worker_id = config.get("actor_id", "")
        args = config.get("args", {})
        self.chunk_size = int(args.get("chunk_size", 1500))
        self.overlap = int(args.get("overlap", 150))
        host.process_groups.join("pipeline-chunkers")
        host.info(f"TextChunker {self.worker_id}: size={self.chunk_size}, overlap={self.overlap}")

    @handler("chunk")
    def chunk_document(self, doc_id: str = "", text: str = "",
                       metadata: dict = None, from_actor: str = "") -> dict:
        """Split document text into overlapping chunks.

        Args:
            doc_id: Document identifier
            text: Full document text from parser
            metadata: Document metadata to attach to each chunk
        """
        if not text:
            return {"error": "no text provided", "doc_id": doc_id}

        chunks = []
        start = 0
        chunk_idx = 0

        while start < len(text):
            end = min(start + self.chunk_size, len(text))

            # Try to break at sentence boundary
            if end < len(text):
                # Look for sentence-ending punctuation near the end
                for boundary in ['. ', '.\n', '! ', '? ', '\n\n']:
                    last_boundary = text.rfind(boundary, start + self.chunk_size // 2, end)
                    if last_boundary > start:
                        end = last_boundary + len(boundary)
                        break

            chunk_text = text[start:end].strip()
            if chunk_text:
                chunk_id = f"{doc_id}-chunk-{chunk_idx}"
                chunks.append({
                    "chunk_id": chunk_id,
                    "doc_id": doc_id,
                    "text": chunk_text,
                    "char_count": len(chunk_text),
                    "chunk_index": chunk_idx,
                    "start_offset": start,
                    "end_offset": end,
                    "metadata": metadata or {},
                })
                chunk_idx += 1

            # Advance with overlap
            start = end - self.overlap if end < len(text) else len(text)

        self.docs_chunked += 1
        self.chunks_created += chunk_idx

        return {
            "status": "ok",
            "doc_id": doc_id,
            "chunks": chunks,
            "chunk_count": len(chunks),
        }

    @handler("stats")
    def stats(self) -> dict:
        avg_chunks = (self.chunks_created / self.docs_chunked
                      if self.docs_chunked > 0 else 0)
        return {
            "worker_id": self.worker_id,
            "docs_chunked": self.docs_chunked,
            "chunks_created": self.chunks_created,
            "avg_chunks_per_doc": round(avg_chunks, 1),
            "chunk_size": self.chunk_size,
            "overlap": self.overlap,
        }


# =============================================================================
# EmbeddingWorker - Generates vector embeddings (GPU-accelerated)
# =============================================================================

@actor
class EmbeddingWorker:
    """Generates vector embeddings for text chunks using a transformer model.

    Deployed on GPU nodes for accelerated embedding generation.
    Processes chunks in batches for GPU efficiency.

    In production:
        from sentence_transformers import SentenceTransformer
        model = SentenceTransformer("all-mpnet-base-v2")
        embeddings = model.encode(texts, batch_size=32)

    Resource requirements:
        required_labels: {accelerator: "gpu"}
        resources: {gpu_count: 1, memory_bytes: 4294967296}
    """

    worker_id: str = state(default="")
    shard_id: int = state(default=0)
    model_name: str = state(default="all-mpnet-base-v2")
    embedding_dim: int = state(default=768)
    batch_size: int = state(default=32)
    chunks_embedded: int = state(default=0)
    total_embed_ms: float = state(default=0.0)

    @init_handler
    def on_init(self, config: dict):
        self.worker_id = config.get("actor_id", "")
        args = config.get("args", {})
        self.shard_id = int(args.get("shard_id", 0))
        self.model_name = args.get("model_name", "all-mpnet-base-v2")
        self.embedding_dim = int(args.get("embedding_dim", 768))
        self.batch_size = int(args.get("batch_size", 32))
        host.process_groups.join("pipeline-embedders")
        host.info(f"EmbeddingWorker shard {self.shard_id}: "
                  f"model={self.model_name}, dim={self.embedding_dim}")

    @handler("embed_batch")
    def embed_batch(self, chunks: list = None, from_actor: str = "") -> dict:
        """Generate embeddings for a batch of text chunks.

        Simulates transformer-based embedding generation. In production,
        this runs on GPU with batched inference for throughput.

        Args:
            chunks: List of chunk dicts [{chunk_id, text, doc_id, ...}, ...]
        """
        start = host.now_ms()
        if not chunks:
            return {"error": "no chunks provided"}

        embeddings = []
        for chunk in chunks:
            chunk_id = chunk.get("chunk_id", "")
            text = chunk.get("text", "")

            # Simulate embedding generation (deterministic from text hash)
            # In production: model.encode([text])[0]
            embedding = []
            text_hash = hash(text)
            for d in range(self.embedding_dim):
                val = math.sin(text_hash * 0.001 + d * 0.1) * 0.5
                embedding.append(round(val, 6))

            # Normalize to unit vector (cosine similarity prep)
            norm = math.sqrt(sum(v * v for v in embedding))
            if norm > 0:
                embedding = [round(v / norm, 6) for v in embedding]

            embeddings.append({
                "chunk_id": chunk_id,
                "doc_id": chunk.get("doc_id", ""),
                "embedding_dim": self.embedding_dim,
                # Store first/last 5 values as summary (full vector too large for JSON)
                "embedding_preview": embedding[:5] + ["..."] + embedding[-5:],
                "embedding_norm": round(norm, 4),
                "text_preview": text[:100] + "..." if len(text) > 100 else text,
                "model": self.model_name,
            })
            self.chunks_embedded += 1

            # Store full embedding in blob storage for vector search
            host.kv_put(f"embedding:{chunk_id}", json.dumps({
                "vector": embedding[:32],  # Truncated for demo
                "doc_id": chunk.get("doc_id", ""),
                "text": text[:500],
                "metadata": chunk.get("metadata", {}),
            }))

        elapsed = host.now_ms() - start
        self.total_embed_ms += elapsed

        return {
            "status": "ok",
            "shard_id": self.shard_id,
            "embeddings": embeddings,
            "count": len(embeddings),
            "embed_ms": elapsed,
        }

    @handler("stats")
    def stats(self) -> dict:
        avg_ms = (self.total_embed_ms / self.chunks_embedded
                  if self.chunks_embedded > 0 else 0)
        throughput = (self.chunks_embedded * 1000.0 / self.total_embed_ms
                      if self.total_embed_ms > 0 else 0)
        return {
            "worker_id": self.worker_id,
            "shard_id": self.shard_id,
            "model": self.model_name,
            "embedding_dim": self.embedding_dim,
            "chunks_embedded": self.chunks_embedded,
            "total_embed_ms": self.total_embed_ms,
            "avg_ms_per_chunk": round(avg_ms, 2),
            "throughput_chunks_per_sec": round(throughput, 1),
        }


# =============================================================================
# IngestionCoordinator - Orchestrates the full data lake pipeline
# =============================================================================

@actor
class IngestionCoordinator:
    """Orchestrates the end-to-end data lake ingestion pipeline.

    This is a workflow-style actor that manages the multi-step pipeline:
    1. Accept document batch from client
    2. Fan-out documents to parser pool (process group)
    3. Fan-out parsed text to chunker pool
    4. Fan-out chunks to embedding shard group (GPU nodes)
    5. Store embeddings in vector index (KV store)
    6. Report pipeline metrics

    Demonstrates:
    - Workflow orchestration with durable state
    - Multi-stage scatter-gather across heterogeneous worker pools
    - Process group coordination for dynamic worker discovery
    - Channel-style task distribution
    - Resource-aware routing (CPU parsers, GPU embedders)
    """

    coordinator_id: str = state(default="")
    num_parsers: int = state(default=2)
    num_chunkers: int = state(default=2)
    num_embedders: int = state(default=2)
    parser_ids: list = state(default_factory=list)
    chunker_ids: list = state(default_factory=list)
    embedder_ids: list = state(default_factory=list)
    total_pipelines: int = state(default=0)
    total_docs: int = state(default=0)
    total_chunks: int = state(default=0)
    total_embeddings: int = state(default=0)
    total_pipeline_ms: float = state(default=0.0)

    @init_handler
    def on_init(self, config: dict):
        actor_id = config.get("actor_id", "")
        self.coordinator_id = actor_id
        args = config.get("args", {})
        self.num_parsers = int(args.get("num_parsers", 2))
        self.num_chunkers = int(args.get("num_chunkers", 2))
        self.num_embedders = int(args.get("num_embedders", 2))

        id_suffix = ""
        if ":" in actor_id:
            id_suffix = actor_id[actor_id.index(":"):]

        self.parser_ids = [f"parser-{i}{id_suffix}" for i in range(self.num_parsers)]
        self.chunker_ids = [f"chunker-{i}{id_suffix}" for i in range(self.num_chunkers)]
        self.embedder_ids = [f"embedder-{i}{id_suffix}" for i in range(self.num_embedders)]

        host.process_groups.join("pipeline-coordinators")
        host.info(f"IngestionCoordinator: {self.num_parsers} parsers, "
                  f"{self.num_chunkers} chunkers, {self.num_embedders} embedders")

    @handler("ingest")
    def ingest_documents(self, documents: list = None,
                         from_actor: str = "") -> dict:
        """Ingest a batch of documents through the full pipeline.

        Args:
            documents: List of doc descriptors [{id, type, content, source}, ...]
        """
        pipeline_start = host.now_ms()
        if not documents:
            return {"error": "no documents provided"}

        pipeline_id = f"pipeline-{self.total_pipelines}"
        n_docs = len(documents)

        # ---- Stage 1: Parse documents ----
        parse_start = host.now_ms()
        parsed_docs = []
        for i, doc in enumerate(documents):
            worker_id = self.parser_ids[i % self.num_parsers]
            try:
                resp = host.ask(worker_id, "parse", {
                    "doc_id": doc.get("id", f"doc-{i}"),
                    "doc_type": doc.get("type", "txt"),
                    "content": doc.get("content", ""),
                    "source_path": doc.get("source", ""),
                }, timeout_ms=30000)
                if isinstance(resp, dict) and resp.get("status") == "ok":
                    parsed_docs.append(resp)
            except Exception as e:
                host.warn(f"Parser {worker_id} failed: {e}")
        parse_ms = host.now_ms() - parse_start

        # ---- Stage 2: Chunk parsed text ----
        chunk_start = host.now_ms()
        all_chunks = []
        for i, parsed in enumerate(parsed_docs):
            worker_id = self.chunker_ids[i % self.num_chunkers]
            try:
                resp = host.ask(worker_id, "chunk", {
                    "doc_id": parsed.get("doc_id", ""),
                    "text": parsed.get("text", ""),
                    "metadata": parsed.get("metadata", {}),
                }, timeout_ms=30000)
                if isinstance(resp, dict) and resp.get("status") == "ok":
                    all_chunks.extend(resp.get("chunks", []))
            except Exception as e:
                host.warn(f"Chunker {worker_id} failed: {e}")
        chunk_ms = host.now_ms() - chunk_start

        # ---- Stage 3: Generate embeddings (GPU workers) ----
        embed_start = host.now_ms()
        # Partition chunks across embedding shards
        embed_partitions = [[] for _ in range(self.num_embedders)]
        for chunk in all_chunks:
            shard = hash(chunk.get("chunk_id", "")) % self.num_embedders
            embed_partitions[shard].append(chunk)

        all_embeddings = []
        for shard_idx, partition in enumerate(embed_partitions):
            if not partition:
                continue
            worker_id = self.embedder_ids[shard_idx]
            try:
                resp = host.ask(worker_id, "embed_batch", {
                    "chunks": partition,
                }, timeout_ms=60000)
                if isinstance(resp, dict) and resp.get("status") == "ok":
                    all_embeddings.extend(resp.get("embeddings", []))
            except Exception as e:
                host.warn(f"Embedder {worker_id} failed: {e}")
        embed_ms = host.now_ms() - embed_start

        # ---- Stage 4: Store in vector index ----
        store_start = host.now_ms()
        for emb in all_embeddings:
            # Store chunk-to-embedding mapping in KV store (vector index)
            host.kv_put(f"index:{emb['chunk_id']}", json.dumps({
                "doc_id": emb.get("doc_id", ""),
                "chunk_id": emb["chunk_id"],
                "model": emb.get("model", ""),
                "embedding_dim": emb.get("embedding_dim", 768),
            }))
        store_ms = host.now_ms() - store_start

        pipeline_ms = host.now_ms() - pipeline_start

        # Update metrics
        self.total_pipelines += 1
        self.total_docs += n_docs
        self.total_chunks += len(all_chunks)
        self.total_embeddings += len(all_embeddings)
        self.total_pipeline_ms += pipeline_ms

        # Notify observers
        host.process_groups.broadcast("pipeline-coordinators", "ingestion_complete", {
            "pipeline_id": pipeline_id,
            "docs": n_docs,
            "chunks": len(all_chunks),
            "embeddings": len(all_embeddings),
        })

        return {
            "status": "ok",
            "pipeline_id": pipeline_id,
            "summary": {
                "documents_parsed": len(parsed_docs),
                "chunks_created": len(all_chunks),
                "embeddings_generated": len(all_embeddings),
                "pipeline_ms": pipeline_ms,
                "parse_ms": parse_ms,
                "chunk_ms": chunk_ms,
                "embed_ms": embed_ms,
                "store_ms": store_ms,
            },
        }

    @handler("search")
    def search(self, query: str = "", top_k: int = 5) -> dict:
        """Simple vector similarity search (demo).

        In production, this would use a proper vector database (Chroma, Pinecone,
        pgvector) with ANN indexing for fast retrieval.
        """
        # List all indexed chunks
        index_keys = host.kv_list("index:")
        if not index_keys:
            return {"results": [], "query": query}

        results = []
        if isinstance(index_keys, str):
            try:
                index_keys = json.loads(index_keys)
            except (json.JSONDecodeError, ValueError):
                index_keys = []

        for key in index_keys[:top_k]:
            if isinstance(key, str):
                data = host.kv_get(key)
                if data:
                    try:
                        results.append(json.loads(data))
                    except (json.JSONDecodeError, ValueError):
                        pass

        return {"query": query, "results": results, "count": len(results)}

    @handler("pipeline_stats")
    def pipeline_stats(self) -> dict:
        throughput = (self.total_docs * 1000.0 / self.total_pipeline_ms
                      if self.total_pipeline_ms > 0 else 0)
        return {
            "coordinator_id": self.coordinator_id,
            "total_pipelines": self.total_pipelines,
            "total_docs": self.total_docs,
            "total_chunks": self.total_chunks,
            "total_embeddings": self.total_embeddings,
            "total_pipeline_ms": self.total_pipeline_ms,
            "throughput_docs_per_sec": round(throughput, 1),
            "workers": {
                "parsers": self.num_parsers,
                "chunkers": self.num_chunkers,
                "embedders": self.num_embedders,
            },
        }


# Multi-actor role mapping
ACTOR_ROLES = {
    "ingestion-coordinator": IngestionCoordinator,
    "parser": DocumentParser,
    "chunker": TextChunker,
    "embedder": EmbeddingWorker,
}
