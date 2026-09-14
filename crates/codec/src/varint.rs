//! LEB128 varints, zigzag for signed values.

pub fn put_u(buf: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        buf.push((v as u8) | 0x80);
        v >>= 7;
    }
    buf.push(v as u8);
}

pub fn put_i(buf: &mut Vec<u8>, v: i64) {
    put_u(buf, ((v << 1) ^ (v >> 63)) as u64);
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let out = self.bytes.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(out)
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    pub fn u32_le(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    pub fn i64_le(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    pub fn u(&mut self) -> Option<u64> {
        let mut out = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            out |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Some(out);
            }
        }
        None
    }

    pub fn i(&mut self) -> Option<i64> {
        let u = self.u()?;
        Some(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    pub fn is_empty(&self) -> bool {
        self.pos == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_extremes() {
        let values = [0i64, 1, -1, 63, -64, 64, 1 << 40, -(1 << 40), i64::MAX, i64::MIN];
        let mut buf = Vec::new();
        for v in values {
            put_i(&mut buf, v);
            put_u(&mut buf, v as u64);
        }
        let mut r = Reader::new(&buf);
        for v in values {
            assert_eq!(r.i(), Some(v));
            assert_eq!(r.u(), Some(v as u64));
        }
        assert!(r.is_empty());
        assert_eq!(Reader::new(&[0x80]).u(), None, "a truncated varint is an error, not a zero");
    }
}
