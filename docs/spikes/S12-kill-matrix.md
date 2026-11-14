# S12 — does one executable with a leash kill a render's tree as surely as today's agent?

**Question (§21.3; `docs/phase4/step0-kill-path.md`):** minRender's renders are supervised by a second program,
`mr-agent`, whose brute-force tree kill the owner credits for the product's stability. A port wants one executable.
Can ONE executable - the application, starting itself a second time as a tiny "leash" that owns the renderer's tree
and kills it when its pipe to the application closes (shape B) - give every kill guarantee today's agent gives? And
what does a plain single process (shape A, no second process at all) lose?
**Pass bar (for a shape to replace the agent):** across six scenarios on Windows and macOS, **no synthetic process
alive 2 s after the event wherever today's agent leaves none**, and **the last process gone no later than with
today's agent**.
**Time box:** one session, 2026-11-14. **Status: see the verdict below.**

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
