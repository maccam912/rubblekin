//! One wire format, one socket. Rendering never blocks on a socket operation.
use bevy::prelude::Resource;
use rubblekin_core::protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage, SessionMode};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    net::{Shutdown, TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

const MAX_SERVER_FRAME: usize = 16 * 1024 * 1024;
const MAX_OUTBOX: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionStage {
    ResolvingAddress,
    Connecting,
    AwaitingWelcome,
}

#[derive(Resource)]
pub struct Connection {
    stream: TcpStream,
    incoming: Vec<u8>,
    outgoing: VecDeque<Vec<u8>>,
    written: usize,
    deferred: VecDeque<ServerMessage>,
    pub error: Option<String>,
}

impl Connection {
    #[cfg(test)]
    pub fn connect(
        address: &str,
        name: String,
        mode: SessionMode,
    ) -> io::Result<(Self, ServerMessage)> {
        Self::connect_with_progress(address, name, mode, |_| Ok(()))
    }

    /// The callback runs on the connection worker at each real stage and while
    /// awaiting a welcome. Returning an error cancels and closes the socket.
    pub(crate) fn connect_with_progress(
        address: &str,
        name: String,
        mode: SessionMode,
        mut progress: impl FnMut(ConnectionStage) -> io::Result<()>,
    ) -> io::Result<(Self, ServerMessage)> {
        progress(ConnectionStage::ResolvingAddress)?;
        // A hostname can resolve to both IPv6 and IPv4; try alternatives if the
        // first family is unavailable instead of rejecting a reachable server.
        let addresses: Vec<_> = address.to_socket_addrs()?.take(8).collect();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut last_error = io::Error::other("Address did not resolve");
        let mut connected = None;
        for address in addresses {
            progress(ConnectionStage::Connecting)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match TcpStream::connect_timeout(&address, remaining.min(Duration::from_secs(2))) {
                Ok(stream) => {
                    connected = Some(stream);
                    break;
                }
                Err(error) => last_error = error,
            }
        }
        let stream = connected.ok_or(last_error)?;
        stream.set_nodelay(true)?;
        stream.set_nonblocking(true)?;
        let mut connection = Self {
            stream,
            incoming: Vec::new(),
            outgoing: VecDeque::new(),
            written: 0,
            deferred: VecDeque::new(),
            error: None,
        };
        connection.send(ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            name,
            mode,
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            progress(ConnectionStage::AwaitingWelcome)?;
            let mut messages = connection.poll().into_iter();
            while let Some(message) = messages.next() {
                match message {
                    welcome @ ServerMessage::Welcome { .. } => {
                        connection.deferred.extend(messages);
                        return Ok((connection, welcome));
                    }
                    ServerMessage::Notice { text } => return Err(io::Error::other(text)),
                    _ => {}
                }
            }
            if let Some(error) = &connection.error {
                return Err(io::Error::other(error.clone()));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Server did not complete the handshake",
        ))
    }

    pub fn fail(&mut self, message: String) {
        self.error = Some(message);
        let _ = self.stream.shutdown(Shutdown::Both);
    }

    pub fn send(&mut self, message: ClientMessage) {
        if self.error.is_some() {
            return;
        }
        if self.outgoing.len() >= MAX_OUTBOX {
            self.fail("Connection is too slow; reconnect to resynchronize".into());
            return;
        }
        match serde_json::to_vec(&message) {
            Ok(mut bytes) => {
                bytes.push(b'\n');
                self.outgoing.push_back(bytes);
            }
            Err(error) => self.fail(error.to_string()),
        }
        // Input changes (including release) leave in the frame that predicted
        // them, rather than waiting for the next receive_network call.
        if self.error.is_none()
            && let Err(error) = self.flush_outgoing()
        {
            self.fail(error.to_string());
        }
    }

    pub fn poll(&mut self) -> Vec<ServerMessage> {
        if !self.deferred.is_empty() {
            return self.deferred.drain(..).collect();
        }
        if self.error.is_some() {
            return Vec::new();
        }
        let result = self.poll_inner();
        match result {
            Ok(messages) => messages,
            Err(error) => {
                self.fail(error.to_string());
                Vec::new()
            }
        }
    }

    fn flush_outgoing(&mut self) -> io::Result<()> {
        while let Some(bytes) = self.outgoing.front() {
            match self.stream.write(&bytes[self.written..]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Server disconnected",
                    ));
                }
                Ok(count) => {
                    self.written += count;
                    if self.written == bytes.len() {
                        self.outgoing.pop_front();
                        self.written = 0;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn poll_inner(&mut self) -> io::Result<Vec<ServerMessage>> {
        self.flush_outgoing()?;
        let mut buffer = [0; 16384];
        let mut closed = false;
        for _ in 0..128 {
            match self.stream.read(&mut buffer) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Ok(count) => {
                    self.incoming.extend_from_slice(&buffer[..count]);
                    if self.incoming.len() > MAX_SERVER_FRAME {
                        return Err(io::Error::other("Server message exceeded the size limit"));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        let mut messages = Vec::new();
        let mut consumed = 0;
        for line in self.incoming.split_inclusive(|byte| *byte == b'\n') {
            if line.last() != Some(&b'\n') {
                break;
            }
            messages.push(serde_json::from_slice(line).map_err(io::Error::other)?);
            consumed += line.len();
        }
        self.incoming.drain(..consumed);
        if closed {
            self.fail(
                "Server disconnected. Accepted changes are saved; reconnect to continue.".into(),
            );
        }
        Ok(messages)
    }
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
