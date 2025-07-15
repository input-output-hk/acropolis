#!/bin/bash
# Run a local swagger on our openapi.yaml
set -e

PORT=${1:-28080}
HERE=$(realpath "$(dirname "${BASHSOURCE[0]}")")

sudo docker run --name swagger --rm -p $PORT:8080 -e SWAGGER_JSON=/mount/openapi.yaml -v $HERE:/mount swaggerapi/swagger-ui
