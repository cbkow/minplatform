//! The frame link: a HOST (a viewer, a transcoder) starts a PROVIDER - another program that can read clips the
//! host cannot - and pulls numbered frames and sound from it. Lines of JSON travel over the provider's standard
//! input and output; pictures and sound land in shared memory the host made and owns. `PROTOCOL.md` beside this
//! crate describes it for the author of either end.
//!
//! - [`msg`]: the messages, version 1.
//! - [`shm`]: a shared block cut into slots (POSIX systems so far).
//! - [`host`]: starting a provider and talking to it.
//! - [`described`]: the small files by which a provider makes itself known to hosts, so that no host carries a
//!   provider's name or its file types in its code.
pub mod described;
pub mod host;
pub mod msg;
#[cfg(unix)]
pub mod shm;
