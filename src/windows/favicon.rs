//! Native HTTP transport. Runs only on the dedicated favicon worker.
//! No browser process, cookies, integrated authentication or third-party service.
use super::wide;
use crate::favicon::{Response, WebUrl, resolve};
use std::{
    ffi::c_void,
    mem::size_of,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::Networking::WinHttp::*;

struct Handle(*mut c_void);
impl Handle {
    fn new(value: *mut c_void) -> Option<Self> {
        (!value.is_null()).then_some(Self(value))
    }
    fn option(&self, name: u32, value: u32) -> bool {
        unsafe {
            WinHttpSetOption(
                self.0,
                name,
                (&value as *const u32).cast(),
                size_of::<u32>() as u32,
            ) != 0
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}
pub struct Client {
    session: Handle,
    deadline: Instant,
}
impl Client {
    pub fn new() -> Option<Self> {
        let session = Handle::new(unsafe {
            WinHttpOpen(
                wide("Constellation site icons").as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                null(),
                null(),
                0,
            )
        })?;
        if unsafe { WinHttpSetTimeouts(session.0, 1500, 1500, 1500, 1500) } == 0 {
            return None;
        }
        Some(Self {
            session,
            deadline: Instant::now() + Duration::from_secs(12),
        })
    }
    pub fn get(&mut self, url: &str, limit: usize) -> Option<Response> {
        let mut url = url.to_owned();
        for _ in 0..4 {
            if Instant::now() >= self.deadline {
                return None;
            }
            let parsed = WebUrl::parse(&url)?;
            let connect = Handle::new(unsafe {
                WinHttpConnect(self.session.0, wide(&parsed.host).as_ptr(), parsed.port, 0)
            })?;
            let request = Handle::new(unsafe {
                WinHttpOpenRequest(
                    connect.0,
                    wide("GET").as_ptr(),
                    wide(&parsed.resource).as_ptr(),
                    null(),
                    null(),
                    null(),
                    if parsed.secure {
                        WINHTTP_FLAG_SECURE
                    } else {
                        0
                    },
                )
            })?;
            if !request.option(
                WINHTTP_OPTION_DISABLE_FEATURE,
                WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION,
            ) || !request.option(
                WINHTTP_OPTION_AUTOLOGON_POLICY,
                WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH,
            ) || !request.option(
                WINHTTP_OPTION_REDIRECT_POLICY,
                WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
            ) {
                return None;
            }
            if unsafe { WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) } == 0
                || unsafe { WinHttpReceiveResponse(request.0, null_mut()) } == 0
            {
                return None;
            }
            let mut status = 0u32;
            let mut length = size_of::<u32>() as u32;
            if unsafe {
                WinHttpQueryHeaders(
                    request.0,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    null(),
                    (&mut status as *mut u32).cast(),
                    &mut length,
                    null_mut(),
                )
            } == 0
            {
                return None;
            }
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let mut location = [0u16; 8192];
                let mut length = size_of_val(&location) as u32;
                if unsafe {
                    WinHttpQueryHeaders(
                        request.0,
                        WINHTTP_QUERY_LOCATION,
                        null(),
                        location.as_mut_ptr().cast(),
                        &mut length,
                        null_mut(),
                    )
                } == 0
                {
                    return None;
                }
                let end = location
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(length as usize / 2);
                url = resolve(&url, &String::from_utf16_lossy(&location[..end]))?;
                continue;
            }
            if status != 200 {
                return None;
            }
            let mut bytes = Vec::new();
            loop {
                if Instant::now() >= self.deadline {
                    return None;
                }
                let mut chunk = [0u8; 8192];
                let mut received = 0;
                if unsafe {
                    WinHttpReadData(
                        request.0,
                        chunk.as_mut_ptr().cast(),
                        chunk.len() as u32,
                        &mut received,
                    )
                } == 0
                {
                    return None;
                }
                if received == 0 {
                    return Some(Response { url, bytes });
                }
                if bytes.len() + received as usize > limit {
                    // A head prefix is sufficient for discovery. Images must
                    // arrive in full, but large HTML pages need not be buffered.
                    if limit == crate::favicon::HTML_LIMIT {
                        bytes.extend_from_slice(&chunk[..limit - bytes.len()]);
                        return Some(Response { url, bytes });
                    }
                    return None;
                }
                bytes.extend_from_slice(&chunk[..received as usize]);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    #[test]
    fn transport_follows_relative_redirects_without_cookies_and_bounds_bodies() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for n in 0..3 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut b = [0];
                    socket.read_exact(&mut b).unwrap();
                    request.push(b[0]);
                    assert!(request.len() < 16384);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(!request.to_ascii_lowercase().contains("cookie:"));
                assert!(!request.to_ascii_lowercase().contains("authorization:"));
                let response = if n == 0 {
                    assert!(request.starts_with("GET /redirect "));
                    "HTTP/1.1 302 Found\r\nLocation: /icon.png\r\nSet-Cookie: secret=x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndata"
                };
                socket.write_all(response.as_bytes()).unwrap();
            }
        });
        let mut client = Client::new().unwrap();
        let response = client.get(&format!("{origin}/redirect"), 4).unwrap();
        assert_eq!(response.bytes, b"data");
        assert_eq!(response.url, format!("{origin}/icon.png"));
        assert!(client.get(&format!("{origin}/too-big"), 3).is_none());
        server.join().unwrap();
    }
}
