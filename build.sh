#!/bin/sh

cd frontend &&\
trunk clean &&\
trunk build --release &&\
cd .. &&\
cargo build -p backend --release &&
podman build .