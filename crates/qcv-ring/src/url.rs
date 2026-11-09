//! `qcbae://` (QCView `host_bridge_url.h`): the host after the scheme up to the first `/` or `?`, lower-cased; its ring names,
//! the item label and the host app's name for the waiting text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host { AfterEffects, Premiere, Probe }
impl Host {
    pub fn parse(url: &str) -> Option<Host> {
        let rest = url.strip_prefix("qcbae://")?;
        let host: String = rest.split(['/', '?']).next().unwrap_or("").to_lowercase();
        match host.as_str() { "ae" => Some(Host::AfterEffects), "premiere" => Some(Host::Premiere), "probe" => Some(Host::Probe), _ => None }
    }
    pub fn url(self) -> &'static str { match self { Host::AfterEffects => "qcbae://ae", Host::Premiere => "qcbae://premiere", Host::Probe => "qcbae://probe" } }
    /// The frame ring's name (QCAE): `/qcbae-ae`, `/qcbae-premiere`, `/qcbae-probe`.
    pub fn ring_name(self) -> &'static str { match self { Host::AfterEffects => "/qcbae-ae", Host::Premiere => "/qcbae-premiere", Host::Probe => "/qcbae-probe" } }
    /// The audio ring's name (QCAA): the frame ring's + `-audio`.
    pub fn audio_ring_name(self) -> String { format!("{}-audio", self.ring_name()) }
    /// QCView `hostbridge::label`: the media item's name.
    pub fn label(self) -> &'static str { match self { Host::AfterEffects => "QCBridge After Effects", Host::Premiere => "QCBridge Premiere Pro", Host::Probe => "QCBridge Test Signal" } }
    /// The host app, for the waiting / not-running texts.
    pub fn app(self) -> &'static str { match self { Host::AfterEffects => "After Effects", Host::Premiere => "Premiere Pro", Host::Probe => "the QCBridge test producer" } }
    /// The key of `audio/liveSyncOffsetMs/<host>`.
    pub fn key(self) -> &'static str { match self { Host::AfterEffects => "ae", Host::Premiere => "premiere", Host::Probe => "probe" } }
}
pub fn is_qcbae(url: &str) -> bool { url.starts_with("qcbae://") }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn qcviews_rules() {
        assert_eq!(Host::parse("qcbae://AE"), Some(Host::AfterEffects));
        assert_eq!(Host::parse("qcbae://premiere/x?y=1"), Some(Host::Premiere));
        assert_eq!(Host::parse("qcbae://resolve"), None);
        assert_eq!(Host::parse("srt://h:1"), None);
        assert_eq!(Host::Premiere.audio_ring_name(), "/qcbae-premiere-audio");
        assert!(Host::Probe.ring_name().len() <= 31);
    }
}
