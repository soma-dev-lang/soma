//! soma patch: a strict `Transfer-Encoding: chunked` decoder (RFC 9112 §7.1).
//!
//! The `chunked_transfer` 1.5 decoder it replaces had three framing holes:
//! a trailer section after the last chunk (`0\r\nX-T: 1\r\n\r\n`) was an
//! error that left the trailer bytes to be parsed as the next request; an
//! EOF inside a chunk read as the end of the body (a truncated body looked
//! complete); and chunk sizes were trimmed / `+`-prefixed leniently. Here
//! every deviation is an `InvalidData` / `UnexpectedEof` error, the error is
//! sticky, trailers are read and discarded, and the size line and trailer
//! section are bounded.

use std::io::{Error as IoError, ErrorKind, Read, Result as IoResult};

const MAX_LINE: usize = 4096;
const MAX_TRAILERS: usize = 16 * 1024;

enum State {
    Size,
    Data(u64),
    Done,
    Failed,
}

pub struct ChunkedDecoder<R> {
    source: R,
    state: State,
}

fn bad(msg: &'static str) -> IoError {
    IoError::new(ErrorKind::InvalidData, msg)
}

impl<R: Read> ChunkedDecoder<R> {
    pub fn new(source: R) -> ChunkedDecoder<R> {
        ChunkedDecoder { source, state: State::Size }
    }

    fn byte(&mut self) -> IoResult<u8> {
        let mut b = [0u8; 1];
        loop {
            match self.source.read(&mut b) {
                Ok(0) => return Err(IoError::new(ErrorKind::UnexpectedEof, "the chunked body ended early")),
                Ok(_) => return Ok(b[0]),
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn crlf(&mut self) -> IoResult<()> {
        if self.byte()? != b'\r' || self.byte()? != b'\n' {
            return Err(bad("a chunk does not end with CRLF"));
        }
        Ok(())
    }

    /// one line up to CRLF (a bare LF or CR is an error), without the CRLF
    fn line(&mut self, limit: usize) -> IoResult<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let b = self.byte()?;
            if b == b'\r' {
                if self.byte()? != b'\n' {
                    return Err(bad("a bare CR in a chunked body"));
                }
                return Ok(out);
            }
            if b == b'\n' {
                return Err(bad("a bare LF in a chunked body"));
            }
            out.push(b);
            if out.len() > limit {
                return Err(bad("a chunk-size or trailer line is too long"));
            }
        }
    }

    fn read_size(&mut self) -> IoResult<u64> {
        let line = self.line(MAX_LINE)?;
        let (size, ext) = match line.iter().position(|&b| b == b';') {
            Some(i) => (&line[..i], Some(&line[i + 1..])),
            None => (&line[..], None),
        };
        if size.is_empty() || size.len() > 15 || !size.iter().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad("a chunk size is not plain hex digits"));
        }
        if let Some(ext) = ext {
            // chunk extensions are ignored, but only visible ASCII / tab / space
            if ext.iter().any(|&b| b < 0x20 && b != b'\t' || b == 0x7f) {
                return Err(bad("a chunk extension holds a control byte"));
            }
        }
        let s = std::str::from_utf8(size).map_err(|_| bad("a chunk size is not plain hex digits"))?;
        u64::from_str_radix(s, 16).map_err(|_| bad("a chunk size is not plain hex digits"))
    }

    fn read_trailers(&mut self) -> IoResult<()> {
        let mut total = 0usize;
        loop {
            let line = self.line(MAX_LINE)?;
            if line.is_empty() {
                return Ok(());
            }
            total += line.len() + 2;
            if total > MAX_TRAILERS {
                return Err(bad("the chunked trailer section is too long"));
            }
            // a trailer field is `name: value`: a name made of token bytes
            let colon = line.iter().position(|&b| b == b':').ok_or_else(|| bad("a chunked trailer line is not a field"))?;
            if colon == 0 || !line[..colon].iter().all(|&b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)) {
                return Err(bad("a chunked trailer line is not a field"));
            }
        }
    }

    fn step(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        loop {
            match self.state {
                State::Done => return Ok(0),
                State::Failed => return Err(bad("the chunked body is malformed")),
                State::Size => {
                    let n = self.read_size()?;
                    if n == 0 {
                        self.read_trailers()?;
                        self.state = State::Done;
                        return Ok(0);
                    }
                    self.state = State::Data(n);
                }
                State::Data(left) => {
                    if buf.is_empty() {
                        return Ok(0);
                    }
                    let want = std::cmp::min(left, buf.len() as u64) as usize;
                    let got = match self.source.read(&mut buf[..want]) {
                        Ok(0) => return Err(IoError::new(ErrorKind::UnexpectedEof, "the chunked body ended early")),
                        Ok(g) => g,
                        Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                        Err(e) => return Err(e),
                    };
                    let rest = left - got as u64;
                    if rest == 0 {
                        self.crlf()?;
                        self.state = State::Size;
                    } else {
                        self.state = State::Data(rest);
                    }
                    return Ok(got);
                }
            }
        }
    }
}

impl<R: Read> Read for ChunkedDecoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        match self.step(buf) {
            Ok(n) => Ok(n),
            Err(e) => {
                self.state = State::Failed;
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::ChunkedDecoder;
    use std::io::Read;

    fn dec(s: &[u8]) -> Result<Vec<u8>, ()> {
        let mut out = Vec::new();
        ChunkedDecoder::new(s).read_to_end(&mut out).map(|_| out).map_err(|_| ())
    }

    #[test]
    fn strict() {
        assert_eq!(dec(b"3\r\nhel\r\nb\r\nlo world!!!\r\n0\r\n\r\n").unwrap(), b"hello world!!!");
        assert_eq!(dec(b"3;x=1\r\nabc\r\n0\r\nX-T: 1\r\n\r\n").unwrap(), b"abc");
        assert!(dec(b"5\r\nab").is_err());
        assert!(dec(b"3\r\nabc\r\n").is_err());
        assert!(dec(b" 3\r\nabc\r\n0\r\n\r\n").is_err());
        assert!(dec(b"+3\r\nabc\r\n0\r\n\r\n").is_err());
        assert!(dec(b"3\r\nabcX\r\n0\r\n\r\n").is_err());
        assert!(dec(b"3\nabc\r\n0\r\n\r\n").is_err());
        assert!(dec(b"0\r\nbad trailer\r\n\r\n").is_err());
    }
}
