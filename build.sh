#!/bin/sh
# Builds the frontend, then the binary that embeds it (`binary/build.rs` only checks that
# `frontend/dist` is there).

cd frontend/node &&\
npm install &&\
cd .. &&\
trunk clean &&\
trunk build --release &&\
cd .. &&\
cargo build -p binary --release
