#!/bin/zsh
# Phase 2 step 1 — the DISPLAY rows of the pass bar (docs/phase2/step1-open-present.md), run on an awake screen.
# Usage: step1-display.sh [screen-index]   (the external 60 Hz display S1/S2 measured on; `qcv` prints the monitors)
# Each run opens a window for ~10 s and takes focus. Output: fixtures/phase2/*.out|.trace (ignored by git).
set -u
ROOT=${0:A:h:h:h}; FIX=$ROOT/fixtures; OUT=$FIX/phase2; mkdir -p $OUT
SCREEN=${1:-}; S=(); [[ -n $SCREEN ]] && S=(--screen $SCREEN)
T=$ROOT/tools/phase2/trace.sh
cd $ROOT && cargo build -q --release -p qcv || exit 1
echo "== 1. 4K ProRes, zero-copy, EDR surface (pacing + lateness + the fused GPU figure)"; $T p4k-hw 12 $FIX/prores_4k.mov $S
echo "== 2. 4K ProRes, software + upload (the control)"; $T p4k-sw 12 $FIX/prores_4k.mov --sw $S
echo "== 3. H.264 4K zero-copy"; $T h264-hw 10 $FIX/h264_4k.mp4 $S
echo "== 4. HEVC Main10 4K zero-copy"; $T hevc10-hw 10 $FIX/hevc10_pq_4k.mp4 $S
echo "== 5. 1080p ProRes zero-copy"; $T p1080-hw 10 $FIX/prores_1080.mov $S
echo "== 6. the EXR sequence from one frame"; $T exr 8 $FIX/exr_4k/frame_0010.exr $S
echo "== 7. PQ ramp on the PQ surface + the arithmetic"; $ROOT/target/release/QCView $FIX/pq_ramp_4k.mp4 --out pq --verify-pq --stats --seconds 6 $S > $OUT/pq-surface.out 2>&1; grep -E "presenter:|layer:|verify-pq|band|present ms|EDR" $OUT/pq-surface.out | head -12
echo "== 8. SDR surface"; $ROOT/target/release/QCView $FIX/prores_4k.mov --out sdr --stats --seconds 6 $S > $OUT/sdr.out 2>&1; grep -E "presenter:|present ms|play:" $OUT/sdr.out | tail -3
echo "== 9. the windowed snapshot equals the headless one"; $ROOT/target/release/QCView $FIX/prores_4k.mov --snap-out $OUT/qcv_p4k_hw_windowed.png@1 --seconds 3 $S > $OUT/snap-windowed.out 2>&1; cmp $OUT/qcv_p4k_hw_windowed.png $OUT/qcv_p4k_hw.png && echo "   identical"
echo "== footprint: run 'footprint <pid>' by hand during run 1 if the RSS row needs Apple's number (S1 compared footprint, not RSS)"
