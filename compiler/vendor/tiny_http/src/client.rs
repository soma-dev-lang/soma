use ascii::AsciiString;

use std::io::Error as IoError;
use std::io::Result as IoResult;
use std::io::{BufReader, BufWriter, ErrorKind, Read};

use std::net::SocketAddr;
use std::str::FromStr;

use crate::common::{HTTPVersion, Method};
use crate::util::RefinedTcpStream;
use crate::util::{SequentialReader, SequentialReaderBuilder, SequentialWriterBuilder};
use crate::Request;

/// A ClientConnection is an object that will store a socket to a client
/// and return Request objects.
pub struct ClientConnection {
    // address of the client
    remote_addr: IoResult<Option<SocketAddr>>,

    // sequence of Readers to the stream, so that the data is not read in
    //  the wrong order
    source: SequentialReaderBuilder<BufReader<RefinedTcpStream>>,

    // sequence of Writers to the stream, to avoid writing response #2 before
    //  response #1
    sink: SequentialWriterBuilder<BufWriter<RefinedTcpStream>>,

    // Reader to read the next header from
    next_header_source: SequentialReader<BufReader<RefinedTcpStream>>,

    // set to true if we know that the previous request is the last one
    no_more_requests: bool,

    // true if the connection goes through SSL
    secure: bool,

    // soma patch: shuts the socket down (Request::respond_and_close)
    closer: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,

    // soma patch: set by the closer — no request is parsed after it (bytes
    // already buffered behind a refused request are never a next request)
    closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// soma patch: request-line / header-section limits (431 past them)
const MAX_LINE: usize = 16 * 1024;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_HEADERS: usize = 200;

/// Error that can happen when reading a request.
#[derive(Debug)]
enum ReadError {
    WrongRequestLine,
    WrongHeader(HTTPVersion),
    /// soma patch: a request line or header line past MAX_LINE, a header
    /// section past MAX_HEADER_BYTES or MAX_HEADERS (431, connection closed)
    TooLarge,
    /// the client sent an unrecognized `Expect` header
    ExpectationFailed(HTTPVersion),
    /// soma patch: a version other than HTTP/1.0 or HTTP/1.1 (505, closed)
    VersionNotSupported,
    ReadIoError(IoError),
}

impl ClientConnection {
    /// Creates a new `ClientConnection` that takes ownership of the `TcpStream`.
    pub fn new(
        write_socket: RefinedTcpStream,
        mut read_socket: RefinedTcpStream,
    ) -> ClientConnection {
        let remote_addr = read_socket.peer_addr();
        let secure = read_socket.secure();
        let closed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let closer = read_socket.closer().map(|c| {
            let flag = closed.clone();
            std::sync::Arc::new(move || {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                c();
            }) as std::sync::Arc<dyn Fn() + Send + Sync>
        });

        let mut source = SequentialReaderBuilder::new(BufReader::with_capacity(1024, read_socket));
        let first_header = source.next().unwrap();

        ClientConnection {
            source,
            sink: SequentialWriterBuilder::new(BufWriter::with_capacity(1024, write_socket)),
            remote_addr,
            next_header_source: first_header,
            no_more_requests: false,
            secure,
            closer,
            closed,
        }
    }

    /// true if the connection is HTTPS
    pub fn secure(&self) -> bool {
        self.secure
    }

    /// Reads the next line from self.next_header_source.
    ///
    /// Reads until `CRLF` is reached. The next read will start
    ///  at the first byte of the new line.
    ///
    /// soma patch: a bare CR, a bare LF or a NUL inside the line is an
    /// `InvalidData` error (a bare LF hid a second header in one line), and
    /// a line past `limit` bytes is a `WriteZero`-kind error (431).
    fn read_next_line(&mut self, limit: usize) -> IoResult<AsciiString> {
        let mut buf = Vec::new();
        let mut prev_byte_was_cr = false;

        loop {
            let byte = self.next_header_source.by_ref().bytes().next();

            let byte = match byte {
                Some(b) => b?,
                None => return Err(IoError::new(ErrorKind::ConnectionAborted, "Unexpected EOF")),
            };

            if byte == b'\n' && prev_byte_was_cr {
                buf.pop(); // removing the '\r'
                return AsciiString::from_ascii(buf)
                    .map_err(|_| IoError::new(ErrorKind::InvalidData, "Header is not in ASCII"));
            }
            if prev_byte_was_cr || byte == b'\n' || byte == 0 {
                return Err(IoError::new(ErrorKind::InvalidData, "a bare CR, LF or NUL in a request line or header"));
            }

            prev_byte_was_cr = byte == b'\r';

            buf.push(byte);
            if buf.len() > limit + 1 {
                return Err(IoError::new(ErrorKind::WriteZero, "a request line or header line is too long"));
            }
        }
    }

    /// Reads a request from the stream.
    /// Blocks until the header has been read.
    fn read(&mut self) -> Result<Request, ReadError> {
        let (method, path, version, headers) = {
            let line_err = |e: IoError, ver: Option<HTTPVersion>| match e.kind() {
                ErrorKind::WriteZero => ReadError::TooLarge,
                ErrorKind::InvalidData => match ver {
                    Some(v) => ReadError::WrongHeader(v),
                    None => ReadError::WrongRequestLine,
                },
                _ => ReadError::ReadIoError(e),
            };
            // reading the request line
            let (method, mut path, version) = {
                let line = self.read_next_line(MAX_LINE).map_err(|e| line_err(e, None))?;

                parse_request_line(line.as_str())?
            };
            // soma patch: HTTP/0.9, 2.0, 3.0 — refused before the headers.
            // The 505 was written while the request (and its writer) was
            // still alive: the connection's writers are sequential, so the
            // 505 waited for that request forever — no answer, a thread and
            // a socket held (and HTTP/0.9 was served as a keep-alive 1.x)
            if version != HTTPVersion(1, 0) && version != HTTPVersion(1, 1) {
                return Err(ReadError::VersionNotSupported);
            }

            // getting all headers
            let mut headers = {
                let mut headers: Vec<crate::Header> = Vec::new();
                let mut total = 0usize;
                loop {
                    let line = self.read_next_line(MAX_LINE).map_err(|e| line_err(e, Some(version.clone())))?;

                    if line.is_empty() {
                        break;
                    };
                    total += line.len() + 2;
                    if total > MAX_HEADER_BYTES || headers.len() >= MAX_HEADERS {
                        return Err(ReadError::TooLarge);
                    }
                    // soma patch: an obs-fold line (leading SP / HTAB) was
                    // trimmed into a header of its own — refused (RFC 9112
                    // §5.2); and a field name is a non-empty token
                    let text = line.as_str();
                    if text.starts_with(' ') || text.starts_with('\t') {
                        return Err(ReadError::WrongHeader(version));
                    }
                    let name_ok = text.find(':').map_or(false, |i| {
                        i > 0 && text[..i].bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
                    });
                    if !name_ok {
                        return Err(ReadError::WrongHeader(version));
                    }
                    headers.push(match FromStr::from_str(text) {
                        Ok(h) => h,
                        _ => return Err(ReadError::WrongHeader(version)),
                    });
                }

                headers
            };

            // soma patch: absolute-form (`GET http://host/x HTTP/1.1`, RFC
            // 9112 §3.2.2) — the target's path is routed and its authority
            // replaces any Host header
            if let Some((authority, origin)) = split_absolute_form(&path) {
                headers.retain(|h| !h.field.equiv("Host"));
                if let Ok(h) = crate::Header::from_bytes(&b"Host"[..], authority.as_bytes()) {
                    headers.push(h);
                }
                path = origin;
            }

            (method, path, version, headers)
        };

        // building the writer for the request
        let writer = self.sink.next().unwrap();

        // follow-up for next potential request
        let mut data_source = self.source.next().unwrap();
        std::mem::swap(&mut self.next_header_source, &mut data_source);

        // building the next reader
        let request = crate::request::new_request(
            self.secure,
            method,
            path,
            version.clone(),
            headers,
            // soma patch: a client that reset the connection before it was
            // accepted has no peer address (ENOTCONN) — its already-buffered
            // request panicked here, and the panic ended `soma serve`
            self.remote_addr.as_ref().ok().copied().flatten(),
            data_source,
            writer,
        )
        .map_err(|e| {
            use crate::request;
            match e {
                request::RequestCreationError::CreationIoError(e) => ReadError::ReadIoError(e),
                request::RequestCreationError::ExpectationFailed => {
                    ReadError::ExpectationFailed(version)
                }
            }
        })?;

        // return the request
        Ok(request.with_closer(self.closer.clone()))
    }
}

impl Iterator for ClientConnection {
    type Item = Request;

    /// Blocks until the next Request is available.
    /// Returns None when no new Requests will come from the client.
    fn next(&mut self) -> Option<Request> {
        use crate::{Response, StatusCode};

        // the client sent a "connection: close" header in this previous request
        //  or is using HTTP 1.0, meaning that no new request will come
        if self.no_more_requests || self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return None;
        }

        loop {
            let rq = match self.read() {
                Err(ReadError::WrongRequestLine) => {
                    let writer = self.sink.next().unwrap();
                    let response = Response::new_empty(StatusCode(400)).with_header(acao());
                    response
                        .raw_print(writer, HTTPVersion(1, 1), &[], false, None)
                        .ok();
                    return None; // we don't know where the next request would start,
                                 // se we have to close
                }

                Err(ReadError::TooLarge) => {
                    let writer = self.sink.next().unwrap();
                    let response = Response::new_empty(StatusCode(431)).with_header(acao());
                    response
                        .raw_print(writer, HTTPVersion(1, 1), &[], false, None)
                        .ok();
                    return None; // closing the connection
                }

                Err(ReadError::WrongHeader(ver)) => {
                    let writer = self.sink.next().unwrap();
                    let response = Response::new_empty(StatusCode(400)).with_header(acao());
                    response.raw_print(writer, ver, &[], false, None).ok();
                    return None; // we don't know where the next request would start,
                                 // se we have to close
                }

                Err(ReadError::ReadIoError(ref err)) if err.kind() == ErrorKind::TimedOut => {
                    // request timeout
                    let writer = self.sink.next().unwrap();
                    let response = Response::new_empty(StatusCode(408)).with_header(acao());
                    response
                        .raw_print(writer, HTTPVersion(1, 1), &[], false, None)
                        .ok();
                    return None; // closing the connection
                }

                Err(ReadError::ExpectationFailed(ver)) => {
                    let writer = self.sink.next().unwrap();
                    let response = Response::new_empty(StatusCode(417)).with_header(acao());
                    response.raw_print(writer, ver, &[], true, None).ok();
                    return None; // TODO: should be recoverable, but needs handling in case of body
                }

                Err(ReadError::VersionNotSupported) => {
                    let writer = self.sink.next().unwrap();
                    let response = Response::from_string(
                        "This server only supports HTTP versions 1.0 and 1.1".to_owned(),
                    )
                    .with_status_code(StatusCode(505)).with_header(acao());
                    response
                        .raw_print(writer, HTTPVersion(1, 1), &[], false, None)
                        .ok();
                    return None; // closing the connection
                }

                Err(ReadError::ReadIoError(_)) => return None,

                Ok(rq) => {
                    // soma patch: the previous request closed the connection
                    // while this one was parsed from already-buffered bytes
                    if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
                        drop(rq);
                        return None;
                    }
                    rq
                }
            };

            // updating the status of the connection
            let connection_header = rq
                .headers()
                .iter()
                .find(|h| h.field.equiv("Connection"))
                .map(|h| h.value.as_str());

            let lowercase = connection_header.map(|h| h.to_ascii_lowercase());

            match lowercase {
                Some(ref val) if val.contains("close") => self.no_more_requests = true,
                Some(ref val) if val.contains("upgrade") => self.no_more_requests = true,
                Some(ref val)
                    if !val.contains("keep-alive") && *rq.http_version() == HTTPVersion(1, 0) =>
                {
                    self.no_more_requests = true
                }
                None if *rq.http_version() == HTTPVersion(1, 0) => self.no_more_requests = true,
                _ => (),
            };

            // returning the request
            return Some(rq);
        }
    }
}

/// soma patch: every answer soma serve gives carries
/// `Access-Control-Allow-Origin: *` (the documented rule) — the refusals made
/// here too (a 431 for a browser's oversized cookies was unreadable to a
/// page on another origin)
fn acao() -> crate::Header {
    crate::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap()
}

/// Parses a "HTTP/1.1" string.
fn parse_http_version(version: &str) -> Result<HTTPVersion, ReadError> {
    let (major, minor) = match version {
        "HTTP/0.9" => (0, 9),
        "HTTP/1.0" => (1, 0),
        "HTTP/1.1" => (1, 1),
        "HTTP/2.0" => (2, 0),
        "HTTP/3.0" => (3, 0),
        _ => return Err(ReadError::WrongRequestLine),
    };

    Ok(HTTPVersion(major, minor))
}

/// Parses the request line of the request.
/// eg. GET / HTTP/1.1
///
/// soma patch: exactly `METHOD SP target SP version` — junk after the
/// version, doubled spaces or a leading space are refused (400).
fn parse_request_line(line: &str) -> Result<(Method, String, HTTPVersion), ReadError> {
    let mut parts = line.split(' ');

    let method = parts.next().filter(|w| !w.is_empty()).and_then(|w| w.parse().ok());
    let path = parts.next().filter(|w| !w.is_empty()).map(ToOwned::to_owned);
    let version = parts.next().and_then(|w| parse_http_version(w).ok());
    if parts.next().is_some() {
        return Err(ReadError::WrongRequestLine);
    }

    method
        .and_then(|method| Some((method, path?, version?)))
        .ok_or(ReadError::WrongRequestLine)
}

/// soma patch: `http://host:port/p?q` → (`host:port`, `/p?q`)
fn split_absolute_form(target: &str) -> Option<(String, String)> {
    let lower = target.get(..8).unwrap_or(target).to_ascii_lowercase();
    let rest = if lower.starts_with("http://") {
        &target[7..]
    } else if lower.starts_with("https://") {
        &target[8..]
    } else {
        return None;
    };
    let end = rest.find(|c| c == '/' || c == '?').unwrap_or(rest.len());
    let authority = &rest[..end];
    let mut origin = rest[end..].to_string();
    if !origin.starts_with('/') {
        origin.insert(0, '/');
    }
    Some((authority.to_string(), origin))
}

#[cfg(test)]
mod test {
    #[test]
    fn test_parse_request_line() {
        let (method, path, ver) = super::parse_request_line("GET /hello HTTP/1.1").unwrap();

        assert!(method == crate::Method::Get);
        assert!(path == "/hello");
        assert!(ver == crate::common::HTTPVersion(1, 1));

        assert!(super::parse_request_line("GET /hello").is_err());
        assert!(super::parse_request_line("qsd qsd qsd").is_err());
        assert!(super::parse_request_line("GET /hello HTTP/1.1 junk").is_err());
        assert!(super::parse_request_line("GET  /hello HTTP/1.1").is_err());
        assert_eq!(
            super::split_absolute_form("http://localhost:80/x?y=1"),
            Some(("localhost:80".to_string(), "/x?y=1".to_string()))
        );
        assert_eq!(super::split_absolute_form("/x"), None);
    }
}
