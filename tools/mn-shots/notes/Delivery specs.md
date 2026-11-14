# Delivery specs

The full document is attached below; the commands are the ones the pipeline runs.

## Web master

```bash
# the web master from the graded ProRes
ffmpeg -i TW_reel2_v003.mov -vf scale=1920:-2 \
  -c:v libx264 -crf 18 -pix_fmt yuv420p \
  -c:a aac -b:a 192k TW_reel2_v003_web.mp4
```

## Naming

```python
def delivery_name(reel: int, version: int, ext: str = "mov") -> str:
    """TW_<reel>_<version>.<ext>, as the specification's page 1 has it."""
    return f"TW_reel{reel}_v{version:03d}.{ext}"

print(delivery_name(2, 3))  # TW_reel2_v003.mov
```

| Format | Size | Rate | Use |
| --- | --- | --- | --- |
| Master | 3840 × 2160 | 24 | Archive, festivals |
| Web | 1920 × 1080 | 24 | Streaming |
| Vertical | 1080 × 1920 | 24 | Social |
