//! Parsing implementation for Tor microdescriptors.
//!
//! A "microdescriptor" is an incomplete, infrequently-changing
//! summary of a relay's information that is generated by
//! the directory authorities.
//!
//! Microdescriptors are much smaller than router descriptors, and
//! change less frequently. For this reason, they're currently used
//! for building circuits by all relays and clients.
//!
//! Microdescriptors can't be used on their own: you need to know
//! which relay they are for, which requires a valid consensus
//! directory.

use crate::parse::keyword::Keyword;
use crate::parse::parser::SectionRules;
use crate::parse::tokenize::{ItemResult, NetDocReader};
use crate::types::family::RelayFamily;
use crate::types::misc::*;
use crate::types::policy::PortPolicy;
use crate::util;
use crate::util::str::Extent;
use crate::{AllowAnnotations, Error, ParseErrorKind as EK, Result};
use tor_error::internal;
use tor_llcrypto::d;
use tor_llcrypto::pk::{curve25519, ed25519, rsa};

use digest::Digest;
use once_cell::sync::Lazy;
use std::sync::Arc;

use std::time;

#[cfg(feature = "build_docs")]
mod build;

#[cfg(feature = "build_docs")]
pub use build::MicrodescBuilder;

/// Annotations prepended to a microdescriptor that has been stored to
/// disk.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct MicrodescAnnotation {
    /// A time at which this microdescriptor was last listed in some
    /// consensus document.
    last_listed: Option<time::SystemTime>,
}

/// The digest of a microdescriptor as used in microdesc consensuses
pub type MdDigest = [u8; 32];

/// A single microdescriptor.
#[allow(dead_code)]
#[cfg_attr(
    feature = "dangerous-expose-struct-fields",
    visible::StructFields(pub),
    non_exhaustive
)]
#[derive(Clone, Debug)]
pub struct Microdesc {
    /// The SHA256 digest of the text of this microdescriptor.  This
    /// value is used to identify the microdescriptor when downloading
    /// it, and when listing it in a consensus document.
    // TODO: maybe this belongs somewhere else. Once it's used to store
    // correlate the microdesc to a consensus, it's never used again.
    #[cfg_attr(docsrs, doc(cfg(feature = "dangerous-expose-struct-fields")))]
    sha256: MdDigest,
    /// Public key used for the ntor circuit extension protocol.
    #[cfg_attr(docsrs, doc(cfg(feature = "dangerous-expose-struct-fields")))]
    ntor_onion_key: curve25519::PublicKey,
    /// Declared family for this relay.
    #[cfg_attr(docsrs, doc(cfg(feature = "dangerous-expose-struct-fields")))]
    family: Arc<RelayFamily>,
    /// List of IPv4 ports to which this relay will exit
    #[cfg_attr(docsrs, doc(cfg(feature = "dangerous-expose-struct-fields")))]
    ipv4_policy: Arc<PortPolicy>,
    /// List of IPv6 ports to which this relay will exit
    #[cfg_attr(docsrs, doc(cfg(feature = "dangerous-expose-struct-fields")))]
    ipv6_policy: Arc<PortPolicy>,
    /// Ed25519 identity for this relay
    #[cfg_attr(docsrs, doc(cfg(feature = "dangerous-expose-struct-fields")))]
    ed25519_id: ed25519::Ed25519Identity,
    // addr is obsolete and doesn't go here any more
    // pr is obsolete and doesn't go here any more.
    // The legacy "tap" onion-key is obsolete, and though we parse it, we don't
    // save it.
}

impl Microdesc {
    /// Create a new MicrodescBuilder that can be used to construct
    /// microdescriptors.
    ///
    /// This function is only available when the crate is built with the
    /// `build_docs` feature.
    ///
    /// # Limitations
    ///
    /// The generated microdescriptors cannot yet be encoded, and do
    /// not yet have correct sha256 digests. As such they are only
    /// useful for testing.
    #[cfg(feature = "build_docs")]
    pub fn builder() -> MicrodescBuilder {
        MicrodescBuilder::new()
    }

    /// Return the sha256 digest of this microdesc.
    pub fn digest(&self) -> &MdDigest {
        &self.sha256
    }
    /// Return the ntor onion key for this microdesc
    pub fn ntor_key(&self) -> &curve25519::PublicKey {
        &self.ntor_onion_key
    }
    /// Return the ipv4 exit policy for this microdesc
    pub fn ipv4_policy(&self) -> &Arc<PortPolicy> {
        &self.ipv4_policy
    }
    /// Return the ipv6 exit policy for this microdesc
    pub fn ipv6_policy(&self) -> &Arc<PortPolicy> {
        &self.ipv6_policy
    }
    /// Return the relay family for this microdesc
    pub fn family(&self) -> &RelayFamily {
        self.family.as_ref()
    }
    /// Return the ed25519 identity for this microdesc, if its
    /// Ed25519 identity is well-formed.
    pub fn ed25519_id(&self) -> &ed25519::Ed25519Identity {
        &self.ed25519_id
    }
}

/// A microdescriptor annotated with additional data
///
/// TODO: rename this.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct AnnotatedMicrodesc {
    /// The microdescriptor
    md: Microdesc,
    /// The annotations for the microdescriptor
    ann: MicrodescAnnotation,
    /// Where did we find the microdescriptor with the originally parsed
    /// string?
    location: Option<Extent>,
}

impl AnnotatedMicrodesc {
    /// Consume this annotated microdesc and discard its annotations.
    pub fn into_microdesc(self) -> Microdesc {
        self.md
    }

    /// Return a reference to the microdescriptor within this annotated
    /// microdescriptor.
    pub fn md(&self) -> &Microdesc {
        &self.md
    }

    /// If this Microdesc was parsed from `s`, return its original text.
    pub fn within<'a>(&self, s: &'a str) -> Option<&'a str> {
        self.location.as_ref().and_then(|ext| ext.reconstruct(s))
    }
}

decl_keyword! {
    /// Keyword type for recognized objects in microdescriptors.
    MicrodescKwd {
        annotation "@last-listed" => ANN_LAST_LISTED,
        "onion-key" => ONION_KEY,
        "ntor-onion-key" => NTOR_ONION_KEY,
        "family" => FAMILY,
        "p" => P,
        "p6" => P6,
        "id" => ID,
    }
}

/// Rules about annotations that can appear before a Microdescriptor
static MICRODESC_ANNOTATIONS: Lazy<SectionRules<MicrodescKwd>> = Lazy::new(|| {
    use MicrodescKwd::*;
    let mut rules = SectionRules::new();
    rules.add(ANN_LAST_LISTED.rule().args(1..));
    rules.add(ANN_UNRECOGNIZED.rule().may_repeat().obj_optional());
    rules
});
/// Rules about entries that must appear in an Microdesc, and how they must
/// be formed.
static MICRODESC_RULES: Lazy<SectionRules<MicrodescKwd>> = Lazy::new(|| {
    use MicrodescKwd::*;

    let mut rules = SectionRules::new();
    rules.add(ONION_KEY.rule().required().no_args().obj_required());
    rules.add(NTOR_ONION_KEY.rule().required().args(1..));
    rules.add(FAMILY.rule().args(1..));
    rules.add(P.rule().args(2..));
    rules.add(P6.rule().args(2..));
    rules.add(ID.rule().may_repeat().args(2..));
    rules.add(UNRECOGNIZED.rule().may_repeat().obj_optional());
    rules
});

impl MicrodescAnnotation {
    /// Extract a (possibly empty) microdescriptor annotation from a
    /// reader.
    #[allow(dead_code)]
    fn parse_from_reader(
        reader: &mut NetDocReader<'_, MicrodescKwd>,
    ) -> Result<MicrodescAnnotation> {
        use MicrodescKwd::*;

        let mut items = reader.pause_at(|item| item.is_ok_with_non_annotation());
        let body = MICRODESC_ANNOTATIONS.parse(&mut items)?;

        let last_listed = match body.get(ANN_LAST_LISTED) {
            None => None,
            Some(item) => Some(item.args_as_str().parse::<Iso8601TimeSp>()?.into()),
        };

        Ok(MicrodescAnnotation { last_listed })
    }
}

impl Microdesc {
    /// Parse a string into a new microdescriptor.
    pub fn parse(s: &str) -> Result<Microdesc> {
        let mut items = crate::parse::tokenize::NetDocReader::new(s);
        let (result, _) = Self::parse_from_reader(&mut items).map_err(|e| e.within(s))?;
        items.should_be_exhausted()?;
        Ok(result)
    }

    /// Extract a single microdescriptor from a NetDocReader.
    fn parse_from_reader(
        reader: &mut NetDocReader<'_, MicrodescKwd>,
    ) -> Result<(Microdesc, Option<Extent>)> {
        use MicrodescKwd::*;
        let s = reader.str();

        let mut first_onion_key = true;
        // We'll pause at the next annotation, or at the _second_ onion key.
        let mut items = reader.pause_at(|item| match item {
            Err(_) => false,
            Ok(item) => {
                item.kwd().is_annotation()
                    || if item.kwd() == ONION_KEY {
                        let was_first = first_onion_key;
                        first_onion_key = false;
                        !was_first
                    } else {
                        false
                    }
            }
        });

        let body = MICRODESC_RULES.parse(&mut items)?;

        // We have to start with onion-key
        let start_pos = {
            // unwrap here is safe because parsing would have failed
            // had there not been at least one item.
            #[allow(clippy::unwrap_used)]
            let first = body.first_item().unwrap();
            if first.kwd() != ONION_KEY {
                return Err(EK::WrongStartingToken
                    .with_msg(first.kwd_str().to_string())
                    .at_pos(first.pos()));
            }
            // Unwrap is safe here because we are parsing these strings from s
            #[allow(clippy::unwrap_used)]
            util::str::str_offset(s, first.kwd_str()).unwrap()
        };

        // Legacy (tap) onion key.  We parse this to make sure it's well-formed,
        // but then we discard it immediately, since we never want to use it.
        let _: rsa::PublicKey = body
            .required(ONION_KEY)?
            .parse_obj::<RsaPublic>("RSA PUBLIC KEY")?
            .check_len_eq(1024)?
            .check_exponent(65537)?
            .into();

        // Ntor onion key
        let ntor_onion_key = body
            .required(NTOR_ONION_KEY)?
            .parse_arg::<Curve25519Public>(0)?
            .into();

        // family
        //
        // (We don't need to add the relay's own ID to this family, as we do in
        // RouterDescs: the authorities already took care of that for us.)
        let family = body
            .maybe(FAMILY)
            .parse_args_as_str::<RelayFamily>()?
            .unwrap_or_else(RelayFamily::new)
            .intern();

        // exit policies.
        let ipv4_policy = body
            .maybe(P)
            .parse_args_as_str::<PortPolicy>()?
            .unwrap_or_else(PortPolicy::new_reject_all);
        let ipv6_policy = body
            .maybe(P6)
            .parse_args_as_str::<PortPolicy>()?
            .unwrap_or_else(PortPolicy::new_reject_all);

        // ed25519 identity
        let ed25519_id = {
            let id_tok = body
                .slice(ID)
                .iter()
                .find(|item| item.arg(0) == Some("ed25519"));
            match id_tok {
                None => {
                    return Err(EK::MissingToken.with_msg("id ed25519"));
                }
                Some(tok) => tok.parse_arg::<Ed25519Public>(1)?.into(),
            }
        };

        let end_pos = {
            // unwrap here is safe because parsing would have failed
            // had there not been at least one item.
            #[allow(clippy::unwrap_used)]
            let last_item = body.last_item().unwrap();
            last_item.offset_after(s).ok_or_else(|| {
                Error::from(internal!("last item was not within source string"))
                    .at_pos(last_item.end_pos())
            })?
        };

        let text = &s[start_pos..end_pos];
        let sha256 = d::Sha256::digest(text.as_bytes()).into();

        let location = Extent::new(s, text);

        let md = Microdesc {
            sha256,
            ntor_onion_key,
            family,
            ipv4_policy: ipv4_policy.intern(),
            ipv6_policy: ipv6_policy.intern(),
            ed25519_id,
        };
        Ok((md, location))
    }
}

/// Consume tokens from 'reader' until the next token is the beginning
/// of a microdescriptor: an annotation or an ONION_KEY.  If no such
/// token exists, advance to the end of the reader.
fn advance_to_next_microdesc(reader: &mut NetDocReader<'_, MicrodescKwd>, annotated: bool) {
    use MicrodescKwd::*;
    let iter = reader.iter();
    loop {
        let item = iter.peek();
        match item {
            Some(Ok(t)) => {
                let kwd = t.kwd();
                if (annotated && kwd.is_annotation()) || kwd == ONION_KEY {
                    return;
                }
            }
            Some(Err(_)) => {
                // We skip over broken tokens here.
                //
                // (This case can't happen in practice, since if there had been
                // any error tokens, they would have been handled as part of
                // handling the previous microdesc.)
            }
            None => {
                return;
            }
        };
        let _ = iter.next();
    }
}

/// An iterator that parses one or more (possibly annotated)
/// microdescriptors from a string.
#[derive(Debug)]
pub struct MicrodescReader<'a> {
    /// True if we accept annotations; false otherwise.
    annotated: bool,
    /// An underlying reader to give us Items for the microdescriptors
    reader: NetDocReader<'a, MicrodescKwd>,
}

impl<'a> MicrodescReader<'a> {
    /// Construct a MicrodescReader to take microdescriptors from a string
    /// 's'.
    pub fn new(s: &'a str, allow: &AllowAnnotations) -> Self {
        let reader = NetDocReader::new(s);
        let annotated = allow == &AllowAnnotations::AnnotationsAllowed;
        MicrodescReader { annotated, reader }
    }

    /// If we're annotated, parse an annotation from the reader. Otherwise
    /// return a default annotation.
    fn take_annotation(&mut self) -> Result<MicrodescAnnotation> {
        if self.annotated {
            MicrodescAnnotation::parse_from_reader(&mut self.reader)
        } else {
            Ok(MicrodescAnnotation::default())
        }
    }

    /// Parse a (possibly annotated) microdescriptor from the reader.
    ///
    /// On error, parsing stops after the first failure.
    fn take_annotated_microdesc_raw(&mut self) -> Result<AnnotatedMicrodesc> {
        let ann = self.take_annotation()?;
        let (md, location) = Microdesc::parse_from_reader(&mut self.reader)?;
        Ok(AnnotatedMicrodesc { md, ann, location })
    }

    /// Parse a (possibly annotated) microdescriptor from the reader.
    ///
    /// On error, advance the reader to the start of the next microdescriptor.
    fn take_annotated_microdesc(&mut self) -> Result<AnnotatedMicrodesc> {
        let pos_orig = self.reader.pos();
        let result = self.take_annotated_microdesc_raw();
        if result.is_err() {
            if self.reader.pos() == pos_orig {
                // No tokens were consumed from the reader.  We need to
                // drop at least one token to ensure we aren't looping.
                //
                // (This might not be able to happen, but it's easier to
                // explicitly catch this case than it is to prove that
                // it's impossible.)
                let _ = self.reader.iter().next();
            }
            advance_to_next_microdesc(&mut self.reader, self.annotated);
        }
        result
    }
}

impl<'a> Iterator for MicrodescReader<'a> {
    type Item = Result<AnnotatedMicrodesc>;
    fn next(&mut self) -> Option<Self::Item> {
        // If there is no next token, we're at the end.
        self.reader.iter().peek()?;

        Some(
            self.take_annotated_microdesc()
                .map_err(|e| e.within(self.reader.str())),
        )
    }
}

#[cfg(test)]
mod test {
    // @@ begin test lint list maintained by maint/add_warning @@
    #![allow(clippy::bool_assert_comparison)]
    #![allow(clippy::clone_on_copy)]
    #![allow(clippy::dbg_macro)]
    #![allow(clippy::print_stderr)]
    #![allow(clippy::print_stdout)]
    #![allow(clippy::single_char_pattern)]
    #![allow(clippy::unwrap_used)]
    //! <!-- @@ end test lint list maintained by maint/add_warning @@ -->
    use super::*;
    use hex_literal::hex;
    const TESTDATA: &str = include_str!("../../testdata/microdesc1.txt");
    const TESTDATA2: &str = include_str!("../../testdata/microdesc2.txt");

    fn read_bad(fname: &str) -> String {
        use std::fs;
        use std::path::PathBuf;
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("testdata");
        path.push("bad-mds");
        path.push(fname);

        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn parse_single() -> Result<()> {
        let _md = Microdesc::parse(TESTDATA)?;
        Ok(())
    }

    #[test]
    fn parse_multi() -> Result<()> {
        use std::time::{Duration, SystemTime};
        let mds: Result<Vec<_>> =
            MicrodescReader::new(TESTDATA2, &AllowAnnotations::AnnotationsAllowed).collect();
        let mds = mds?;
        assert_eq!(mds.len(), 4);

        assert_eq!(
            mds[0].ann.last_listed.unwrap(),
            SystemTime::UNIX_EPOCH + Duration::new(1580151129, 0)
        );
        assert_eq!(
            mds[0].md().digest(),
            &hex!("38c71329a87098cb341c46c9c62bd646622b4445f7eb985a0e6adb23a22ccf4f")
        );
        assert_eq!(
            mds[0].md().ntor_key().as_bytes(),
            &hex!("5e895d65304a3a1894616660143f7af5757fe08bc18045c7855ee8debb9e6c47")
        );
        assert!(mds[0].md().ipv4_policy().allows_port(993));
        assert!(mds[0].md().ipv6_policy().allows_port(993));
        assert!(!mds[0].md().ipv4_policy().allows_port(25));
        assert!(!mds[0].md().ipv6_policy().allows_port(25));
        assert_eq!(
            mds[0].md().ed25519_id().as_bytes(),
            &hex!("2d85fdc88e6c1bcfb46897fca1dba6d1354f93261d68a79e0b5bc170dd923084")
        );

        Ok(())
    }

    #[test]
    fn test_bad() {
        use crate::types::policy::PolicyError;
        use crate::Pos;
        fn check(fname: &str, e: &Error) {
            let content = read_bad(fname);
            let res = Microdesc::parse(&content);
            assert!(res.is_err());
            assert_eq!(&res.err().unwrap(), e);
        }

        check(
            "wrong-start",
            &EK::WrongStartingToken
                .with_msg("family")
                .at_pos(Pos::from_line(1, 1)),
        );
        check(
            "bogus-policy",
            &EK::BadPolicy
                .at_pos(Pos::from_line(9, 1))
                .with_source(PolicyError::InvalidPort),
        );
        check("wrong-id", &EK::MissingToken.with_msg("id ed25519"));
    }

    #[test]
    fn test_recover() {
        let mut data = read_bad("wrong-start");
        data += TESTDATA;
        data += &read_bad("wrong-id");

        let res: Vec<Result<_>> =
            MicrodescReader::new(&data, &AllowAnnotations::AnnotationsAllowed).collect();

        assert_eq!(res.len(), 3);
        assert!(res[0].is_err());
        assert!(res[1].is_ok());
        assert!(res[2].is_err());
    }
}
