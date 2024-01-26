#!/bin/bash

set -e

[ -z "$IMAGE" ] && echo "Need to set IMAGE" && exit 1;
[ -z "$ARTIFACT_REGISTRY_HOST" ] && echo "Need to set ARTIFACT_REGISTRY_HOST" && exit 1;
[ -z "$ARTIFACT_REGISTRY_REPOSITORY" ] && echo "Need to set ARTIFACT_REGISTRY_REPOSITORY" && exit 1;
[ -z "$PROJECT_ID" ] && echo "Need to set PROJECT_ID" && exit 1;
[ -z "$GITHUB_SHA" ] && echo "Need to set GITHUB_SHA" && exit 1;

export CONTAINER_REPO="$ARTIFACT_REGISTRY_HOST/$PROJECT_ID/$ARTIFACT_REGISTRY_REPOSITORY"

echo $CONTAINER_REPO

# Configure Docker to use the gcloud command-line tool as a credential
# helper for authentication
gcloud auth configure-docker $ARTIFACT_REGISTRY_HOST

[ -e build/ ] && rm -rf build 

echo "-------------------------------------------------------------"
echo "Create build directory"
echo "-------------------------------------------------------------"

mkdir build && cp *.yaml build && cd build

echo "-------------------------------------------------------------"
echo "Replace environment variables in files"
echo "-------------------------------------------------------------"

sed -i -e 's|@IMAGE@|'"$IMAGE"'|g' *.yaml
sed -i -e 's|@CONTAINER_REPO@|'"$CONTAINER_REPO/$IMAGE:$GITHUB_SHA"'|g' *.yaml

echo "-------------------------------------------------------------"
echo "Display yaml files"
echo "-------------------------------------------------------------"

for f in *.yaml; do printf "\n---\n"; cat "${f}"; done

cd ../../agent_application

echo "-------------------------------------------------------------"
echo "Build and push docker container"
echo "-------------------------------------------------------------"

docker build -t "$CONTAINER_REPO/$IMAGE:$GITHUB_SHA" -f docker/Dockerfile ..
docker push "$CONTAINER_REPO/$IMAGE:$GITHUB_SHA"
