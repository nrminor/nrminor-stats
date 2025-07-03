#!/bin/bash
set -e

echo "=== GitHub Stats Performance Benchmark ==="
echo

# Check if ACCESS_TOKEN is set
if [ -z "$ACCESS_TOKEN" ]; then
    echo "Error: ACCESS_TOKEN environment variable is not set"
    exit 1
fi

# Set default GITHUB_ACTOR if not provided
if [ -z "$GITHUB_ACTOR" ]; then
    export GITHUB_ACTOR="nrminor"
fi

echo "Benchmarking for user: $GITHUB_ACTOR"
echo

# Clean cache for fair comparison
echo "Cleaning cache directories..."
rm -rf .github_stats_cache __pycache__

# Build Rust version
echo "Building Rust version..."
cargo build --release --quiet
echo

# Benchmark Python version
echo "=== Python Version ==="
START=$(date +%s.%N)
python3 generate_images.py
END=$(date +%s.%N)
PYTHON_TIME=$(echo "$END - $START" | bc)
echo "Python execution time: ${PYTHON_TIME} seconds"
echo

# Move Python outputs
mv generated/overview.svg generated/overview_python.svg
mv generated/languages.svg generated/languages_python.svg

# Benchmark Rust version
echo "=== Rust Version ==="
START=$(date +%s.%N)
./target/release/github-stats
END=$(date +%s.%N)
RUST_TIME=$(echo "$END - $START" | bc)
echo "Rust execution time: ${RUST_TIME} seconds"
echo

# Move Rust outputs
mv generated/overview.svg generated/overview_rust.svg
mv generated/languages.svg generated/languages_rust.svg

# Compare results
echo "=== Performance Comparison ==="
SPEEDUP=$(echo "scale=2; $PYTHON_TIME / $RUST_TIME" | bc)
echo "Rust is ${SPEEDUP}x faster than Python"
echo

# Check if outputs are identical
echo "=== Output Comparison ==="
if diff -q generated/overview_python.svg generated/overview_rust.svg >/dev/null; then
    echo "✓ Overview SVGs are identical"
else
    echo "✗ Overview SVGs differ"
fi

if diff -q generated/languages_python.svg generated/languages_rust.svg >/dev/null; then
    echo "✓ Languages SVGs are identical"
else
    echo "✗ Languages SVGs differ"
fi

# Restore original files
mv generated/overview_rust.svg generated/overview.svg
mv generated/languages_rust.svg generated/languages.svg
rm -f generated/overview_python.svg generated/languages_python.svg