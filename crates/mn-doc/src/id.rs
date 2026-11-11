//! Block ids: ULIDs — 48 bits of milliseconds, 80 random bits, 26 Crockford base-32 characters, which sort as text in
//! creation order. The `.mnd` contract (survey §5.1); an id read from a file is kept as written.
use std::{fmt, sync::Mutex, time::{SystemTime, UNIX_EPOCH}};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(u128);

static LAST: Mutex<u128> = Mutex::new(0);

impl BlockId {
    /// A new id. Ids made in the same millisecond by this process still ascend (the last one + 1).
    pub fn new() -> Self {
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0) & 0xFFFF_FFFF_FFFF;
        let random = uuid::Uuid::new_v4().as_u128() & ((1u128 << 80) - 1);
        let mut last = LAST.lock().unwrap();
        let mut v = (ms << 80) | random;
        if v <= *last && (*last >> 80) >= ms { v = *last + 1; }
        *last = v;
        BlockId(v)
    }
    pub fn from_u128(v: u128) -> Self { BlockId(v) }
    /// An id made from this one, the same every time: the parts a conversion has to invent (a table around its first
    /// row, the cells of a row) get ids that a second conversion of the same file gives again.
    pub fn derived(self, n: u128) -> Self { BlockId(self.0.wrapping_add(n)) }
    /// Parses the 26-character form (case-insensitive; I, L → 1 and O → 0 as Crockford reads them).
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() != 26 { return None }
        let mut v: u128 = 0;
        for (i, &c) in b.iter().enumerate() {
            let c = match c.to_ascii_uppercase() { b'I' | b'L' => b'1', b'O' => b'0', x => x };
            let d = ALPHABET.iter().position(|&a| a == c)? as u128;
            if i == 0 && d > 7 { return None }   // 130 bits of digits hold a 128-bit value
            v = (v << 5) | d;
        }
        Some(BlockId(v))
    }
}
impl Default for BlockId { fn default() -> Self { Self::new() } }
impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = [0u8; 26];
        for (i, o) in out.iter_mut().enumerate() { *o = ALPHABET[((self.0 >> (5 * (25 - i))) & 31) as usize]; }
        f.write_str(std::str::from_utf8(&out).unwrap())
    }
}
impl fmt::Debug for BlockId { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{self}") } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_id_round_trips_through_its_text() {
        let a = BlockId::new(); let s = a.to_string();
        assert_eq!(s.len(), 26);
        assert_eq!(BlockId::parse(&s), Some(a));
        assert_eq!(BlockId::parse(&s.to_lowercase()), Some(a));
        assert_eq!(BlockId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap().to_string(), "01ARZ3NDEKTSV4RRFFQ69G5FAV");
    }
    #[test]
    fn ids_ascend_and_their_text_sorts_the_same_way() {
        let ids: Vec<BlockId> = (0..2000).map(|_| BlockId::new()).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]));
        assert!(ids.windows(2).all(|w| w[0].to_string() < w[1].to_string()));
    }
    #[test]
    fn a_malformed_id_is_refused() {
        assert_eq!(BlockId::parse(""), None);
        assert_eq!(BlockId::parse("01ARZ3NDEKTSV4RRFFQ69G5FA"), None);     // 25 characters
        assert_eq!(BlockId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAU"), None);    // U is not in the alphabet
        assert_eq!(BlockId::parse("81ARZ3NDEKTSV4RRFFQ69G5FAV"), None);    // overflows 128 bits
    }
}
