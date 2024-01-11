#!/bin/bash

export IMAGE="unicore"
export ARTIFACTORY_HOST=europe-west4-docker.pkg.dev
export ARTIFACTORY_REPO=impierce-repo
export GITHUB_SHA=test_sha
export APISIX_PATH=unicore

./build.sh
