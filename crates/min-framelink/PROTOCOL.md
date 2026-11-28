# The frame link - version 1, DRAFT

For the author of a HOST (a viewer, a transcoder that wants frames, sound and facts of a clip it cannot read itself)
or of a PROVIDER (a program that can read such clips and hands them over). `crates/min-framelink` holds the messages,
the shared blocks and the host's side in Rust; this page is enough to write either end in any language. It is a
draft: Windows is still to come, and names may change until a second host or provider uses it.

How a host learns that a provider exists, and which files to hand it, is in `src/described.rs`: a small JSON file a
provider in a per-user folder.

## The shape

- The host starts the provider as a child process, with the arguments its description names. One provider serves one host.
- The two talk over the child's standard input and output: **one JSON object a line**, UTF-8, in both directions.
- Pictures and sound never travel in messages. The host makes **shared blocks** of memory, tells the provider their
  names, and each request names the slot the answer is to be written to.
- The provider ends when the host says `bye` or the host's end of the pipe closes (the host quit or died). The host
  learns that the provider died when the provider's output closes.
- The provider's standard error carries its log lines. They are not part of the link.

## Rules

1. The first message is the host's `hello`; the provider answers with its own. A version the provider does not speak is
   answered by an `error` with `fatal`, and the provider ends.
2. Every message carrying an `id` is answered **exactly once** by a message carrying the same `id`. The host chooses
   the ids; the provider only echoes them.
3. A `frame` is always answered by `frame-done`, whatever became of it: `made`, `dropped` or `failed`.
4. **A slot belongs to the provider from the request that names it until that request's answer.** The host does not
   read it, write it or name it in another request meanwhile. After a jump this still holds: a slot comes back only
   with its answer, never earlier.
5. A line the provider does not understand is answered by an `error` without an `id`; the provider lives on.

## Shared blocks

A block is named shared memory (POSIX `shm_open` on macOS) cut into `slots` slots of `slot_bytes` each, one after the
other with nothing between. The host creates it and maps it; the provider maps it on `attach`.

- Two uses: `frames` and `sound`. A slot of a frames block is a whole multiple of the `page` the provider's `hello`
  states (16384 bytes), and at least the `bytes` the clip's answer states for the size asked.
- Once the provider has answered `attached`, the host should take the name away again (`shm_unlink`). The memory
  lives on while either process has it mapped, and nothing is left behind if both die.
- On macOS a name is at most 31 bytes and starts with a slash.

## A frame in a slot

RGBA, four 16-bit half floats a pixel, little endian. Rows are packed (`width * 8` bytes a row, no padding), the top
row first, starting at the slot's first byte. The values are **scene-linear** in the gamut the clip's answer names;
alpha is 1. `width` and `height` are stated in the `frame-done`.

## Sound in a slot

Interleaved PCM as it lies in the clip: little endian, `bits` a sample (24 bits are three bytes), `channels` samples
a sample frame. `sound-done` states how many sample frames were written, fewer than asked at the clip's end.

## Host to provider

| `t` | Fields | Answer |
|---|---|---|
| `hello` | `version` (1), `host` (a name, for the log) | `hello` |
| `open` | `id`, `path`, `gamut` (optional; `rec2020` when left out) | `clip`, or `error` |
| `attach` | `id`, `block` (the host's number for it), `name`, `slots`, `slot_bytes`, `use` (`frames` or `sound`) | `attached`, or `error` |
| `detach` | `id`, `block` | `detached`, after every request naming the block has been answered |
| `frame` | `id`, `clip`, `frame` (from 0), `scale` (1, 2, 4 or 8), `generation`, `block`, `slot` | `frame-done` |
| `jump` | `clip`, `generation` | none of its own |
| `sound` | `id`, `clip`, `first` (sample frame, from 0), `count`, `block`, `slot` | `sound-done`, or `error` |
| `controls` | `id`, `clip` | `controls` |
| `show-settings` | `id`, `clip` | `settings-shown`, or `error` where the provider has no screen |
| `close` | `id`, `clip` | `closed`, after every request for the clip has been answered |
| `bye` | | the provider ends |

## Provider to host

| `t` | Fields |
|---|---|
| `hello` | `version`, `provider`, `decoders` (names, for the log), `page` |
| `clip` | `id`, `clip` (the number to name it by), `width`, `height`, `rate` (`[numerator, denominator]`), `frames`, `timecode` (of frame 0), `timecode_base_frame`, `drop_frame`, `sound` (`bits`, `channels`, `rate`, `samples`; or null), `camera`, `metadata` (a list of `[key, value]`), `picture` |
| `attached`, `detached` | `id`, `block` |
| `frame-done` | `id`, `result` (`made`, `dropped`, `failed`), `width`, `height`, `message` (when failed) |
| `sound-done` | `id`, `samples` |
| `closed` | `id`, `clip` |
| `controls` | `id`, `clip`, `controls` (a list, see below) |
| `settings-shown` | `id`, `clip` |
| `settings-changed` | `clip`. Not an answer: the user changed the clip's settings in the provider |
| `error` | `id` (when it answers a request), `message`, `fatal` |

`picture` in a `clip` answer: `format` (`rgba16f`), `encoding` (`scene-linear`), `gamut` (the one this clip was opened
with), `gamuts` (those an `open` may ask for: `rec2020`, `rec709`, `p3-d65`, `aces-ap0`, `aces-ap1`, `camera`), and
`sizes`: for each `scale` its `width`, `height` and the `bytes` a slot must have.

## A source's settings

**The settings are the provider's.** A host never sets one: it can read them, ask the provider to show its panel, and
is told when the user has changed something there. What the user sets is kept by the provider for the clip, and a clip
opened later - by any host, or in the provider's own window - is decoded with it.

- `controls` answers with a list. Each entry: `id`, `label` (English), `kind`, `as_shot` (the clip's own value) and
  `value` (what frames are decoded with now). `kind` is `choice` (with `options`: a list of `[value, text]`), `range`
  (with `min`, `max`, `step`) or `switch` (0 or 1). Which controls a clip has depends on its maker and camera; a host
  shows what it is given and assumes none.
- `show-settings` brings up the provider's panel for the clip, a small window of the provider's own.
- `settings-changed` arrives at every change. **Frames made from then on carry the new settings; frames already in
  the host's slots do not.** The host counts its generation up and asks again for what it shows, as after a jump.

## Jumps and generations

Every `frame` carries a `generation`, a number the host counts up. When the user jumps, the host sends `jump` with
the new generation and asks for the new frames with it. The provider answers every waiting `frame` of an older
generation as `dropped` without decoding it; one already being decoded is finished or abandoned, and answered as
`dropped` too. The host frees a slot when its answer arrives, whatever the answer says.

The provider decodes four frames at once and keeps the rest in the order asked. A host asks ahead as far as it has
slots; there is no window to announce.

## An exchange

```
host      {"t":"hello","version":1,"host":"example"}
provider  {"t":"hello","version":1,"provider":"example-reader","decoders":["..."],"page":16384}
host      {"t":"open","id":1,"path":"/clips/A001.ext"}
provider  {"t":"clip","id":1,"clip":1,"width":6144,"height":3456,"rate":[24,1],"frames":95, ... }
host      {"t":"attach","id":2,"block":1,"name":"/example-1","slots":85,"slot_bytes":42467328,"use":"frames"}
provider  {"t":"attached","id":2,"block":1}
host      {"t":"frame","id":3,"clip":1,"frame":0,"scale":2,"generation":0,"block":1,"slot":0}
provider  {"t":"frame-done","id":3,"result":"made","width":3072,"height":1728}
host      {"t":"bye"}
```

## Not in this draft

A display
rendering for a host that wants pictures ready to show; Windows (other calls for shared memory and for starting the
child, the same messages).
