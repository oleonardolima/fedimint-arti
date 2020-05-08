/// Macro for declaring a keyword enumeration to help parse a document.
///
/// A keyword enumber implements the Keyword trait.
///
/// These enums are a bit different from those made by `caret`, in a
/// few ways.  Notably, they are optimized for parsing, they are
/// required to be compact, and they allow multiple strings to be mapped to
/// a single index.
///
/// ```ignore
/// decl_keyword! {
///    Location {
//         "start" => START,
///        "middle" | "center" => MID,
///        "end" => END
///    }
/// }
///
/// assert_eq!(Location::from_str("start"), Location::START);
/// assert_eq!(Location::from_str("stfff"), Location::UNRECOGNIZED);
/// ```
macro_rules! decl_keyword {
    { $name:ident { $( $($s:literal)|+ => $i:ident),* $(,)? } } => {
        #[derive(Copy,Clone,Eq,PartialEq,Debug,std::hash::Hash)]
        #[allow(non_camel_case_types)]
        enum $name {
            $( $i , )*
            UNRECOGNIZED
        }
        impl $crate::rules::Keyword for $name {
            fn idx(self) -> usize { self as usize }
            fn n_vals() -> usize { ($name::UNRECOGNIZED as usize) + 1 }
            fn from_str(s : &str) -> Self {
                // Note usage of phf crate to create a perfect hash over
                // the possible keywords.  It will be even better if someday
                // the phf crate can find hash functions that are better
                // than siphash.
                const KEYWORD: phf::Map<&'static str, $name> = phf::phf_map! {
                    $( $( $s => $name::$i , )+ )*
                };
                * KEYWORD.get(s).unwrap_or(& $name::UNRECOGNIZED)
            }
            fn from_idx(i : usize) -> Option<Self> {
                // Note looking up the value in a vec.  This may or may
                // not be faster than a case statement would be.
                lazy_static::lazy_static! {
                    static ref VALS: Vec<$name> =
                        vec![ $($name::$i , )* $name::UNRECOGNIZED ];
                };
                VALS.get(i).copied()
            }
            fn to_str(&self) -> &'static str {
                use $name::*;
                match self {
                    $( $i => concat!{ $($s),+ } , )*
                    UNRECOGNIZED => "<unrecognized>"
                }
            }
        }
        impl $name {
            pub fn rule(self) -> $crate::rules::TokenFmtBuilder<Self> {
                $crate::rules::TokenFmtBuilder::new(self)
            }
        }
    }
}
