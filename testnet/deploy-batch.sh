#!/bin/bash
set -e
podman play kube /home/ubuntu/testnet-generation-tool/testnet/monitoring.yaml
/home/ubuntu/testnet-generation-tool/cardano-node/bin/cardano-node run --bulk-credentials-file /home/ubuntu/testnet-generation-tool/testnet/recovery/keys/bulk.creds.json --config /home/ubuntu/testnet-generation-tool/testnet/recovery/configs/config.json --topology /home/ubuntu/testnet-generation-tool/testnet/recovery/configs/topology.json --database-path /home/ubuntu/testnet-generation-tool/testnet/recovery/db --socket-path /home/ubuntu/testnet-generation-tool/testnet/recovery/node.socket --port 3001 >/dev/null 2>&1 &
echo $! >/home/ubuntu/testnet-generation-tool/testnet/recovery/pid.file
echo "Started batch node ..." 
echo "monitoring: http://localhost:3000"