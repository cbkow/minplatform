#!/bin/zsh
# Builds the C++ test producer against the vendored QCBridgeAE ring code (its headers include each other as "common/…").
set -e
ROOT=${0:A:h:h:h}; OUT=${1:-$ROOT/target/s4-cxx}; mkdir -p $OUT/inc; rm -rf $OUT/inc/common; cp -R $ROOT/vendor/qcbae $OUT/inc/common
clang++ -std=c++20 -O2 -I$OUT/inc -o $OUT/cxx_producer $ROOT/tools/s4/cxx_producer.cpp $OUT/inc/common/surface/shared_ring.cpp $OUT/inc/common/surface/ring_mapping.cpp
echo "built $OUT/cxx_producer"
