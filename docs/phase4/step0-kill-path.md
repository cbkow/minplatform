# minRender: the kill path, read closely (2026-11-14)

The owner's word on the first audit: "that brute force process kill is one of the things that makes this app so stable
… let's look a little closer and see how much we can preserve with a tidier single app. It's ugly, I know it, but it's
proven its mettle more than once." This note reads the kill path line by line (MinRender `min-paths` @ 9526557) and
says, guarantee by guarantee, where it comes from and what a single binary keeps, loses or must rebuild. Nothing was
run. Linux is an eventual target, not this release's (the owner, same day).

## 1. What actually kills a render today

All of it is in `mr-agent/src/kill.rs` (218 lines, two Unix tests) and `mr-agent/src/executor.rs`:

| Step | Windows | macOS |
|---|---|---|
| Before the renderer runs | nothing (`prepare_command` is empty) | `process_group(0)`: the child is its own group before it runs a line |
| Just after spawn | a Job Object with `KILL_ON_JOB_CLOSE`; the child is assigned to it (`create_job_for`). If that fails: remembered, `taskkill /PID /T /F` is the fallback | nothing |
| Kill | `TerminateJobObject` | `kill(-pgid, SIGKILL)` |
| Guards | first caller wins and records the reason; a no-op once the child was reaped (no recycled id is ever signalled) | the same |
| Who asks for a kill | the app's `abort` message, the task's timeout thread (`executor.rs`), a pipe that could not be reconnected | the same |

There is no graceful step anywhere: no SIGTERM, no "please stop", no waiting. That is the brute force, and it is why a
cancel takes effect on every node at once.

## 2. The guarantees, and which of them come from there being a second process

| # | Guarantee an operator relies on | Where it comes from today | Needs a second process? |
|---|---|---|---|
| G1 | Cancel kills the renderer AND everything it started, at once | job object / process group (`kill.rs`) | No. The same calls work from any process |
| G2 | A renderer that ignores signals or hangs still dies | SIGKILL / TerminateJobObject cannot be caught | No |
| G3 | A chunk that runs past its timeout dies | a timer thread calling the same kill (`executor.rs`) | No |
| G4 | If the thing supervising the render dies, the render dies (Windows) | the OS closes the agent's job handle: `KILL_ON_JOB_CLOSE` | No - and it gets STRONGER in one process: the handle is the application's, so an application crash kills the tree at once, where today the tree outlives the application by up to 30 s (G5) |
| G5 | If the application dies, the render does not run on unowned (macOS, and Windows today) | the agent notices the broken pipe, tries to reconnect 6 times 5 s apart, then kills (`main.rs` `attempt_reconnect`) | **Yes on macOS.** macOS has no "kill my children when I die". Without a second process an application crash leaves the renderer running in its group, unowned |
| G6 | A stuck supervisor cannot block the application | the application can `TerminateProcess` the agent after 5 s (`shutdownAgent`), which on Windows also kills the tree (G4) | Partly. In one process a stuck reader is a stuck thread; the kill is still one system call from another thread. What is lost is "throw the whole supervisor away" |
| G7 | Every chunk starts from nothing | one agent process per chunk since 0.3.7 | No, but it is free with a process and must be written with care without one (no state carried between chunks) |
| G8 | A crash in the supervising code (a bad regex, a panic while parsing output) does not take the node down | the agent is another process; its panic hook logs and it exits | **Yes**, unless panics are caught at the thread boundary - which Rust can do for panics, not for aborts or stack overflows |

So of eight, six do not need the split, one (G4) improves without it, and two (G5 on macOS, G8) are what the second
process really buys.

Things that are NOT there today, whatever the folklore:
- `AgentSupervisor::killAgent()` (the application killing the agent outright) is never called by anything.
- `watchdog.rs` is one comment line: the agent has no watchdog on the application beyond the pipe.
- Nothing kills by executable name, and nothing sweeps for strays at startup.

## 3. Where today's kill is weaker than it looks

1. **Windows: the job is assigned after the child is already running.** A renderer that starts a helper in its first
   milliseconds can leave that helper outside the job. (A child started suspended, assigned, then resumed - or given
   its job at creation - closes this.)
2. **macOS: kill the agent with SIGKILL and the renderer is orphaned** - the mirror image of G4, with nothing to
   catch it. Nothing looks for it at the next start.
3. **macOS: a renderer that calls `setsid` / `setpgid` leaves the group** and survives the group kill. (None of the
   three shipped DCC programs is known to; nothing checks.)
4. **A cancel is reported as a failed chunk** (retry spent, node counted towards suspension) - visible to the farm,
   so a protocol matter, listed in `step0-contracts.md`.
5. **The application's Exit in the menu kills the render; the tray's Exit waits for it.**

## 4. What a tidier single application can be

Three shapes, from least to most process:

**A. One process, kill code as a module.** `kill.rs` and `executor.rs` move in nearly unchanged (they have no pipe in
them). Keeps G1-G4, G7; G4 improves on Windows. Loses G5 on macOS and G8. Remedies inside one process: catch panics at
the supervising thread; write the renderer's group id to a small file and kill a leftover group at the next start (a
"sweep", which today's application does not have either).

**B. One executable, with a leash.** The same binary started a second time in a tiny mode (`minRender --leash`, a few
dozen lines with no farm logic): it starts the renderer in its group / job, holds a pipe to the application, and
kills the tree the moment that pipe closes. It IS today's agent reduced to the two things only a process can do (G5,
G8), without the reconnect dance, the JSON protocol, the single-instance mutex or a second file to sign. Output
parsing, timeouts and progress live in the application. Cost: one hidden relaunch of our own signed binary per chunk -
the same shape as today's agent launch, so no new exposure to antivirus heuristics or the Store, but to be proven.

**C. Today's shape kept**: a separate `mr-agent` with its pipe protocol, rewritten on the application's side.

| | G1-G3 | G4 app crash kills tree (Win) | G5 app crash kills tree (mac) | G6 | G7 | G8 | Executables |
|---|---|---|---|---|---|---|---|
| Today | yes | after up to 30 s | after up to 30 s | yes | yes | yes | 2 (+ restart helper) |
| A. module | yes | at once | **no** (sweep at next start) | thread only | by care | panics only | 1 |
| B. leash | yes | at once | at once | yes | yes | yes | 1 |
| C. agent kept | yes | after up to 30 s | after up to 30 s | yes | yes | yes | 2 |

## 5. What would settle it: a kill matrix, run rather than argued

A spike (one question, one pass bar, a time box, a written verdict; nothing of it becomes product code): a synthetic
"renderer" that can be told to start grandchildren at once, start them late, ignore signals, leave its group, and run
past a timeout; driven by today's `mr-agent`, by shape A and by shape B, on Windows and macOS. For each: cancel; kill
the supervising process outright; kill the application outright; a helper started in the first millisecond; a
renderer that leaves its group. The measure: processes left alive after 2 s, and the time from the request to the
last process gone. The pass bar for a shape: no survivor where today has none, and no slower. On Windows it runs on a
machine that is not the production node, or with nothing but the synthetic renderer and never while a real render is
running there.

## Open questions for the owner

1. Is "the application crashed, the render dies with it" (G5) wanted on macOS as it is on Windows? It decides between
   A and B.
2. Is the leash's hidden relaunch acceptable given the 0.5.6 antivirus history, if the spike shows it behaves as
   today's agent launch does?
3. Should a start sweep for a stray renderer left by a crash (new behaviour, either shape)?
4. Should the Windows race (a helper escaping in the first milliseconds) be closed, or reproduced?
5. Which Exit rule is meant: wait for the render, or kill it?
