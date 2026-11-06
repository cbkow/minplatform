# vendor/qcbae — QCBridgeAE `src/common` (MIT, see LICENSE here), vendored 2026-11-06 at QCBridgeAE dff13bb

The canonical ring code behind the three wire contracts (canon §20): `protocol/` (QCAE `frame_desc.h`, QCAA `audio_desc.h`,
QCVO `output_desc.h`), `surface/` (`ring_mapping`, `ring_slots.h`, `shared_ring`, `output_ring`, `audio_ring`,
`audio_publisher.h`), `convert/` (half conversion). Copied verbatim — never edited here; re-copy to update and bump this stamp.
QCView's own copy (`src/decode/qcbae`, stamped 3d7de5b) differs from this only by its vendoring banner comments.
S4 (`spikes/s4-ring`) builds its C++ layout check and its C++ test producer/consumer from these files; `qcv-ring` (row 102)
tests against them.
