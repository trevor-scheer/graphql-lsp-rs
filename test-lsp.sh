#!/bin/bash

# Test script to verify the LSP server responds to initialization

echo "Testing GraphQL LSP server..."
echo ""

# Create a properly formatted LSP message
MESSAGE='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}'
LENGTH=${#MESSAGE}

# Send with proper Content-Length header
(
  echo -e "Content-Length: $LENGTH\r\n\r\n$MESSAGE"
  sleep 1
) | RUST_LOG=info /Users/trevor/Repositories/graphql-lsp/target/debug/graphql-lsp 2>&1 | head -100

echo ""
echo "Test complete!"
