use alloc::{boxed::Box, string::String};
use async_trait::async_trait;
use coap_lite::error::MessageError;
use coap_lite::Packet;
use core::fmt::{self, Debug};
use core::pin::Pin;
use futures::{Sink, Stream};

/// Generalization of the underlying CoAP transport, intended primarily to make it easy to support a
/// wide range of protocols (TCP, DTLS, websockets, BLE, etc) but also to eventually support
/// alternative runtime frameworks such as Embassy for embedded devices.
#[async_trait]
pub trait Transport {
    type Endpoint: Debug + Send + Clone;

    /// Perform the binding, that is, begin accepting new data from this transport even if
    /// there isn't yet a handler serving the data source yet.  Note that this is not quite
    /// the same as TcpSocket::bind which requires a loop to accept the incoming connections.  In
    /// our case, we expect a continuous async stream of (Packet, Endpoint) pairs which distinguish
    /// each individual socket.  The transport impl is expected to spawn new tasks for each
    /// accepted source as necessary.
    async fn bind(self) -> Result<BoxedFramedBinding<Self::Endpoint>, TransportError>;
}

pub type BoxedFramedBinding<Endpoint> = Pin<Box<dyn FramedBinding<Endpoint>>>;

/// Trait generalizing a common feature of async libraries like tokio where a socket is exposed
/// as both a stream and a sink.  It should be possible even for libraries that have them split
/// to unify in the bridging layer with this crate.
///
/// Note that this binding is intended to generalize cases like TCP and UDP together which is why
/// the abstraction doesn't have an additional layer for accepting an incoming connection.  For
/// UDP there is no such concept and framed items can simply arrive at any time from any source.
pub trait FramedBinding<Endpoint>:
    Send
    + Stream<Item = Result<FramedItem<Endpoint>, FramedReadError<Endpoint>>>
    + Sink<FramedItem<Endpoint>, Error = FramedWriteError>
{
    /// Access the link's MTU which can be used to determine things like the ideal block
    /// transfer size to recommend.  If it cannot be determined by the link, a suitable
    /// default one will be selected based on the CoAP specification.
    fn mtu(&self) -> Option<u32>;
}

/// Parsed CoAP packet coming from a remote peer, as designated by [`Endpoint`].  Note that
/// the endpoint is delivered with each packet so that packet-oriented protocols can
/// avoid the leaky abstraction of a "connection" to a given Endpoint.
pub type FramedItem<Endpoint> = (Packet, Endpoint);

/// Error when receiving from a remote peer.  Note that here [`Endpoint`] is optional as there may
/// be a generic read error unrelated to any remote peer, for example if the underlying bound
/// socket is closed.
pub type FramedReadError<Endpoint> = (TransportError, Option<Endpoint>);

/// Error when sending to a remote peer.  Note that [`Endpoint`] is omitted in this case as the
/// endpoint is provided to the send APIs themselves so we can easily tell which peer generated
/// the error.
pub type FramedWriteError = TransportError;

/// Generalized errors indicating a range of transport-related issues such as being unable to bind,
/// disconnections from remote peers, malformed input, etc.  Most of these errors are non-fatal
/// and the server can happily continue serving other customers.
#[derive(Debug)]
pub enum TransportError {
    #[cfg(feature = "std")]
    IoError(Option<std::io::Error>),
    MalformedPacket(MessageError),
    Unspecified(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "std")]
            Self::IoError(_) => write!(f, "generic I/O error"),
            Self::MalformedPacket(_) => write!(f, "packet was malformed"),
            Self::Unspecified(err) => write!(f, "unspecified: {}", err),
        }
    }
}

#[cfg(feature = "std")]
impl From<Option<std::io::Error>> for TransportError {
    fn from(x: Option<std::io::Error>) -> Self {
        Self::IoError(x)
    }
}

impl From<MessageError> for TransportError {
    fn from(x: MessageError) -> Self {
        Self::MalformedPacket(x)
    }
}
