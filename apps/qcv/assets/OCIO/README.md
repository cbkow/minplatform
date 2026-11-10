# The bundled OCIO configs

The three configs QCView offers in its Color panel, moved here from the QCView-Player repo on 2026-11-09 so the app no
longer reaches into a sibling checkout. They ship beside the app (`assets/OCIO` next to the executable on Windows, in the
bundle's `Resources` on macOS); `QCV_OCIO_CONFIGS` points the app at another folder.

| Folder | What | Where it came from |
|---|---|---|
| `ACES_2.0` | ACES 2.0 studio config, with QCView's display additions | ASWF `studio-config-all-views` v4.0.0 (see `patches/README.md`) |
| `ACES_1.3` | ACES 1.3 config; its own `LICENSE` and `README` | the OpenColorIO project |
| `Blender5.2` | Blender 5.2's stock colour management folder (config, LUTs, ICC profiles) with QCView's EDR patch | Blender 5.2 `datafiles/colormanagement` + `patches/blender52-edr.patch` |

`patches/` records what QCView changed on top of the stock configs, so the next upgrade is mechanical. The licences of
these files are their upstreams'; the app's third-party notices list them.
