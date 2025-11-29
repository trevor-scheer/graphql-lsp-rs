#!/bin/sh
#
# Custom pre-commit checks for VSCode extension
#

set -e

# Check if VSCode extension files are staged
if git diff --cached --name-only | grep -q "^editors/vscode/.*\.ts$\|^editors/vscode/package\.json$"; then
    echo '+cd editors/vscode && npm run lint'
    cd editors/vscode && npm run lint
fi
