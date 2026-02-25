#!/bin/bash
set -euo pipefail

# Build the WASM package
wasm-pack build --target web --release

echo ""
echo "Build complete! To serve locally:"
echo "  cd www"
echo "  python3 -m http.server 8080"
echo "  # Then open http://localhost:8080"
echo ""
echo "Note: The www/app.js imports from ./rattler_build_playground.js"
echo "You may need to symlink or copy pkg/ files into www/ for local dev:"
echo "  ln -sf ../pkg/rattler_build_playground.js www/rattler_build_playground.js"
echo "  ln -sf ../pkg/rattler_build_playground_bg.wasm www/rattler_build_playground_bg.wasm"
