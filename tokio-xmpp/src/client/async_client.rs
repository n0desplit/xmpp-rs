use futures::{sink::SinkExt, task::Poll, Future, Sink, Stream};
use sasl::common::{ChannelBinding, Credentials};
use std::mem::replace;
use std::pin::Pin;
use std::task::Context;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
#[cfg(feature = "tls-native")]
use tokio_native_tls::TlsStream;
#[cfg(feature = "tls-rust")]
use tokio_rustls::client::TlsStream;
use xmpp_parsers::{ns, Element, Jid};

use super::auth::auth;
use super::bind::bind;
use crate::event::Event;
use crate::happy_eyeballs::{connect_to_host, connect_with_srv};
use crate::starttls::starttls;
use crate::xmpp_codec::Packet;
use crate::xmpp_stream::{self, add_stanza_id};
use crate::{Error, ProtocolError};

/// XMPP client connection and state
///
/// It is able to reconnect. TODO: implement session management.
///
/// This implements the `futures` crate's [`Stream`](#impl-Stream) and
/// [`Sink`](#impl-Sink<Packet>) traits.
pub struct Client {
    config: Config,
    state: ClientState,
    reconnect: bool,
    // TODO: tls_required=true
}

/// XMPP server connection configuration
#[derive(Clone, Debug)]
pub enum ServerConfig {
    /// Use SRV record to find server host
    UseSrv,
    #[allow(unused)]
    /// Manually define server host and port
    Manual {
        /// Server host name
        host: String,
        /// Server port
        port: u16,
    },
}

/// XMMPP client configuration
#[derive(Clone, Debug)]
pub struct Config {
    /// jid of the account
    pub jid: Jid,
    /// password of the account
    pub password: String,
    /// server configuration for the account
    pub server: ServerConfig,
}

type XMPPStream = xmpp_stream::XMPPStream<TlsStream<TcpStream>>;

enum ClientState {
    Invalid,
    Disconnected,
    Connecting(JoinHandle<Result<XMPPStream, Error>>),
    Connected(XMPPStream),
}

impl Client {
    /// Start a new XMPP client
    ///
    /// Start polling the returned instance so that it will connect
    /// and yield events.
    pub fn new<J: Into<Jid>, P: Into<String>>(jid: J, password: P) -> Self {
        let config = Config {
            jid: jid.into(),
            password: password.into(),
            server: ServerConfig::UseSrv,
        };
        Self::new_with_config(config)
    }

    /// Start a new client given that the JID is already parsed.
    pub fn new_with_config(config: Config) -> Self {
        let connect = tokio::spawn(Self::connect(
            config.server.clone(),
            config.jid.clone(),
            config.password.clone(),
        ));
        let client = Client {
            config,
            state: ClientState::Connecting(connect),
            reconnect: false,
        };
        client
    }

    /// Set whether to reconnect (`true`) or let the stream end
    /// (`false`) when a connection to the server has ended.
    pub fn set_reconnect(&mut self, reconnect: bool) -> &mut Self {
        self.reconnect = reconnect;
        self
    }

    async fn connect(
        server: ServerConfig,
        jid: Jid,
        password: String,
    ) -> Result<XMPPStream, Error> {
        let username = jid.node_str().unwrap();
        let password = password;

        // TCP connection
        let tcp_stream = match server {
            ServerConfig::UseSrv => {
                connect_with_srv(jid.domain_str(), "_xmpp-client._tcp", 5222).await?
            }
            ServerConfig::Manual { host, port } => connect_to_host(host.as_str(), port).await?,
        };

        // Unencryped XMPPStream
        let xmpp_stream =
            xmpp_stream::XMPPStream::start(tcp_stream, jid.clone(), ns::JABBER_CLIENT.to_owned())
                .await?;

        let xmpp_stream = if xmpp_stream.stream_features.can_starttls() {
            // TlsStream
            let tls_stream = starttls(xmpp_stream).await?;
            // Encrypted XMPPStream
            xmpp_stream::XMPPStream::start(tls_stream, jid.clone(), ns::JABBER_CLIENT.to_owned())
                .await?
        } else {
            return Err(Error::Protocol(ProtocolError::NoTls));
        };

        let creds = Credentials::default()
            .with_username(username)
            .with_password(password)
            .with_channel_binding(ChannelBinding::None);
        // Authenticated (unspecified) stream
        let stream = auth(xmpp_stream, creds).await?;
        // Authenticated XMPPStream
        let xmpp_stream =
            xmpp_stream::XMPPStream::start(stream, jid, ns::JABBER_CLIENT.to_owned()).await?;

        // XMPPStream bound to user session
        let xmpp_stream = bind(xmpp_stream).await?;
        Ok(xmpp_stream)
    }

    /// Get the client's bound JID (the one reported by the XMPP
    /// server).
    pub fn bound_jid(&self) -> Option<&Jid> {
        match self.state {
            ClientState::Connected(ref stream) => Some(&stream.jid),
            _ => None,
        }
    }

    /// Send stanza
    pub async fn send_stanza(&mut self, stanza: Element) -> Result<(), Error> {
        self.send(Packet::Stanza(add_stanza_id(stanza, ns::JABBER_CLIENT)))
            .await
    }

    /// End connection by sending `</stream:stream>`
    ///
    /// You may expect the server to respond with the same. This
    /// client will then drop its connection.
    ///
    /// Make sure to disable reconnect.
    pub async fn send_end(&mut self) -> Result<(), Error> {
        self.send(Packet::StreamEnd).await
    }
}

/// Incoming XMPP events
///
/// In an `async fn` you may want to use this with `use
/// futures::stream::StreamExt;`
impl Stream for Client {
    type Item = Event;

    /// Low-level read on the XMPP stream, allowing the underlying
    /// machinery to:
    ///
    /// * connect,
    /// * starttls,
    /// * authenticate,
    /// * bind a session, and finally
    /// * receive stanzas
    ///
    /// ...for your client
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let state = replace(&mut self.state, ClientState::Invalid);

        match state {
            ClientState::Invalid => panic!("Invalid client state"),
            ClientState::Disconnected if self.reconnect => {
                // TODO: add timeout
                let connect = tokio::spawn(Self::connect(
                    self.config.server.clone(),
                    self.config.jid.clone(),
                    self.config.password.clone(),
                ));
                self.state = ClientState::Connecting(connect);
                self.poll_next(cx)
            }
            ClientState::Disconnected => Poll::Ready(None),
            ClientState::Connecting(mut connect) => match Pin::new(&mut connect).poll(cx) {
                Poll::Ready(Ok(Ok(stream))) => {
                    let bound_jid = stream.jid.clone();
                    self.state = ClientState::Connected(stream);
                    Poll::Ready(Some(Event::Online {
                        bound_jid,
                        resumed: false,
                    }))
                }
                Poll::Ready(Ok(Err(e))) => {
                    self.state = ClientState::Disconnected;
                    return Poll::Ready(Some(Event::Disconnected(e.into())));
                }
                Poll::Ready(Err(e)) => {
                    self.state = ClientState::Disconnected;
                    panic!("connect task: {}", e);
                }
                Poll::Pending => {
                    self.state = ClientState::Connecting(connect);
                    Poll::Pending
                }
            },
            ClientState::Connected(mut stream) => {
                // Poll sink
                match Pin::new(&mut stream).poll_ready(cx) {
                    Poll::Pending => (),
                    Poll::Ready(Ok(())) => (),
                    Poll::Ready(Err(e)) => {
                        self.state = ClientState::Disconnected;
                        return Poll::Ready(Some(Event::Disconnected(e.into())));
                    }
                };

                // Poll stream
                //
                // This needs to be a loop in order to ignore packets we don’t care about, or those
                // we want to handle elsewhere.  Returning something isn’t correct in those two
                // cases because it would signal to tokio that the XMPPStream is also done, while
                // there could be additional packets waiting for us.
                //
                // The proper solution is thus a loop which we exit once we have something to
                // return.
                loop {
                    match Pin::new(&mut stream).poll_next(cx) {
                        Poll::Ready(None) => {
                            // EOF
                            self.state = ClientState::Disconnected;
                            return Poll::Ready(Some(Event::Disconnected(Error::Disconnected)));
                        }
                        Poll::Ready(Some(Ok(Packet::Stanza(stanza)))) => {
                            // Receive stanza
                            self.state = ClientState::Connected(stream);
                            return Poll::Ready(Some(Event::Stanza(stanza)));
                        }
                        Poll::Ready(Some(Ok(Packet::Text(_)))) => {
                            // Ignore text between stanzas
                        }
                        Poll::Ready(Some(Ok(Packet::StreamStart(_)))) => {
                            // <stream:stream>
                            self.state = ClientState::Disconnected;
                            return Poll::Ready(Some(Event::Disconnected(
                                ProtocolError::InvalidStreamStart.into(),
                            )));
                        }
                        Poll::Ready(Some(Ok(Packet::StreamEnd))) => {
                            // End of stream: </stream:stream>
                            self.state = ClientState::Disconnected;
                            return Poll::Ready(Some(Event::Disconnected(Error::Disconnected)));
                        }
                        Poll::Pending => {
                            // Try again later
                            self.state = ClientState::Connected(stream);
                            return Poll::Pending;
                        }
                        Poll::Ready(Some(Err(e))) => {
                            self.state = ClientState::Disconnected;
                            return Poll::Ready(Some(Event::Disconnected(e.into())));
                        }
                    }
                }
            }
        }
    }
}

/// Outgoing XMPP packets
///
/// See `send_stanza()` for an `async fn`
impl Sink<Packet> for Client {
    type Error = Error;

    fn start_send(mut self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        match self.state {
            ClientState::Connected(ref mut stream) => {
                Pin::new(stream).start_send(item).map_err(|e| e.into())
            }
            _ => Err(Error::InvalidState),
        }
    }

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match self.state {
            ClientState::Connected(ref mut stream) => {
                Pin::new(stream).poll_ready(cx).map_err(|e| e.into())
            }
            _ => Poll::Pending,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match self.state {
            ClientState::Connected(ref mut stream) => {
                Pin::new(stream).poll_flush(cx).map_err(|e| e.into())
            }
            _ => Poll::Pending,
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match self.state {
            ClientState::Connected(ref mut stream) => {
                Pin::new(stream).poll_close(cx).map_err(|e| e.into())
            }
            _ => Poll::Pending,
        }
    }
}
