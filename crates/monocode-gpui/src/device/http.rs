//! A GET whose endless body is read on a thread and can be cut off from
//! another: ureq offers no way to close a stream mid-read, and the device
//! panel must let go of the hub the moment it is hidden.
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpStream},
    time::Duration,
};

/// Closes the connection from another thread; the reader then sees EOF.
#[derive(Clone)]
pub(super) struct Closer(std::sync::Arc<TcpStream>);
impl Closer {
    pub(super) fn close(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
    pub(super) fn same(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

pub(super) struct Body {
    reader: Box<dyn Read + Send>,
}
impl Read for Body {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buf)
    }
}

/// Sends the request and reads the head. An error status comes back as
/// `Err` with the hub's message.
pub(super) fn get(
    port: u16,
    path: &str,
    authorization: &str,
    read_timeout: Duration,
) -> Result<(Body, Closer), String> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    stream.set_nodelay(true).ok();
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|e| e.to_string())?;
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let closer = Closer(std::sync::Arc::new(
        stream.try_clone().map_err(|e| e.to_string())?,
    ));
    write!(
        writer,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {authorization}\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    let mut reader = BufReader::with_capacity(256 * 1024, stream);
    let mut status = String::new();
    reader.read_line(&mut status).map_err(|e| e.to_string())?;
    let code: u16 = status
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or("The Device Hub sent an invalid response")?;
    let mut chunked = false;
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("The Device Hub closed the stream".into());
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            if name.eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            } else if name.eq_ignore_ascii_case("content-length") {
                length = value.parse::<u64>().ok();
            }
        }
    }
    let body: Box<dyn Read + Send> = if chunked {
        Box::new(Chunked {
            inner: reader,
            left: 0,
            done: false,
        })
    } else if let Some(length) = length {
        Box::new(reader.take(length))
    } else {
        Box::new(reader)
    };
    let mut body = Body { reader: body };
    if !(200..300).contains(&code) {
        let mut text = String::new();
        let _ = (&mut body).take(4096).read_to_string(&mut text);
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|value| value["error"].as_str().map(str::to_owned))
            .unwrap_or(text);
        return Err(format!("{code}: {}", message.trim()));
    }
    Ok((body, closer))
}

/// HTTP/1.1 chunked transfer coding.
struct Chunked<R> {
    inner: R,
    left: usize,
    done: bool,
}

impl<R: BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            let mut line = String::new();
            if self.inner.read_line(&mut line)? == 0 {
                self.done = true;
                return Ok(0);
            }
            // The CRLF that ends the previous chunk.
            if line.trim().is_empty() {
                line.clear();
                if self.inner.read_line(&mut line)? == 0 {
                    self.done = true;
                    return Ok(0);
                }
            }
            let size = line.trim().split(';').next().unwrap_or("");
            self.left = usize::from_str_radix(size, 16).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk size")
            })?;
            if self.left == 0 {
                self.done = true;
                return Ok(0);
            }
        }
        let wanted = buf.len().min(self.left);
        let read = self.inner.read(&mut buf[..wanted])?;
        if read == 0 {
            self.done = true;
        }
        self.left -= read;
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_bodies_are_reassembled() {
        let wire = b"4\r\nWiki\r\n6;ext=1\r\npedia \r\nE\r\nin \r\n\r\nchunks.\r\n0\r\n\r\n";
        let mut body = Chunked {
            inner: BufReader::new(&wire[..]),
            left: 0,
            done: false,
        };
        let mut out = String::new();
        // Small reads cross chunk boundaries the way socket reads do.
        let mut buf = [0u8; 3];
        loop {
            let n = body.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            out.push_str(std::str::from_utf8(&buf[..n]).unwrap());
        }
        assert_eq!(out, "Wikipedia in \r\n\r\nchunks.");
    }
}
