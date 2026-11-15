# minRender, the port: a real Blender job across macOS and Windows (2026-11-14)

Canon row 399. Until now every render was the harness's stand-in. This is Blender 5.2 itself, on a scene of the
owner's, on a share both machines reach, with two Rust nodes and no 0.5.x node.

## The set-up

- A scene made for the test (frames 1-10, Cycles on the GPU, 1920 x 1080, multilayer EXR) in a test job on the
  studio's share; a new folder in that job as the PRIVATE sync root. The real farm's sync root is on another volume
  and was never used.
- One Rust node on the Mac, one on the Windows machine: the tag `blend`, port 8441, UDP off, their own scratch data
  folders, and the path mappings of the old application copied from its settings on each machine (read only).
- On the Windows machine: the node started in the desktop session (the share is not reachable from a remote shell),
  and ONE inbound firewall rule for the test program's port from the local subnet, removed afterwards. The 0.5.11
  application there was running throughout and was not touched.
- The first node made the farm; the Blender plug-in template it brought to the share is what the job was baked from.

## What happened

**1. The Mac leads.** A submission in the Blender add-on's own form was dropped into the Mac node's box (the scene
and the output as macOS paths, frames 1-10 in chunks of 2).

- The manifest reached the queue in Windows form (`submitted_os` `windows`, the scene under a drive letter): the
  macOS submission was put through the mappings.
- The Windows node was given four chunks with the paths as they were; the Mac leader gave itself one, translated
  back to macOS paths - the case 0.5.x gets wrong (row 396).
- 122 s later: `completed`, five chunks, no retry, nobody barred; **ten EXR files** in the output folder.

**2. The Windows machine leads** (the tag `leader` moved; it took the queue over from the snapshot).

- The Mac, now a follower, answered `not_leader` and handed its submissions to the Windows leader.
- **A cancel while both rendered:** Blender was running on both machines; one second after the cancel (the check
  takes about that long) it was gone on both. The leader recorded both chunks as failures of their nodes, as 0.5.x
  does.
- A job of four frames to its end: the Windows leader translated the paths FOR THE MAC with its own mappings; two
  frames each; four EXR files.

## What it shows

The whole path works on real machines: a farm made by the port on an SMB share, the phonebook across systems, an
election, a takeover, the add-on's drop box, a manifest in travelling form, translation both ways, Blender started
under the leash on both systems, its `Saved:` lines counted as frames, the reports, and the kill.

## What it does not show

- A farm with 0.5.x nodes on real machines (that has only been done on the private farm on one Mac).
- Anything under load or over hours: fifteen frames in all.
- The stale-listing behaviour of a Windows SMB client (contract 1.2), which needs a share that caches and time.
- Staging, UDP, and the window.

Left on the share for the owner to look at: the private sync root's folder in the test job, and three output
folders of frames beside the scene's renders.
