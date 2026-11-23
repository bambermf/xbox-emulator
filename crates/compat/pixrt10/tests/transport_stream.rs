use pixrt10::transport_stream;

use std::io::{self, Read, Write};
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

impl Write for Link {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let buffer = WSABUF { len: bytes.len() as u32, buf: bytes.as_ptr().cast_mut() };
        let mut sent = 0;
        let hr = unsafe { XbtpLinkSend(self.0, &buffer, 1, &mut sent, 0, null_mut()) };
        if hr < 0 { Err(io::Error::other(format!("XBTP send returned {hr:#x}"))) }
        else { Ok(sent.saturating_sub(MSG_SIZE as u32) as usize) }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn version_query_round_trip_matches_the_19041_reply() {
    unsafe {
        let listener = Link(XbtpLinkCreateHandle());
        let mut ready = null_mut();
        assert_eq!(XbtpLinkListen(listener.0, null_mut(), 0x1085, &mut ready), 0);
        let client = std::thread::spawn(|| {
            let mut peer = Link(XbtpLinkCreateHandle());
            let mut addr = [0u8; 16];
            addr[0] = 2;
            addr[2..4].copy_from_slice(&0x1085u16.to_be_bytes());
            addr[4..8].copy_from_slice(&[127, 0, 0, 1]);
            assert_eq!(XbtpLinkConnect(peer.0, addr.as_ptr(), 16, 5000), 0);
            for opcode in [1200u32, 0xc000_042a, 0x4000_051a] {
                for word in [52u32, 44, 44, 0x7654_3210, 1, 0x1020_3040, opcode, 0, 0, 1, 2, 3, 4] {
                    peer.write_all(&word.to_le_bytes()).unwrap();
                }
            }
            let mut messages = transport_stream::Messages::new(peer, 64, 128);
            let unsupported = [68u32, 0x7654_3210, 0, 0x1020_3040, 0xc000_042a, 0x8000_4001, 10, 1, 2, 3, 4, 0x42a, 0, 0, 0, 0, 0];
            let unsupported: Vec<u8> = unsupported.into_iter().flat_map(u32::to_le_bytes).collect();
            assert_eq!(messages.next_message().unwrap(), Some(unsupported));
            let expected = [84u32, 0x7654_3210, 0, 0x1020_3040, 0x4000_051a, 0, 14, 1, 2, 3, 4, 1306, 0, 0x4a61_0f6c, 0, 0x31064, 0, 1, 1, 1, 1];
            let expected: Vec<u8> = expected.into_iter().flat_map(u32::to_le_bytes).collect();
            assert_eq!(messages.next_message().unwrap(), Some(expected));
        });
        let mut accepted = null_mut();
        assert_eq!(XbtpLinkAccept(listener.0, &mut accepted, 5000), 0);
        let mut messages = transport_stream::Messages::new(Link(accepted), 64, 128);
        messages.serve_19041(0).unwrap();
        drop(messages);
        client.join().unwrap();
    }
}
