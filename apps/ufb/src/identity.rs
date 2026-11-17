//! Which UFB this build is (canon rows 519 point 1, 521). Decided when it is BUILT - `UFB_IDENTITY=test` or `release`
//! in the build's environment; nothing at run time can turn one into another:
//!
//! | | development (no word) | the test package | the release |
//! |---|---|---|---|
//! | its data folders' name | `UFB-test` | `UFB-2-test` | `ufb` (1.4.2's) |
//! | the mesh's epoch and ports | the test ones | the test ones | 1.4.2's |
//! | the safety belt (row 468) | on | off | off |
//! | what it is called | UFB | UFB Test | UFB |
//!
//! The development build is what `cargo build` makes: it changes nothing outside the folders it is started with and
//! shares no data with anything installed. The test package installs beside a real 1.4.2 and works on real folders -
//! with metadata, settings and a mesh of its own. The release takes 1.4.2's place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Dev, Test, Release }

pub fn kind() -> Kind { match option_env!("UFB_IDENTITY") { Some("release") => Kind::Release, Some("test") => Kind::Test, _ => Kind::Dev } }
/// The name of this build's folders for settings, the database, the cache and the log.
pub fn data_name() -> &'static str { match kind() { Kind::Dev => "UFB-test", Kind::Test => "UFB-2-test", Kind::Release => "ufb" } }
pub fn display_name() -> &'static str { if kind() == Kind::Test { "UFB Test" } else { "UFB" } }
/// The belt: this build changes nothing outside the folders it is started with.
pub fn belt() -> bool { kind() == Kind::Dev }
/// The mesh this build speaks in: an installed 1.4.2's only for the release.
pub fn epoch() -> u32 { if kind() == Kind::Release { crate::mesh::EPOCH } else { crate::mesh::TEST_EPOCH } }
pub fn ports() -> (u16, u16, u16) { if kind() == Kind::Release { (crate::mesh::CTL_PORT, crate::mesh::DATA_PORT, crate::mesh::UDP_PORT) } else { crate::mesh::TEST_PORTS } }
/// Where this build keeps its settings and its database (`UFB_SETTINGS_DIR` names another folder).
pub fn data_dir() -> Option<std::path::PathBuf> { min_settings::Settings::dir_for(data_name(), "UFB") }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_build_made_without_a_word_is_the_development_one() {
        // (the tests are built without `UFB_IDENTITY`: what they run is never the release)
        assert_eq!(kind(), Kind::Dev); assert_eq!(data_name(), "UFB-test"); assert!(belt()); assert_eq!(epoch(), crate::mesh::TEST_EPOCH); assert_eq!(ports(), crate::mesh::TEST_PORTS); assert_ne!(ports().0, crate::mesh::CTL_PORT);
    }
}
