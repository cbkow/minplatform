//! Order among siblings: a rank string per block, compared as plain text (the `.mnd` contract's alphabet, spec §2.2).
//! A block's place is then ONE value on that block — a new block between two others is one row written, and nothing
//! else changes. Row 284 (c): inputs are checked, ranks are rebalanced before they grow long, appending does not
//! lengthen them.
//!
//! A rank is one or more of `0-9A-Za-z` and never ends in `0` — so between any two different ranks there is always
//! another, and below any rank there is always one.
use std::fmt;

const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
fn digit(c: u8) -> Option<usize> { ALPHABET.iter().position(|&a| a == c) }

/// Past this length a caller re-spreads the siblings ([`spread`]).
pub const MAX_LEN: usize = 32;
/// Appending and prepending step a counter held in the first `COUNTER` characters, `62^2` apart, so a document written
/// top to bottom keeps short ranks with room between neighbours.
const COUNTER: usize = 6;
const STRIDE: u64 = 62 * 62;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rank(String);

#[derive(Debug, PartialEq, Eq)]
pub enum RankError { Malformed, NotInOrder }

impl Rank {
    pub fn parse(s: &str) -> Result<Rank, RankError> {
        if s.is_empty() || s.ends_with('0') || !s.bytes().all(|c| digit(c).is_some()) { return Err(RankError::Malformed) }
        Ok(Rank(s.to_string()))
    }
    pub fn as_str(&self) -> &str { &self.0 }
    pub fn len(&self) -> usize { self.0.len() }
    pub fn is_empty(&self) -> bool { false }
    /// The rank of the only block in an empty list.
    pub fn first() -> Rank { Rank("V".into()) }

    fn counter(&self) -> u64 { let b = self.0.as_bytes(); (0..COUNTER).fold(0, |n, i| n * 62 + b.get(i).and_then(|&c| digit(c)).unwrap_or(0) as u64) }
    fn from_counter(mut n: u64) -> Rank {
        let mut out = [b'0'; COUNTER];
        for o in out.iter_mut().rev() { *o = ALPHABET[(n % 62) as usize]; n /= 62; }
        Rank(std::str::from_utf8(&out).unwrap().trim_end_matches('0').to_string())
    }
    /// A rank after `self` (appending).
    pub fn after(&self) -> Rank {
        let n = self.counter() + STRIDE;
        if n < 62u64.pow(COUNTER as u32) { Rank::from_counter(n) } else { Rank::midpoint(Some(self), None) }
    }
    /// A rank before `self` (prepending).
    pub fn before(&self) -> Rank {
        let n = self.counter();
        if n > STRIDE { Rank::from_counter(n - STRIDE) } else { Rank::midpoint(None, Some(self)) }
    }
    /// A rank strictly between `a` and `b`; None is the open end. `a` must sort before `b`.
    pub fn between(a: Option<&Rank>, b: Option<&Rank>) -> Result<Rank, RankError> {
        match (a, b) {
            (None, None) => Ok(Rank::first()),
            (Some(a), None) => Ok(a.after()),
            (None, Some(b)) => Ok(b.before()),
            (Some(a), Some(b)) => if a < b { Ok(Rank::midpoint(Some(a), Some(b))) } else { Err(RankError::NotInOrder) },
        }
    }
    /// The Qt app's `rankBetween` (spec §2.2), on checked inputs: walk the digits, emit the midpoint as soon as the two
    /// are more than one apart. `a` reads as padded with the lowest digit, `b` with one past the highest.
    fn midpoint(a: Option<&Rank>, b: Option<&Rank>) -> Rank {
        let (a, b) = (a.map(|r| r.0.as_bytes()).unwrap_or(&[]), b.map(|r| r.0.as_bytes()));
        let mut out = Vec::new(); let mut bounded = b.is_some(); let mut i = 0;
        loop {
            let ca = a.get(i).and_then(|&c| digit(c)).unwrap_or(0);
            let cb = if bounded { b.unwrap().get(i).and_then(|&c| digit(c)).unwrap_or(62) } else { 62 };
            if ca + 1 < cb { out.push(ALPHABET[(ca + cb) / 2]); break }
            out.push(ALPHABET[ca]);
            if ca < cb { bounded = false; }
            i += 1;
        }
        Rank(String::from_utf8(out).unwrap())
    }
}
impl fmt::Debug for Rank { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) } }
impl fmt::Display for Rank { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) } }

/// `n` ranks in ascending order, evenly spaced and all of one short length: what a list gets when it is first written
/// in bulk, and when its ranks are rebalanced.
pub fn spread(n: usize) -> Vec<Rank> {
    // a fixed width whose LAST digit runs 1..=61 (never 0); room for four times as many
    let mut width = 2; let mut room: u128 = 62 * 61;
    while room < 4 * (n as u128 + 1) { width += 1; room *= 62; }
    (0..n).map(|i| {
        let v = (i as u128 + 1) * room / (n as u128 + 1);
        let (mut high, low) = (v / 61, (v % 61) as usize + 1);
        let mut out = vec![b'0'; width]; out[width - 1] = ALPHABET[low];
        for o in out[..width - 1].iter_mut().rev() { *o = ALPHABET[(high % 62) as usize]; high /= 62; }
        Rank(String::from_utf8(out).unwrap())
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(s: &str) -> Rank { Rank::parse(s).unwrap() }
    fn mid(a: &str, b: &str) -> String { Rank::between(Some(&r(a)), Some(&r(b))).unwrap().to_string() }

    /// The old function's table (spec §2.2) where its inputs were valid.
    #[test]
    fn between_two_ranks_matches_the_old_files_arithmetic() {
        assert_eq!((mid("F", "H"), mid("F", "G"), mid("Az", "B"), mid("A", "AV"), mid("V", "k")), ("G".into(), "FV".into(), "AzV".into(), "AF".into(), "c".into()));
        for (a, b) in [("F", "G"), ("Az", "B"), ("A", "AV"), ("A", "A1"), ("0V", "1"), ("zz", "zzV")] { let m = mid(a, b); assert!(a < m.as_str() && m.as_str() < b, "{a} < {m} < {b}"); }
    }
    /// The two answers the old function got wrong cannot be asked: a rank never ends in 0, and order is checked.
    #[test]
    fn bad_inputs_are_refused() {
        for bad in ["", "0", "A0", "a-b", "é"] { assert_eq!(Rank::parse(bad), Err(RankError::Malformed)); }
        assert_eq!(Rank::between(Some(&r("V")), Some(&r("V"))), Err(RankError::NotInOrder));
        assert_eq!(Rank::between(Some(&r("V")), Some(&r("F"))), Err(RankError::NotInOrder));
    }
    #[test]
    fn writing_top_to_bottom_or_bottom_to_top_keeps_ranks_short() {
        let mut last = Rank::first(); let mut first = Rank::first();
        for _ in 0..6000 {
            let (next, prev) = (last.after(), first.before());
            assert!(next > last && prev < first && Rank::parse(next.as_str()).is_ok() && Rank::parse(prev.as_str()).is_ok());
            last = next; first = prev;
        }
        assert!(last.len() <= COUNTER, "{last}");
        assert!(first.len() <= MAX_LEN, "{first}");       // prepending runs out of counter after a while and halves instead
        assert!(Rank::between(Some(&last), Some(&last.after())).is_ok());
    }
    #[test]
    fn spread_ranks_ascend_are_valid_and_leave_room() {
        for n in [0usize, 1, 2, 61, 62, 1000, 50_000] {
            let v = spread(n);
            assert_eq!(v.len(), n);
            assert!(v.windows(2).all(|w| w[0] < w[1]));
            assert!(v.iter().all(|x| Rank::parse(x.as_str()).is_ok() && x.len() <= 5));
            if let [a, b, ..] = v.as_slice() { let m = Rank::between(Some(a), Some(b)).unwrap(); assert!(a < &m && &m < b); }
        }
    }
    #[test]
    fn a_chain_of_inserts_at_one_place_grows_and_says_when_to_rebalance() {
        let (a, mut b) = (r("V"), r("k")); let mut n = 0;
        while b.len() <= MAX_LEN { b = Rank::between(Some(&a), Some(&b)).unwrap(); assert!(a < b); n += 1; }
        assert!(n > 100, "only {n} inserts before a rebalance");
    }
}
