#!/usr/bin/env bash
# run this from the root directory for the project
set -e
TARGET=x86_64-unknown-linux-musl
PROJECT=rook
TAG=localhost/$PROJECT:$TARGET

docker build -t $TAG -f docker-build/Dockerfile.release --build-arg PROJECT=$PROJECT .
CID=$(docker container create $TAG)
mkdir -p target/optimized/
docker cp -q $CID:target/${TARGET}/release/${PROJECT} target/optimized/${PROJECT}.${TARGET}
docker container rm $CID &> /dev/null