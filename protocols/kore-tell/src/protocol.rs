//
//

//! # Definte protocol for tell
//! 

use futures::future::{ready, Ready};
use libp2p::core::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p::swarm::Stream;
use smallvec::SmallVec;

/// The level of support for a particular protocol.
#[derive(Debug, Clone)]
pub enum ProtocolSupport {
    /// The protocol is only supported for inbound requests.
    Inbound,
    /// The protocol is only supported for outbound requests.
    Outbound,
    /// The protocol is supported for inbound and outbound requests.
    Full,
}

impl ProtocolSupport {
    /// Whether inbound requests are supported.
    pub fn inbound(&self) -> bool {
        match self {
            ProtocolSupport::Inbound | ProtocolSupport::Full => true,
            ProtocolSupport::Outbound => false,
        }
    }

    /// Whether outbound requests are supported.
    pub fn outbound(&self) -> bool {
        match self {
            ProtocolSupport::Outbound | ProtocolSupport::Full => true,
            ProtocolSupport::Inbound => false,
        }
    }
}

/// A protocol for sending a message without waiting for a response.
/// 
/// This is a simple protocol that only sends a message without waiting for a response.
/// 
#[derive(Debug)]
pub struct TellProtocol<P> {
    pub(crate) protocols: SmallVec<[P; 2]>,
}

impl<P> UpgradeInfo for TellProtocol<P>
where
    P: AsRef<str> + Clone,
{
    type Info = P;
    type InfoIter = smallvec::IntoIter<[Self::Info; 2]>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.protocols.clone().into_iter()
    }
}

impl<P> InboundUpgrade<Stream> for TellProtocol<P>
where
    P: AsRef<str> + Clone,
{
    type Output = (Stream, P);
    type Error = void::Void;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, io: Stream, protocol: Self::Info) -> Self::Future {
        ready(Ok((io, protocol)))
    }
}

impl<P> OutboundUpgrade<Stream> for TellProtocol<P>
where
    P: AsRef<str> + Clone,
{
    type Output = (Stream, P);
    type Error = void::Void;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, io: Stream, protocol: Self::Info) -> Self::Future {
        ready(Ok((io, protocol)))
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_protocol_support() {
        let support = ProtocolSupport::Inbound;
        assert!(support.inbound());
        assert!(!support.outbound());

        let support = ProtocolSupport::Outbound;
        assert!(!support.inbound());
        assert!(support.outbound());

        let support = ProtocolSupport::Full;
        assert!(support.inbound());
        assert!(support.outbound());
    }

    #[test]
    fn test_tell_protocol() {
        let protocol = TellProtocol {
            protocols: smallvec::smallvec!["/tell/1.0.0".to_string()],
        };

        let info = protocol.protocol_info().collect::<Vec<_>>();
        assert_eq!(info.len(), 1);
        assert_eq!(info[0], "/tell/1.0.0");
    }
}