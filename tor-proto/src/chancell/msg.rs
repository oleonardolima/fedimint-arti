//! Different kinds of messages that can be encoded in channel cells.

use super::ChanCmd;
use crate::crypto::cell::{RawCellBody, CELL_BODY_LEN};
use std::net::{IpAddr, Ipv4Addr};
use tor_bytes::{self, Error, Readable, Reader, Result, Writer};

/// Trait for the 'bodies' of channel messages.
pub trait Body: Readable {
    /// Convert this type into a ChanMsg, wrapped as appropriate.
    fn as_message(self) -> ChanMsg;
    /// Consume this message and encode its body onto `w`.
    ///
    /// Does not encode anything _but_ the cell body, and does not pad
    /// to the cell length.
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W);
}

/// Decoded message from a channel.
///
/// A ChanMsg is an item received on a channel -- a message
/// from another Tor node that we are connected to directly over a TLS
/// connection.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum ChanMsg {
    /// A Padding message
    Padding(Padding),
    /// Variable-length padding message
    VPadding(VPadding),
    /// (Deprecated) TAP-based cell to create a new circuit.
    Create(Create),
    /// (Mostly deprecated) HMAC-based cell to create a new circuit.
    CreateFast(CreateFast),
    /// Cell to create a new circuit
    Create2(Create2),
    /// (Deprecated) Answer to a Create cell
    Created(Created),
    /// (Mostly Deprecated) Answer to a CreateFast cell
    CreatedFast(CreatedFast),
    /// Answer to a Create2 cell
    Created2(Created2),
    /// A message sent along a circuit, likely to a more-distant relay.
    Relay(Relay),
    /// A message sent along a circuit (limited supply)
    RelayEarly(Relay),
    /// Tear down a circuit
    Destroy(Destroy),
    /// Part of channel negotiation: describes our position on the network
    Netinfo(Netinfo),
    /// Part of channel negotiation: describes what link protocol versions
    /// we support
    Versions(Versions),
    /// Negotiates what kind of channel padding to send
    PaddingNegotiate(PaddingNegotiate),
    /// Part of channel negotiation: additional certificates not in the
    /// TLS handshake
    Certs(Certs),
    /// Part of channel negotiation: additional random material to be used
    /// as part of authentication
    AuthChallenge(AuthChallenge),
    /// Part of channel negotiation: used to authenticate relays when they
    /// initiate connection
    Authenticate(Authenticate),
    /// Not yet used
    Authorize(Authorize),
    /// Any cell whose command we don't recognize
    Unrecognized(Unrecognized),
}

impl ChanMsg {
    /// Return the ChanCmd for this message.
    pub fn get_cmd(&self) -> ChanCmd {
        use ChanMsg::*;
        match self {
            Padding(_) => ChanCmd::PADDING,
            VPadding(_) => ChanCmd::VPADDING,
            Create(_) => ChanCmd::CREATE,
            CreateFast(_) => ChanCmd::CREATE_FAST,
            Create2(_) => ChanCmd::CREATE2,
            Created(_) => ChanCmd::CREATED,
            CreatedFast(_) => ChanCmd::CREATED_FAST,
            Created2(_) => ChanCmd::CREATED2,
            Relay(_) => ChanCmd::RELAY,
            RelayEarly(_) => ChanCmd::RELAY_EARLY,
            Destroy(_) => ChanCmd::DESTROY,
            Netinfo(_) => ChanCmd::NETINFO,
            Versions(_) => ChanCmd::VERSIONS,
            PaddingNegotiate(_) => ChanCmd::PADDING_NEGOTIATE,
            Certs(_) => ChanCmd::CERTS,
            AuthChallenge(_) => ChanCmd::AUTH_CHALLENGE,
            Authenticate(_) => ChanCmd::AUTHENTICATE,
            Authorize(_) => ChanCmd::AUTHORIZE,
            Unrecognized(c) => c.get_cmd(),
        }
    }

    /// Write the body of this message (not including length or command).
    pub fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        use ChanMsg::*;
        match self {
            Padding(b) => b.write_body_onto(w),
            VPadding(b) => b.write_body_onto(w),
            Create(b) => b.write_body_onto(w),
            CreateFast(b) => b.write_body_onto(w),
            Create2(b) => b.write_body_onto(w),
            Created(b) => b.write_body_onto(w),
            CreatedFast(b) => b.write_body_onto(w),
            Created2(b) => b.write_body_onto(w),
            Relay(b) => b.write_body_onto(w),
            RelayEarly(b) => b.write_body_onto(w),
            Destroy(b) => b.write_body_onto(w),
            Netinfo(b) => b.write_body_onto(w),
            Versions(b) => b.write_body_onto(w),
            PaddingNegotiate(b) => b.write_body_onto(w),
            Certs(b) => b.write_body_onto(w),
            AuthChallenge(b) => b.write_body_onto(w),
            Authenticate(b) => b.write_body_onto(w),
            Authorize(b) => b.write_body_onto(w),
            Unrecognized(b) => b.write_body_onto(w),
        }
    }

    /// Decode this message from a given reader, according to a specified
    /// command value. The reader must be truncated to the exact length
    /// of the body.
    pub fn take(r: &mut Reader<'_>, cmd: ChanCmd) -> Result<Self> {
        use ChanMsg::*;
        Ok(match cmd {
            ChanCmd::PADDING => Padding(r.extract()?),
            ChanCmd::VPADDING => VPadding(r.extract()?),
            ChanCmd::CREATE => Create(r.extract()?),
            ChanCmd::CREATE_FAST => CreateFast(r.extract()?),
            ChanCmd::CREATE2 => Create2(r.extract()?),
            ChanCmd::CREATED => Created(r.extract()?),
            ChanCmd::CREATED_FAST => CreatedFast(r.extract()?),
            ChanCmd::CREATED2 => Created2(r.extract()?),
            ChanCmd::RELAY => Relay(r.extract()?),
            ChanCmd::RELAY_EARLY => RelayEarly(r.extract()?),
            ChanCmd::DESTROY => Destroy(r.extract()?),
            ChanCmd::NETINFO => Netinfo(r.extract()?),
            ChanCmd::VERSIONS => Versions(r.extract()?),
            ChanCmd::PADDING_NEGOTIATE => PaddingNegotiate(r.extract()?),
            ChanCmd::CERTS => Certs(r.extract()?),
            ChanCmd::AUTH_CHALLENGE => AuthChallenge(r.extract()?),
            ChanCmd::AUTHENTICATE => Authenticate(r.extract()?),
            ChanCmd::AUTHORIZE => Authorize(r.extract()?),
            _ => Unrecognized(unrecognized_with_cmd(cmd, r)?),
        })
    }
}

/// A Padding message is a fixed-length message on a channel that is
/// ignored.
///
/// Padding message can be used to disguise the true amount of data on a
/// channel, or as a "keep-alive".
///
/// The correct response to a padding cell is to drop it and do nothing.
#[derive(Clone, Debug)]
pub struct Padding {}
impl Body for Padding {
    fn as_message(self) -> ChanMsg {
        ChanMsg::Padding(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, _w: &mut W) {}
}
impl Readable for Padding {
    fn take_from(_r: &mut Reader<'_>) -> Result<Self> {
        Ok(Padding {})
    }
}

/// A VPadding message is a variable-length padding message.
///
/// The correct response to a padding cell is to drop it and do nothing.
#[derive(Clone, Debug)]
pub struct VPadding {
    len: u16,
}
impl Body for VPadding {
    fn as_message(self) -> ChanMsg {
        ChanMsg::VPadding(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        w.write_zeros(self.len as usize);
    }
}
impl Readable for VPadding {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        if r.remaining() > std::u16::MAX as usize {
            return Err(Error::BadMessage("Too many bytes in VPADDING cell"));
        }
        Ok(VPadding {
            len: r.remaining() as u16,
        })
    }
}

/// helper -- declare a fixed-width cell where a fixed number of bytes
/// matter and the rest are ignored
macro_rules! fixed_len {
    {
        $(#[$meta:meta])*
        $name:ident , $cmd:ident, $len:ident
    } => {
        $(#[$meta])*
        #[derive(Clone,Debug)]
        pub struct $name {
            handshake: Vec<u8>
        }
        impl Body for $name {
            fn as_message(self) -> ChanMsg {
                ChanMsg::$name(self)
            }
            fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
                w.write_all(&self.handshake[..])
            }
        }
        impl Readable for $name {
            fn take_from(r: &mut Reader<'_>) -> Result<Self> {
                Ok($name {
                    handshake: r.take($len)?.into(),
                })
            }
        }
    }
}

// XXXX MOVE THESE
/// Number of bytes used for a TAP handshake by the initiator.
pub const TAP_C_HANDSHAKE_LEN: usize = 128 * 2 + 42;
/// Number of bytes used for a TAP handshake response
pub const TAP_S_HANDSHAKE_LEN: usize = 128 + 20;

/// Number of bytes used for a "CREATE_FAST" handshake by the initiator.
const FAST_C_HANDSHAKE_LEN: usize = 20;
/// Number of bytes used for a "CREATE_FAST" handshake by the responder
const FAST_S_HANDSHAKE_LEN: usize = 20 * 2;

fixed_len! {
    /// A Create cell creates a circuit, using the TAP handshake
    ///
    /// TAP is an obsolete handshake based on RSA-1024.
    Create, CREATE, TAP_C_HANDSHAKE_LEN
}
fixed_len! {
    /// A Creatd cell responds to a Create cell, using the TAP handshake
    ///
    /// TAP is an obsolete handshake based on RSA-1024.
    Created, CREATED, TAP_S_HANDSHAKE_LEN
}
fixed_len! {
    /// A CreateFast cell creates a circuit using no public-key crypto.
    ///
    /// This handshake was originally used for the first hop of every
    /// circuit.  Nowadays it is used for creating one-hop circuits in
    /// the case where we don't know any onion key for the first hop.
    CreateFast, CREATE_FAST, FAST_C_HANDSHAKE_LEN
}
fixed_len! {
    /// A CreatedFast cell responds to a CreateFast cell
    CreatedFast, CREATED_FAST, FAST_S_HANDSHAKE_LEN
}

/// Create a circuit on the current channel.
///
/// To create a circuit, the client sends a Create2 cell containing a
/// handshake of a given type; the relay responds with a Created2 cell.
#[derive(Clone, Debug)]
pub struct Create2 {
    handshake_type: u16,
    handshake: Vec<u8>,
}
impl Body for Create2 {
    fn as_message(self) -> ChanMsg {
        ChanMsg::Create2(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        w.write_u16(self.handshake_type);
        assert!(self.handshake.len() <= std::u16::MAX as usize);
        w.write_u16(self.handshake.len() as u16);
        w.write_all(&self.handshake[..]);
    }
}
impl Readable for Create2 {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        let handshake_type = r.take_u16()?;
        let hlen = r.take_u16()?;
        let handshake = r.take(hlen as usize)?.into();
        Ok(Create2 {
            handshake_type,
            handshake,
        })
    }
}

/// Response to a Create2 cell
#[derive(Clone, Debug)]
pub struct Created2 {
    handshake: Vec<u8>,
}
impl Body for Created2 {
    fn as_message(self) -> ChanMsg {
        ChanMsg::Created2(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        assert!(self.handshake.len() <= std::u16::MAX as usize);
        w.write_u16(self.handshake.len() as u16);
        w.write_all(&self.handshake[..]);
    }
}
impl Readable for Created2 {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        let hlen = r.take_u16()?;
        let handshake = r.take(hlen as usize)?.into();
        Ok(Created2 { handshake })
    }
}

/// A Relay cell-- that is, one transmitted over a circuit.
///
/// Once a circuit has been established, relay cells can be sent over
/// it.  Clients can send relay cells to any relay on the circuit. Any
/// relay on the circuit can send relay cells to the client, either
/// directly (if it is the first hop), or indirectly through the
/// intermediate hops.
///
/// A different protocol is defined over the relay cells; it is implemented
/// XXXX.
#[derive(Clone)]
pub struct Relay {
    body: Box<RawCellBody>,
}
impl std::fmt::Debug for Relay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Relay").finish()
    }
}
impl Body for Relay {
    fn as_message(self) -> ChanMsg {
        ChanMsg::Relay(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        w.write_all(&self.body[..])
    }
}
impl Readable for Relay {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        let mut body = Box::new([0u8; CELL_BODY_LEN]);
        (&mut body[..]).copy_from_slice(r.take(CELL_BODY_LEN)?);
        Ok(Relay { body })
    }
}

/// Tear down a circuit
///
/// On receiving a Destroy message, a Tor implementation should
/// tear down the associated circuit, and relay the destroy message
/// down the circuit to later/earlier nodes on the circuit (if any).
#[derive(Clone, Debug)]
pub struct Destroy {}
impl Body for Destroy {
    fn as_message(self) -> ChanMsg {
        ChanMsg::Destroy(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, _w: &mut W) {}
}
impl Readable for Destroy {
    fn take_from(_r: &mut Reader<'_>) -> Result<Self> {
        Ok(Destroy {})
    }
}

/// The netinfo message ends channel negotiation.
///
/// It tells the other party on the channel our view of the current time,
/// our own list of public addresses, and our view of its address.
///
/// When we get a netinfo cell, we can start creating circuits on a
/// channel and sending data.
#[derive(Clone, Debug)]
pub struct Netinfo {
    timestamp: u32,
    their_addr: IpAddr,
    my_addr: Vec<IpAddr>,
}
/// helper: encode a single address in the form that netinfo messages expect
fn enc_one_netinfo_addr<W: Writer + ?Sized>(w: &mut W, addr: &IpAddr) {
    match addr {
        IpAddr::V4(ipv4) => {
            w.write_u8(0x04); // type.
            w.write_u8(4); // length.
            w.write_all(&ipv4.octets()[..]);
        }
        IpAddr::V6(ipv6) => {
            w.write_u8(0x06); // type.
            w.write_u8(16); // length.
            w.write_all(&ipv6.octets()[..]);
        }
    }
}
/// helper: take an address as encoded in a netinfo message
fn take_one_netinfo_addr(r: &mut Reader<'_>) -> Result<Option<IpAddr>> {
    let atype = r.take_u8()?;
    let alen = r.take_u8()?;
    let abody = r.take(alen as usize)?;
    match (atype, alen) {
        (0x04, 4) => {
            let bytes = [abody[0], abody[1], abody[2], abody[3]];
            Ok(Some(IpAddr::V4(bytes.into())))
        }
        (0x06, 16) => {
            // XXXX is there a better way?
            let mut bytes = [0u8; 16];
            (&mut bytes[..]).copy_from_slice(abody);
            Ok(Some(IpAddr::V6(bytes.into())))
        }
        (0x04, _) => Ok(None), // ignore this? Or call it an error?
        (0x06, _) => Ok(None), // ignore this, or call it an error?
        (_, _) => Ok(None),
    }
}
impl Netinfo {
    /// Construct a new Netinfo to be sent by a client.
    pub fn for_client(their_addr: IpAddr) -> Self {
        Netinfo {
            timestamp: 0, // clients don't report their timestamps.
            their_addr,
            my_addr: Vec::new(), // clients don't report their addrs.
        }
    }
}
impl Body for Netinfo {
    fn as_message(self) -> ChanMsg {
        ChanMsg::Netinfo(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        w.write_u32(self.timestamp);
        enc_one_netinfo_addr(w, &self.their_addr);
        w.write_u8(self.my_addr.len() as u8); // XXXX overflow?
        for addr in self.my_addr.iter() {
            enc_one_netinfo_addr(w, &addr);
        }
    }
}
impl Readable for Netinfo {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        let timestamp = r.take_u32()?;
        let their_addr = take_one_netinfo_addr(r)?.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let mut my_addr = Vec::new();
        let my_n_addrs = r.take_u8()?;
        for _ in 0..my_n_addrs {
            if let Some(a) = take_one_netinfo_addr(r)? {
                my_addr.push(a);
            }
        }
        Ok(Netinfo {
            timestamp,
            their_addr,
            my_addr,
        })
    }
}

/// A Versions cell begins channel negotiation.
///
/// Every channel must begin by sending a Versions message.  This message
/// lists the link protocol versions that this Tor implementation supports.
///
/// Note that we should never actually send Versions cells using the
/// usual channel encoding: Versions cells use two-byte circuit IDs,
/// whereas all the other cell types use four-byte circuit IDs
/// [assuming a non-obsolete version is negotiated].
#[derive(Clone, Debug)]
pub struct Versions {
    versions: Vec<u16>,
}
impl Versions {
    /// Construct a new Versions message using a provided list of link
    /// protocols
    pub fn new(vs: &[u16]) -> Self {
        let versions = vs.into();
        assert!(vs.len() < (std::u16::MAX / 2) as usize);
        Self { versions }
    }
    /// Encode this VERSIONS cell in the manner expected for a handshake.
    ///
    /// (That's different from a standard cell encoding, since we
    /// have not negotiated versions yet, and so our circuit-ID length
    /// is an obsolete 2 bytes).
    pub fn encode_for_handshake(self) -> Vec<u8> {
        let mut v = Vec::new();
        v.write_u16(0); // obsolete circuit ID length.
        v.write_u8(ChanCmd::VERSIONS.into());
        v.write_u16((self.versions.len() * 2) as u16); // message length.
        self.write_body_onto(&mut v);
        v
    }
    /// Return the best (numerically highest) link protocol that is
    /// shared by this versions cell and my_protos.
    pub fn best_shared_link_protocol(&self, my_protos: &[u16]) -> Option<u16> {
        // NOTE: this implementation is quadratic, but it shouldn't matter
        // much so long as my_protos is small.
        let p = my_protos
            .iter()
            .filter(|p| self.versions.contains(p))
            .fold(0u16, |a, b| u16::max(a, *b));
        if p == 0 {
            None
        } else {
            Some(p)
        }
    }
}
impl Body for Versions {
    fn as_message(self) -> ChanMsg {
        ChanMsg::Versions(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        for v in self.versions.iter() {
            w.write_u16(*v);
        }
    }
}
impl Readable for Versions {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        let mut versions = Vec::new();
        while r.remaining() > 0 {
            versions.push(r.take_u16()?);
        }
        Ok(Versions { versions })
    }
}

/// Used to negotiate channel padding
#[derive(Clone, Debug)]
pub struct PaddingNegotiate {
    command: u8,
    ito_low_ms: u16,
    ito_high_ms: u16,
}
impl Body for PaddingNegotiate {
    fn as_message(self) -> ChanMsg {
        ChanMsg::PaddingNegotiate(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        w.write_u8(0); // version
        w.write_u8(self.command);
        w.write_u16(self.ito_low_ms);
        w.write_u16(self.ito_high_ms);
    }
}
impl Readable for PaddingNegotiate {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        let v = r.take_u8()?;
        if v != 0 {
            return Err(Error::BadMessage(
                "Unrecognized padding negotiation version",
            ));
        }
        let command = r.take_u8()?;
        let ito_low_ms = r.take_u16()?;
        let ito_high_ms = r.take_u16()?;
        Ok(PaddingNegotiate {
            command,
            ito_low_ms,
            ito_high_ms,
        })
    }
}

/// A single certificate in a Certs cell.
///
/// The formats used here are implemented in tor-cert. Ed25519Cert is the
/// most common.
#[derive(Clone, Debug)]
struct TorCert {
    certtype: u8,
    cert: Vec<u8>,
}
fn enc_one_tor_cert<W: Writer + ?Sized>(w: &mut W, c: &TorCert) {
    w.write_u8(c.certtype);
    w.write_u16(c.cert.len() as u16); // XXXX overflow?
    w.write_all(&c.cert[..]);
}
fn take_one_tor_cert(r: &mut Reader<'_>) -> Result<TorCert> {
    let certtype = r.take_u8()?;
    let certlen = r.take_u16()?;
    let cert = r.take(certlen as usize)?;
    Ok(TorCert {
        certtype,
        cert: cert.into(),
    })
}
/// Used as part of the channel handshake to send additioinal certificates
///
/// These certificates are not presented as part of the TLS handshake.
/// Originally this was meant to make Tor TLS handshakes look "normal", but
/// nowadays it serves less purpose, especially now that we have TLS 1.3.
///
/// Every relay sends these cells as part of negotiation; clients do not
/// send them.
#[derive(Clone, Debug)]
pub struct Certs {
    certs: Vec<TorCert>,
}
impl Certs {
    /// Look for a certificate of type 'tp' in this cell; return it if
    /// there is one.
    pub fn parse_ed_cert(&self, tp: tor_cert::CertType) -> crate::Result<tor_cert::KeyUnknownCert> {
        let cert = self
            .certs
            .iter()
            .find(|c| c.certtype == tp.into())
            .ok_or_else(|| crate::Error::ChanProto(format!("Missing {} certificate", tp)))?;

        let cert = tor_cert::Ed25519Cert::decode(&cert.cert)?;
        if cert.peek_cert_type() != tp {
            return Err(crate::Error::ChanProto(format!(
                "Found a {} certificate labeled as {}",
                cert.peek_cert_type(),
                tp
            )));
        }

        Ok(cert)
    }
}

impl Body for Certs {
    fn as_message(self) -> ChanMsg {
        ChanMsg::Certs(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        w.write_u8(self.certs.len() as u8); //XXXXX overflow?
        for c in self.certs.iter() {
            enc_one_tor_cert(w, &c)
        }
    }
}
impl Readable for Certs {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        let n = r.take_u8()?;
        let mut certs = Vec::new();
        for _ in 0..n {
            certs.push(take_one_tor_cert(r)?);
        }
        Ok(Certs { certs })
    }
}

/// Part of negotiation: sent by responders to initiators.
///
/// The AuthChallenge cell is used to ensure that some unpredictable material
/// has been sent on the channel, and to tell the initiator what
/// authentication methods will be extended.
///
/// Clients can safely ignore this message: they don't need to authenticate.
#[derive(Clone, Debug)]
pub struct AuthChallenge {
    challenge: Vec<u8>,
    methods: Vec<u16>,
}
const CHALLENGE_LEN: usize = 32;
impl Body for AuthChallenge {
    fn as_message(self) -> ChanMsg {
        ChanMsg::AuthChallenge(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        w.write_all(&self.challenge[..]);
        assert!(self.methods.len() <= std::u16::MAX as usize);
        w.write_u16(self.methods.len() as u16);
        for m in self.methods.iter() {
            w.write_u16(*m);
        }
    }
}
impl Readable for AuthChallenge {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        let challenge = r.take(CHALLENGE_LEN)?.into();
        let n_methods = r.take_u16()?;
        let mut methods = Vec::new();
        for _ in 0..n_methods {
            methods.push(r.take_u16()?);
        }
        Ok(AuthChallenge { challenge, methods })
    }
}

/// Part of negotiation: sent by initiators to responders.
///
/// The Authenticate cell proves the initiator's identity to the
/// responder, even if TLS client authentication was not used.
///
/// Clients do not use this.
#[derive(Clone, Debug)]
pub struct Authenticate {
    authtype: u16,
    auth: Vec<u8>,
}
impl Body for Authenticate {
    fn as_message(self) -> ChanMsg {
        ChanMsg::Authenticate(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        w.write_u16(self.authtype);
        assert!(self.auth.len() <= std::u16::MAX as usize);
        w.write_u16(self.auth.len() as u16);
        w.write_all(&self.auth[..]);
    }
}
impl Readable for Authenticate {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        let authtype = r.take_u16()?;
        let authlen = r.take_u16()?;
        let auth = r.take(authlen as usize)?.into();
        Ok(Authenticate { authtype, auth })
    }
}

/// The Authorize cell type is not yet used.
#[derive(Clone, Debug)]
pub struct Authorize {
    content: Vec<u8>,
}
impl Body for Authorize {
    fn as_message(self) -> ChanMsg {
        ChanMsg::Authorize(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        w.write_all(&self.content[..])
    }
}
impl Readable for Authorize {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        Ok(Authorize {
            content: r.take(r.remaining())?.into(),
        })
    }
}

/// Holds any cell whose command we don't recognize.
///
/// Well-behaved Tor implementations are required to ignore commands
/// like this.
#[derive(Clone, Debug)]
pub struct Unrecognized {
    cmd: ChanCmd,
    content: Vec<u8>,
}
fn unrecognized_with_cmd(cmd: ChanCmd, r: &mut Reader<'_>) -> Result<Unrecognized> {
    let mut u = Unrecognized::take_from(r)?;
    u.cmd = cmd;
    Ok(u)
}
impl Unrecognized {
    fn get_cmd(&self) -> ChanCmd {
        self.cmd
    }
}
impl Body for Unrecognized {
    fn as_message(self) -> ChanMsg {
        ChanMsg::Unrecognized(self)
    }
    fn write_body_onto<W: Writer + ?Sized>(self, w: &mut W) {
        w.write_all(&self.content[..])
    }
}
impl Readable for Unrecognized {
    fn take_from(r: &mut Reader<'_>) -> Result<Self> {
        Ok(Unrecognized {
            cmd: 0.into(),
            content: r.take(r.remaining())?.into(),
        })
    }
}

// Helper: declare an Into implementation for cells that don't take a circid.
macro_rules! msg_into_cell {
    ($body:ident) => {
        impl Into<super::ChanCell> for $body {
            fn into(self) -> super::ChanCell {
                super::ChanCell {
                    circid: 0.into(),
                    msg: self.as_message(),
                }
            }
        }
    };
}

msg_into_cell!(Padding);
msg_into_cell!(VPadding);
msg_into_cell!(Netinfo);
msg_into_cell!(Versions);
msg_into_cell!(PaddingNegotiate);
msg_into_cell!(Certs);
msg_into_cell!(AuthChallenge);
msg_into_cell!(Authenticate);
msg_into_cell!(Authorize);
