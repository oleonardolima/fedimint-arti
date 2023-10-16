//! The [`KeySpecifier`] trait and its implementations.

use crate::Result;
use derive_more::{Deref, DerefMut, Display, Into};
use serde::{Deserialize, Serialize};
use tor_error::internal;

/// The path of a key in the Arti key store.
///
/// An `ArtiPath` is a nonempty sequence of [`ArtiPathComponent`]s, separated by `/`.  Path
/// components may contain UTF-8 alphanumerics, and (except as the first or last character) `-`,
/// `_`, or  `.`.
/// Consequently, leading or trailing or duplicated / are forbidden.
///
/// NOTE: There is a 1:1 mapping between a value that implements `KeySpecifier` and its
/// corresponding `ArtiPath`. A `KeySpecifier` can be converted to an `ArtiPath`, but the reverse
/// conversion is not supported.
//
// TODO HSS: Create an error type for ArtiPath errors instead of relying on internal!
// TODO HSS: disallow consecutive `.` to prevent path traversal.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deref, DerefMut, Into, Display)]
pub struct ArtiPath(String);

/// A separator for `ArtiPath`s.
const PATH_SEP: char = '/';

impl ArtiPath {
    /// Create a new [`ArtiPath`].
    ///
    /// This function returns an error if `inner` is not a valid `ArtiPath`.
    // TODO HSS this function (and validate_str) should have a bespoke error type
    pub fn new(inner: String) -> Result<Self> {
        if let Some(e) = inner
            .split(PATH_SEP)
            .find_map(|s| ArtiPathComponent::validate_str(s).err())
        {
            return Err(e);
        }

        Ok(Self(inner))
    }
}

/// A component of an [`ArtiPath`].
///
/// Path components may contain UTF-8 alphanumerics, and (except as the first or last character)
/// `-`,  `_`, or `.`.
#[derive(
    Clone,
    Debug,
    derive_more::Deref,
    derive_more::DerefMut,
    derive_more::Into,
    derive_more::Display,
    Hash,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Serialize,
    Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct ArtiPathComponent(String);

impl ArtiPathComponent {
    /// Create a new [`ArtiPathComponent`].
    ///
    /// This function returns an error if `inner` is not a valid `ArtiPathComponent`.
    pub fn new(inner: String) -> Result<Self> {
        Self::validate_str(&inner)?;

        Ok(Self(inner))
    }

    /// Check whether `c` can be used within an `ArtiPathComponent`.
    fn is_allowed_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_' || c == '-' || c == '.'
    }

    /// Validate the underlying representation of an `ArtiPath` or `ArtiPathComponent`.
    fn validate_str(inner: &str) -> Result<()> {
        /// These cannot be the first or last chars of an `ArtiPath` or `ArtiPathComponent`.
        const MIDDLE_ONLY: &[char] = &['-', '_', '.'];

        if inner.is_empty()
            || inner.chars().any(|c| !Self::is_allowed_char(c))
            || inner.contains("..")
        {
            return Err(Box::new(internal!("Invalid arti path: {inner}")));
        }

        for c in MIDDLE_ONLY {
            if inner.starts_with(*c) || inner.ends_with(*c) {
                return Err(Box::new(internal!("Invalid arti path: {inner}")));
            }
        }

        Ok(())
    }
}

impl TryFrom<String> for ArtiPathComponent {
    type Error = crate::Error; // TODO HSS should be bespoke error type
    fn try_from(s: String) -> Result<ArtiPathComponent> {
        Self::new(s)
    }
}

impl AsRef<str> for ArtiPathComponent {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// The path of a key in the C Tor key store.
#[derive(Clone, Debug, derive_more::Deref, derive_more::DerefMut)]
pub struct CTorPath(String);

/// The "specifier" of a key, which identifies an instance of a key.
///
/// [`KeySpecifier::arti_path()`] should uniquely identify an instance of a key.
pub trait KeySpecifier {
    /// The location of the key in the Arti key store.
    ///
    /// This also acts as a unique identifier for a specific key instance.
    fn arti_path(&self) -> Result<ArtiPath>;

    /// The location of the key in the C Tor key store (if supported).
    ///
    /// This function should return `None` for keys that are recognized by Arti's key stores, but
    /// not by C Tor's key store (such as `HsClientIntroAuthKeypair`).
    fn ctor_path(&self) -> Option<CTorPath>;
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
    #![allow(clippy::unchecked_duration_subtraction)]
    #![allow(clippy::useless_vec)]
    #![allow(clippy::needless_pass_by_value)]
    //! <!-- @@ end test lint list maintained by maint/add_warning @@ -->
    use super::*;

    fn is_invalid_arti_path_error(err: &crate::Error) -> bool {
        err.to_string().contains("Invalid arti path")
    }

    macro_rules! check_valid {
        ($ty:ident, $inner:expr, $expect_valid:expr) => {{
            let path = $ty::new($inner.to_string());

            if $expect_valid {
                assert!(path.is_ok(), "{} should be valid", $inner);
                assert_eq!(path.unwrap().to_string(), *$inner);
            } else {
                assert!(path.is_err(), "{} should be invalid", $inner);
                assert!(
                    is_invalid_arti_path_error(path.as_ref().unwrap_err()),
                    "wrong error type for {}: {path:?}",
                    $inner
                );
            }
        }};
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn arti_path_validation() {
        const VALID_ARTI_PATHS: &[&str] = &[
            "my-hs-client-2",
            "hs_client",
            "client٣¾",
            "clientß",
            "client.key",
        ];

        const INVALID_ARTI_PATHS: &[&str] = &[
            "alice//bob",
            "/alice/bob",
            "alice/bob/",
            "-hs_client",
            "_hs_client",
            "hs_client-",
            "hs_client_",
            ".client",
            "client.",
            "-",
            "_",
            "c++",
            "client?",
            "no spaces please",
            "/",
            "/////",
            "./bob",
            "alice/../bob",
        ];

        for path in VALID_ARTI_PATHS {
            check_valid!(ArtiPath, path, true);
            check_valid!(ArtiPathComponent, path, true);
        }

        for path in INVALID_ARTI_PATHS {
            check_valid!(ArtiPath, path, false);
            check_valid!(ArtiPathComponent, path, false);
        }

        const SEP: char = PATH_SEP;
        // This is a valid ArtiPath, but not a valid ArtiPathComponent
        let path = format!("a{SEP}client{SEP}key.private");
        check_valid!(ArtiPath, &path, true);
        check_valid!(ArtiPathComponent, &path, false);
    }

    #[test]
    fn serde() {
        // TODO HSS clone-and-hack with tor_hsservice::::nickname::test::serde
        // perhaps there should be some utility in tor-basic-utils for testing
        // validated string newtypes, or something
        #[derive(Serialize, Deserialize, Debug)]
        struct T {
            n: ArtiPathComponent,
        }
        let j = serde_json::from_str(r#"{ "n": "x" }"#).unwrap();
        let t: T = serde_json::from_value(j).unwrap();
        assert_eq!(&t.n.to_string(), "x");

        assert_eq!(&serde_json::to_string(&t).unwrap(), r#"{"n":"x"}"#);

        let j = serde_json::from_str(r#"{ "n": "!" }"#).unwrap();
        let e = serde_json::from_value::<T>(j).unwrap_err();
        // TODO HSS this is the wrong error message, this might not be a bug;
        // it could be bad config or something
        assert!(e.to_string().contains("internal error"), "wrong msg {e:?}");
    }
}
