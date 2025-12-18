#!/bin/bash
# Run a local swagger on our openapi.yaml
set -e

PORT=${1:-28080}
HERE=$(realpath "$(dirname "${BASHSOURCE[0]}")")

docker run --name swagger --rm -p "${PORT}:8080" \
  -v "$HERE/openapi.yaml:/usr/share/nginx/html/openapi.yaml" \
  -v "$HERE/index.html:/usr/share/nginx/html/index.html" \
  swaggerapi/swagger-ui