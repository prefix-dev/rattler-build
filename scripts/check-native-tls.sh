#!/bin/bash
# Verifies that rustls is not included when building with native-tls feature.
# This ensures clean TLS backend separation.

set -e

echo "Checking that rustls is excluded from native-tls builds..."

# Check if rustls appears in the dependency tree with native-tls feature
output=$(cargo tree -i rustls --no-default-features --features native-tls 2>&1)

if echo "$output" | grep -q "nothing to print"; then
    echo "✓ Success: rustls is not in the dependency tree with native-tls"
    exit 0
else
    echo "✗ Error: rustls is still being pulled in with native-tls feature"
    echo ""
    echo "Dependency path:"
    echo "$output"
    exit 1
fi
