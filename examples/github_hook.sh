#!/usr/bin/env bash
echo "github: $(date +%s)"    >> output.log
echo "  repo: $GITHUB_REPO"   >> output.log
echo "commit: $GITHUB_COMMIT" >> output.log
echo "   ref: $GITHUB_REF"    >> output.log
echo "========================================"