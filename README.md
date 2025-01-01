# MuopDB - A vector database for machine learning

## Introduction

MuopDB is a vector database for machine learning. Currently, it supports:
* Index type: HNSW, IVF, SPANN. All on-disk with mmap.
* Quantization: product quantization

Here are the plans for future MuopDB:
### V0 (Done)
- [x] Query path
  - [x] Vector similarity search
  - [x] Hierarchical Navigable Small Worlds (HNSW)
  - [x] Product Quantization (PQ)
- [x] Indexing path
  - [x] Support periodic offline indexing
- [x] Database Management
  - [x] Doc-sharding & query fan-out with aggregator-leaf architecture
  - [x] In-memory & disk-based storage with mmap
### V1 (Done)
- [x] Query & Indexing
  - [x] Inverted File (IVF)
  - [x] Improve locality for HNSW
  - [x] SPANN
### V2
- [ ] Query
  - [x] Multiple index segments
  - [ ] Support realtime indexing
  - [ ] Elias-Fano encoding for IVF
- [ ] Quantization
  - [ ] RabitQ

## Why MuopDB?
This is an educational project for me to learn Rust & vector database.

## Building

Install prerequisites:
* Rust: https://www.rust-lang.org/tools/install
* Others
```
# macos
brew install hdf5 protobuf

export HDF5_DIR="$(brew --prefix hdf5)"
```

Build:
```
# from top-level workspace
cargo build --release
```

Test:
```
cargo test --release
```

## Contributions
This project is done with [TechCare Coaching](https://techcarecoaching.com/). I am mentoring mentees who made contributions to this project.
