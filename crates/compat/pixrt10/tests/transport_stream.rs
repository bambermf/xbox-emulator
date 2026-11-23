#[path = "../../pixrt/src/transport_stream.rs"]
mod transport_stream;

use std::io::{self, Read};
use std::ptr::null_mut;
use xbtp::link::*;

struct Link(*mut u8);

impl Drop for Link {
    fn drop(&mut self) {
        unsafe { XbtpLinkCloseHandle(self.0) }
    }
}

impl Read for Link {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut n = 0;
        let hr = unsafe { XbtpLinkRecv(self.0, out.as_mut_ptr(), out.len() as u32, &mut n, 0, null_mut()) };
        if hr < 0 { Err(io::Error::other(format!("XBTP receive returned {hr:#x}"))) } else { Ok(n as usize) }
    }
}

#[test]
fn fragmented_xpress_message_arrives_on_an_accepted_xbtp_link() {
    unsafe {
        let listener = Link(XbtpLinkCreateHandle());
        let mut ready = null_mut();
        assert_eq!(XbtpLinkListen(listener.0, null_mut(), 0x1080, &mut ready), 0);
        let sender = std::thread::spawn(|| {
            let peer = Link(XbtpLinkCreateHandle());
            let mut addr = [0u8; 16];
            addr[0] = 2;
            addr[2..4].copy_from_slice(&0x1080u16.to_be_bytes());
            addr[4..8].copy_from_slice(&[127, 0, 0, 1]);
            assert_eq!(XbtpLinkConnect(peer.0, addr.as_ptr(), 16, 5000), 0);
            let mut wire = Vec::new();
            for word in [20u32, 12, 47, 0x1234, 0, 17, 35] {
                wire.extend(word.to_le_bytes());
            }
            wire.extend([0xff, 0xff, 0xff, 0x7f, b'A', 7, 0, 15, 9]);
            for chunk in wire.chunks_mut(3) {
                let buffer = WSABUF { len: chunk.len() as u32, buf: chunk.as_mut_ptr() };
                let mut sent = 0;
                assert_eq!(XbtpLinkSend(peer.0, &buffer, 1, &mut sent, 0, null_mut()), 0);
            }
        });
        let mut accepted = null_mut();
        assert_eq!(XbtpLinkAccept(listener.0, &mut accepted, 5000), 0);
        sender.join().unwrap();
        let mut messages = transport_stream::Messages::new(Link(accepted), 64, 64);
        let message = messages.next_message().unwrap().unwrap();
        assert_eq!(message.len(), 47);
        assert_eq!(message[4..8], 0x1234u32.to_le_bytes());
        assert_eq!(message[12..], [b'A'; 35]);
        assert!(messages.next_message().unwrap().is_none());
    }
}
