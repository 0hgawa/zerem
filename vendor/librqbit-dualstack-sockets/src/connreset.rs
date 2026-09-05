//! NOT UPSTREAM. Stop Windows from killing a UDP socket over somebody else's
//! closed port.
//!
//! Windows reports an inbound ICMP "port unreachable" by failing the *next*
//! `recv_from` on the socket that sent the datagram, with `WSAECONNRESET`. On a
//! connectionless socket that is nonsense — nothing was connected and nothing
//! was reset — but it is decades-old behaviour and it is not going to change.
//!
//! It matters here because a DHT bootstrap fires at a table full of nodes, some
//! fraction of which are gone. One of those answers with an ICMP, the next read
//! fails, and `librqbit-dht`'s frame loop treats a read error as fatal:
//!
//! ```text
//! INFO  DHT listening on [::]:58485
//! ERROR dht: dht finished with error: framer failed: Recv(Os { code: 10054 })
//! ```
//!
//! Two milliseconds of DHT, on every launch. Everything after it says `dht is
//! dead`, and a magnet whose trackers are down or slow then has nowhere left to
//! ask — it sits on "fetching the file list" until the person gives up.
//!
//! `SIO_UDP_CONNRESET` is the switch that turns the report off, and every
//! Windows program that holds a UDP socket sets it. It is done here, at the one
//! place this crate makes UDP sockets, so the DHT, uTP, the trackers and local
//! discovery are all covered by the same three lines.

/// Ask Windows to stop reporting ICMP unreachables as read errors.
///
/// Silent everywhere else: there is nothing to fix on platforms that already
/// let a connectionless socket be connectionless.
#[cfg(windows)]
pub(crate) fn ignore_icmp_unreachable(socket: &socket2::Socket) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{SIO_UDP_CONNRESET, SOCKET_ERROR, WSAIoctl};

    let mut off: u32 = 0;
    let mut returned: u32 = 0;
    // SAFETY: the handle outlives the call, `off` is a live `u32` of the width
    // named after it, and the output buffer is the null/zero pair the ioctl
    // documents for a control code that returns nothing.
    let result = unsafe {
        WSAIoctl(
            socket.as_raw_socket() as _,
            SIO_UDP_CONNRESET,
            std::ptr::addr_of_mut!(off).cast(),
            u32::try_from(size_of::<u32>()).unwrap_or(4),
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
            None,
        )
    };

    if result == SOCKET_ERROR {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn ignore_icmp_unreachable(_socket: &socket2::Socket) -> std::io::Result<()> {
    Ok(())
}
