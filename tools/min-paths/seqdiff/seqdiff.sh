#!/bin/zsh
# Row 120's differential test: QCView's detectImageSequence (sliced read-only from the QCView checkout) vs min-paths, over
# tools/min-paths/seqdiff/cases.txt. Needs Qt (QT_DIR or ~/Qt/6.*/macos) and the QCView checkout (QCVIEW_SRC).
set -e
HERE=${0:A:h}; ROOT=${HERE:h:h:h}
QT=${QT_DIR:-$(ls -d ~/Qt/6.*/macos | tail -1)}
QCVIEW=${QCVIEW_SRC:-$HOME/Documents/GitHub/QCView-Player}
# The reference is QCView's ORIGINAL C++ at a pinned revision (its last commit before QCView's own min-paths adoption,
# row 125), read from git history — the working tree's detector now delegates to the crate and cannot be the reference.
REF=${QCVIEW_REF:-cb890143}
OUT=${1:-$ROOT/target/seqdiff}; mkdir -p $OUT/scratch-cpp $OUT/scratch-rs
# Slice the function verbatim: from its definition line to the first line that is exactly "}".
git -C $QCVIEW show $REF:src/project/project_manager.cpp \
  | awk '/^bool ProjectManager::detectImageSequence\(/{p=1} p{print} p&&/^}$/{exit}' > $OUT/qcview_detect.inc
test -s $OUT/qcview_detect.inc || { echo "could not slice detectImageSequence from $QCVIEW @ $REF"; exit 1 }
echo "sliced $(wc -l < $OUT/qcview_detect.inc) lines of QCView's detector (unmodified, @ $REF)"
clang++ -std=c++17 -O1 -fPIC -I$OUT -I$QT/lib/QtCore.framework/Headers -F$QT/lib -framework QtCore -Wl,-rpath,$QT/lib \
  -o $OUT/harness $HERE/harness.cpp
cargo build -q -p min-paths --example seqprobe
QT_LOGGING_RULES='*=false' $OUT/harness $HERE/cases.txt $OUT/scratch-cpp 2>/dev/null > $OUT/cpp.out
$ROOT/target/debug/examples/seqprobe $HERE/cases.txt $OUT/scratch-rs > $OUT/rs.out
if diff -u $OUT/cpp.out $OUT/rs.out > $OUT/diff.out; then
  echo "seqdiff: $(grep -c . $OUT/cpp.out) cases IDENTICAL between QCView's C++ and min-paths"
else
  echo "seqdiff: DIFFERENCES"; cat $OUT/diff.out; exit 1
fi
