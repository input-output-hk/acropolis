
#!/bin/bash
for i in {1..30};
do kill $(cat /home/ubuntu/testnet-generation-tool/testnet/deployment/pid.$i.file); 
done
kill $(cat /home/ubuntu/testnet-generation-tool/testnet/recovery/pid.file); 
rm -rf /home/ubuntu/testnet-generation-tool/testnet/deployment/db* 
rm -rf /home/ubuntu/testnet-generation-tool/testnet/deployment/node*log 
rm /home/ubuntu/testnet-generation-tool/testnet/deployment/node.*.socket 
rm /home/ubuntu/testnet-generation-tool/testnet/deployment/pid.*.file 
rm /home/ubuntu/testnet-generation-tool/testnet/recovery/node*log
rm /home/ubuntu/testnet-generation-tool/testnet/recovery/node.socket 
rm /home/ubuntu/testnet-generation-tool/testnet/recovery/pid.file 
podman kube down /home/ubuntu/testnet-generation-tool/testnet/monitoring.yaml
podman volume rm testnet_promentheus_data --force
