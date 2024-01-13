//
//

//! Kore-Tell Protocol
//!

mod codec;
mod handler;
mod json;
mod protocol;

pub(crate) use codec::Codec;

#[cfg(test)]
mod tests {}
