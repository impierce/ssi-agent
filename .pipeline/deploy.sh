#!/bin/bash

# Automatically fail build if script fail
set -e

cd build

[ -z "$IMAGE" ] && echo "Need to set IMAGE" && exit 1;

echo "-------------------------------------------------------------"
echo "Download kustomize"
echo "-------------------------------------------------------------"

curl -sfLo kustomize.tar.gz https://github.com/kubernetes-sigs/kustomize/releases/download/kustomize%2Fv5.3.0/kustomize_v5.3.0_linux_amd64.tar.gz
tar -xvzf kustomize.tar.gz
chmod u+x ./kustomize

echo "-------------------------------------------------------------"
echo "Apply kustomize"
echo "-------------------------------------------------------------"

# Set namespace need to match kustomize
kubectl config set-context --current --namespace=ingress-apisix

./kustomize build . | kubectl apply -f - 

kubectl rollout status deployment/$IMAGE-deployment
kubectl get services -o wide
