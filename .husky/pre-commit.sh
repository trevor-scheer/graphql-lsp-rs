#!/bin/sh
#
# Custom pre-commit checks for VSCode extension
#

set -e

# Check if VSCode extension files are staged
if git diff --cached --name-only | grep -q "^editors/vscode/"; then
    echo '+(cd editors/vscode && npm run format:check)'
    (cd editors/vscode && npm run format:check)
    echo '+(cd editors/vscode && npm run lint)'
    (cd editors/vscode && npm run lint)
fi
