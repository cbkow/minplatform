# S13 - does Windows object to the leash? (opened 2026-11-14)

**Status: VERDICT - PASS (2026-11-14). Defender is silent in all three forms, the leash works inside a package, and the certification kit's one failure is not the leash's: every program of ours that starts a process has it, the agent in today's Store package included.**

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

## Findings (2026-11-14, the Windows machine: Windows 11, Defender 4.18 with real-time and cloud protection on, no exclusions, signatures of the day)

| Form | Chunks cancelled | Application deaths | Started whole | Job refused | Alive after 2 s | Slowest kill |
|---|---|---|---|---|---|---|
| as built (unsigned) | 300 | 20 | all | 0 | 0 | 6 ms |
| signed (test certificate) | 300 | 20 | all | 0 | 0 | 6 ms |
| inside a signed package, installed | 300 | 20 | all | 0 | 0 | 6 ms |

(6 ms is the measuring loop's step, not a difference between the forms.)

- **Defender:** no detection on record before or after, no event (1006, 1007, 1015, 1116, 1117, 1121, 1122) during
  the run, a scan of each of the three files finds nothing, and the files are still where they were. That is 960
  hidden self-relaunches and 2,880 hidden child processes.
- **Inside a package** the application is already in a job (its container's) and the leash's own job is accepted
  inside it - nested, never refused. An application killed outright still takes the tree with it. (The unpackaged runs
  were in a job too - the scheduled task's - so all three forms tested the nested case.)
- **The certification kit:** 24 tests, 22 pass, overall WARNING.
  - FAIL "Blocked executables": `s13.exe` refers to `CreateProcessW` and holds the text `cmd.exe`. Neither is the
    leash: the first is what any program that starts a renderer does, the second is in Rust's standard library (its
    handling of batch files). The same two are in `mr-agent.exe` - which is in minRender's Store package today - and in
    `minNotes.exe` and `QCView.exe` (looked for in the built files; the kit was NOT run over those packages, because it
    installs and removes what it tests and would have taken an installed copy's data with it).
  - WARNING "DPI awareness": the spike has no manifest. The product has one.
- Nothing of the spike was left: no process, the package removed. The production minRender on that machine kept its
  process throughout.

## Verdict

**PASS.** A hidden self-relaunch per chunk is, to Defender and to a package, no different from today's hidden
`mr-agent.exe` per chunk - and it is one signed file fewer. The process shape is the leash (canon row 387, no longer
conditional).

Carried into the port:
- The leash enters its job BEFORE it starts anything, and treats a refusal as fatal (exit, never run unfenced).
- The kit's "Blocked executables" failure will appear for the product too, as it does for the agent now. It is to be
  named in the notes for certification, with the reason (it launches the user's installed render applications).
- Still unmeasured, and only a real submission answers them: the Store's own review, Defender's reputation for a new
  file seen on many machines, other antivirus products. minRender's history says the first was about capabilities
  and the second about unsigned files, neither about the agent.
