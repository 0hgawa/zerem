//! NOT UPSTREAM. A UDP socket that outlives the peers it talks to.
//!
//! A datagram sent to a host that is gone draws an ICMP error back. The
//! operating system cannot deliver that to a connectionless socket in any
//! meaningful way — by the time it arrives the socket may be talking to
//! somebody else entirely — so it queues it and hands it to whoever calls
//! `recv_from` next, as a failure of *that* call.
//!
//! It is a report about a datagram already sent, wearing the costume of a
//! broken socket. Callers reasonably believe the costume:
//!
//! ```text
//! INFO  DHT listening on [::]:58485
//! ERROR dht: dht finished with error: framer failed: Recv(Os { code: 10054 })
//! ```
//!
//! Two milliseconds of DHT, and every lookup for the rest of the session
//! answering `dht is dead`. A magnet whose trackers are down then has nowhere
//! left to ask and never fills its file list.
//!
//! There are two halves to being right about this, and this module is both.
//!
//! **Stop Windows reporting what it should not.** `SIO_UDP_CONNRESET` and
//! `SIO_UDP_NETRESET` are the switches, and every serious Windows program that
//! holds a UDP socket clears them. libtorrent does it in `udp_socket`, which is
//! the same layer as this one, and for the same reason: it is the only place
//! that knows the socket is connectionless.
//!
//! **Survive the report anyway.** The ioctls are Windows-only and the problem
//! is not: Linux delivers the same news as `ECONNREFUSED`, and macOS as
//! `EHOSTUNREACH`. So the read below skips these and reads again, which is what
//! makes the DHT proof against one dead peer on every platform rather than one.
//!
//! Skipping is bounded and cannot spin. Each of these errors *is* one queued
//! report, consumed by the call that returns it, and tokio clears the socket's
//! readiness along with it — so a second read waits for a real event. Nothing
//! here skips a condition, only an event: `NetworkDown` is a state the socket is
//! in, and goes to the caller untouched.

use std::io::ErrorKind;

/// Ask the operating system to stop reporting other hosts' problems as this
/// socket's.
///
/// Windows only. Nothing else invents these, and the read path is what covers
/// the ones that are genuinely delivered.
#[cfg(windows)]
pub(crate) fn quiet_stale_reports(socket: &socket2::Socket) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        SIO_UDP_CONNRESET, SIO_UDP_NETRESET, SOCKET_ERROR, WSAIoctl,
    };

    // Both, as libtorrent does: CONNRESET is the unreachable port, NETRESET the
    // expired hop. Same fiction, two codes, and a client that clears only the
    // famous one still dies on the other.
    for code in [SIO_UDP_CONNRESET, SIO_UDP_NETRESET] {
        let mut off: u32 = 0;
        let mut returned: u32 = 0;
        // SAFETY: the handle outlives the call, `off` is a live `u32` of the
        // width named after it, and the output buffer is the null/zero pair the
        // ioctl documents for a control code that returns nothing.
        let result = unsafe {
            WSAIoctl(
                socket.as_raw_socket() as _,
                code,
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
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn quiet_stale_reports(_socket: &socket2::Socket) -> std::io::Result<()> {
    Ok(())
}

/// Whether a failed read described a datagram already sent, rather than the
/// socket it was read from.
///
/// The distinction is the whole point. An event has been consumed by the call
/// that reported it, so reading again is correct; a state would still be true
/// on the next read, and the caller has to hear about it.
pub(crate) fn is_a_stale_report(error: &std::io::Error) -> bool {
    // WSAEMSGSIZE: a datagram too big for the buffer, which Windows discards
    // and then complains about. The bytes are gone either way and the next one
    // is fine. Nothing in `ErrorKind` names it.
    #[cfg(windows)]
    if error.raw_os_error() == Some(10040) {
        return true;
    }

    matches!(
        error.kind(),
        // Windows says the first, Linux the second, macOS the fourth. All of
        // them are the same ICMP, in the local dialect.
        ErrorKind::ConnectionReset
            | ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionAborted
            | ErrorKind::HostUnreachable
            | ErrorKind::NetworkUnreachable
            // A signal arrived mid-call. Nothing was read, and nothing is wrong.
            | ErrorKind::Interrupted
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_dialect_of_the_same_icmp_is_a_stale_report() {
        for kind in [
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionRefused,
            ErrorKind::ConnectionAborted,
            ErrorKind::HostUnreachable,
            ErrorKind::NetworkUnreachable,
            ErrorKind::Interrupted,
        ] {
            assert!(
                is_a_stale_report(&std::io::Error::new(kind, "somebody else")),
                "{kind:?} should not end a socket"
            );
        }
    }

    #[test]
    fn the_state_of_the_socket_reaches_the_caller() {
        // NetworkDown is still true on the next read, so skipping it would spin
        // rather than report. Everything unrecognised is a state by default.
        for kind in [
            ErrorKind::NetworkDown,
            ErrorKind::AddrNotAvailable,
            ErrorKind::PermissionDenied,
            ErrorKind::NotConnected,
        ] {
            assert!(
                !is_a_stale_report(&std::io::Error::new(kind, "this socket")),
                "{kind:?} is the caller's to hear"
            );
        }
    }

    /// The bug itself, reproduced: send to a port nobody is listening on, then
    /// read. Windows queues the ICMP against the sending socket and fails that
    /// read. With the ioctls cleared there is nothing to fail with, so the read
    /// simply waits — and the timeout below is what passing looks like.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_closed_port_does_not_come_back_as_a_read_error() {
        use crate::{BindOpts, socket::MaybeDualstackSocket};
        use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

        let here = || SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
        let socket =
            MaybeDualstackSocket::<tokio::net::UdpSocket>::bind_udp(here(), BindOpts::default()).unwrap();

        // Bound and then dropped, so the port is certainly ours and certainly
        // shut — which is what draws the ICMP back.
        let shut =
            MaybeDualstackSocket::<tokio::net::UdpSocket>::bind_udp(here(), BindOpts::default()).unwrap();
        let shut_addr = shut.bind_addr();
        drop(shut);

        socket.send_to(b"anyone there", shut_addr).await.unwrap();

        let mut buf = [0u8; 64];
        let read =
            tokio::time::timeout(std::time::Duration::from_millis(500), socket.recv_from(&mut buf)).await;

        match read {
            Err(_elapsed) => {} // Nothing arrived, which is the whole point.
            Ok(Ok((size, from))) => panic!("read {size} bytes from {from}, expected silence"),
            Ok(Err(error)) => panic!("a closed port ended the socket: {error}"),
        }
    }
}
