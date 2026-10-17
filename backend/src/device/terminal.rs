//! An interactive RouterOS console over SSH, for what the API can't do: a `/tool mac-telnet`
//! session to a device that has no IP address yet. Logs in with the [`LOGIN_FLAGS`], so the
//! output is plain text the prompts can be matched in.
use crate::job::Log;
use async_graphql::SimpleObject;
use log::debug;
use russh::{
    ChannelMsg, ChannelStream, Disconnect,
    client::{self, Handle},
    keys::PublicKeyOrCertificate,
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::{Instant, timeout, timeout_at},
};

/// Drops carriage returns and the terminal control sequences RouterOS sends (cursor moves
/// like `ESC [9999B`, cursor save/restore `ESC 7`/`ESC 8`).
fn strip_escapes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {}
            '\x1b' => {
                if chars.next() == Some('[') {
                    while chars.next_if(|c| !c.is_ascii_alphabetic()).is_some() {}
                    chars.next();
                }
            }
            c => result.push(c),
        }
    }
    result
}

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("ssh: {0}")]
    Ssh(#[from] russh::Error),
    #[error("ssh login as {0} refused")]
    LoginRefused(Box<str>),
    #[error("waited {waited:?} for {expected:?}, got: {tail}")]
    Timeout {
        expected: Box<[Box<str>]>,
        waited: Duration,
        tail: Box<str>,
    },
    #[error("connection closed, last output: {0}")]
    Closed(Box<str>),
    #[error("no forwarding to {0}:{1} (forwarding-enabled=local on the device in between?)")]
    NoForwarding(IpAddr, u16),
    #[error("upload refused at {0}: {1}")]
    Upload(&'static str, Box<str>),
    #[error("upload: no answer within {waited:?} at {step}")]
    UploadNoAnswer { step: &'static str, waited: Duration },
    #[error("upload stalled: nothing went through for {waited:?} after {sent} of {total} bytes")]
    UploadStalled {
        sent: usize,
        total: usize,
        waited: Duration,
    },
}

/// How long an upload may go without a part of the file going through (or, at the end, without
/// the device confirming it), before it counts as stalled.
const UPLOAD_STALL: Duration = Duration::from_secs(60);
/// The parts an upload sends the file in; each must go through within [`UPLOAD_STALL`].
const UPLOAD_CHUNK: usize = 64 * 1024;

/// Appended to the user name: no colors (`c`), no terminal detection (`t`), and wide enough
/// (`400w`) that the echo of a command isn't wrapped.
pub const LOGIN_FLAGS: &str = "+ct400w";

struct AcceptAnyKey;

impl client::Handler for AcceptAnyKey {
    type Error = russh::Error;

    // A device of the managed network, reached by its address from NetBox; the password is
    // what proves it's ours, not a pinned host key (factory devices make up a new one).
    async fn check_server_key(&mut self, _: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// russh's defaults, then what RouterOS 6 offers at most (`diffie-hellman-group14-sha1`,
/// `hmac-sha1`), last so anything newer still negotiates the safer choice. Devices on v6 are
/// reached so they can be updated. (6.45.1 offers `diffie-hellman-group-exchange-sha256`, with
/// a 2048 bit group, `ssh-rsa`, `aes*-ctr`, but only `hmac-sha1`/`hmac-md5`.)
fn preferred() -> russh::Preferred {
    let default = russh::Preferred::DEFAULT;
    let mut kex = default.kex.to_vec();
    // ahead of the extension pseudo-algorithms
    let at = kex
        .iter()
        .position(|k| *k == russh::kex::EXTENSION_SUPPORT_AS_CLIENT)
        .unwrap_or(kex.len());
    kex.splice(at..at, [russh::kex::DH_G14_SHA1, russh::kex::DH_G1_SHA1]);
    let mut mac = default.mac.to_vec();
    mac.push(russh::mac::HMAC_SHA1);
    russh::Preferred {
        kex: kex.into(),
        mac: mac.into(),
        ..default
    }
}

async fn open_session<S>(
    stream: S,
    login: &str,
    password: Option<&str>,
) -> Result<Handle<AcceptAnyKey>, TerminalError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    open_session_with(
        client::Config {
            inactivity_timeout: Some(Duration::from_secs(120)),
            ..Default::default()
        },
        stream,
        login,
        password,
    )
    .await
}

async fn open_session_with<S>(
    config: client::Config,
    stream: S,
    login: &str,
    password: Option<&str>,
) -> Result<Handle<AcceptAnyKey>, TerminalError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let config = Arc::new(client::Config {
        preferred: preferred(),
        // RouterOS 6 answers a group exchange with 2048 bits, below russh's default minimum
        gex: client::GexParams::new(2048, 8192, 8192).expect("valid group sizes"),
        ..config
    });
    let mut session = client::connect_stream(config, stream, AcceptAnyKey).await?;
    if !session
        .authenticate_password(login, password.unwrap_or_default())
        .await?
        .success()
    {
        return Err(TerminalError::LoginRefused(login.into()));
    }
    Ok(session)
}

/// An SSH session to a device that forwards connections to others it reaches (`direct-tcpip`,
/// `/ip ssh forwarding-enabled=local`): from the device's own address, so the far end needs no
/// route back to the provisioner.
pub struct Tunnel {
    session: Handle<AcceptAnyKey>,
}

impl Tunnel {
    /// Logs in over SSH on an established connection to the forwarding device.
    pub async fn connect_with_stream<S>(
        stream: S,
        user: &str,
        password: Option<&str>,
    ) -> Result<Tunnel, TerminalError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let config = client::Config {
            // stays open while a device behind it reboots
            inactivity_timeout: None,
            keepalive_interval: Some(Duration::from_secs(15)),
            ..Default::default()
        };
        Ok(Tunnel {
            session: open_session_with(config, stream, user, password).await?,
        })
    }

    /// A connection to `port` at `address`, made by the forwarding device.
    pub async fn open(
        &self,
        address: IpAddr,
        port: u16,
    ) -> Result<ChannelStream<client::Msg>, TerminalError> {
        // RouterOS doesn't refuse a forward with `forwarding-enabled=no`, it never answers
        let channel = tokio::time::timeout(
            Duration::from_secs(10),
            self.session.channel_open_direct_tcpip(
                address.to_string(),
                port.into(),
                "127.0.0.1",
                0,
            ),
        )
        .await
        .map_err(|_| TerminalError::NoForwarding(address, port))??;
        Ok(channel.into_stream())
    }
}

/// Copies `data` to the file `name` in the device's root directory over SCP, telling
/// `progress` the bytes sent once the device takes the file (0) and at each quarter. Every step
/// has [`UPLOAD_STALL`]: the SSH login, starting SCP and each answer end with
/// [`TerminalError::UploadNoAnswer`] (an update of berghof-stall-haus hung somewhere there,
/// right after allowing password logins), sending with [`TerminalError::UploadStalled`] when
/// nothing goes through (a slow link takes long, but keeps going).
pub async fn upload<S>(
    stream: S,
    user: &str,
    password: Option<&str>,
    name: &str,
    data: &[u8],
    progress: &(dyn Fn(usize) + Sync),
) -> Result<(), TerminalError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let session = answered("ssh login", open_session(stream, user, password)).await?;
    let mut channel = answered("scp start", async {
        let channel = session.channel_open_session().await?;
        channel.exec(true, format!("scp -t {name}")).await?;
        Ok(channel)
    })
    .await?;
    answered("scp start", acknowledge(&mut channel, "start")).await?;
    answered("scp header", async {
        channel
            .data_bytes(format!("C0644 {} {name}\n", data.len()).into_bytes())
            .await?;
        acknowledge(&mut channel, "header").await
    })
    .await?;
    progress(0);
    let total = data.len();
    let stalled = |sent| TerminalError::UploadStalled {
        sent,
        total,
        waited: UPLOAD_STALL,
    };
    let mut sent = 0;
    for chunk in data.chunks(UPLOAD_CHUNK) {
        timeout(UPLOAD_STALL, channel.data(chunk))
            .await
            .map_err(|_| stalled(sent))??;
        let before = sent;
        sent += chunk.len();
        if quarter(before, total) != quarter(sent, total) {
            progress(sent);
        }
    }
    channel.data_bytes(vec![0]).await?;
    timeout(UPLOAD_STALL, acknowledge(&mut channel, "data"))
        .await
        .map_err(|_| stalled(sent))??;
    let _ = channel.eof().await;
    let _ = session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;
    Ok(())
}

/// `work`, the upload's `step`, within [`UPLOAD_STALL`].
async fn answered<T>(
    step: &'static str,
    work: impl Future<Output = Result<T, TerminalError>>,
) -> Result<T, TerminalError> {
    timeout(UPLOAD_STALL, work)
        .await
        .map_err(|_| TerminalError::UploadNoAnswer {
            step,
            waited: UPLOAD_STALL,
        })?
}

/// How many quarters of `total` `sent` is.
fn quarter(sent: usize, total: usize) -> usize {
    (sent * 4).checked_div(total).unwrap_or(4)
}

/// Waits for the SCP sink's answer to `step`: a zero byte, or else an error message.
async fn acknowledge(
    channel: &mut russh::Channel<client::Msg>,
    step: &'static str,
) -> Result<(), TerminalError> {
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                return match data.first() {
                    Some(0) => Ok(()),
                    _ => Err(TerminalError::Upload(
                        step,
                        String::from_utf8_lossy(data.get(1..).unwrap_or_default())
                            .trim()
                            .into(),
                    )),
                };
            }
            Some(ChannelMsg::Eof | ChannelMsg::Close) | None => {
                return Err(TerminalError::Upload(step, "connection closed".into()));
            }
            Some(_) => {}
        }
    }
}

pub struct Terminal {
    session: Handle<AcceptAnyKey>,
    channel: russh::Channel<client::Msg>,
    /// output not yet consumed by [`Terminal::expect`]
    buffer: String,
    /// output before the last match of [`Terminal::expect`]
    consumed: String,
    /// everything that happened, for the operator; secrets replaced by `***`
    transcript: String,
    secrets: Vec<Box<str>>,
    /// where the transcript goes line by line as it comes ([`Terminal::stream_to`]), and the
    /// start of a line not complete yet
    stream: Option<(Log, String)>,
    /// blank lines just streamed
    blank: usize,
    /// [`Terminal::sync`]s so far, which tells their output apart
    syncs: u32,
}

/// Output of a console session, a few lines at a time; secrets replaced by `***`.
#[derive(Debug, Clone, PartialEq, SimpleObject)]
pub struct ConsoleOutput {
    pub text: Box<str>,
}

impl Terminal {
    /// Logs in over SSH at `address` (normally port 22).
    pub async fn connect(
        address: SocketAddr,
        user: &str,
        password: Option<&str>,
    ) -> Result<Terminal, TerminalError> {
        let stream = TcpStream::connect(address)
            .await
            .map_err(russh::Error::from)?;
        Self::connect_with_stream(stream, user, password).await
    }

    /// Logs in over SSH on an established connection, e.g. one through the management proxy.
    pub async fn connect_with_stream<S>(
        stream: S,
        user: &str,
        password: Option<&str>,
    ) -> Result<Terminal, TerminalError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let session = open_session(stream, &format!("{user}{LOGIN_FLAGS}"), password).await?;
        let channel = session.channel_open_session().await?;
        channel
            .request_pty(false, "dumb", 200, 50, 0, 0, &[])
            .await?;
        channel.request_shell(false).await?;
        let mut terminal = Terminal {
            session,
            channel,
            buffer: String::new(),
            consumed: String::new(),
            transcript: String::new(),
            stream: None,
            blank: 0,
            syncs: 0,
            secrets: password
                .filter(|p| !p.is_empty())
                .map(Box::from)
                .into_iter()
                .collect(),
        };
        terminal.expect(&["] > "], Duration::from_secs(30)).await?;
        // RouterOS redraws the prompt after the login banner
        terminal.sync(Duration::from_secs(30)).await?;
        Ok(terminal)
    }

    /// Sends the transcript from now on to `log` too, a line as soon as it is complete.
    pub fn stream_to(&mut self, log: &Log) {
        self.stream = Some((log.clone(), String::new()));
    }

    /// Streams the complete lines of `shown`, without runs of blank lines (like
    /// [`Terminal::transcript`]); with `all` also an incomplete last one.
    fn stream(&mut self, shown: &str, all: bool) {
        let Some((log, pending)) = &mut self.stream else {
            return;
        };
        pending.push_str(shown);
        let end = if all {
            pending.len()
        } else {
            pending.rfind('\n').map_or(0, |at| at + 1)
        };
        let mut text = String::new();
        for line in pending[..end].split_inclusive('\n') {
            self.blank = if line.trim().is_empty() { self.blank + 1 } else { 0 };
            if self.blank < 2 {
                text.push_str(line);
            }
        }
        pending.drain(..end);
        if !text.is_empty() {
            log.emit(ConsoleOutput { text: text.into() });
        }
    }

    /// Replaces `secret` by `***` in the transcript from now on.
    pub fn hide(&mut self, secret: &str) {
        if !secret.is_empty() {
            self.secrets.push(secret.into());
        }
    }

    /// Everything the session showed, without runs of blank lines (RouterOS clears the screen
    /// with them).
    pub fn transcript(&self) -> String {
        let mut result = String::with_capacity(self.transcript.len());
        let mut blank = 0;
        for line in self.transcript.split_inclusive('\n') {
            blank = if line.trim().is_empty() { blank + 1 } else { 0 };
            if blank < 2 {
                result.push_str(line);
            }
        }
        result
    }

    /// Runs a console command, waits until it is done ([`Terminal::sync`]) and returns its echo
    /// and what it printed. (A prompt is no sign of the end: one shows up in the echo of
    /// `[:len [find]] > 0` too.)
    pub async fn run(&mut self, command: &str, within: Duration) -> Result<String, TerminalError> {
        self.send_line(command).await?;
        let output = self.sync(within).await?;
        // without the last line, the prompt with the echo of the sync
        let output = output
            .trim_end_matches(['\r', '\n'])
            .rsplit_once('\n')
            .map(|(o, _)| o)
            .unwrap_or_default();
        Ok(output.replace('\r', ""))
    }

    /// Waits until the console has run everything typed before, and returns what it printed
    /// meanwhile. The console runs lines one after the other, so `insync<n>`, which only the
    /// command after them prints (its echo has `"in" . "sync" . <n>`), comes after everything
    /// they printed, the prompts RouterOS redraws included; then the prompt follows. `<n>`
    /// counts the syncs, so the output of an earlier one is never taken for this one's.
    pub async fn sync(&mut self, within: Duration) -> Result<String, TerminalError> {
        match self.sync_unless(&[], within).await? {
            Ok(output) => Ok(output),
            Err(_) => unreachable!("no other pattern"),
        }
    }

    /// [`Terminal::sync`], unless one of `ends` shows up before (the client of a session in
    /// this console printing that the session is over): `Err` with its index then.
    pub async fn sync_unless(
        &mut self,
        ends: &[&str],
        within: Duration,
    ) -> Result<Result<String, usize>, TerminalError> {
        self.syncs += 1;
        let n = self.syncs;
        self.send_line(&format!(r#":put ("in" . "sync" . {n})"#)).await?;
        let done = format!("insync{n}");
        let patterns = [&[done.as_str()], ends].concat();
        match self.expect(&patterns, within).await? {
            0 => {
                let output = std::mem::take(&mut self.consumed);
                self.expect(&["] > "], within).await?;
                Ok(Ok(output))
            }
            end => Ok(Err(end - 1)),
        }
    }

    /// Types a line, as if followed by Enter.
    pub async fn send_line(&mut self, line: &str) -> Result<(), TerminalError> {
        self.send(&format!("{line}\r")).await
    }

    pub async fn send(&mut self, text: &str) -> Result<(), TerminalError> {
        debug!("terminal <- {text:?}");
        self.channel.data_bytes(text.as_bytes().to_vec()).await?;
        Ok(())
    }

    /// Waits until one of `patterns` shows up in the output and returns its index; the output
    /// up to and including the match is consumed, what came before the match is left in
    /// `consumed`.
    pub async fn expect(
        &mut self,
        patterns: &[&str],
        within: Duration,
    ) -> Result<usize, TerminalError> {
        let deadline = Instant::now() + within;
        loop {
            if let Some((index, end)) = patterns
                .iter()
                .enumerate()
                .filter_map(|(index, p)| self.buffer.find(p).map(|at| (index, at + p.len())))
                .min_by_key(|(_, end)| *end)
            {
                self.consumed = self.buffer.drain(..end).collect();
                self.consumed
                    .truncate(self.consumed.len() - patterns[index].len());
                return Ok(index);
            }
            if !self.receive(deadline).await? {
                return Err(TerminalError::Timeout {
                    expected: patterns.iter().map(|p| Box::from(*p)).collect(),
                    waited: within,
                    tail: self.tail(),
                });
            }
        }
    }

    /// Takes in the next output; `false` if there was none until `deadline`.
    async fn receive(&mut self, deadline: Instant) -> Result<bool, TerminalError> {
        loop {
            let Ok(message) = timeout_at(deadline, self.channel.wait()).await else {
                return Ok(false);
            };
            match message {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                    let text = String::from_utf8_lossy(&data);
                    debug!("terminal -> {text:?}");
                    self.buffer.push_str(&text);
                    let mut shown = strip_escapes(&text);
                    for secret in &self.secrets {
                        shown = shown.replace(secret.as_ref(), "***");
                    }
                    self.transcript.push_str(&shown);
                    self.stream(&shown, false);
                    return Ok(true);
                }
                Some(ChannelMsg::Eof | ChannelMsg::Close) | None => {
                    return Err(TerminalError::Closed(self.tail()));
                }
                Some(_) => {}
            }
        }
    }

    fn tail(&self) -> Box<str> {
        let start = self.transcript.len().saturating_sub(300);
        let start = (start..self.transcript.len())
            .find(|i| self.transcript.is_char_boundary(*i))
            .unwrap_or(start);
        self.transcript[start..].into()
    }

    pub async fn close(mut self) {
        self.stream("", true);
        let _ = self.channel.eof().await;
        let _ = self
            .session
            .disconnect(Disconnect::ByApplication, "", "en")
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::strip_escapes;

    #[test]
    fn strips_terminal_escapes() {
        assert_eq!(
            strip_escapes("\x1b[9999B[admin@CHR] > /quit\r\ninterrupted\r\n\x1b7\x1b8"),
            "[admin@CHR] > /quit\ninterrupted\n"
        );
    }
}

#[cfg(test)]
mod upload_tests {
    use super::quarter;

    /// progress at each quarter, the last at the end
    #[test]
    fn upload_progress_comes_in_quarters() {
        let total = 1000;
        let reported: Vec<usize> = (0..total)
            .step_by(64)
            .map(|before| (before, (before + 64).min(total)))
            .filter(|&(before, sent)| quarter(before, total) != quarter(sent, total))
            .map(|(_, sent)| sent)
            .collect();
        assert_eq!(reported, [256, 512, 768, 1000]);
        assert_eq!(quarter(0, 0), 4);
    }
}
