# S12 — does one executable with a leash kill a render's tree as surely as today's agent?

**Question (§21.3; `docs/phase4/step0-kill-path.md`):** minRender's renders are supervised by a second program,
`mr-agent`, whose brute-force tree kill the owner credits for the product's stability. A port wants one executable.
Can ONE executable - the application, starting itself a second time as a tiny "leash" that owns the renderer's tree
and kills it when its pipe to the application closes (shape B) - give every kill guarantee today's agent gives? And
what does a plain single process (shape A, no second process at all) lose?
**Pass bar (for a shape to replace the agent):** across six scenarios on Windows and macOS, **no synthetic process
alive 2 s after the event wherever today's agent leaves none**, and **the last process gone no later than with
today's agent**.
**Time box:** one session, 2026-11-14. **Status: VERDICT - shape B (the leash) PASSES on both systems and beats today's agent in three scenarios; shape A FAILS on macOS (2026-11-14).**

## What the spike is
`spikes/s12-killmatrix` (binary `s12`; it dies with this verdict). A synthetic renderer that registers its process id
in a folder, starts helper processes (at its very first instruction, or after 300 ms), can ignore signals, and whose
helpers can try to leave its group (macOS: `setsid`) or its job (Windows: started with the break-away flag). A
stand-in for the application supervises it in three ways:
- **today**: the real `mr-agent` built from MinRender 0.5.11's source, spoken to over its own pipe (`task`, `abort`);
  its log is sent to a scratch folder;
- **A**: one process - the renderer is the application's own child, in a process group (macOS) or a job the
  application holds, entered while the renderer is still suspended (Windows);
- **B**: the leash - the same executable relaunched; on Windows it puts ITSELF into a kill-on-close job before it
  starts the renderer, so whatever the renderer starts is born inside; on macOS it starts the renderer in a group
  and the application kills that group as well when the leash goes.
A driver runs each shape through: cancel; cancel of a renderer that ignores signals; the supervisor killed outright;
the application killed outright; helpers started before a fence could be up; helpers that leave. It counts the
registered processes alive 2 s after the event and times the last one's end (10 ms steps).

## Findings
Three synthetic processes per run (a renderer and two helpers). macOS on Apple silicon; Windows 11 on the farm's
Windows node, which had no render running (the owner's word) - its own minRender kept its process and its agent's log
was not written to. Release builds.

| Scenario | Shape | macOS: alive after 2 s, all gone after | Windows: alive after 2 s, all gone after |
|---|---|---|---|
| cancel | today's agent | 0 of 3, 0.03 s | 0 of 3, 0.07 s |
| cancel | A: one process | 0 of 3, 0.04 s | 0 of 3, 0.01 s |
| cancel | B: the leash | 0 of 3, 0.02 s | 0 of 3, 0.01 s |
| cancel, signals ignored | today's agent | 0 of 3, 0.11 s | 0 of 3, 0.07 s |
| cancel, signals ignored | A: one process | 0 of 3, 0.01 s | 0 of 3, 0.01 s |
| cancel, signals ignored | B: the leash | 0 of 3, 0.02 s | 0 of 3, 0.01 s |
| supervisor killed | today's agent | 3 of 3, never (waited 5 s) | 0 of 3, 0.01 s |
| supervisor killed | A: one process | -, (no supervisor) | -, (no supervisor) |
| supervisor killed | B: the leash | 0 of 3, 0.02 s | 0 of 3, 0.01 s |
| application killed | today's agent | 3 of 3, 66.26 s | 3 of 3, 66.10 s |
| application killed | A: one process | 3 of 3, never (waited 120 s) | 0 of 3, 0.01 s |
| application killed | B: the leash | 0 of 3, 0.02 s | 0 of 3, 0.01 s |
| helpers before the fence | today's agent | 0 of 3, 0.11 s | 0 of 3, 0.08 s |
| helpers before the fence | A: one process | 0 of 3, 0.03 s | 0 of 3, 0.01 s |
| helpers before the fence | B: the leash | 0 of 3, 0.02 s | 0 of 3, 0.01 s |
| helpers that leave | today's agent | 2 of 3, never (waited 5 s) | 0 of 3, 0.07 s |
| helpers that leave | A: one process | 2 of 3, never (waited 5 s) | 0 of 3, 0.01 s |
| helpers that leave | B: the leash | 2 of 3, never (waited 5 s) | 0 of 3, 0.01 s |

"-, (no supervisor)": shape A has no supervising process to kill. "never (waited N s)": still alive when the wait
ended.

What the table says:
1. **Cancel** (the first two scenarios): every shape kills all three processes, signals ignored or not. The
   single-executable shapes are faster (10-30 ms against 70-110 ms): today's cancel crosses a pipe and the agent's
   loop first.
2. **The supervisor killed**: on Windows today's agent and the leash both take the tree with them (the job closes).
   On macOS today's agent leaves ALL THREE running for good - the weakness the reading predicted - and the leash
   leaves none, because the application watches the leash and kills the group itself.
3. **The application killed**: today, on BOTH systems, the render runs on for **66 s** (the agent's six reconnect
   rounds of 5 s, each with its connect's own retries) before it is killed. The leash kills it in 10-20 ms on both.
   Shape A kills it at once on Windows (the job is the application's) and **never on macOS** - the one guarantee a
   single process cannot give there, as predicted.
4. **Helpers started before a fence could be up**: no shape lost one. Today's "job assigned after spawn" race was
   NOT reproduced by a renderer that starts helpers at its first instruction - the agent wins that race in practice;
   the window exists on paper. Shapes A (started suspended) and B (born in the job) have no window at all.
5. **Helpers that leave**: on Windows none can (a job without the break-away permission refuses it), in any shape.
   On macOS a helper that calls `setsid` survives EVERY shape, today's included - nothing here changes that, and none
   of the shipped renderers is known to do it.

A fault of the spike's own first run, kept for the record: the leash's backup kill on macOS waited for the renderer's
output to END before looking at the leash, and a renderer that outlives the leash keeps that pipe open - so nothing
was killed. The leash's end must be watched by itself. (Fixed; the table is from the run after.)

Not measured: a timeout (the same kill call from a timer), a render in progress on a real DCC program, antivirus
or Store behaviour towards a program that relaunches itself hidden, a machine under load.

## Verdict
**Shape B passes the bar on both systems**: wherever today's agent leaves nothing alive, the leash leaves nothing
alive, and it is never slower. It is BETTER than today in three places - an application crash kills the render in
milliseconds instead of 66 s (both systems), and on macOS a killed supervisor no longer orphans the tree.
**Shape A fails on macOS** (an application crash leaves the render running for good) and passes on Windows.
So: one executable, with the leash, keeps the brute-force kill whole. What it does not answer is whether a hidden
relaunch of our own signed program is treated by antivirus tools and the Store as today's agent launch is - that is
a packaging question for its own test.

The spike's code dies with this verdict; what it showed that the product should take:
- Windows: the supervising process puts ITSELF in a kill-on-close job before it starts the renderer.
- macOS: the renderer in its own group; the application watches the supervisor's process (not its output) and kills
  the group when it ends; the supervisor kills the group when its pipe to the application closes.
- No reconnect dance: a closed pipe means the application is gone.
