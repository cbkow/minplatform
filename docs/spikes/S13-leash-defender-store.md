# S13 - does Windows object to the leash? (opened 2026-11-14)

**Status: OPEN.**

## Why

S12 showed that one executable with a leash kills a render's tree as surely as today's separate agent. The owner's
answer: "windows defender and store submission is very tricky with minRender." The leash means the application starts
ITSELF again, hidden, for every chunk, and that second copy puts itself in a job object and starts the renderer. If
Windows' antivirus or the Store's packaging dislikes that, the shape is dead whatever S12 measured.

What minRender's own history says (its `installer/STORE.md`): the Store's troubles were about capabilities and a
hidden second application entry, never about the agent; the antivirus trouble was machine-learning detections of
UNSIGNED binaries on the direct channel, which is why Windows became Store-only in 0.5.6. Today's Store package
already starts a hidden `mr-agent.exe` per chunk from inside the package.

## The question

On Windows 11 with Defender on (real-time, cloud): does a windowed program that relaunches itself hidden for every
chunk, enters a kill-on-close job and kills its tree

1. run without a Defender detection, block or quarantine - as built, signed, and inside a package;
2. WORK inside a package (a packaged process is already in its container's job: is the leash's own job refused? does
   an application's death still kill the tree?);
3. pass the Windows App Certification Kit.

## Pass bar

- 300 chunks cancelled and 20 application deaths in each of the three forms: every chunk starts whole, **0** processes
  alive 2 s after the kill, **0** refusals of the job.
- **0** Defender detections or events (1006, 1007, 1015, 1116, 1117, 1121, 1122) during the run; a scan of each file
  finds nothing; the files are still there afterwards.
- The certification kit's overall result is PASS (or its only failures are ones the minNotes and QCView packages,
  which are in the Store, share).

## Time box

Three hours.

## What it cannot show

- The Store's certification itself (people and Microsoft's own scanners) - only the kit that precedes it.
- Defender's cloud reputation for a file many machines have seen, or other antivirus products.
- A real renderer. The "renderer" here sleeps.

## How to run

`cargo build --release -p s13-leash-store`, then in the desktop session
`spikes\s13-leash-store\run-s13.ps1 -Work <folder> -SignThumbprint <test certificate> -Kit`.
