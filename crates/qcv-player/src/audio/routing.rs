//! `computeRoutingMatrix` (QCView `audio/audio_routing_matrix.h`), verbatim: the 2 × N output-from-input matrix handed
//! to swresample. Auto = let swresample fold 1 / 2 / 6 channels itself (BS.775 for 5.1 via its own rematrix), pluck
//! 7–8 for ≥ 8 channels, else L / R = channels 0 / 1; Downmix5_1 = L + 0.707 C + 0.707 Ls (and R / Rs); Stereo7_8 = channels 6 / 7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RoutingMode { #[default] Auto = 0, Downmix5_1 = 1, Stereo7_8 = 2 }

/// `None` = no matrix: swresample's default fold (with `rematrix_volume` 0.9).
pub fn routing_matrix(mode: RoutingMode, src_channels: usize) -> Option<Vec<f64>> {
    if src_channels == 0 { return None }
    let pluck = |l: usize, r: usize| -> Vec<f64> {
        let (l, r) = (if l >= src_channels { 0.min(src_channels - 1) } else { l }, if r >= src_channels { 1.min(src_channels - 1) } else { r });
        let mut m = vec![0.0; 2 * src_channels]; m[l] = 1.0; m[src_channels + r] = 1.0; m
    };
    match mode {
        RoutingMode::Downmix5_1 => { if src_channels < 6 { return None } let mut m = vec![0.0; 2 * src_channels]; m[0] = 1.0; m[2] = 0.707; m[4] = 0.707; m[src_channels + 1] = 1.0; m[src_channels + 2] = 0.707; m[src_channels + 5] = 0.707; Some(m) }
        RoutingMode::Stereo7_8 => if src_channels >= 8 { Some(pluck(6, 7)) } else { None },
        RoutingMode::Auto => { if matches!(src_channels, 1 | 2 | 6) { return None } if src_channels >= 8 { Some(pluck(6, 7)) } else { Some(pluck(0, 1)) } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_three_rules() {
        assert!(routing_matrix(RoutingMode::Auto, 2).is_none() && routing_matrix(RoutingMode::Auto, 6).is_none());
        assert_eq!(routing_matrix(RoutingMode::Auto, 4).unwrap(), vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let m = routing_matrix(RoutingMode::Auto, 8).unwrap(); assert_eq!((m[6], m[8 + 7]), (1.0, 1.0));
        let m = routing_matrix(RoutingMode::Downmix5_1, 6).unwrap(); assert_eq!((m[0], m[2], m[4], m[7], m[8], m[11]), (1.0, 0.707, 0.707, 1.0, 0.707, 0.707));
        assert!(routing_matrix(RoutingMode::Stereo7_8, 6).is_none());
    }
}
